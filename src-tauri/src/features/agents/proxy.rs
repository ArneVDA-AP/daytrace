//! A small reverse proxy in front of the AgentPlayback dashboard. The dashboard's own server
//! (agentplayback 1.2.1, bin/cli.js) answers every request that reaches its port: it checks no
//! Host header and no Origin. A web page could read the day's sessions through DNS rebinding (a
//! host name of its own that resolves to 127.0.0.1) and start scans with cross-site POSTs.
//!
//! Daytrace starts the dashboard on a random port it tells nobody and serves it through this
//! proxy on the port the section shows (the reused one, `agents::bind_port`):
//! - the Host header must be `127.0.0.1:<port>` or `localhost:<port>`, so a rebinding page, whose
//!   requests carry its own host name, gets 403;
//! - a request with an Origin header must come from that same origin. Browsers send Origin with
//!   every POST and every cross-origin read, so cross-site POSTs and reads get 403;
//! - one request per connection: it goes up with `Connection: close` and the answer comes back
//!   with `Connection: close`, so no later request on a kept-alive connection skips the checks;
//! - bodies are copied as they arrive, never buffered whole, so a streamed answer (chunked or
//!   server-sent events) streams.
//!
//! Other accounts on the same PC can still connect to either port while the dashboard runs; the
//! Agents section says so.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The largest request or response head taken (request line or status line plus headers).
const MAX_HEAD: usize = 32 * 1024;
const MAX_HEADERS: usize = 100;
/// The largest request body passed on. The dashboard's requests have none.
const MAX_BODY: u64 = 1 << 20;
/// How long a client may take to send its request head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Connections served at the same time; more wait for a free slot.
const MAX_CONNECTIONS: usize = 64;

/// Request headers that belong to one connection and are not passed on. `Expect` goes too, so
/// the dashboard never sends an interim `100 Continue`.
const HOP_BY_HOP: [&str; 9] = ["connection", "keep-alive", "proxy-connection", "proxy-authorization", "upgrade", "te", "trailer", "transfer-encoding", "expect"];

/// The running proxy. Dropping it stops taking connections (the dashboard's process is ended
/// next to it, which ends the connections still open).
pub struct Gate {
    port: u16,
    upstream: Arc<AtomicU16>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl Gate {
    /// Serves `listener` (bound on 127.0.0.1) in front of the dashboard on `upstream`, on the app's
    /// async runtime, until the gate is dropped.
    pub fn start(listener: std::net::TcpListener, upstream: u16) -> std::io::Result<Gate> {
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let up = Arc::new(AtomicU16::new(upstream));
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let target = up.clone();
        tauri::async_runtime::spawn(async move {
            let listener = match TcpListener::from_std(listener) {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("agents proxy: could not listen on port {port}: {e}");
                    return;
                }
            };
            tokio::select! {
                _ = serve(listener, port, target) => {}
                _ = stopped => {}
            }
        });
        Ok(Gate { port, upstream: up, _stop: stop })
    }

    /// The port the dashboard is shown on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The dashboard's own port, as it reported it once it listened.
    pub fn set_upstream(&self, port: u16) {
        self.upstream.store(port, Ordering::SeqCst);
    }
}

async fn serve(listener: TcpListener, port: u16, upstream: Arc<AtomicU16>) {
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let client = match listener.accept().await {
            Ok((c, _)) => c,
            Err(e) => {
                // Out of handles, or a connection reset before it was taken: wait a moment.
                log::debug!("agents proxy: accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Ok(slot) = slots.clone().acquire_owned().await else { return };
        let up = upstream.load(Ordering::SeqCst);
        tokio::spawn(async move {
            let _slot = slot;
            if let Err(e) = handle(client, port, up).await {
                log::debug!("agents proxy: connection ended: {e}");
            }
        });
    }
}

/// A refused request: status, reason phrase and the text sent back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub reason: &'static str,
    pub text: String,
}

fn refusal(status: u16, reason: &'static str, text: impl Into<String>) -> Refusal {
    Refusal { status, reason, text: text.into() }
}

/// A request that passed the checks, as it goes to the dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// Request line and headers, ending in `Connection: close` and the empty line.
    pub head: Vec<u8>,
    /// Bytes of body that follow the head.
    pub body_len: u64,
}

