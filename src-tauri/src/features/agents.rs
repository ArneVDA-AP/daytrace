//! Agents section: Dayflow's Agents tab (Views/UI/Agents/AgentPlaybackView.swift, MIT, (c) 2025
//! Jerry Liu) for Windows. When the user selects Start, Daytrace runs the `agentplayback` npm
//! package (MIT, github.com/JerryZLiu/AgentPlayback) through npx and reports the local dashboard
//! URL it prints, so the section can embed it. The dashboard shows the day's Claude Code and Codex
//! sessions on a 24-hour dial; it reads their session logs and never calls a model.
//!
//! Differences from Dayflow:
//! - The version is pinned (`PINNED_VERSION`) instead of `latest`. A published npm version cannot
//!   be changed, so the pin fixes the code that runs with the user's rights on the PC that holds
//!   the screenshots. A Daytrace release moves the pin after reading the new tarball; there is no
//!   in-app update button.
//! - npx and node are found the way the CLI providers are (`llm/cli/discover.rs`): PATH first,
//!   then the folders the Node.js installers use. Windows has no login shell to ask.
//! - The process goes into the CLI job object (`llm/cli/process.rs`), so the npx tree ends with
//!   Daytrace however Daytrace ends; Stop and Quit also kill the tree right away.
//! - It only starts from the Start button, never by itself.
//! - Dayflow's wrapper script (lease file, host PID polling, fallback to the last working
//!   version) is not needed: the job object ties the tree to Daytrace and the version is fixed.
//!
//! As in Dayflow, AGENTPLAYBACK_HOME points into Daytrace's data folder (`<data dir>\agents`), so
//! the scan results and the cached price list live next to Daytrace's own data.

use crate::llm::cli::discover::{RealEnv, SysEnv};
use crate::llm::cli::process::{ProcessRunner, ProcessSpec, SystemRunner};
use crate::state::AppState;
use crate::timeutil::now_ts;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// The npm package and the exact version Daytrace runs (read before pinning: no runtime
/// dependencies, Node >= 20, listens on 127.0.0.1 only, one outgoing request for a price list).
pub const PACKAGE: &str = "agentplayback";
pub const PINNED_VERSION: &str = "1.2.1";
/// `engines.node` of the pinned version.
pub const MIN_NODE_MAJOR: u32 = 20;
/// Dayflow's start timeout; the first start downloads about 11 MB.
pub const START_TIMEOUT: Duration = Duration::from_secs(120);
/// Lines of output kept for the error details.
const LOG_LINES: usize = 20;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ---- status ----

/// Where node and npx were found.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    /// As `node --version` prints it ("v22.11.0").
    pub version: String,
    pub node_path: String,
    pub npx_path: String,
}

/// The dashboard's state. `NotInstalled` is reported when no run is active and Node.js cannot be
/// used.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RunState {
    NotInstalled { reason: String },
    Stopped,
    Starting { since: i64 },
    Running { url: String, since: i64 },
    Failed { title: String, message: String, log: Vec<String> },
}

/// A folder of session logs the dashboard reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// "Claude Code" or "Codex".
    pub agent: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsStatus {
    #[serde(flatten)]
    pub run: RunState,
    pub node: Option<NodeInfo>,
    /// The pinned AgentPlayback version.
    pub version: String,
    /// AGENTPLAYBACK_HOME: scan results, scan cache and the price list.
    pub data_dir: String,
    pub sources: Vec<Source>,
}

// ---- finding node and npx ----

fn join(dir: &str, name: &str, windows: bool) -> String {
    let sep = if windows { '\\' } else { '/' };
    let dir = dir.trim().trim_matches('"').trim_end_matches(['\\', '/']);
    format!("{dir}{sep}{name}")
}

/// Folders that may hold node and npx, in order: every PATH folder, then where nvm-windows,
/// the Node.js installer, Volta, Scoop and a global `npm install -g npm` put them.
pub fn node_dirs(env: &dyn SysEnv, windows: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(path) = env.var("PATH").or_else(|| env.var("Path")) {
        let sep = if windows { ';' } else { ':' };
        out.extend(path.split(sep).map(|d| d.trim().trim_matches('"').trim_end_matches(['\\', '/']).to_string()).filter(|d| !d.is_empty()));
    }
    if windows {
        if let Some(d) = env.var("NVM_SYMLINK") {
            out.push(d);
        }
        for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(d) = env.var(var) {
                out.push(join(&d, "nodejs", true));
            }
        }
        if let Some(local) = env.var("LOCALAPPDATA") {
            out.push(join(&local, "Volta\\bin", true));
        }
        if let Some(home) = env.var("USERPROFILE") {
            out.push(join(&home, "scoop\\apps\\nodejs-lts\\current", true));
            out.push(join(&home, "scoop\\apps\\nodejs\\current", true));
        }
        if let Some(roaming) = env.var("APPDATA") {
            out.push(join(&roaming, "npm", true));
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(if windows { d.to_ascii_lowercase() } else { d.clone() }));
    out
}

/// The folder of a file path, by the separators of the target platform (`Path::parent` follows the
/// build platform, so tests of Windows paths would differ on Linux).
fn parent_dir(path: &str, windows: bool) -> Option<&str> {
    let seps: &[char] = if windows { &['\\', '/'] } else { &['/'] };
    path.rfind(seps).map(|i| &path[..i]).filter(|d| !d.is_empty())
}

fn first_file(dirs: &[String], names: &[&str], env: &dyn SysEnv, windows: bool) -> Option<PathBuf> {
    dirs.iter().flat_map(|d| names.iter().map(move |n| join(d, n, windows))).find(|p| env.is_file(p)).map(PathBuf::from)
}

