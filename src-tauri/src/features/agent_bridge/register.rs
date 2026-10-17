//! One-click setup of `daytrace-mcp` in Claude Code and Claude Desktop (Dayflow's
//! Core/AgentAccess/AgentClientRegistration.swift, MIT, (c) 2025 Jerry Liu), for Windows.
//!
//! - Claude Code: `claude mcp add --scope user --transport stdio daytrace -- <daytrace-mcp.exe>`,
//!   run through the `claude` the CLI provider finds (llm/cli/discover.rs), instead of editing
//!   ~/.claude.json the way Dayflow does. The status is read from that file's user-scope
//!   `mcpServers` (read only).
//! - Claude Desktop: the `daytrace` entry is added to or removed from
//!   claude_desktop_config.json; every other key and server stays. The file is copied to a
//!   timestamped backup first, and a file that isn't a JSON object is left alone. The Microsoft
//!   Store (MSIX) build reads the copy under
//!   %LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\Roaming\Claude; the classic installer reads
//!   %APPDATA%\Claude. Daytrace writes every copy that exists, or the one the installed build
//!   reads.
//!
//! Only the entry named "daytrace" is ever written or removed.

use crate::llm::cli::{discover, process, CliBackend, CliTool};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SERVER_NAME: &str = "daytrace";

/// `daytrace-mcp.exe` next to the running app.
pub fn mcp_exe() -> PathBuf {
    let name = if cfg!(windows) { "daytrace-mcp.exe" } else { "daytrace-mcp" };
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).unwrap_or_else(|| PathBuf::from(name))
}

// ---- Claude Desktop config editing (pure) ----

fn parse_object(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err("isn't valid JSON".into()),
    }
}

/// The config text with the daytrace entry added or replaced; everything else kept.
pub fn with_entry(existing: Option<&str>, exe: &str) -> Result<String, String> {
    let mut root = parse_object(existing.unwrap_or(""))?;
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else { return Err("has an \"mcpServers\" value that isn't an object".into()) };
    servers.insert(SERVER_NAME.into(), json!({"command": exe, "args": []}));
    Ok(serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default() + "\n")
}

/// The config text without the daytrace entry, or None when there is none.
pub fn without_entry(existing: &str) -> Result<Option<String>, String> {
    let mut root = parse_object(existing)?;
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else { return Ok(None) };
    if servers.remove(SERVER_NAME).is_none() {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default() + "\n"))
}

/// The `command` of the daytrace entry in an MCP config (`mcpServers.daytrace.command`).
pub fn recorded_command(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.get("mcpServers")?.get(SERVER_NAME)?.get("command")?.as_str().map(String::from)
}

// ---- where the configs are ----

/// The two Claude Desktop config locations: (MSIX copies, classic copy).
fn desktop_candidates(env: &dyn Fn(&str) -> Option<String>) -> (Vec<PathBuf>, Option<PathBuf>) {
    let mut msix = Vec::new();
    if let Some(local) = env("LOCALAPPDATA") {
        if let Ok(entries) = std::fs::read_dir(Path::new(&local).join("Packages")) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("Claude_") && e.path().is_dir() {
                    msix.push(e.path().join("LocalCache").join("Roaming").join("Claude").join("claude_desktop_config.json"));
                }
            }
        }
    }
    msix.sort();
    let classic = env("APPDATA").map(|a| Path::new(&a).join("Claude").join("claude_desktop_config.json"));
    (msix, classic)
}

