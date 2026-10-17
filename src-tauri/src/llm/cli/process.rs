//! Runs a command-line tool the way Dayflow's ChatCLIProcessRunner does (prompt on stdin, output
//! collected from pipes, a timeout), adapted to Windows: no console window, and the whole process
//! tree is killed on timeout or when the caller stops waiting (an npm `claude.cmd` shim starts
//! `node` as a child, which a plain kill would leave running).
//!
//! `ProcessRunner` is a trait so tests can replace the real process with a fake.

use anyhow::{anyhow, Result};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One process to run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Written to the process's stdin, which is then closed.
    pub stdin: Vec<u8>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub timeout: Duration,
}

/// What a finished process printed. `code` is None when it was ended by a signal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcessOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// The process did not finish in time and was killed; carries what it printed so far.
#[derive(Debug)]
pub struct TimedOut {
    pub secs: u64,
    pub stdout: String,
    pub stderr: String,
}
impl std::fmt::Display for TimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // "timed out" is what the failure classifier looks for.
        write!(f, "CLI process timed out after {} seconds", self.secs)
    }
}
impl std::error::Error for TimedOut {}

/// The executable could not be started at all.
#[derive(Debug)]
pub struct SpawnFailed {
    pub program: String,
    pub reason: String,
    pub not_found: bool,
}
impl std::fmt::Display for SpawnFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Could not start {}: {}", self.program, self.reason)
    }
}
impl std::error::Error for SpawnFailed {}

/// Receives stdout one line at a time while the process runs (without the line ending).
pub type LineSender = tokio::sync::mpsc::UnboundedSender<String>;

pub trait ProcessRunner: Send + Sync {
    fn run(&self, spec: ProcessSpec) -> BoxFuture<'_, Result<ProcessOutput>>;

    /// `run`, also sending each stdout line to `lines` as it arrives (the chat's stream-json
    /// events). The default sends the lines after the process ended.
    fn run_lines(&self, spec: ProcessSpec, lines: LineSender) -> BoxFuture<'_, Result<ProcessOutput>> {
        Box::pin(async move {
            let out = self.run(spec).await?;
            for l in out.stdout.lines() {
                let _ = lines.send(l.trim_end_matches('\r').to_string());
            }
            Ok(out)
        })
    }
}

/// Runs real processes.
pub struct SystemRunner;

impl ProcessRunner for SystemRunner {
    fn run(&self, spec: ProcessSpec) -> BoxFuture<'_, Result<ProcessOutput>> {
        Box::pin(run_system(spec, None))
    }

    fn run_lines(&self, spec: ProcessSpec, lines: LineSender) -> BoxFuture<'_, Result<ProcessOutput>> {
        Box::pin(run_system(spec, Some(lines)))
    }
}