/// The npx to run: `npx.cmd` (or a Volta `npx.exe`) on Windows, `npx` elsewhere.
pub fn find_npx(env: &dyn SysEnv, windows: bool) -> Option<PathBuf> {
    let names: &[&str] = if windows { &["npx.cmd", "npx.exe"] } else { &["npx"] };
    first_file(&node_dirs(env, windows), names, env, windows)
}

/// The node that npx will use: the one next to npx first (that is what `npx.cmd` runs), then the
/// usual folders.
pub fn find_node(npx: &Path, env: &dyn SysEnv, windows: bool) -> Option<PathBuf> {
    let name = if windows { "node.exe" } else { "node" };
    let mut dirs = Vec::new();
    if let Some(parent) = parent_dir(&npx.to_string_lossy(), windows) {
        dirs.push(parent.to_string());
    }
    dirs.extend(node_dirs(env, windows));
    first_file(&dirs, &[name], env, windows)
}

/// (major, minor, patch) from `node --version` output ("v22.11.0").
pub fn parse_node_version(out: &str) -> Option<(u32, u32, u32)> {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    let v = line.strip_prefix('v').unwrap_or(line);
    let mut parts = v.split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

pub const NODE_MISSING: &str = "Node.js was not found. AgentPlayback runs on Node.js 20 or later with npm.";

/// Finds npx and node and checks the node version.
pub async fn detect_node(runner: &dyn ProcessRunner, env: &dyn SysEnv, windows: bool) -> Result<NodeInfo, String> {
    let npx = find_npx(env, windows).ok_or_else(|| NODE_MISSING.to_string())?;
    let node = find_node(&npx, env, windows).ok_or_else(|| NODE_MISSING.to_string())?;
    let out = runner
        .run(ProcessSpec { program: node.clone(), args: vec!["--version".into()], timeout: Duration::from_secs(15), ..Default::default() })
        .await
        .map_err(|e| format!("Node.js was found at {} but did not start: {e:#}", node.display()))?;
    let text = crate::llm::cli::output::strip_ansi(&out.stdout);
    let Some((major, ..)) = parse_node_version(&text).filter(|_| out.code == Some(0)) else {
        return Err(format!("Node.js was found at {} but did not report its version.", node.display()));
    };
    let version = text.trim().lines().next().unwrap_or("").trim().to_string();
    if major < MIN_NODE_MAJOR {
        return Err(format!("Node.js {version} is installed, but AgentPlayback needs Node.js {MIN_NODE_MAJOR} or later."));
    }
    Ok(NodeInfo { version, node_path: node.display().to_string(), npx_path: npx.display().to_string() })
}

// ---- what it reads ----

fn home(env: &dyn SysEnv, windows: bool) -> Option<String> {
    if windows { env.var("USERPROFILE").or_else(|| env.var("HOME")) } else { env.var("HOME") }
}

fn expand_home(p: &str, home: Option<&str>, windows: bool) -> String {
    match home {
        Some(h) if p == "~" => h.to_string(),
        Some(h) if p.starts_with("~/") || p.starts_with("~\\") => join(h, &p[2..], windows),
        _ => p.to_string(),
    }
}

/// The session-log folders the pinned version reads (its `scripts/lib/roots.mjs`, which follows
/// ccusage): CLAUDE_CONFIG_DIR (comma-separated config or `projects` folders), else
/// `<XDG_CONFIG_HOME or ~/.config>/claude/projects` when it exists and `~/.claude/projects`;
/// CODEX_HOME (comma-separated), else `~/.codex`.
pub fn sources(env: &dyn SysEnv, windows: bool) -> Vec<Source> {
    let home = home(env, windows);
    let list = |name: &str| -> Vec<String> {
        env.var(name)
            .map(|v| v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| expand_home(s, home.as_deref(), windows)).collect())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    let mut add = |agent: &str, path: String, always: bool| {
        let exists = env.is_dir(&path);
        if (exists || always) && !out.iter().any(|s: &Source| s.path == path) {
            out.push(Source { agent: agent.into(), path, exists });
        }
    };
    let claude = list("CLAUDE_CONFIG_DIR");
    if !claude.is_empty() {
        for p in claude {
            let trimmed = p.trim_end_matches(['\\', '/']);
            let is_projects = trimmed.rsplit(['\\', '/']).next() == Some("projects");
            add("Claude Code", if is_projects { trimmed.to_string() } else { join(trimmed, "projects", windows) }, true);
        }
    } else if let Some(h) = &home {
        let xdg = env.var("XDG_CONFIG_HOME").map(|x| expand_home(&x, Some(h), windows)).unwrap_or_else(|| join(h, ".config", windows));
        add("Claude Code", join(&join(&xdg, "claude", windows), "projects", windows), false);
        add("Claude Code", join(&join(h, ".claude", windows), "projects", windows), true);
    }
    let codex = list("CODEX_HOME");
    if !codex.is_empty() {
        for p in codex {
            add("Codex", p, true);
        }
    } else if let Some(h) = &home {
        add("Codex", join(h, ".codex", windows), true);
    }
    out
}

// ---- the command line ----

/// One process to start and keep running.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

/// `npx --yes --prefer-offline agentplayback@<pin> --no-open --port <port>`, run in the data
/// folder. npx passes everything after the package to it. `--prefer-offline` lets later starts use
/// npm's cache without asking the registry (the version cannot change); node's folder goes first
/// on PATH so npx and the scripts it starts find the same node even when Daytrace was started
/// before Node.js was installed.
///
/// The GitHub CLI sees an empty config folder and no token: the dashboard's server answers
/// `/api/github/status` and `/api/github/star` by running `gh` with the user's GitHub sign-in
/// (a daily "have you starred the repo" request, and a star on a click). Those endpoints take
/// requests from any web page that finds the port, so Daytrace keeps the sign-in out of reach;
/// the dashboard then treats gh as unavailable.
pub fn npx_spec(node: &NodeInfo, port: u16, data_dir: &Path, path_var: Option<&str>, windows: bool) -> LaunchSpec {
    let node_dir = parent_dir(&node.node_path, windows).unwrap_or_default().to_string();
    let sep = if windows { ";" } else { ":" };
    let path = match path_var.filter(|p| !p.trim().is_empty()) {
        Some(p) if !node_dir.is_empty() => format!("{node_dir}{sep}{p}"),
        Some(p) => p.to_string(),
        None => node_dir,
    };
    LaunchSpec {
        program: PathBuf::from(&node.npx_path),
        args: vec![
            "--yes".into(),
            "--prefer-offline".into(),
            format!("{PACKAGE}@{PINNED_VERSION}"),
            "--no-open".into(),
            "--port".into(),
            port.to_string(),
        ],
        env: vec![
            ("AGENTPLAYBACK_HOME".into(), data_dir.display().to_string()),
            ("npm_config_update_notifier".into(), "false".into()),
            ("npm_config_fund".into(), "false".into()),
            ("npm_config_audit".into(), "false".into()),
            ("NO_COLOR".into(), "1".into()),
            ("GH_CONFIG_DIR".into(), data_dir.join(GH_CONFIG_DIR).display().to_string()),
            ("GH_TOKEN".into(), String::new()),
            ("GITHUB_TOKEN".into(), String::new()),
            ("PATH".into(), path),
        ],
        cwd: Some(data_dir.to_path_buf()),
    }
}

/// An empty folder in the data folder that stands in for the GitHub CLI's config (see `npx_spec`).
const GH_CONFIG_DIR: &str = "gh-config";
/// The port of the last start, in the data folder.
const PORT_FILE: &str = "port.txt";

/// A port nobody listens on right now, on the loopback address the dashboard binds to.
pub fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// The port of the last start when it is still free, else a new free one (saved for next time).
/// The dashboard keeps its own settings (skin, project colors, dismissed hints) in the browser
/// storage of its origin, which includes the port; the same port keeps them across starts.
pub fn pick_port(data_dir: &Path) -> std::io::Result<u16> {
    let file = data_dir.join(PORT_FILE);
    let last = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse::<u16>().ok()).filter(|p| *p >= 1024);
    if let Some(port) = last.filter(|p| port_is_free(*p)) {
        return Ok(port);
    }
    let port = free_port()?;
    if let Err(e) = std::fs::write(&file, port.to_string()) {
        log::warn!("agentplayback: could not save the port: {e}");
    }
    Ok(port)
}

