//! The write channel between `daytrace-mcp` and the running app (Dayflow's AgentBridgeServer.swift
//! and the CLI's Bridge.swift, MIT, (c) 2025 Jerry Liu). Reads go straight to the database;
//! writes come here, because the app owns the categories (settings.json, held in memory) and card
//! edits must not race the analysis loop.
//!
//! Protocol (Dayflow's): one JSON request line per connection, one JSON response line back, then
//! the connection closes. `{"protocol_version":1,"operation":...,"arguments":{...}}` →
//! `{"ok":true,"data":{...}}` or `{"ok":false,"error":{"code","message"}}`. Requests are capped
//! at 1 MB.
//!
//! Transport:
//! - Windows: a named pipe `\\.\pipe\daytrace-agent-<user SID>-<data folder hash>`. Its DACL
//!   allows only the current user (`D:P(A;;GA;;;<SID>)`, protected, so nothing is inherited),
//!   remote clients are rejected, and the app creates it as the first instance, so another
//!   program cannot hold the name first. The client checks that the process serving the pipe
//!   runs as the same user before it sends anything.
//! - Elsewhere (tests, development): a Unix socket `agent.sock` in the data folder, mode 0600,
//!   which is what Dayflow uses on macOS.
//!
//! The pipe name carries a hash of the data folder so a development copy with DAYTRACE_DATA_DIR
//! and the installed app do not answer for each other.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const PROTOCOL_VERSION: i64 = 1;
pub const MAX_LINE: usize = 1 << 20;
/// How long the client waits for the app's answer.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// A refused or failed request, with Dayflow's error codes (`app_not_running`, `edits_disabled`,
/// `not_found`, `invalid_argument`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeError {
    pub code: String,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: &str, message: impl Into<String>) -> BridgeError {
        BridgeError { code: code.into(), message: message.into() }
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for BridgeError {}

pub const APP_NOT_RUNNING: &str = "Daytrace isn't running. Reads work with the app closed; edits need Daytrace open.";

/// What the app does with one operation. Runs on a blocking thread.
pub type Handler = Arc<dyn Fn(&str, &Value) -> Result<Value, BridgeError> + Send + Sync>;

/// FNV-1a of the lower-cased data folder, as 8 hex digits.
#[cfg_attr(not(windows), allow(dead_code))]
fn folder_hash(data_dir: &Path) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in data_dir.to_string_lossy().to_lowercase().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// The socket path (Unix) for a data folder.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent.sock")
}

/// Answers one request line. `ping` needs no permission; everything else goes to `handler`,
/// which checks "Allow edits" itself (the authoritative check, whatever the client checked).
pub fn process_line(line: &[u8], handler: &Handler) -> Value {
    let failure = |e: BridgeError| json!({"ok": false, "error": {"code": e.code, "message": e.message}});
    let Ok(msg) = serde_json::from_slice::<Value>(line) else {
        return failure(BridgeError::new("protocol_error", "Unreadable request."));
    };
    let Some(op) = msg.get("operation").and_then(Value::as_str) else {
        return failure(BridgeError::new("protocol_error", "Unreadable request."));
    };
    if msg.get("protocol_version").and_then(Value::as_i64) != Some(PROTOCOL_VERSION) {
        return failure(BridgeError::new("protocol_mismatch", "Update Daytrace: daytrace-mcp and the app disagree on the protocol."));
    }
    if op == "ping" {
        return json!({"ok": true, "data": {"app": "daytrace", "version": env!("CARGO_PKG_VERSION")}});
    }
    let args = msg.get("arguments").cloned().unwrap_or_else(|| json!({}));
    match handler(op, &args) {
        Ok(data) => json!({"ok": true, "data": data}),
        Err(e) => failure(e),
    }
}

fn request_line(operation: &str, arguments: &Value) -> Vec<u8> {
    let mut line = serde_json::to_vec(&json!({"protocol_version": PROTOCOL_VERSION, "operation": operation, "arguments": arguments})).unwrap_or_default();
    line.push(b'\n');
    line
}