/// Sets the flag when the waiting future is dropped, so the worker thread kills the process.
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn run_system(spec: ProcessSpec, lines: Option<LineSender>) -> Result<ProcessOutput> {
    let cancel = Arc::new(AtomicBool::new(false));
    let _guard = CancelOnDrop(cancel.clone());
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new().name("daytrace-cli".into()).spawn(move || {
        let _ = tx.send(run_blocking(spec, &cancel, lines));
    })?;
    rx.await.map_err(|_| anyhow!("The CLI runner stopped unexpectedly"))?
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn run_blocking(spec: ProcessSpec, cancel: &AtomicBool, lines: Option<LineSender>) -> Result<ProcessOutput> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for k in &spec.env_remove {
        cmd.env_remove(k);
    }
    for (k, v) in &spec.env_set {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| SpawnFailed {
        program: spec.program.display().to_string(),
        reason: e.to_string(),
        not_found: e.kind() == std::io::ErrorKind::NotFound,
    })?;
    // Ends with Daytrace, however Daytrace ends (Quit, a crash, the end of the session).
    exit_guard::adopt(&child);
    let pid = child.id();

    let out_buf = Arc::new(Mutex::new(Vec::new()));
    let err_buf = Arc::new(Mutex::new(Vec::new()));
    let readers = [spawn_reader(child.stdout.take(), out_buf.clone(), lines), spawn_reader(child.stderr.take(), err_buf.clone(), None)];
    if let Some(mut stdin) = child.stdin.take() {
        let data = spec.stdin.clone();
        // A process that exits without reading stdin makes this write fail; that is fine.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }

    let deadline = Instant::now() + spec.timeout;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        let cancelled = cancel.load(Ordering::SeqCst);
        if cancelled || Instant::now() >= deadline {
            kill_tree(&mut child, pid);
            let _ = child.wait();
            wait_for(&readers, Duration::from_secs(2));
            if cancelled {
                return Err(anyhow!("CLI process was cancelled"));
            }
            return Err(TimedOut { secs: spec.timeout.as_secs(), stdout: lossy(&out_buf), stderr: lossy(&err_buf) }.into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // A grandchild that inherited the pipes can keep them open after the shim exits; do not wait
    // for it forever.
    wait_for(&readers, Duration::from_secs(5));
    Ok(ProcessOutput { code: status.code(), stdout: lossy(&out_buf), stderr: lossy(&err_buf) })
}

fn spawn_reader<R: std::io::Read + Send + 'static>(pipe: Option<R>, buf: Arc<Mutex<Vec<u8>>>, lines: Option<LineSender>) -> Option<std::thread::JoinHandle<()>> {
    let mut pipe = pipe?;
    Some(std::thread::spawn(move || {
        let mut chunk = [0u8; 64 * 1024];
        let mut pending: Vec<u8> = Vec::new();
        let send = |line: &[u8]| {
            if let Some(tx) = &lines {
                let _ = tx.send(String::from_utf8_lossy(line).trim_end_matches('\r').to_string());
            }
        };
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buf.lock().unwrap().extend_from_slice(&chunk[..n]);
                    if lines.is_some() {
                        pending.extend_from_slice(&chunk[..n]);
                        while let Some(at) = pending.iter().position(|b| *b == b'\n') {
                            send(&pending[..at]);
                            pending.drain(..=at);
                        }
                    }
                }
            }
        }
        if !pending.is_empty() {
            send(&pending);
        }
    }))
}

fn wait_for(readers: &[Option<std::thread::JoinHandle<()>>], max: Duration) {
    let until = Instant::now() + max;
    while Instant::now() < until && readers.iter().flatten().any(|h| !h.is_finished()) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn lossy(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&buf.lock().unwrap()).into_owned()
}

/// Ends the process and everything it started.
#[cfg(windows)]
fn kill_tree(child: &mut std::process::Child, pid: u32) {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let taskkill = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32").join("taskkill.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("taskkill"));
    let _ = Command::new(taskkill)
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = child.kill();
}

#[cfg(not(windows))]
fn kill_tree(child: &mut std::process::Child, _pid: u32) {
    let _ = child.kill();
}

/// Ties every CLI process to Daytrace's lifetime. On Windows each child goes into one job object
/// with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE; Daytrace holds the only handle to the job and never
/// closes it, so when Daytrace exits (Quit calls `app.exit`, which skips the runner's own kill)
/// Windows closes the handle and ends the CLI and everything it started (the `node` behind an
/// npm `claude.cmd` shim joins the job automatically). Elsewhere nothing is done.
#[cfg(windows)]
mod exit_guard {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// The job's handle as an integer (a HANDLE is not Sync); None when it could not be made.
    fn job() -> Option<HANDLE> {
        static JOB: OnceLock<Option<usize>> = OnceLock::new();
        let raw = *JOB.get_or_init(|| unsafe {
            let job = match CreateJobObjectW(None, windows::core::PCWSTR::null()) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!("could not create the CLI job object: {e}");
                    return None;
                }
            };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if let Err(e) = set {
                log::warn!("could not set up the CLI job object: {e}");
                let _ = CloseHandle(job);
                return None;
            }
            Some(job.0 as usize)
        });
        raw.map(|h| HANDLE(h as *mut core::ffi::c_void))
    }

    pub fn adopt(child: &std::process::Child) {
        let Some(job) = job() else { return };
        // SAFETY: the child's handle stays valid while `child` is borrowed; the job handle is
        // never closed after it was set up.
        if let Err(e) = unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as *mut core::ffi::c_void)) } {
            log::warn!("could not tie the CLI process to Daytrace: {e}");
        }
    }
}