/// The port from the line the CLI prints once it listens:
/// `  ◐ agentplayback is running at http://localhost:53124/`.
pub fn parse_ready_port(line: &str) -> Option<u16> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"agentplayback is running at http://(?:localhost|127\.0\.0\.1):(\d{1,5})/").unwrap());
    let port: u32 = re.captures(line)?.get(1)?.as_str().parse().ok()?;
    u16::try_from(port).ok().filter(|p| *p > 0)
}

/// The URL the section embeds. The server listens on 127.0.0.1 only; "localhost" could resolve to
/// ::1 first.
pub fn dashboard_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

/// Title and message for a process that ended by itself. `was_running`: the dashboard had
/// printed its URL.
pub fn exit_message(was_running: bool, code: Option<i32>, log: &[String]) -> (String, String) {
    let has = |needles: &[&str]| log.iter().any(|l| needles.iter().any(|n| l.contains(n)));
    let title = if was_running { "AgentPlayback stopped" } else { "AgentPlayback didn’t start" };
    let message = if code == Some(127) || code == Some(9009) || has(&["is not recognized as an internal or external command", "command not found"]) {
        "npx could not run AgentPlayback. Check that Node.js is installed, then select Re-check.".to_string()
    } else if !was_running && has(&["ENOTFOUND", "EAI_AGAIN", "ETIMEDOUT", "ECONNRESET", "ECONNREFUSED", "network request to"]) {
        "npx couldn’t download AgentPlayback. Check the internet connection and try again.".to_string()
    } else if has(&["EADDRINUSE"]) {
        "Another program took the port Daytrace picked for the dashboard. Try again.".to_string()
    } else {
        let how = match code {
            Some(c) => format!(" (exit code {c})"),
            None => String::new(),
        };
        if was_running { format!("The dashboard process ended unexpectedly{how}.") } else { format!("The process ended before the dashboard was ready{how}.") }
    };
    (title.to_string(), message)
}

pub fn timeout_message() -> (String, String) {
    (
        "AgentPlayback didn’t start".to_string(),
        "It did not start within 2 minutes. The first start downloads it with npx; check the internet connection and try again.".to_string(),
    )
}

// ---- the running process ----

type Lines = Arc<Mutex<VecDeque<String>>>;

struct Run {
    child: Arc<Mutex<Child>>,
    stdout: Lines,
    stderr: Lines,
}

struct Inner {
    /// Increases with every start and stop; threads of an older run leave the state alone.
    generation: u64,
    state: RunState,
    run: Option<Run>,
    node: Option<Result<NodeInfo, String>>,
}