/// Config files to write: every one that exists; else the one the installed build reads; empty
/// when Claude Desktop isn't installed.
pub fn desktop_targets(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let (msix, classic) = desktop_candidates(env);
    let all: Vec<PathBuf> = msix.iter().cloned().chain(classic.clone()).collect();
    let existing: Vec<PathBuf> = all.iter().filter(|p| p.is_file()).cloned().collect();
    if !existing.is_empty() {
        return existing;
    }
    if let Some(first) = msix.into_iter().next() {
        return vec![first];
    }
    classic.into_iter().filter(|p| p.parent().is_some_and(Path::is_dir)).collect()
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json`), where Claude Code keeps user-scope
/// MCP servers.
fn claude_code_config(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(dir) = env("CLAUDE_CONFIG_DIR") {
        return Some(Path::new(&dir).join(".claude.json"));
    }
    env("USERPROFILE").or_else(|| env("HOME")).map(|h| Path::new(&h).join(".claude.json"))
}

// ---- status ----

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClientStatus {
    /// The client is installed (Claude Code: its CLI was found; Claude Desktop: its folder).
    pub installed: bool,
    /// Where the CLI or config file is, for display.
    pub location: Option<String>,
    /// The daytrace entry's command, when one is configured.
    pub recorded_command: Option<String>,
    /// Why the client can't be set up from here, in plain words.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccess {
    pub mcp_path: String,
    pub mcp_installed: bool,
    /// The folder that goes on PATH.
    pub install_dir: String,
    pub on_path: bool,
    /// PATH can be changed from here (Windows).
    pub path_supported: bool,
    pub claude_code: ClientStatus,
    pub claude_desktop: ClientStatus,
    pub plugin_marketplace_command: String,
    pub plugin_install_command: String,
}

/// The `claude` CLI to use: the path set for the Claude Code provider (main or fallback), else a
/// search.
pub fn claude_backend(settings: &crate::config::Settings) -> CliBackend {
    use crate::config::ProviderKind;
    let path = if settings.provider == ProviderKind::ClaudeCli {
        settings.base_url.clone()
    } else if settings.fallback_provider == Some(ProviderKind::ClaudeCli) {
        settings.fallback_base_url.clone()
    } else {
        String::new()
    };
    CliBackend::new(CliTool::Claude, &path, "")
}

pub fn status(settings: &crate::config::Settings) -> AgentAccess {
    let exe = mcp_exe();
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let claude = claude_backend(settings).executable();
    let code_config = claude_code_config(&real_env);
    let code_recorded = code_config.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| recorded_command(&t));
    let desktop = desktop_targets(&real_env);
    let desktop_recorded = desktop.iter().find_map(|p| std::fs::read_to_string(p).ok().and_then(|t| recorded_command(&t)));
    AgentAccess {
        mcp_path: exe.to_string_lossy().to_string(),
        mcp_installed: exe.is_file(),
        install_dir: dir.to_string_lossy().to_string(),
        on_path: super::path::is_linked(&dir),
        path_supported: cfg!(windows),
        claude_code: ClientStatus {
            installed: claude.is_ok(),
            location: claude.as_ref().ok().map(|p| p.to_string_lossy().to_string()),
            recorded_command: code_recorded,
            problem: claude.err().map(|e| e.to_string()),
        },
        claude_desktop: ClientStatus {
            installed: !desktop.is_empty(),
            location: desktop.first().map(|p| p.to_string_lossy().to_string()),
            recorded_command: desktop_recorded,
            problem: desktop.is_empty().then(|| "Claude Desktop isn't installed for this Windows user.".to_string()),
        },
        plugin_marketplace_command: "/plugin marketplace add ArneVDA-AP/daytrace".into(),
        plugin_install_command: "/plugin install daytrace@daytrace".into(),
    }
}

// ---- actions ----

fn run_claude(backend: &CliBackend, args: Vec<String>) -> impl std::future::Future<Output = anyhow::Result<process::ProcessOutput>> + '_ {
    async move {
        let program = backend.executable()?;
        std::fs::create_dir_all(backend.work_root())?;
        let spec = process::ProcessSpec { program, args, cwd: Some(backend.work_root().to_path_buf()), timeout: Duration::from_secs(60), ..Default::default() };
        backend.runner().run(spec).await
    }
}

fn output_text(out: &process::ProcessOutput) -> String {
    let text = format!("{}\n{}", crate::llm::cli::output::strip_ansi(&out.stdout).trim(), out.stderr.trim());
    text.trim().to_string()
}

/// Adds daytrace-mcp to Claude Code's user-scope MCP servers (replacing an older daytrace entry).
pub async fn add_to_claude_code(backend: &CliBackend, exe: &Path) -> anyhow::Result<String> {
    if !exe.is_file() {
        anyhow::bail!("daytrace-mcp isn't next to Daytrace ({}). Install Daytrace again to get it.", exe.display());
    }
    let add = || {
        let mut a: Vec<String> = ["mcp", "add", "--scope", "user", "--transport", "stdio", SERVER_NAME, "--"].iter().map(|s| s.to_string()).collect();
        a.push(exe.to_string_lossy().to_string());
        a
    };
    let out = run_claude(backend, add()).await.map_err(|e| not_found(backend, e))?;
    if out.code == Some(0) {
        return Ok("Added to Claude Code for all your projects. New Claude Code sessions can read your timeline.".into());
    }
    if output_text(&out).to_lowercase().contains("already exists") {
        let removed = run_claude(backend, vec!["mcp".into(), "remove".into(), "--scope".into(), "user".into(), SERVER_NAME.into()]).await?;
        if removed.code != Some(0) {
            anyhow::bail!("Claude Code already has a daytrace server and didn't let Daytrace replace it: {}", output_text(&removed));
        }
        let again = run_claude(backend, add()).await?;
        if again.code == Some(0) {
            return Ok("Updated the daytrace server in Claude Code. New Claude Code sessions use it.".into());
        }
        anyhow::bail!("Claude Code didn't add the server: {}", output_text(&again));
    }
    anyhow::bail!("Claude Code didn't add the server: {}", output_text(&out))
}

/// Removes the daytrace server from Claude Code's user scope.
pub async fn remove_from_claude_code(backend: &CliBackend) -> anyhow::Result<String> {
    let out = run_claude(backend, vec!["mcp".into(), "remove".into(), "--scope".into(), "user".into(), SERVER_NAME.into()]).await.map_err(|e| not_found(backend, e))?;
    if out.code == Some(0) {
        return Ok("Removed from Claude Code.".into());
    }
    anyhow::bail!("Claude Code didn't remove the server: {}", output_text(&out))
}

fn not_found(backend: &CliBackend, e: anyhow::Error) -> anyhow::Error {
    if e.downcast_ref::<process::SpawnFailed>().is_some_and(|s| s.not_found) {
        let custom = (!backend.path_override.is_empty()).then_some(backend.path_override.as_str());
        return anyhow::anyhow!(discover::not_found_message(CliTool::Claude, custom));
    }
    e
}

/// Copies `file` to `<file>.daytrace-backup-<time>` and returns the backup's path.
pub(super) fn backup(file: &Path) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = format!("{}.daytrace-backup-{stamp}", file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    let to = file.with_file_name(name);
    std::fs::copy(file, &to)?;
    Ok(to)
}

/// Writes through a temporary file in the same folder and a rename.
pub(super) fn write_atomic(file: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = file.with_file_name(format!("{name}.daytrace-{}.tmp", std::process::id()));
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, file) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Adds the daytrace entry to each Claude Desktop config in `targets`.
pub fn add_to_claude_desktop(targets: &[PathBuf], exe: &Path) -> anyhow::Result<String> {
    if !exe.is_file() {
        anyhow::bail!("daytrace-mcp isn't next to Daytrace ({}). Install Daytrace again to get it.", exe.display());
    }
    if targets.is_empty() {
        anyhow::bail!("Claude Desktop isn't installed for this Windows user. Install it, open it once, then try again.");
    }
    let mut backups = Vec::new();
    for file in targets {
        let existing = std::fs::read_to_string(file).ok();
        let text = with_entry(existing.as_deref(), &exe.to_string_lossy())
            .map_err(|why| anyhow::anyhow!("{} {why}, so Daytrace left it alone. Fix or remove it in Claude Desktop (Settings > Developer > Edit Config), then try again.", file.display()))?;
        if existing.is_some() {
            backups.push(backup(file)?);
        }
        write_atomic(file, &text)?;
    }
    let mut m = "Added to Claude Desktop. Quit Claude Desktop from its tray icon and open it again to load it.".to_string();
    if !backups.is_empty() {
        m.push_str(&format!(" The previous file was copied to {}.", backups.iter().map(|b| b.display().to_string()).collect::<Vec<_>>().join(" and ")));
    }
    Ok(m)
}

/// Removes the daytrace entry from each Claude Desktop config in `targets` that has one.
pub fn remove_from_claude_desktop(targets: &[PathBuf]) -> anyhow::Result<String> {
    let mut changed = 0;
    for file in targets {
        let Ok(existing) = std::fs::read_to_string(file) else { continue };
        let next = without_entry(&existing).map_err(|why| anyhow::anyhow!("{} {why}, so Daytrace left it alone.", file.display()))?;
        if let Some(text) = next {
            backup(file)?;
            write_atomic(file, &text)?;
            changed += 1;
        }
    }
    Ok(if changed == 0 { "Claude Desktop had no daytrace server.".into() } else { "Removed from Claude Desktop. Restart Claude Desktop to apply it.".into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::cli::discover::SysEnv;
    use crate::llm::cli::process::FakeRunner;
    use std::sync::Arc;

    #[test]
    fn desktop_entry_merge_keeps_everything_else() {
        let before = r#"{"globalShortcut":"Ctrl+Space","mcpServers":{"files":{"command":"npx","args":["-y","server-files"]}}}"#;
        let after = with_entry(Some(before), r"C:\Users\me\AppData\Local\Daytrace\daytrace-mcp.exe").unwrap();
        let v: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(v["globalShortcut"], "Ctrl+Space");
        assert_eq!(v["mcpServers"]["files"]["args"], json!(["-y", "server-files"]));
        assert_eq!(v["mcpServers"]["daytrace"], json!({"command": r"C:\Users\me\AppData\Local\Daytrace\daytrace-mcp.exe", "args": []}));
        assert_eq!(recorded_command(&after).as_deref(), Some(r"C:\Users\me\AppData\Local\Daytrace\daytrace-mcp.exe"));
        // Again: replaced, not duplicated.
        let twice = with_entry(Some(&after), "D:\\daytrace-mcp.exe").unwrap();
        assert_eq!(serde_json::from_str::<Value>(&twice).unwrap()["mcpServers"].as_object().unwrap().len(), 2);
        // A new or empty file.
        assert_eq!(serde_json::from_str::<Value>(&with_entry(None, "x").unwrap()).unwrap(), json!({"mcpServers": {"daytrace": {"command": "x", "args": []}}}));
        assert!(with_entry(Some("  "), "x").is_ok());
        // Files Daytrace must not touch.
        assert!(with_entry(Some("{ not json"), "x").is_err());
        assert!(with_entry(Some("[1,2]"), "x").is_err());
        assert!(with_entry(Some(r#"{"mcpServers":[]}"#), "x").is_err());

        let removed = without_entry(&after).unwrap().unwrap();
        let v: Value = serde_json::from_str(&removed).unwrap();
        assert!(v["mcpServers"].get("daytrace").is_none());
        assert_eq!(v["mcpServers"]["files"]["command"], "npx");
        assert_eq!(without_entry(before).unwrap(), None);
        assert_eq!(without_entry("{}").unwrap(), None);
    }

    #[test]
    fn desktop_files_get_a_backup_and_msix_wins_when_alone() {
        let dir = tempfile::tempdir().unwrap();
        let roaming = dir.path().join("Roaming");
        let local = dir.path().join("Local");
        let env = |k: &str| match k {
            "APPDATA" => Some(roaming.to_string_lossy().to_string()),
            "LOCALAPPDATA" => Some(local.to_string_lossy().to_string()),
            _ => None,
        };
        // Not installed: nothing to write.
        assert!(desktop_targets(&env).is_empty());
        // Classic install (folder only): the classic file.
        std::fs::create_dir_all(roaming.join("Claude")).unwrap();
        assert_eq!(desktop_targets(&env), vec![roaming.join("Claude").join("claude_desktop_config.json")]);
        // MSIX installed, no files yet: the packaged copy.
        let pkg = local.join("Packages").join("Claude_pzs8sxrjxfjjc");
        std::fs::create_dir_all(&pkg).unwrap();
        let msix_file = pkg.join("LocalCache").join("Roaming").join("Claude").join("claude_desktop_config.json");
        assert_eq!(desktop_targets(&env), vec![msix_file.clone()]);
        // Both files exist: both.
        std::fs::create_dir_all(msix_file.parent().unwrap()).unwrap();
        std::fs::write(&msix_file, r#"{"mcpServers":{"other":{"command":"o"}}}"#).unwrap();
        std::fs::write(roaming.join("Claude").join("claude_desktop_config.json"), "{}").unwrap();
        let targets = desktop_targets(&env);
        assert_eq!(targets.len(), 2);

        let exe = dir.path().join("daytrace-mcp.exe");
        assert!(add_to_claude_desktop(&targets, &exe).unwrap_err().to_string().contains("isn't next to Daytrace"));
        std::fs::write(&exe, b"").unwrap();
        let msg = add_to_claude_desktop(&targets, &exe).unwrap();
        assert!(msg.starts_with("Added to Claude Desktop."), "{msg}");
        assert!(msg.contains(".daytrace-backup-"), "{msg}");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&msix_file).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "o");
        assert_eq!(v["mcpServers"]["daytrace"]["command"], exe.to_string_lossy().as_ref());
        let backups: Vec<_> = std::fs::read_dir(msix_file.parent().unwrap()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("daytrace-backup")).collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read_to_string(backups[0].path()).unwrap(), r#"{"mcpServers":{"other":{"command":"o"}}}"#);

        // A broken file is left alone, the others are not written either.
        std::fs::write(&msix_file, "{ broken").unwrap();
        let e = add_to_claude_desktop(&targets, &exe).unwrap_err().to_string();
        assert!(e.contains("isn't valid JSON, so Daytrace left it alone"), "{e}");
        assert_eq!(std::fs::read_to_string(&msix_file).unwrap(), "{ broken");

        std::fs::write(&msix_file, "{}").unwrap();
        assert_eq!(remove_from_claude_desktop(&targets).unwrap(), "Removed from Claude Desktop. Restart Claude Desktop to apply it.");
        assert_eq!(remove_from_claude_desktop(&targets).unwrap(), "Claude Desktop had no daytrace server.");
        assert!(add_to_claude_desktop(&[], &exe).unwrap_err().to_string().contains("isn't installed"));
    }

    struct Env;
    impl SysEnv for Env {
        fn var(&self, _key: &str) -> Option<String> {
            None
        }
        fn is_file(&self, path: &str) -> bool {
            Path::new(path).is_file()
        }
        fn is_dir(&self, path: &str) -> bool {
            Path::new(path).is_dir()
        }
    }

    #[tokio::test]
    async fn claude_code_add_replaces_an_older_entry() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::write(&claude, b"").unwrap();
        let exe = dir.path().join("daytrace-mcp.exe");
        std::fs::write(&exe, b"").unwrap();
        let runner = Arc::new(FakeRunner::new(vec![
            FakeRunner::exit(1, "", "MCP server daytrace already exists in user config"),
            FakeRunner::ok("Removed MCP server daytrace from user config"),
            FakeRunner::ok("Added stdio MCP server daytrace"),
        ]));
        let backend = CliBackend::with_parts(CliTool::Claude, &claude.to_string_lossy(), "", runner.clone(), Arc::new(Env), dir.path().join("work"), false);
        let m = add_to_claude_code(&backend, &exe).await.unwrap();
        assert!(m.starts_with("Updated the daytrace server"), "{m}");
        let calls = runner.calls();
        let exe_s = exe.to_string_lossy().to_string();
        assert_eq!(calls[0].args, ["mcp", "add", "--scope", "user", "--transport", "stdio", "daytrace", "--", exe_s.as_str()]);
        assert_eq!(calls[1].args, ["mcp", "remove", "--scope", "user", "daytrace"]);
        assert_eq!(calls[2].args, calls[0].args);

        let runner = Arc::new(FakeRunner::new(vec![FakeRunner::exit(1, "", "error: unknown option")]));
        let backend = CliBackend::with_parts(CliTool::Claude, &claude.to_string_lossy(), "", runner, Arc::new(Env), dir.path().join("work"), false);
        assert!(add_to_claude_code(&backend, &exe).await.unwrap_err().to_string().contains("unknown option"));
    }
}
