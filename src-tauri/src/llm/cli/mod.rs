//! The Claude Code and Codex CLIs as model providers (Dayflow's ClaudeProvider, CodexProvider,
//! AgentCLISupport, ChatCLIProcessRunner), for Windows.
//!
//! Every request starts the CLI once: the prompt goes to stdin, screenshots are written as JPEG
//! files into a temporary folder that the CLI may read, and the answer is read from the JSON the
//! CLI prints. The CLI signs in with the user's own Claude or ChatGPT account.

pub mod args;
pub mod discover;
pub mod output;
pub mod process;

use crate::llm::client::Part;
use anyhow::{anyhow, bail, Result};
use args::{Profile, Session};
use base64::Engine;
use discover::{RealEnv, SysEnv};
use output::Usage;
use process::{ProcessRunner, ProcessSpec, SpawnFailed, SystemRunner, TimedOut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Dayflow's ChatCLIProcessRunner timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliTool {
    Claude,
    Codex,
}

impl CliTool {
    pub fn from_kind(kind: crate::config::ProviderKind) -> Option<CliTool> {
        match kind {
            crate::config::ProviderKind::ClaudeCli => Some(CliTool::Claude),
            crate::config::ProviderKind::CodexCli => Some(CliTool::Codex),
            _ => None,
        }
    }
    pub fn bin_name(self) -> &'static str {
        match self {
            CliTool::Claude => "claude",
            CliTool::Codex => "codex",
        }
    }
    pub fn display_name(self) -> &'static str {
        match self {
            CliTool::Claude => "Claude Code CLI",
            CliTool::Codex => "Codex CLI",
        }
    }
    pub fn install_hint(self) -> &'static str {
        match self {
            CliTool::Claude => "in PowerShell: irm https://claude.ai/install.ps1 | iex, or npm install -g @anthropic-ai/claude-code",
            CliTool::Codex => "npm install -g @openai/codex",
        }
    }
    /// Models offered in Settings (the CLI also accepts full model names).
    pub fn suggested_models(self) -> Vec<String> {
        match self {
            CliTool::Claude => vec!["haiku".into(), "sonnet".into(), "opus".into()],
            CliTool::Codex => vec!["gpt-6-luna".into(), "gpt-5.6-luna".into()],
        }
    }
}

/// One CLI run.
#[derive(Debug, Clone)]
pub struct CliRequest {
    pub prompt: String,
    pub profile: Profile,
    pub session: Session,
    pub system: String,
    /// Overrides the profile's effort (Claude) or sets `model_reasoning_effort` (Codex).
    pub effort: Option<String>,
    /// Folder the Claude CLI may read (frames or images).
    pub read_dir: Option<PathBuf>,
    /// Images attached with `--image` (Codex only; Claude reads the files named in the prompt).
    pub images: Vec<String>,
    pub timeout: Duration,
    /// Model to use instead of the backend's.
    pub model: Option<String>,
}

/// What a successful run returned.
#[derive(Debug, Clone, Default)]
pub struct CliReply {
    pub text: String,
    pub usage: Option<Usage>,
    pub session_id: Option<String>,
    pub stderr: String,
}

/// A run that ended with an error the CLI reported itself (signed out, usage limit, model).
#[derive(Debug)]
pub struct CliFailed {
    pub tool: CliTool,
    pub message: String,
    pub usage: Option<Usage>,
}
impl std::fmt::Display for CliFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.tool.display_name(), self.message)
    }
}
impl std::error::Error for CliFailed {}

pub struct CliBackend {
    pub tool: CliTool,
    /// Executable path from Settings; empty = search.
    pub path_override: String,
    pub model: String,
    runner: Arc<dyn ProcessRunner>,
    env: Arc<dyn SysEnv>,
    /// Working directory of every run; frames and images go in subfolders.
    work_root: PathBuf,
    windows: bool,
}

impl CliBackend {
    pub fn new(tool: CliTool, path_override: &str, model: &str) -> CliBackend {
        CliBackend::with_parts(tool, path_override, model, Arc::new(SystemRunner), Arc::new(RealEnv), default_work_root(), cfg!(windows))
    }

    pub fn with_parts(
        tool: CliTool,
        path_override: &str,
        model: &str,
        runner: Arc<dyn ProcessRunner>,
        env: Arc<dyn SysEnv>,
        work_root: PathBuf,
        windows: bool,
    ) -> CliBackend {
        CliBackend { tool, path_override: path_override.trim().to_string(), model: model.trim().to_string(), runner, env, work_root, windows }
    }