impl Default for Inner {
    fn default() -> Inner {
        Inner { generation: 0, state: RunState::Stopped, run: None, node: None }
    }
}

/// Owns the one AgentPlayback process of this Daytrace session, across section switches.
#[derive(Default)]
pub struct Host {
    inner: Mutex<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn push_line(lines: &Lines, line: String) {
    let mut l = lock(lines);
    if l.len() == LOG_LINES {
        l.pop_front();
    }
    l.push_back(line);
}

/// The error details: the last lines of stderr, or of stdout when stderr is empty.
fn tail(run: &Run) -> Vec<String> {
    let err: Vec<String> = lock(&run.stderr).iter().cloned().collect();
    if err.is_empty() { lock(&run.stdout).iter().cloned().collect() } else { err }
}

fn clean(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    crate::llm::cli::output::strip_ansi(s.trim_end_matches(['\r', '\n'])).trim_end().to_string()
}

fn kill(child: &mut Child) {
    #[cfg(unix)]
    {
        // The process group made at spawn: npx, the shell it starts and the server. Without "--"
        // procps kill reads "-<pgid>" as another option and kills nothing.
        let _ = Command::new("kill").args(["-s", "KILL", "--", &format!("-{}", child.id())]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    crate::llm::cli::process::kill_process_tree(child);
    let _ = child.wait();
}

impl Host {
    /// The state without the node check (see `run_state`).
    pub fn state(&self) -> RunState {
        lock(&self.inner).state.clone()
    }

    /// What the section shows: an active run, else "not installed" when Node.js cannot be used,
    /// else the last result.
    pub fn run_state(&self) -> RunState {
        let inner = lock(&self.inner);
        match (&inner.state, &inner.node) {
            (RunState::Starting { .. } | RunState::Running { .. }, _) => inner.state.clone(),
            (_, Some(Err(reason))) => RunState::NotInstalled { reason: reason.clone() },
            _ => inner.state.clone(),
        }
    }

    pub fn node(&self) -> Option<Result<NodeInfo, String>> {
        lock(&self.inner).node.clone()
    }

    pub fn set_node(&self, node: Result<NodeInfo, String>) {
        lock(&self.inner).node = Some(node);
    }

    /// Starts `spec` unless a run is starting or running. Returns the state afterwards.
    pub fn start_with(self: &Arc<Self>, spec: LaunchSpec, timeout: Duration) -> RunState {
        let mut inner = lock(&self.inner);
        if matches!(inner.state, RunState::Starting { .. } | RunState::Running { .. }) {
            return inner.state.clone();
        }
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        inner.generation += 1;
        let generation = inner.generation;
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("agentplayback: could not start {}: {e}", spec.program.display());
                inner.state = RunState::Failed {
                    title: "AgentPlayback didn’t start".into(),
                    message: format!("Couldn’t start npx ({}): {e}", spec.program.display()),
                    log: Vec::new(),
                };
                return inner.state.clone();
            }
        };
        // Ends with Daytrace, however Daytrace ends.
        crate::llm::cli::process::adopt_into_job(&child);
        log::info!("agentplayback {PINNED_VERSION}: started npx (pid {})", child.id());
        let stdout: Lines = Arc::default();
        let stderr: Lines = Arc::default();
        let readers = [
            child.stdout.take().map(|p| self.spawn_reader(p, stdout.clone(), generation, true)),
            child.stderr.take().map(|p| self.spawn_reader(p, stderr.clone(), generation, false)),
        ];
        let child = Arc::new(Mutex::new(child));
        inner.run = Some(Run { child: child.clone(), stdout, stderr });
        inner.state = RunState::Starting { since: now_ts() };
        drop(inner);

        let host = self.clone();
        let deadline = Instant::now() + timeout;
        let _ = std::thread::Builder::new().name("agentplayback-watch".into()).spawn(move || {
            loop {
                if lock(&host.inner).generation != generation {
                    return;
                }
                let exited = lock(&child).try_wait();
                match exited {
                    Ok(Some(status)) => {
                        // Let the readers take the last lines (a grandchild may hold the pipes).
                        let until = Instant::now() + Duration::from_secs(1);
                        while Instant::now() < until && readers.iter().flatten().any(|h| !h.is_finished()) {
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        host.on_exit(generation, status.code());
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => log::warn!("agentplayback: could not check the process: {e}"),
                }
                if Instant::now() >= deadline && host.on_timeout(generation) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        self.state()
    }

    fn spawn_reader<R: Read + Send + 'static>(self: &Arc<Self>, pipe: R, lines: Lines, generation: u64, watch_url: bool) -> std::thread::JoinHandle<()> {
        let host = self.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = clean(&buf);
                        if watch_url {
                            if let Some(port) = parse_ready_port(&line) {
                                host.on_ready(generation, port);
                            }
                        }
                        if !line.trim().is_empty() {
                            push_line(&lines, line);
                        }
                    }
                }
            }
        })
    }

    fn on_ready(&self, generation: u64, port: u16) {
        let mut inner = lock(&self.inner);
        if inner.generation == generation {
            if let RunState::Starting { since } = inner.state {
                log::info!("agentplayback: dashboard ready on port {port}");
                inner.state = RunState::Running { url: dashboard_url(port), since };
            }
        }
    }

    fn on_exit(&self, generation: u64, code: Option<i32>) {
        let mut inner = lock(&self.inner);
        if inner.generation != generation {
            return;
        }
        let was_running = matches!(inner.state, RunState::Running { .. });
        let log = inner.run.take().map(|r| tail(&r)).unwrap_or_default();
        let (title, message) = exit_message(was_running, code, &log);
        log::warn!("agentplayback ended (code {code:?}): {}", log.last().map(String::as_str).unwrap_or(""));
        inner.state = RunState::Failed { title, message, log };
    }