#[cfg(not(windows))]
mod exit_guard {
    pub fn adopt(_child: &std::process::Child) {}
}

/// Scripted runner for tests: records every spec and answers from a queue.
#[cfg(test)]
pub struct FakeRunner {
    pub calls: Mutex<Vec<ProcessSpec>>,
    pub replies: Mutex<std::collections::VecDeque<Result<ProcessOutput, String>>>,
    /// Called with each spec before it answers (e.g. to write a file the real CLI would write).
    pub on_run: Option<Box<dyn Fn(&ProcessSpec) + Send + Sync>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn new(replies: Vec<Result<ProcessOutput, String>>) -> FakeRunner {
        FakeRunner { calls: Mutex::new(Vec::new()), replies: Mutex::new(replies.into()), on_run: None }
    }
    pub fn ok(stdout: &str) -> Result<ProcessOutput, String> {
        Ok(ProcessOutput { code: Some(0), stdout: stdout.into(), stderr: String::new() })
    }
    pub fn exit(code: i32, stdout: &str, stderr: &str) -> Result<ProcessOutput, String> {
        Ok(ProcessOutput { code: Some(code), stdout: stdout.into(), stderr: stderr.into() })
    }
    pub fn calls(&self) -> Vec<ProcessSpec> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl ProcessRunner for FakeRunner {
    fn run(&self, spec: ProcessSpec) -> BoxFuture<'_, Result<ProcessOutput>> {
        if let Some(f) = &self.on_run {
            f(&spec);
        }
        self.calls.lock().unwrap().push(spec);
        let reply = self.replies.lock().unwrap().pop_front().unwrap_or_else(|| Err("no scripted reply".into()));
        Box::pin(async move { reply.map_err(|e| anyhow!(e)) })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // These run /bin/sh, never a model CLI.
    #[tokio::test]
    async fn collects_output_and_stdin() {
        let spec = ProcessSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "cat; echo err >&2; exit 3".into()],
            stdin: b"hello".to_vec(),
            timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let out = SystemRunner.run(spec).await.unwrap();
        assert_eq!(out.code, Some(3));
        assert_eq!(out.stdout, "hello");
        assert_eq!(out.stderr.trim(), "err");
    }

    #[tokio::test]
    async fn times_out_and_kills() {
        let spec = ProcessSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "echo started; sleep 30".into()],
            timeout: Duration::from_millis(400),
            ..Default::default()
        };
        let started = Instant::now();
        let err = SystemRunner.run(spec).await.unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        let t = err.downcast_ref::<TimedOut>().expect("timeout error");
        assert_eq!(t.stdout.trim(), "started");
    }

    #[tokio::test]
    async fn streams_stdout_lines_while_running() {
        let spec = ProcessSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "printf 'one\\ntw'; sleep 0.2; printf 'o\\r\\nthree'".into()],
            timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let out = SystemRunner.run_lines(spec, tx).await.unwrap();
        assert_eq!(out.stdout, "one\ntwo\r\nthree");
        let mut got = Vec::new();
        while let Ok(l) = rx.try_recv() {
            got.push(l);
        }
        assert_eq!(got, ["one", "two", "three"]);
    }

    #[tokio::test]
    async fn missing_program_is_reported() {
        let spec = ProcessSpec { program: "/nonexistent/claude".into(), timeout: Duration::from_secs(5), ..Default::default() };
        let err = SystemRunner.run(spec).await.unwrap_err();
        assert!(err.downcast_ref::<SpawnFailed>().unwrap().not_found);
    }
}