    pub fn executable(&self) -> Result<PathBuf> {
        discover::find_executable(self.tool, &self.path_override, self.env.as_ref(), self.windows).map_err(|m| anyhow!(m))
    }

    pub fn work_root(&self) -> &Path {
        &self.work_root
    }

    pub fn is_windows(&self) -> bool {
        self.windows
    }

    /// A new folder under the working directory for this run's files, removed on drop.
    pub fn temp_dir(&self, prefix: &str) -> Result<TempFolder> {
        std::fs::create_dir_all(&self.work_root)?;
        remove_stale(&self.work_root);
        let dir = self.work_root.join(format!("{prefix}-{}", new_uuid()));
        std::fs::create_dir_all(&dir)?;
        Ok(TempFolder(dir))
    }

    /// Runs the CLI once.
    pub async fn run(&self, req: &CliRequest) -> Result<CliReply> {
        let program = self.executable()?;
        std::fs::create_dir_all(&self.work_root)?;
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());
        let mut last_message: Option<PathBuf> = None;
        let mut settings_file = None;
        let (args, env_set, env_remove) = match self.tool {
            CliTool::Claude => {
                if let Some(json) = req.profile.settings_json() {
                    let f = self.work_root.join(format!("claude-{}-settings.json", format!("{:?}", req.profile).to_lowercase()));
                    std::fs::write(&f, json)?;
                    settings_file = Some(f);
                }
                let a = args::claude_args(&args::ClaudeArgs {
                    profile: req.profile,
                    session: &req.session,
                    model: &model,
                    effort: req.effort.as_deref(),
                    system_prompt: &req.system,
                    settings_file: settings_file.as_deref(),
                    read_dir: req.read_dir.as_deref(),
                    windows: self.windows,
                });
                let (set, remove) = req.profile.env();
                (a, set, remove)
            }
            CliTool::Codex => {
                let f = self.work_root.join(format!("codex-last-{}.txt", new_uuid()));
                let a = args::codex_args(&args::CodexArgs { model: &model, effort: req.effort.as_deref(), images: &req.images, last_message_file: &f });
                last_message = Some(f);
                (a, vec![], vec![])
            }
        };
        let spec = ProcessSpec {
            program: program.clone(),
            args,
            cwd: Some(self.work_root.clone()),
            stdin: req.prompt.as_bytes().to_vec(),
            env_set,
            env_remove,
            timeout: req.timeout,
        };
        log::info!("running {} ({} model {}, {:?})", program.display(), self.tool.bin_name(), if model.is_empty() { "default" } else { &model }, req.profile);
        let result = self.runner.run(spec).await;
        let last_text = last_message.as_ref().and_then(|f| {
            let t = std::fs::read_to_string(f).ok();
            let _ = std::fs::remove_file(f);
            t
        });
        let out = match result {
            Ok(o) => o,
            Err(e) => {
                if let Some(s) = e.downcast_ref::<SpawnFailed>() {
                    if s.not_found {
                        bail!(discover::not_found_message(self.tool, Some(&program.display().to_string())));
                    }
                }
                if let Some(t) = e.downcast_ref::<TimedOut>() {
                    log::warn!("{} timed out; partial stderr: {}", self.tool.bin_name(), crate::llm::client::truncate(&t.stderr, 400));
                }
                return Err(e);
            }
        };
        self.interpret(out.code, &out.stdout, &out.stderr, last_text.as_deref())
    }

    /// Turns an exit code and output into the answer or a readable error.
    fn interpret(&self, code: Option<i32>, stdout: &str, stderr: &str, last_message: Option<&str>) -> Result<CliReply> {
        let name = self.tool.display_name();
        // cmd.exe's answer when a .cmd shim points at a removed install; 127 is the POSIX one.
        if code == Some(127) || code == Some(9009) || stderr.contains("is not recognized as an internal or external command") || stderr.contains("command not found") {
            bail!(discover::not_found_message(self.tool, None));
        }
        match self.tool {
            CliTool::Claude => {
                let env = output::parse_claude_output(stdout);
                let usage = env.as_ref().and_then(|e| e.usage);
                let text = env.as_ref().map(|e| e.text.clone()).unwrap_or_else(|| output::strip_ansi(stdout).trim().to_string());
                let is_error = env.as_ref().map(|e| e.is_error).unwrap_or(false);
                if code != Some(0) || is_error {
                    let message = if is_error && !text.trim().is_empty() {
                        let mut m = crate::llm::client::truncate(text.trim(), 1000);
                        if !stderr.trim().is_empty() {
                            m = format!("{m} ({})", crate::llm::client::truncate(stderr.trim(), 300));
                        }
                        m
                    } else {
                        output::extract_cli_error(stdout, stderr).unwrap_or_else(|| output::failure_message("claude", code, &text, stderr))
                    };
                    return Err(CliFailed { tool: self.tool, message, usage }.into());
                }
                Ok(CliReply { text, usage, session_id: env.and_then(|e| e.session_id), stderr: stderr.to_string() })
            }
            CliTool::Codex => {
                let text = output::parse_codex_output(last_message, stdout);
                if code != Some(0) {
                    let message = output::extract_cli_error(stdout, stderr).unwrap_or_else(|| output::failure_message("codex", code, &text, stderr));
                    return Err(CliFailed { tool: self.tool, message, usage: None }.into());
                }
                if text.trim().is_empty() {
                    if let Some(e) = output::extract_cli_error(stdout, stderr) {
                        return Err(CliFailed { tool: self.tool, message: e, usage: None }.into());
                    }
                    bail!("{name} returned no answer");
                }
                Ok(CliReply { text, usage: None, session_id: None, stderr: stderr.to_string() })
            }
        }
    }

    /// `LlmClient::complete` for CLI providers: text parts become the prompt; each image is
    /// written to a file the CLI reads (Claude) or attaches (Codex). A single turn, no session.
    pub async fn complete(&self, parts: &[Part], system: Option<&str>, timeout: Duration) -> Result<(String, Option<Usage>)> {
        let has_images = parts.iter().any(|p| matches!(p, Part::Jpeg { .. }));
        let folder = if has_images { Some(self.temp_dir("images")?) } else { None };
        let mut prompt = String::new();
        let mut images = Vec::new();
        for part in parts {
            match part {
                Part::Text(t) => {
                    prompt.push_str(t);
                    prompt.push('\n');
                }
                Part::Jpeg { b64, .. } => {
                    let dir = folder.as_ref().unwrap().path();
                    let file = dir.join(format!("image_{:02}.jpg", images.len() + 1));
                    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| anyhow!("bad image data: {e}"))?;
                    std::fs::write(&file, bytes)?;
                    let path = file.display().to_string();
                    images.push(path.clone());
                    match self.tool {
                        CliTool::Claude => prompt.push_str(&format!("Image file: {path}\n")),
                        // Codex attaches images ahead of the text, so labels name them by position.
                        CliTool::Codex => prompt.push_str(&format!("[Image {}]\n", images.len())),
                    }
                }
            }
        }
        if has_images {
            prompt.push_str(match self.tool {
                CliTool::Claude => "\nEach image file above is a local JPEG screenshot. Tool requirement: use Read exactly once on every listed file before answering. Do not narrate or summarize individual files before the answer.\n",
                CliTool::Codex => "\nThe images are attached to this message in the order of their labels. Do not run commands or use tools; everything you need is in this message.\n",
            });
        }
        let profile = if has_images { Profile::Vision } else { Profile::Text };
        let default_system = if has_images { args::VISION_SYSTEM } else { args::TEXT_SYSTEM };
        let system = match (self.tool, system) {
            (CliTool::Claude, Some(s)) if !s.trim().is_empty() => {
                if has_images { format!("{s} {}", args::VISION_SYSTEM) } else { s.to_string() }
            }
            (CliTool::Claude, _) => default_system.to_string(),
            // Codex has no system prompt flag: it goes at the top of the prompt.
            (CliTool::Codex, s) => {
                if let Some(s) = s.filter(|s| !s.trim().is_empty()) {
                    prompt = format!("{s}\n\n{prompt}");
                }
                String::new()
            }
        };
        let req = CliRequest {
            prompt,
            profile,
            session: Session::Ephemeral,
            system,
            effort: None,
            read_dir: folder.as_ref().map(|f| f.path().to_path_buf()),
            images: if self.tool == CliTool::Codex { images } else { Vec::new() },
            timeout,
            model: None,
        };
        let reply = self.run(&req).await?;
        Ok((reply.text, reply.usage))
    }

    /// `claude --version` / `codex --version`, for the Settings check.
    pub async fn version(&self) -> Result<(PathBuf, String)> {
        let program = self.executable()?;
        let out = self
            .runner
            .run(ProcessSpec { program: program.clone(), args: vec!["--version".into()], timeout: Duration::from_secs(20), ..Default::default() })
            .await?;
        if out.code != Some(0) {
            bail!("{} did not start: {}", self.tool.display_name(), output::failure_message(self.tool.bin_name(), out.code, &out.stdout, &out.stderr));
        }
        let v = output::strip_ansi(&out.stdout);
        Ok((program, v.trim().lines().next().unwrap_or("").trim().to_string()))
    }

    /// Deletes the transcript of a resumable Claude session started by Daytrace (Dayflow
    /// ClaudeSessionCleanup). Claude keeps transcripts in `<config>/projects/<folder>/<id>.jsonl`.
    pub fn cleanup_session(&self, id: &str) {
        if self.tool != CliTool::Claude || id.is_empty() {
            return;
        }
        let Some(config) = claude_config_dir(self.env.as_ref()) else { return };
        let Ok(projects) = std::fs::read_dir(config.join("projects")) else { return };
        for project in projects.flatten() {
            let file = project.path().join(format!("{id}.jsonl"));
            if file.is_file() {
                let _ = std::fs::remove_file(&file);
            }
            let folder = project.path().join(id);
            if folder.is_dir() {
                let _ = std::fs::remove_dir_all(&folder);
            }
        }
    }
}