    /// Kills a run that is still starting at its deadline. False when it is no longer starting.
    fn on_timeout(&self, generation: u64) -> bool {
        let run = {
            let mut inner = lock(&self.inner);
            if inner.generation != generation || !matches!(inner.state, RunState::Starting { .. }) {
                return inner.generation != generation;
            }
            inner.generation += 1;
            let run = inner.run.take();
            let (title, message) = timeout_message();
            let log = run.as_ref().map(tail).unwrap_or_default();
            log::warn!("agentplayback: no dashboard after {} s", START_TIMEOUT.as_secs());
            inner.state = RunState::Failed { title, message, log };
            run
        };
        if let Some(run) = run {
            kill(&mut lock(&run.child));
        }
        true
    }

    /// Stops the dashboard (a no-op when nothing runs). A failed result stays visible until the
    /// next start; a stop after a failure clears it.
    pub fn stop(&self) -> RunState {
        let run = {
            let mut inner = lock(&self.inner);
            inner.generation += 1;
            inner.state = RunState::Stopped;
            inner.run.take()
        };
        if let Some(run) = run {
            log::info!("agentplayback: stopping");
            kill(&mut lock(&run.child));
        }
        self.state()
    }
}

static HOST: OnceLock<Arc<Host>> = OnceLock::new();

fn host() -> &'static Arc<Host> {
    HOST.get_or_init(Arc::default)
}

/// Called when Daytrace exits: ends the dashboard's process tree right away (the job object
/// would end it a moment later anyway).
pub fn shutdown() {
    if let Some(h) = HOST.get() {
        h.stop();
    }
}

// ---- commands ----

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn data_dir(state: &AppState) -> PathBuf {
    state.paths.root.join("agents")
}

async fn refresh_node() -> Result<NodeInfo, String> {
    let node = detect_node(&SystemRunner, &RealEnv, cfg!(windows)).await;
    host().set_node(node.clone());
    node
}

fn status_of(state: &AppState) -> AgentsStatus {
    let h = host();
    AgentsStatus {
        run: h.run_state(),
        node: h.node().and_then(Result::ok),
        version: PINNED_VERSION.to_string(),
        data_dir: data_dir(state).display().to_string(),
        sources: sources(&RealEnv, cfg!(windows)),
    }
}

/// The section's state. Looks for Node.js the first time.
#[tauri::command]
pub async fn agents_status(state: S<'_>) -> CmdResult<AgentsStatus> {
    if host().node().is_none() {
        let _ = refresh_node().await;
    }
    Ok(status_of(&state))
}

/// Looks for Node.js again (after the user installed it).
#[tauri::command]
pub async fn agents_recheck(state: S<'_>) -> CmdResult<AgentsStatus> {
    let _ = refresh_node().await;
    Ok(status_of(&state))
}

/// Starts the dashboard (the user selected Start). Returns at once; the state goes from
/// "starting" to "running" or "failed".
#[tauri::command]
pub async fn agents_start(state: S<'_>) -> CmdResult<AgentsStatus> {
    if matches!(host().state(), RunState::Starting { .. } | RunState::Running { .. }) {
        return Ok(status_of(&state));
    }
    let Ok(node) = refresh_node().await else { return Ok(status_of(&state)) };
    let dir = data_dir(&state);
    std::fs::create_dir_all(dir.join(GH_CONFIG_DIR)).map_err(|e| format!("Couldn’t create {}: {e}", dir.display()))?;
    let port = pick_port(&dir).map_err(|e| format!("Couldn’t find a free local port: {e}"))?;
    let spec = npx_spec(&node, port, &dir, std::env::var("PATH").ok().as_deref(), cfg!(windows));
    let h = host().clone();
    tauri::async_runtime::spawn_blocking(move || h.start_with(spec, START_TIMEOUT)).await.map_err(|e| e.to_string())?;
    Ok(status_of(&state))
}

#[tauri::command]
pub async fn agents_stop(state: S<'_>) -> CmdResult<AgentsStatus> {
    let h = host().clone();
    tauri::async_runtime::spawn_blocking(move || h.stop()).await.map_err(|e| e.to_string())?;
    Ok(status_of(&state))
}