fn header_is(name: &str, wanted: &str) -> bool {
    name.eq_ignore_ascii_case(wanted)
}

fn value_str(v: &[u8]) -> Option<String> {
    std::str::from_utf8(v).ok().map(|s| s.trim().to_string())
}

/// The header names a `Connection` header lists, lower-cased (they are hop-by-hop too).
fn connection_tokens(headers: &[httparse::Header]) -> Vec<String> {
    headers
        .iter()
        .filter(|h| header_is(h.name, "connection"))
        .filter_map(|h| value_str(h.value))
        .flat_map(|v| v.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()).collect::<Vec<_>>())
        .collect()
}

fn passes_on(name: &str, listed: &[String]) -> bool {
    !HOP_BY_HOP.iter().any(|h| header_is(name, h)) && !listed.iter().any(|t| header_is(name, t))
}

/// Checks a request head for the dashboard shown on `port` and builds what goes up.
pub fn check_request(head: &[u8], port: u16) -> Result<Forward, Refusal> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(head) {
        Ok(httparse::Status::Complete(_)) => {}
        Err(httparse::Error::TooManyHeaders) => return Err(refusal(431, "Request Header Fields Too Large", "Too many headers.")),
        _ => return Err(refusal(400, "Bad Request", "Unreadable request.")),
    }
    let (Some(method), Some(path), Some(version)) = (req.method, req.path, req.version) else {
        return Err(refusal(400, "Bad Request", "Unreadable request."));
    };
    // Origin form only: a full URL in the request line asks for a proxy to somewhere else.
    if !path.starts_with('/') {
        return Err(refusal(400, "Bad Request", "Only paths on this dashboard can be requested."));
    }
    let hosts: Vec<String> = req.headers.iter().filter(|h| header_is(h.name, "host")).filter_map(|h| value_str(h.value)).collect();
    let [host] = hosts.as_slice() else {
        return Err(refusal(400, "Bad Request", "The request needs one Host header."));
    };
    let host = host.to_ascii_lowercase();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return Err(refusal(403, "Forbidden", format!("This dashboard only answers requests for http://127.0.0.1:{port}/.")));
    }
    let origins: Vec<Option<String>> = req.headers.iter().filter(|h| header_is(h.name, "origin")).map(|h| value_str(h.value)).collect();
    match origins.as_slice() {
        [] => {}
        [Some(o)] if o.eq_ignore_ascii_case(&format!("http://{host}")) => {}
        _ => return Err(refusal(403, "Forbidden", "Requests from other sites are refused.")),
    }
    if req.headers.iter().any(|h| header_is(h.name, "transfer-encoding")) {
        return Err(refusal(411, "Length Required", "Send the request body with a Content-Length."));
    }
    let lengths: Vec<Option<u64>> = req
        .headers
        .iter()
        .filter(|h| header_is(h.name, "content-length"))
        .map(|h| value_str(h.value).filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).and_then(|v| v.parse().ok()))
        .collect();
    let body_len = match lengths.as_slice() {
        [] => 0,
        [Some(n), rest @ ..] if rest.iter().all(|m| *m == Some(*n)) => *n,
        _ => return Err(refusal(400, "Bad Request", "Unreadable Content-Length.")),
    };
    if body_len > MAX_BODY {
        return Err(refusal(413, "Content Too Large", "The request body is too large."));
    }

    let listed = connection_tokens(req.headers);
    let mut out = Vec::with_capacity(head.len() + 32);
    out.extend_from_slice(format!("{method} {path} HTTP/1.{version}\r\n").as_bytes());
    for h in req.headers.iter().filter(|h| passes_on(h.name, &listed)) {
        out.extend_from_slice(h.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(h.value);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    Ok(Forward { head: out, body_len })
}

/// The dashboard's response head with `Connection: close` in place of its connection headers.
/// Returns the head and its status code; None when it can't be read.
pub fn rewrite_response(head: &[u8]) -> Option<(Vec<u8>, u16)> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut resp = httparse::Response::new(&mut headers);
    if !matches!(resp.parse(head), Ok(httparse::Status::Complete(_))) {
        return None;
    }
    let (code, version) = (resp.code?, resp.version?);
    let listed = connection_tokens(resp.headers);
    let mut out = Vec::with_capacity(head.len() + 32);
    out.extend_from_slice(format!("HTTP/1.{version} {code} {}\r\n", resp.reason.unwrap_or("")).as_bytes());
    for h in resp.headers.iter() {
        let hop = ["connection", "keep-alive", "proxy-connection"].iter().any(|n| header_is(h.name, n)) || listed.iter().any(|t| header_is(h.name, t));
        if !hop {
            out.extend_from_slice(h.name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(h.value);
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    Some((out, code))
}

fn refusal_bytes(r: &Refusal) -> Vec<u8> {
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        r.status,
        r.reason,
        r.text.len(),
        r.text
    )
    .into_bytes()
}

/// Logs a refused request at most once a minute (a page can send many).
fn log_refusal(r: &Refusal) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
        *last = Some(Instant::now());
        log::info!("agents proxy: refused a request ({} {}: {})", r.status, r.reason, r.text);
    }
}