fn read_response(raw: &[u8]) -> Result<Value, BridgeError> {
    let end = raw.iter().position(|b| *b == b'\n').unwrap_or(raw.len());
    let v: Value = serde_json::from_slice(&raw[..end]).map_err(|_| BridgeError::new("protocol_error", "Unreadable response from Daytrace."))?;
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(v.get("data").cloned().unwrap_or_else(|| json!({})));
    }
    let e = v.get("error");
    Err(BridgeError::new(
        e.and_then(|e| e.get("code")).and_then(Value::as_str).unwrap_or("unknown"),
        e.and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("Daytrace reported an error."),
    ))
}

/// Sends one operation to the running app and returns its `data`.
pub fn send(data_dir: &Path, operation: &str, arguments: &Value) -> Result<Value, BridgeError> {
    let line = request_line(operation, arguments);
    let raw = imp::exchange(data_dir, &line)?;
    read_response(&raw)
}

/// Whether the app answers on the channel.
pub fn app_running(data_dir: &Path) -> bool {
    send(data_dir, "ping", &json!({})).is_ok()
}

/// Starts the server on the async runtime; it runs while the app runs.
pub fn spawn(data_dir: PathBuf, handler: Handler) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = serve(&data_dir, handler).await {
            log::warn!("agent bridge stopped: {e:#}");
        }
    });
}

/// Serves requests until an error; needs a tokio runtime with I/O.
pub async fn serve(data_dir: &Path, handler: Handler) -> anyhow::Result<()> {
    imp::serve(data_dir, handler).await
}