/// `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_config_dir(env: &dyn SysEnv) -> Option<PathBuf> {
    if let Some(d) = env.var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    env.var("USERPROFILE").or_else(|| env.var("HOME")).map(|h| PathBuf::from(h).join(".claude"))
}

/// `%TEMP%\daytrace-cli`: the CLIs' working directory. One fixed folder keeps Claude Code from
/// registering a new project for every batch.
pub fn default_work_root() -> PathBuf {
    std::env::temp_dir().join("daytrace-cli")
}

/// A folder removed when dropped.
pub struct TempFolder(PathBuf);
impl TempFolder {
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Frame and image folders left behind by a crash (older than an hour).
fn remove_stale(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !(name.starts_with("frames-") || name.starts_with("images-") || name.starts_with("codex-last-")) {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d > Duration::from_secs(3600)).unwrap_or(false);
        if old {
            let p = e.path();
            let _ = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
        }
    }
}

/// A random version-4 UUID (for `--session-id`), without an extra crate.
pub fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(nanos);
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_usize(i);
        h.write_u32(std::process::id());
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use discover::fake::FakeEnv;
    use process::FakeRunner;

    /// A backend over a fake runner; `claude.exe` / `codex.cmd` "exist" in C:\bin.
    pub fn backend(tool: CliTool, runner: Arc<FakeRunner>, root: &Path) -> CliBackend {
        let env = FakeEnv::new(&[("PATH", r"C:\bin"), ("USERPROFILE", r"C:\Users\arne")], &[r"C:\bin\claude.exe", r"C:\bin\codex.cmd"]);
        CliBackend::with_parts(tool, "", if tool == CliTool::Claude { "haiku" } else { "gpt-6-luna" }, runner, Arc::new(env), root.to_path_buf(), true)
    }

    pub fn claude_result(text: &str) -> String {
        serde_json::json!({"type": "result", "subtype": "success", "is_error": false, "result": text, "session_id": "s",
            "usage": {"input_tokens": 10, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 0, "output_tokens": 20}})
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use process::FakeRunner;

    #[test]
    fn uuids_are_v4_and_unique() {
        let a = new_uuid();
        let b = new_uuid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
    }

    #[tokio::test]
    async fn complete_writes_images_and_reads_answer() {
        let tmp = tempfile::tempdir().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
        let seen2 = seen.clone();
        let mut runner = FakeRunner::new(vec![FakeRunner::ok(&claude_result("Blue"))]);
        // The image files must exist while the CLI runs.
        runner.on_run = Some(Box::new(move |spec: &ProcessSpec| {
            let prompt = String::from_utf8_lossy(&spec.stdin).to_string();
            let path = prompt.lines().find_map(|l| l.strip_prefix("Image file: ")).unwrap().to_string();
            seen2.lock().unwrap().push(std::path::Path::new(&path).is_file());
        }));
        let runner = Arc::new(runner);
        let b = backend(CliTool::Claude, runner.clone(), tmp.path());
        let jpg = base64::engine::general_purpose::STANDARD.encode([0xFFu8, 0xD8, 0xFF, 0xD9]);
        let (text, usage) = b
            .complete(&[Part::Text("What color?".into()), Part::Jpeg { b64: jpg, w: 2, h: 2 }], None, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(text, "Blue");
        assert_eq!(usage.unwrap().for_log(), (Some(110), Some(20)));
        assert_eq!(*seen.lock().unwrap(), vec![true]);
        let call = &runner.calls()[0];
        assert_eq!(call.program, PathBuf::from(r"C:\bin\claude.exe"));
        assert!(call.args.contains(&"Read".to_string()));
        assert!(call.args.contains(&"--no-session-persistence".to_string()));
        assert!(call.env_set.contains(&("MAX_THINKING_TOKENS".to_string(), "0".to_string())));
        assert_eq!(call.cwd.as_deref(), Some(tmp.path()));
        let prompt = String::from_utf8_lossy(&call.stdin);
        assert!(prompt.starts_with("What color?\nImage file: "), "{prompt}");
        assert!(prompt.contains("use Read exactly once"));
        // The image folder is gone afterwards.
        assert!(!std::fs::read_dir(tmp.path()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("images-")));
    }

    #[tokio::test]
    async fn text_only_requests_disable_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::new(vec![FakeRunner::ok(&claude_result("{\"a\":1}"))]));
        let b = backend(CliTool::Claude, runner.clone(), tmp.path());
        let (text, _) = b.complete(&[Part::Text("Give JSON".into())], Some("Return only JSON."), Duration::from_secs(5)).await.unwrap();
        assert_eq!(text, "{\"a\":1}");
        let args = &runner.calls()[0].args;
        let i = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[i + 1], "");
        let j = args.iter().position(|a| a == "--system-prompt").unwrap();
        assert_eq!(args[j + 1], "Return only JSON.");
    }

    #[tokio::test]
    async fn reports_cli_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let signed_out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login"}"#;
        let runner = Arc::new(FakeRunner::new(vec![
            FakeRunner::exit(1, signed_out, ""),
            FakeRunner::exit(1, "", "'claude' is not recognized as an internal or external command,\r\noperable program or batch file."),
            FakeRunner::exit(1, "", ""),
        ]));
        let b = backend(CliTool::Claude, runner, tmp.path());
        let e = b.complete(&[Part::Text("x".into())], None, Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(e.to_string(), "Claude Code CLI: Invalid API key · Please run /login");
        let e = b.complete(&[Part::Text("x".into())], None, Duration::from_secs(5)).await.unwrap_err();
        assert!(e.to_string().contains("Claude Code CLI not found"), "{e}");
        let e = b.complete(&[Part::Text("x".into())], None, Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(e.to_string(), "Claude Code CLI: claude exited with code 1");
    }

    #[tokio::test]
    async fn codex_reads_last_message_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut runner = FakeRunner::new(vec![FakeRunner::ok("noise on stdout")]);
        runner.on_run = Some(Box::new(|spec: &ProcessSpec| {
            let i = spec.args.iter().position(|a| a == "--output-last-message").unwrap();
            std::fs::write(&spec.args[i + 1], "{\"cards\":[]}").unwrap();
        }));
        let runner = Arc::new(runner);
        let b = backend(CliTool::Codex, runner.clone(), tmp.path());
        let jpg = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        let (text, usage) = b.complete(&[Part::Text("Cards?".into()), Part::Jpeg { b64: jpg, w: 1, h: 1 }], Some("SYS"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(text, "{\"cards\":[]}");
        assert!(usage.is_none());
        let call = &runner.calls()[0];
        assert_eq!(call.program, PathBuf::from(r"C:\bin\codex.cmd"));
        assert_eq!(call.args.iter().filter(|a| *a == "--image").count(), 1);
        assert_eq!(call.args.last().unwrap(), "-");
        let prompt = String::from_utf8_lossy(&call.stdin);
        assert!(prompt.starts_with("SYS\n\nCards?\n[Image 1]"), "{prompt}");
    }

    #[test]
    fn session_cleanup_removes_only_that_transcript() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("claude-config");
        let proj = config.join("projects").join("C--Temp-daytrace-cli");
        std::fs::create_dir_all(proj.join("abc")).unwrap();
        std::fs::write(proj.join("abc.jsonl"), "{}").unwrap();
        std::fs::write(proj.join("other.jsonl"), "{}").unwrap();
        let mut env = discover::fake::FakeEnv::new(&[("CLAUDE_CONFIG_DIR", config.to_str().unwrap())], &[]);
        env.files.insert(r"C:\bin\claude.exe".into());
        let b = CliBackend::with_parts(CliTool::Claude, "", "haiku", Arc::new(FakeRunner::new(vec![])), Arc::new(env), tmp.path().into(), true);
        b.cleanup_session("abc");
        assert!(!proj.join("abc.jsonl").exists());
        assert!(!proj.join("abc").exists());
        assert!(proj.join("other.jsonl").exists());
    }
}