enum Head {
    /// The head (with its empty line) and the bytes read after it.
    Complete(Vec<u8>, Vec<u8>),
    /// The other side closed before a whole head.
    Closed,
    TooLarge,
}

/// Reads until the empty line that ends a head. `buf` holds bytes read before.
async fn read_head<R: AsyncRead + Unpin>(stream: &mut R, mut buf: Vec<u8>) -> std::io::Result<Head> {
    let mut searched = 0;
    loop {
        if let Some(i) = buf[searched..].windows(4).position(|w| w == b"\r\n\r\n") {
            let end = searched + i + 4;
            let rest = buf.split_off(end);
            return Ok(Head::Complete(buf, rest));
        }
        if buf.len() > MAX_HEAD {
            return Ok(Head::TooLarge);
        }
        searched = buf.len().saturating_sub(3);
        let mut chunk = [0u8; 8192];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(Head::Closed);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn refuse(client: &mut TcpStream, r: Refusal) -> std::io::Result<()> {
    log_refusal(&r);
    client.write_all(&refusal_bytes(&r)).await?;
    linger(client).await;
    Ok(())
}

/// Ends our side, then reads what the client still sends (a body that was refused, a second
/// request on the connection) for a moment before the socket closes: closing with unread bytes
/// would reset the connection, and the client could lose the answer it was sent.
async fn linger(client: &mut TcpStream) {
    let _ = client.shutdown().await;
    let mut sink = [0u8; 8192];
    let mut total = 0usize;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while total < MAX_BODY as usize {
            match client.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n,
            }
        }
    })
    .await;
}