/// Opens the running dashboard in the default browser.
#[tauri::command]
pub fn agents_open_in_browser(app: tauri::AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let RunState::Running { url, .. } = host().state() else {
        return Err("AgentPlayback is not running.".into());
    };
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::cli::discover::fake::FakeEnv;
    use crate::llm::cli::process::FakeRunner;

    const VARS: &[(&str, &str)] = &[
        ("PATH", r#"C:\Windows\system32;"C:\Tools\";C:\Users\arne\AppData\Roaming\npm"#),
        ("USERPROFILE", r"C:\Users\arne"),
        ("LOCALAPPDATA", r"C:\Users\arne\AppData\Local"),
        ("APPDATA", r"C:\Users\arne\AppData\Roaming"),
        ("ProgramFiles", r"C:\Program Files"),
        ("ProgramW6432", r"C:\Program Files"),
    ];

    #[test]
    fn windows_folders_and_npx_order() {
        let env = FakeEnv::new(VARS, &[]);
        assert_eq!(
            node_dirs(&env, true),
            vec![
                r"C:\Windows\system32",
                r"C:\Tools",
                r"C:\Users\arne\AppData\Roaming\npm",
                r"C:\Program Files\nodejs",
                r"C:\Users\arne\AppData\Local\Volta\bin",
                r"C:\Users\arne\scoop\apps\nodejs-lts\current",
                r"C:\Users\arne\scoop\apps\nodejs\current",
            ]
        );
        // Daytrace started before Node.js was installed: PATH misses it, the installer folder has it.
        let installed = FakeEnv::new(VARS, &[r"C:\Program Files\nodejs\npx.cmd", r"C:\Program Files\nodejs\node.exe"]);
        let npx = find_npx(&installed, true).unwrap();
        assert_eq!(npx, PathBuf::from(r"C:\Program Files\nodejs\npx.cmd"));
        assert_eq!(find_node(&npx, &installed, true).unwrap(), PathBuf::from(r"C:\Program Files\nodejs\node.exe"));
        // A global `npm install -g npm` npx on PATH wins; node comes from the installer folder.
        let upgraded = FakeEnv::new(VARS, &[r"C:\Users\arne\AppData\Roaming\npm\npx.cmd", r"C:\Program Files\nodejs\npx.cmd", r"C:\Program Files\nodejs\node.exe"]);
        let npx = find_npx(&upgraded, true).unwrap();
        assert_eq!(npx, PathBuf::from(r"C:\Users\arne\AppData\Roaming\npm\npx.cmd"));
        assert_eq!(find_node(&npx, &upgraded, true).unwrap(), PathBuf::from(r"C:\Program Files\nodejs\node.exe"));
        // A PowerShell or bash shim is never picked on Windows.
        let shims = FakeEnv::new(VARS, &[r"C:\Tools\npx.ps1", r"C:\Tools\npx"]);
        assert!(find_npx(&shims, true).is_none());
        // nvm-windows
        let mut nvm = FakeEnv::new(&[("NVM_SYMLINK", r"C:\nvm4w\nodejs")], &[r"C:\nvm4w\nodejs\npx.cmd", r"C:\nvm4w\nodejs\node.exe"]);
        nvm.vars.insert("PATH".into(), r"C:\Windows".into());
        assert_eq!(find_npx(&nvm, true).unwrap(), PathBuf::from(r"C:\nvm4w\nodejs\npx.cmd"));
    }

    #[test]
    fn node_next_to_npx_comes_first() {
        assert_eq!(parent_dir(r"C:\Program Files\nodejs\npx.cmd", true), Some(r"C:\Program Files\nodejs"));
        assert_eq!(parent_dir("/usr/bin/npx", false), Some("/usr/bin"));
        assert_eq!(parent_dir("npx.cmd", true), None);
        // A portable Node.js in a folder Daytrace does not know, started from PATH.
        let env = FakeEnv::new(&[("PATH", r"D:\node22;C:\Program Files\nodejs"), ("ProgramFiles", r"C:\Program Files")], &[r"D:\node22\npx.cmd", r"D:\node22\node.exe", r"C:\Program Files\nodejs\node.exe"]);
        let npx = find_npx(&env, true).unwrap();
        assert_eq!(find_node(&npx, &env, true).unwrap(), PathBuf::from(r"D:\node22\node.exe"));
    }

    #[test]
    fn node_versions() {
        assert_eq!(parse_node_version("v22.11.0\n"), Some((22, 11, 0)));
        assert_eq!(parse_node_version("v20.0.0-nightly2023"), Some((20, 0, 0)));
        assert_eq!(parse_node_version("\nv18.19.1\r\n"), Some((18, 19, 1)));
        assert_eq!(parse_node_version("node: not found"), None);
        assert_eq!(parse_node_version(""), None);
    }

    fn node_env() -> FakeEnv {
        FakeEnv::new(VARS, &[r"C:\Program Files\nodejs\npx.cmd", r"C:\Program Files\nodejs\node.exe"])
    }

    #[tokio::test]
    async fn detects_node_and_rejects_old_versions() {
        let runner = FakeRunner::new(vec![FakeRunner::ok("v22.11.0\n"), FakeRunner::ok("v18.19.1\n"), FakeRunner::exit(1, "", "boom")]);
        let ok = detect_node(&runner, &node_env(), true).await.unwrap();
        assert_eq!(
            ok,
            NodeInfo { version: "v22.11.0".into(), node_path: r"C:\Program Files\nodejs\node.exe".into(), npx_path: r"C:\Program Files\nodejs\npx.cmd".into() }
        );
        assert_eq!(runner.calls()[0].args, vec!["--version".to_string()]);
        let old = detect_node(&runner, &node_env(), true).await.unwrap_err();
        assert_eq!(old, "Node.js v18.19.1 is installed, but AgentPlayback needs Node.js 20 or later.");
        let broken = detect_node(&runner, &node_env(), true).await.unwrap_err();
        assert!(broken.contains("did not report its version"), "{broken}");
        let missing = detect_node(&runner, &FakeEnv::new(VARS, &[]), true).await.unwrap_err();
        assert_eq!(missing, NODE_MISSING);
        assert_eq!(runner.calls().len(), 3);
    }

    #[test]
    fn command_line_is_pinned_and_quiet() {
        let node = NodeInfo { version: "v22.11.0".into(), node_path: r"C:\Program Files\nodejs\node.exe".into(), npx_path: r"C:\Program Files\nodejs\npx.cmd".into() };
        let dir = PathBuf::from(r"C:\Users\arne\AppData\Roaming\dev.daytrace.app\agents");
        let spec = npx_spec(&node, 53124, &dir, Some(r"C:\Windows\system32"), true);
        assert_eq!(spec.program, PathBuf::from(r"C:\Program Files\nodejs\npx.cmd"));
        assert_eq!(spec.args, ["--yes", "--prefer-offline", "agentplayback@1.2.1", "--no-open", "--port", "53124"]);
        assert_eq!(spec.cwd.as_deref(), Some(dir.as_path()));
        let env: std::collections::HashMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(env["AGENTPLAYBACK_HOME"], dir.display().to_string());
        assert_eq!(env["npm_config_update_notifier"], "false");
        assert_eq!(env["PATH"], r"C:\Program Files\nodejs;C:\Windows\system32");
        // The GitHub CLI's sign-in stays out of the dashboard's reach.
        assert_eq!(env["GH_CONFIG_DIR"], dir.join("gh-config").display().to_string());
        assert_eq!(env["GH_TOKEN"], "");
        assert_eq!(env["GITHUB_TOKEN"], "");
        // No version range or tag ever reaches npx.
        assert!(!spec.args.iter().any(|a| a.contains("latest") || a.contains('^') || a.contains('~')));
    }

    #[test]
    fn keeps_the_port_while_it_is_free() {
        let tmp = tempfile::tempdir().unwrap();
        let first = pick_port(tmp.path()).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.path().join(PORT_FILE)).unwrap(), first.to_string());
        assert_eq!(pick_port(tmp.path()).unwrap(), first);
        // Taken by someone else: a new port, saved for next time.
        let _taken = std::net::TcpListener::bind(("127.0.0.1", first)).unwrap();
        let second = pick_port(tmp.path()).unwrap();
        assert_ne!(second, first);
        assert_eq!(std::fs::read_to_string(tmp.path().join(PORT_FILE)).unwrap(), second.to_string());
        // A damaged file is ignored.
        std::fs::write(tmp.path().join(PORT_FILE), "eighty").unwrap();
        assert!(pick_port(tmp.path()).unwrap() >= 1024);
    }

    #[test]
    fn ready_line_and_url() {
        let line = "  \u{25d0} agentplayback is running at http://localhost:53124/";
        assert_eq!(parse_ready_port(line), Some(53124));
        assert_eq!(parse_ready_port(&clean(b"\x1b[32m  agentplayback is running at http://localhost:8080/\x1b[0m\r\n")), Some(8080));
        assert_eq!(parse_ready_port("agentplayback is running at http://127.0.0.1:1420/"), Some(1420));
        assert_eq!(parse_ready_port("agentplayback is running at http://localhost:0/"), None);
        assert_eq!(parse_ready_port("agentplayback is running at http://localhost:99999/"), None);
        assert_eq!(parse_ready_port("agentplayback is running at http://evil.example:80/"), None);
        assert_eq!(parse_ready_port("press ctrl+c to stop"), None);
        assert_eq!(dashboard_url(53124), "http://127.0.0.1:53124/");
        assert!(free_port().unwrap() > 0);
    }

    #[test]
    fn exit_messages() {
        let lines = |s: &[&str]| s.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let (t, m) = exit_message(false, Some(1), &lines(&["npm error code ENOTFOUND", "npm error network request to https://registry.npmjs.org/agentplayback failed"]));
        assert_eq!(t, "AgentPlayback didn’t start");
        assert!(m.contains("couldn’t download"), "{m}");
        let (_, m) = exit_message(false, Some(9009), &[]);
        assert!(m.contains("Re-check"), "{m}");
        let (_, m) = exit_message(false, Some(1), &lines(&["Error: listen EADDRINUSE: address already in use 127.0.0.1:53124"]));
        assert!(m.contains("port"), "{m}");
        let (t, m) = exit_message(true, Some(1), &lines(&["TypeError: x is undefined"]));
        assert_eq!((t.as_str(), m.as_str()), ("AgentPlayback stopped", "The dashboard process ended unexpectedly (exit code 1)."));
        let (_, m) = exit_message(false, None, &[]);
        assert_eq!(m, "The process ended before the dashboard was ready.");
    }

    #[test]
    fn session_log_folders() {
        let env = FakeEnv::new(&[("USERPROFILE", r"C:\Users\arne")], &[]);
        assert_eq!(
            sources(&env, true),
            vec![
                Source { agent: "Claude Code".into(), path: r"C:\Users\arne\.claude\projects".into(), exists: false },
                Source { agent: "Codex".into(), path: r"C:\Users\arne\.codex".into(), exists: false },
            ]
        );
        let mut env = FakeEnv::new(&[("USERPROFILE", r"C:\Users\arne"), ("CLAUDE_CONFIG_DIR", r"D:\cc, ~\work\projects"), ("CODEX_HOME", r"D:\codex")], &[]);
        env.dirs.insert(r"D:\cc\projects".into());
        assert_eq!(
            sources(&env, true),
            vec![
                Source { agent: "Claude Code".into(), path: r"D:\cc\projects".into(), exists: true },
                Source { agent: "Claude Code".into(), path: r"C:\Users\arne\work\projects".into(), exists: false },
                Source { agent: "Codex".into(), path: r"D:\codex".into(), exists: false },
            ]
        );
        // The XDG folder counts only when it exists.
        let mut env = FakeEnv::new(&[("HOME", "/home/a")], &[]);
        env.dirs.insert("/home/a/.config/claude/projects".into());
        let s = sources(&env, false);
        assert_eq!(s.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["/home/a/.config/claude/projects", "/home/a/.claude/projects", "/home/a/.codex"]);
    }

    #[test]
    fn not_installed_wins_only_when_idle() {
        let h = Host::default();
        assert_eq!(h.run_state(), RunState::Stopped);
        h.set_node(Err(NODE_MISSING.into()));
        assert_eq!(h.run_state(), RunState::NotInstalled { reason: NODE_MISSING.into() });
        lock(&h.inner).state = RunState::Running { url: dashboard_url(1), since: 0 };
        assert!(matches!(h.run_state(), RunState::Running { .. }));
    }

    // The rest starts /bin/sh in place of npx, never npx or a network call.
    #[cfg(unix)]
    fn sh(script: &str) -> LaunchSpec {
        LaunchSpec { program: "/bin/sh".into(), args: vec!["-c".into(), script.into()], ..Default::default() }
    }

    #[cfg(unix)]
    fn wait_for(h: &Host, pred: impl Fn(&RunState) -> bool) -> RunState {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let s = h.state();
            if pred(&s) || Instant::now() > until {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    fn alive(pid: &str) -> bool {
        // A killed process whose parent is gone can stay a zombie ("Z") until init reaps it.
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            return stat.rsplit(')').next().and_then(|rest| rest.split_whitespace().next()) != Some("Z");
        }
        Command::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }

    #[cfg(unix)]
    #[test]
    fn runs_until_stopped_and_kills_the_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let pidfile = tmp.path().join("grandchild.pid");
        let h = Arc::new(Host::default());
        // Like npx: a shell that starts the server (here `sleep`) as a child and waits for it.
        let script = format!("sleep 60 & echo $! > {}; echo '  agentplayback is running at http://localhost:40404/'; wait", pidfile.display());
        let s = h.start_with(sh(&script), Duration::from_secs(10));
        assert!(matches!(s, RunState::Starting { .. } | RunState::Running { .. }), "{s:?}");
        let s = wait_for(&h, |s| matches!(s, RunState::Running { .. }));
        assert!(matches!(&s, RunState::Running { url, .. } if url == "http://127.0.0.1:40404/"), "{s:?}");
        // A second start while running changes nothing.
        assert_eq!(h.start_with(sh("exit 3"), Duration::from_secs(10)), s);
        let grandchild = std::fs::read_to_string(&pidfile).unwrap().trim().to_string();
        assert!(alive(&grandchild));
        assert_eq!(h.stop(), RunState::Stopped);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!alive(&grandchild), "the server process outlived Stop");
        assert_eq!(h.state(), RunState::Stopped);
    }

    #[cfg(unix)]
    #[test]
    fn reports_the_last_stderr_lines_when_it_fails() {
        let h = Arc::new(Host::default());
        let script = "echo 'scanning today'; for i in $(seq 1 30); do echo \"npm error line $i\" >&2; done; echo 'npm error code ENOTFOUND' >&2; exit 1";
        h.start_with(sh(script), Duration::from_secs(10));
        let s = wait_for(&h, |s| matches!(s, RunState::Failed { .. }));
        let RunState::Failed { title, message, log } = s else { panic!("{s:?}") };
        assert_eq!(title, "AgentPlayback didn’t start");
        assert!(message.contains("couldn’t download"), "{message}");
        assert_eq!(log.len(), LOG_LINES);
        assert_eq!(log.last().unwrap(), "npm error code ENOTFOUND");
        assert_eq!(log[0], "npm error line 12");
    }

    #[cfg(unix)]
    #[test]
    fn a_crash_after_start_is_reported() {
        let h = Arc::new(Host::default());
        h.start_with(sh("echo 'agentplayback is running at http://localhost:40405/'; sleep 0.3; echo 'Error: boom' >&2; exit 7"), Duration::from_secs(10));
        let s = wait_for(&h, |s| matches!(s, RunState::Failed { .. }));
        assert_eq!(
            s,
            RunState::Failed { title: "AgentPlayback stopped".into(), message: "The dashboard process ended unexpectedly (exit code 7).".into(), log: vec!["Error: boom".into()] }
        );
    }

    #[cfg(unix)]
    #[test]
    fn times_out_and_kills_a_silent_process() {
        let h = Arc::new(Host::default());
        let started = Instant::now();
        h.start_with(sh("echo 'scanning today'; sleep 60"), Duration::from_millis(400));
        let s = wait_for(&h, |s| matches!(s, RunState::Failed { .. }));
        assert!(started.elapsed() < Duration::from_secs(5));
        let RunState::Failed { message, log, .. } = s else { panic!("{s:?}") };
        assert!(message.contains("within 2 minutes"), "{message}");
        assert_eq!(log, vec!["scanning today".to_string()]);
        // It is stopped: Stop afterwards is a plain no-op and the state is cleared.
        assert_eq!(h.stop(), RunState::Stopped);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_program_fails_cleanly() {
        let h = Arc::new(Host::default());
        let s = h.start_with(LaunchSpec { program: "/nonexistent/npx".into(), ..Default::default() }, Duration::from_secs(5));
        assert!(matches!(&s, RunState::Failed { message, .. } if message.starts_with("Couldn’t start npx")), "{s:?}");
    }

    #[test]
    fn status_json_shape() {
        let st = AgentsStatus {
            run: RunState::Running { url: dashboard_url(5), since: 10 },
            node: None,
            version: PINNED_VERSION.into(),
            data_dir: "D".into(),
            sources: vec![],
        };
        let v = serde_json::to_value(&st).unwrap();
        assert_eq!(v["state"], "running");
        assert_eq!(v["url"], "http://127.0.0.1:5/");
        assert_eq!(v["dataDir"], "D");
        let v = serde_json::to_value(RunState::NotInstalled { reason: "r".into() }).unwrap();
        assert_eq!(v, serde_json::json!({"state": "notInstalled", "reason": "r"}));
    }
}