/// Reads one request line from a tokio stream (named pipe or Unix socket), answers it, closes.
macro_rules! answer_connection {
    ($stream:expr, $handler:expr) => {{
        let stream = $stream;
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        let complete = loop {
            if stream.readable().await.is_err() {
                break false;
            }
            match stream.try_read(&mut chunk) {
                Ok(0) => break !buf.is_empty(),
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.contains(&b'\n') {
                        break true;
                    }
                    if buf.len() > MAX_LINE {
                        break false;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(_) => break false,
            }
        };
        if complete {
            let end = buf.iter().position(|b| *b == b'\n').unwrap_or(buf.len());
            let line = buf[..end].to_vec();
            let handler = $handler.clone();
            let response = tokio::task::spawn_blocking(move || process_line(&line, &handler))
                .await
                .unwrap_or_else(|_| json!({"ok": false, "error": {"code": "internal_error", "message": "The request failed inside Daytrace."}}));
            let mut out = serde_json::to_vec(&response).unwrap_or_default();
            out.push(b'\n');
            let mut written = 0;
            while written < out.len() {
                if stream.writable().await.is_err() {
                    break;
                }
                match stream.try_write(&out[written..]) {
                    Ok(n) => written += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => break,
                }
            }
        }
        stream
    }};
}

#[cfg(windows)]
mod imp {
    use super::*;
    use anyhow::{anyhow, Result};
    use std::io::{Read, Write};
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

    const ERROR_PIPE_BUSY: i32 = 231;

    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: a handle this module opened, closed once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// The string SID of the user a process runs as.
    fn process_user_sid(process: HANDLE) -> Result<String> {
        // SAFETY: plain Win32 calls on handles owned here; the token buffer is u64-aligned and
        // as large as GetTokenInformation asked for; the SID string is freed with LocalFree.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
            let token = OwnedHandle(token);
            let mut len: u32 = 0;
            let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut len);
            if len == 0 {
                return Err(anyhow!("Could not read the process user"));
            }
            let mut buf: Vec<u64> = vec![0; (len as usize).div_ceil(8)];
            GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr() as *mut core::ffi::c_void), len, &mut len)?;
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut s)?;
            let out = s.to_string();
            let _ = LocalFree(Some(HLOCAL(s.0 as *mut core::ffi::c_void)));
            Ok(out?)
        }
    }

    fn current_user_sid() -> Result<String> {
        // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
        process_user_sid(unsafe { GetCurrentProcess() })
    }

    pub fn pipe_name(data_dir: &Path) -> Result<String> {
        Ok(format!(r"\\.\pipe\daytrace-agent-{}-{}", current_user_sid()?, folder_hash(data_dir)))
    }

    /// A security descriptor that grants the current user full access and nobody else anything.
    struct UserOnly {
        sd: PSECURITY_DESCRIPTOR,
        attrs: SECURITY_ATTRIBUTES,
    }
    impl UserOnly {
        fn new() -> Result<Box<UserOnly>> {
            let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})", current_user_sid()?).encode_utf16().chain(std::iter::once(0)).collect();
            let mut sd = PSECURITY_DESCRIPTOR::default();
            // SAFETY: NUL-terminated SDDL string; the descriptor is freed in Drop.
            unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(windows::core::PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut sd, None)? };
            let mut me = Box::new(UserOnly { sd, attrs: SECURITY_ATTRIBUTES::default() });
            me.attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: me.sd.0,
                bInheritHandle: false.into(),
            };
            Ok(me)
        }
        fn as_ptr(&mut self) -> *mut core::ffi::c_void {
            &mut self.attrs as *mut SECURITY_ATTRIBUTES as *mut core::ffi::c_void
        }
    }
    // SAFETY: the descriptor is plain memory owned by this value; nothing ties it to a thread.
    unsafe impl Send for UserOnly {}
    impl Drop for UserOnly {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.sd.0)));
            }
        }
    }

    pub async fn serve(data_dir: &Path, handler: Handler) -> Result<()> {
        let name = pipe_name(data_dir)?;
        let mut security = UserOnly::new()?;
        let mut options = ServerOptions::new();
        options.reject_remote_clients(true).first_pipe_instance(true);
        // SAFETY: `security` points at a valid SECURITY_ATTRIBUTES for the whole loop.
        let mut server = unsafe { options.create_with_security_attributes_raw(&name, security.as_ptr()) }
            .map_err(|e| anyhow!("Could not create the agent pipe (another program may hold its name): {e}"))?;
        options.first_pipe_instance(false);
        log::info!("agent bridge listening at {name}");
        loop {
            server.connect().await?;
            let connected = server;
            // SAFETY: as above.
            server = unsafe { options.create_with_security_attributes_raw(&name, security.as_ptr()) }?;
            let h = handler.clone();
            tokio::spawn(async move {
                let stream = answer_connection!(connected, h);
                let _ = stream.disconnect();
            });
        }
    }

    /// Refuses a pipe served by a process of another user.
    fn check_server(file: &std::fs::File) -> Result<(), BridgeError> {
        use std::os::windows::io::AsRawHandle;
        let refuse = || BridgeError::new("untrusted_server", "The program answering on Daytrace's channel is not Daytrace running as you. Edits were not sent.");
        let mut pid: u32 = 0;
        // SAFETY: the file is an open pipe handle; pid is a local.
        unsafe { GetNamedPipeServerProcessId(HANDLE(file.as_raw_handle() as *mut core::ffi::c_void), &mut pid) }.map_err(|_| refuse())?;
        // SAFETY: the process handle is closed by OwnedHandle.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|_| refuse())?;
        let process = OwnedHandle(process);
        let theirs = process_user_sid(process.0).map_err(|_| refuse())?;
        let ours = current_user_sid().map_err(|_| refuse())?;
        if theirs != ours {
            return Err(refuse());
        }
        Ok(())
    }

    pub fn exchange(data_dir: &Path, line: &[u8]) -> Result<Vec<u8>, BridgeError> {
        let name = pipe_name(data_dir).map_err(|e| BridgeError::new("socket_error", format!("Could not name Daytrace's channel: {e}")))?;
        let mut tries = 0;
        let mut file = loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(&name) {
                Ok(f) => break f,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 40 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return Err(BridgeError::new("app_not_running", APP_NOT_RUNNING)),
            }
        };
        check_server(&file)?;
        let line = line.to_vec();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| -> std::io::Result<Vec<u8>> {
                file.write_all(&line)?;
                let mut out = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = file.read(&mut chunk)?;
                    if n == 0 {
                        break;
                    }
                    out.extend_from_slice(&chunk[..n]);
                    if out.contains(&b'\n') || out.len() > MAX_LINE {
                        break;
                    }
                }
                Ok(out)
            })();
            let _ = tx.send(result);
        });
        match rx.recv_timeout(CLIENT_TIMEOUT) {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(e)) => Err(BridgeError::new("socket_error", format!("Lost the connection to Daytrace: {e}"))),
            Err(_) => Err(BridgeError::new("timeout", "Daytrace did not answer in time.")),
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use anyhow::Result;
    use std::io::{Read, Write};

    pub async fn serve(data_dir: &Path, handler: Handler) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let path = socket_path(data_dir);
        // A stale socket from a previous run blocks bind().
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path)?;
        // Owner-only: file permissions are the access control (Dayflow).
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        loop {
            let (stream, _) = listener.accept().await?;
            let h = handler.clone();
            tokio::spawn(async move {
                let _stream = answer_connection!(stream, h);
            });
        }
    }

    pub fn exchange(data_dir: &Path, line: &[u8]) -> Result<Vec<u8>, BridgeError> {
        let mut stream = std::os::unix::net::UnixStream::connect(socket_path(data_dir)).map_err(|_| BridgeError::new("app_not_running", APP_NOT_RUNNING))?;
        let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
        let io = |e: std::io::Error| BridgeError::new("socket_error", format!("Lost the connection to Daytrace: {e}"));
        stream.write_all(line).map_err(io)?;
        let mut out = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = stream.read(&mut chunk).map_err(io)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
            if out.contains(&b'\n') || out.len() > MAX_LINE {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo() -> Handler {
        Arc::new(|op: &str, args: &Value| {
            if op == "fail" {
                return Err(BridgeError::new("edits_disabled", "Edits are off."));
            }
            Ok(json!({"op": op, "args": args}))
        })
    }

    #[test]
    fn protocol_lines() {
        let h = echo();
        let ok = process_line(br#"{"protocol_version":1,"operation":"x","arguments":{"a":1}}"#, &h);
        assert_eq!(ok, json!({"ok": true, "data": {"op": "x", "args": {"a": 1}}}));
        let refused = process_line(br#"{"protocol_version":1,"operation":"fail"}"#, &h);
        assert_eq!(refused["error"]["code"], "edits_disabled");
        assert_eq!(process_line(br#"{"protocol_version":2,"operation":"x"}"#, &h)["error"]["code"], "protocol_mismatch");
        assert_eq!(process_line(b"nope", &h)["error"]["code"], "protocol_error");
        assert_eq!(process_line(br#"{"protocol_version":1,"operation":"ping"}"#, &h)["data"]["app"], "daytrace");
        assert_eq!(read_response(b"{\"ok\":true,\"data\":{\"m\":1}}\n").unwrap(), json!({"m": 1}));
        assert_eq!(read_response(br#"{"ok":false,"error":{"code":"not_found","message":"No."}}"#).unwrap_err(), BridgeError::new("not_found", "No."));
        assert_eq!(read_response(b"").unwrap_err().code, "protocol_error");
    }

    #[test]
    fn folder_hash_is_stable_and_case_blind() {
        assert_eq!(folder_hash(Path::new(r"C:\Users\Me\AppData")), folder_hash(Path::new(r"c:\users\me\appdata")));
        assert_ne!(folder_hash(Path::new("/a")), folder_hash(Path::new("/b")));
        assert_eq!(folder_hash(Path::new("/a")).len(), 8);
    }

    #[cfg(unix)]
    #[test]
    fn round_trip_over_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        assert_eq!(send(&path, "x", &json!({})).unwrap_err().code, "app_not_running");
        assert!(!app_running(&path));
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let p2 = path.clone();
        rt.spawn(async move { serve(&p2, echo()).await });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !app_running(&path) {
            assert!(std::time::Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(socket_path(&path)).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(send(&path, "x", &json!({"k": "v"})).unwrap(), json!({"op": "x", "args": {"k": "v"}}));
        assert_eq!(send(&path, "fail", &json!({})).unwrap_err().message, "Edits are off.");
        // A request without a newline that closes its side still gets an answer.
        let mut s = std::os::unix::net::UnixStream::connect(socket_path(&path)).unwrap();
        use std::io::{Read, Write};
        s.write_all(br#"{"protocol_version":1,"operation":"ping"}"#).unwrap();
        s.shutdown(std::net::Shutdown::Write).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.contains("\"ok\":true"), "{out}");
        rt.shutdown_background();
    }
}