async fn handle(mut client: TcpStream, port: u16, upstream: u16) -> std::io::Result<()> {
    let _ = client.set_nodelay(true);
    let (head, rest) = match tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client, Vec::new())).await {
        Ok(Ok(Head::Complete(h, r))) => (h, r),
        Ok(Ok(Head::TooLarge)) => return refuse(&mut client, refusal(431, "Request Header Fields Too Large", "The request head is too large.")).await,
        Ok(Ok(Head::Closed)) | Ok(Err(_)) => return Ok(()),
        Err(_) => return refuse(&mut client, refusal(408, "Request Timeout", "The request took too long.")).await,
    };
    let fwd = match check_request(&head, port) {
        Ok(f) => f,
        Err(r) => return refuse(&mut client, r).await,
    };
    let mut up = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((Ipv4Addr::LOCALHOST, upstream))).await {
        Ok(Ok(s)) => s,
        _ => return refuse(&mut client, refusal(502, "Bad Gateway", "The dashboard is not running.")).await,
    };
    let _ = up.set_nodelay(true);
    up.write_all(&fwd.head).await?;
    // The body: what came with the head, then the rest as it arrives. Bytes after the body (a
    // second request on the same connection) are never passed on.
    let early = rest.len().min(usize::try_from(fwd.body_len).unwrap_or(usize::MAX));
    up.write_all(&rest[..early]).await?;
    let remaining = fwd.body_len - early as u64;
    if remaining > 0 {
        let copied = tokio::io::copy(&mut (&mut client).take(remaining), &mut up).await?;
        if copied < remaining {
            return Ok(()); // the client went away mid-body
        }
    }

    // The response head (after any interim 1xx answers), then its body as it arrives.
    let mut pending = Vec::new();
    let (head, after) = loop {
        let (h, r) = match read_head(&mut up, pending).await? {
            Head::Complete(h, r) => (h, r),
            Head::Closed | Head::TooLarge => return refuse(&mut client, refusal(502, "Bad Gateway", "The dashboard sent no readable answer.")).await,
        };
        let Some((rewritten, code)) = rewrite_response(&h) else {
            return refuse(&mut client, refusal(502, "Bad Gateway", "The dashboard sent no readable answer.")).await;
        };
        if (100..200).contains(&code) && code != 101 {
            client.write_all(&h).await?;
            pending = r;
            continue;
        }
        if code == 101 {
            // No protocol switch: the request went up without Upgrade.
            return refuse(&mut client, refusal(502, "Bad Gateway", "The dashboard asked to switch protocols.")).await;
        }
        break (rewritten, r);
    };
    client.write_all(&head).await?;
    client.write_all(&after).await?;
    tokio::io::copy(&mut up, &mut client).await?;
    linger(&mut client).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream as StdStream;
    use std::sync::mpsc;

    fn head(s: &str) -> Vec<u8> {
        s.replace('\n', "\r\n").into_bytes()
    }

    #[test]
    fn host_and_origin_rules() {
        let ok = |s: &str| check_request(&head(s), 4000);
        let f = ok("GET /days.json HTTP/1.1\nHost: 127.0.0.1:4000\nAccept: */*\nConnection: keep-alive\nKeep-Alive: timeout=5\n\n").unwrap();
        assert_eq!(String::from_utf8(f.head).unwrap(), "GET /days.json HTTP/1.1\r\nHost: 127.0.0.1:4000\r\nAccept: */*\r\nConnection: close\r\n\r\n");
        assert_eq!(f.body_len, 0);
        assert!(ok("GET / HTTP/1.1\nHost: LOCALHOST:4000\n\n").is_ok(), "host names are case-blind");
        // DNS rebinding: the page's own host name.
        assert_eq!(ok("GET /days.json HTTP/1.1\nHost: rebind.example:4000\n\n").unwrap_err().status, 403);
        assert_eq!(ok("GET / HTTP/1.1\nHost: 127.0.0.1:4001\n\n").unwrap_err().status, 403, "another port");
        assert_eq!(ok("GET / HTTP/1.1\nHost: 127.0.0.1\n\n").unwrap_err().status, 403, "no port");
        assert_eq!(ok("GET / HTTP/1.1\nHost: 127.0.0.2:4000\n\n").unwrap_err().status, 403);
        assert_eq!(ok("GET / HTTP/1.1\n\n").unwrap_err().status, 400, "no Host");
        assert_eq!(ok("GET / HTTP/1.0\n\n").unwrap_err().status, 400, "HTTP/1.0 without Host");
        assert_eq!(ok("GET / HTTP/1.1\nHost: 127.0.0.1:4000\nHost: evil.example:4000\n\n").unwrap_err().status, 400);
        assert_eq!(ok("GET http://evil.example/ HTTP/1.1\nHost: 127.0.0.1:4000\n\n").unwrap_err().status, 400, "absolute form");
        // Cross-site POSTs carry their Origin.
        let csrf = ok("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:4000\nOrigin: https://evil.example\nContent-Length: 0\n\n").unwrap_err();
        assert_eq!((csrf.status, csrf.text.as_str()), (403, "Requests from other sites are refused."));
        assert_eq!(ok("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:4000\nOrigin: null\n\n").unwrap_err().status, 403);
        assert_eq!(ok("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:4000\nOrigin: http://localhost:4000\n\n").unwrap_err().status, 403, "the other name is another origin");
        assert_eq!(ok("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:4000\nOrigin: http://127.0.0.1:4000\nOrigin: http://evil.example\n\n").unwrap_err().status, 403);
        let same = ok("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:4000\nOrigin: http://127.0.0.1:4000\nContent-Length: 2\n\n").unwrap();
        assert_eq!(same.body_len, 2);
        // Bodies: Content-Length only, bounded.
        assert_eq!(ok("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:4000\nTransfer-Encoding: chunked\n\n").unwrap_err().status, 411);
        assert_eq!(ok("POST / HTTP/1.1\nHost: 127.0.0.1:4000\nContent-Length: 5\nContent-Length: 6\n\n").unwrap_err().status, 400);
        assert_eq!(ok("POST / HTTP/1.1\nHost: 127.0.0.1:4000\nContent-Length: -1\n\n").unwrap_err().status, 400);
        assert_eq!(ok("POST / HTTP/1.1\nHost: 127.0.0.1:4000\nContent-Length: 99999999\n\n").unwrap_err().status, 413);
        assert_eq!(ok("POST / HTTP/1.1\nHost: 127.0.0.1:4000\nContent-Length: 3\nContent-Length: 3\n\n").unwrap().body_len, 3);
        assert_eq!(ok("nonsense\n\n").unwrap_err().status, 400);
    }

    #[test]
    fn connection_headers_are_replaced() {
        let f = check_request(&head("GET / HTTP/1.1\nHost: 127.0.0.1:9\nConnection: Upgrade, X-Secret\nUpgrade: websocket\nX-Secret: 1\nExpect: 100-continue\nTE: trailers\nRange: bytes=0-1\n\n"), 9).unwrap();
        assert_eq!(String::from_utf8(f.head).unwrap(), "GET / HTTP/1.1\r\nHost: 127.0.0.1:9\r\nRange: bytes=0-1\r\nConnection: close\r\n\r\n");
        let (r, code) = rewrite_response(&head("HTTP/1.1 200 OK\nContent-Type: application/json\nConnection: keep-alive\nKeep-Alive: timeout=5\nTransfer-Encoding: chunked\n\n")).unwrap();
        assert_eq!(code, 200);
        assert_eq!(String::from_utf8(r).unwrap(), "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
        assert!(rewrite_response(b"garbage\r\n\r\n").is_none());
    }

    // ---- the proxy with a fake dashboard ----

    /// A fake dashboard: for each connection, reads one request head (and `body` bytes) and sends
    /// it to the test, then writes `reply`. Returns its port and the requests it saw.
    fn fake_upstream(reply: &'static [u8]) -> (u16, mpsc::Receiver<String>) {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { return };
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut seen = String::new();
                    let mut len = 0usize;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                        seen.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                    }
                    let mut body = vec![0u8; len];
                    let _ = r.read_exact(&mut body);
                    seen.push_str(&String::from_utf8_lossy(&body));
                    let _ = tx.send(seen);
                    let _ = s.write_all(reply);
                });
            }
        });
        (port, rx)
    }

    fn gate_for(upstream: u16) -> Gate {
        Gate::start(std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap(), upstream).unwrap()
    }

    fn exchange(port: u16, request: &str) -> String {
        let mut s = StdStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(request.replace('\n', "\r\n").as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    const OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: keep-alive\r\nKeep-Alive: timeout=5\r\n\r\n{\"days\":[]}";

    #[test]
    fn passes_checked_requests_and_refuses_the_rest() {
        let (up, seen) = fake_upstream(OK);
        let g = gate_for(up);
        let p = g.port();
        let out = exchange(p, &format!("GET /days.json HTTP/1.1\nHost: 127.0.0.1:{p}\nConnection: keep-alive\n\n"));
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out}");
        assert!(out.contains("\r\nConnection: close\r\n") && !out.contains("keep-alive") && !out.contains("Keep-Alive"), "{out}");
        assert!(out.ends_with("{\"days\":[]}"));
        let req = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(req, format!("GET /days.json HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nConnection: close\r\n\r\n"));

        // A rebinding page's read and a cross-site POST never reach the dashboard.
        let out = exchange(p, &format!("GET /days.json HTTP/1.1\nHost: rebind.example:{p}\n\n"));
        assert!(out.starts_with("HTTP/1.1 403 Forbidden\r\n"), "{out}");
        let out = exchange(p, &format!("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:{p}\nOrigin: https://evil.example\nContent-Length: 0\n\n"));
        assert!(out.starts_with("HTTP/1.1 403 Forbidden\r\n") && out.ends_with("Requests from other sites are refused."), "{out}");
        assert!(seen.recv_timeout(Duration::from_millis(300)).is_err(), "refused requests went up");

        // The dashboard's own POST, with its body.
        let out = exchange(p, &format!("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:{p}\nOrigin: http://127.0.0.1:{p}\nContent-Length: 2\n\n{{}}"));
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out}");
        let req = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(req.starts_with("POST /api/scan HTTP/1.1\r\n") && req.ends_with("Connection: close\r\n\r\n{}"), "{req}");
    }

    #[test]
    fn one_request_per_connection() {
        let (up, seen) = fake_upstream(OK);
        let g = gate_for(up);
        let p = g.port();
        // Two requests on one connection: the second (a foreign host) is never passed on, and
        // the client gets one answer and the end of the connection.
        let out = exchange(p, &format!("GET /days.json HTTP/1.1\nHost: 127.0.0.1:{p}\n\nGET /days.json HTTP/1.1\nHost: rebind.example:{p}\n\n"));
        assert_eq!(out.matches("HTTP/1.1 ").count(), 1, "{out}");
        assert!(seen.recv_timeout(Duration::from_secs(5)).unwrap().contains(&format!("Host: 127.0.0.1:{p}")));
        assert!(seen.recv_timeout(Duration::from_millis(300)).is_err(), "the second request went up");
        // A body longer than its Content-Length: the extra bytes stay behind.
        let out = exchange(p, &format!("POST /api/scan HTTP/1.1\nHost: 127.0.0.1:{p}\nContent-Length: 2\n\n{{}}GET /x HTTP/1.1\nHost: evil\n\n"));
        assert_eq!(out.matches("HTTP/1.1 ").count(), 1, "{out}");
        let req = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(req.ends_with("\r\n\r\n{}") && !req.contains("evil"), "{req}");
    }

    #[test]
    fn streams_without_buffering() {
        // A dashboard that sends its head and a first event, then waits before the second.
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let up = l.local_addr().unwrap().port();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n9\r\ndata: 1\n\n\r\n").unwrap();
            s.flush().unwrap();
            go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            s.write_all(b"9\r\ndata: 2\n\n\r\n0\r\n\r\n").unwrap();
        });
        let g = gate_for(up);
        let p = g.port();
        let mut s = StdStream::connect(("127.0.0.1", p)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(format!("GET /events HTTP/1.1\r\nHost: localhost:{p}\r\nAccept: text/event-stream\r\n\r\n").as_bytes()).unwrap();
        let mut got = Vec::new();
        let mut chunk = [0u8; 1024];
        while !String::from_utf8_lossy(&got).contains("data: 1") {
            let n = s.read(&mut chunk).unwrap();
            assert!(n > 0, "closed before the first event");
            got.extend_from_slice(&chunk[..n]);
        }
        assert!(!String::from_utf8_lossy(&got).contains("data: 2"));
        go_tx.send(()).unwrap();
        s.read_to_end(&mut got).unwrap();
        let text = String::from_utf8_lossy(&got);
        assert!(text.contains("Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n") && text.ends_with("data: 2\n\n\r\n0\r\n\r\n"), "{text}");
    }

    #[test]
    fn dashboard_down_and_gate_dropped() {
        // Nothing listens upstream.
        let up = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
        let g = gate_for(up);
        let p = g.port();
        let out = exchange(p, &format!("GET / HTTP/1.1\nHost: 127.0.0.1:{p}\n\n"));
        assert!(out.starts_with("HTTP/1.1 502 Bad Gateway\r\n"), "{out}");
        // A head that never ends is cut off.
        let mut big = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\n");
        big.push_str(&"X-Pad: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n".repeat(600));
        let out = exchange(p, &big);
        assert!(out.starts_with("HTTP/1.1 431 "), "{out}");
        drop(g);
        let deadline = Instant::now() + Duration::from_secs(5);
        while StdStream::connect(("127.0.0.1", p)).is_ok() {
            assert!(Instant::now() < deadline, "still listening after the gate was dropped");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
