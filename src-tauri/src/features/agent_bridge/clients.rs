//! One-click setup of `daytrace-mcp` in Codex, Cursor and VS Code (Dayflow's
//! Core/AgentAccess/AgentClientRegistration.swift and CodexMCPRegistration.swift, MIT, (c) 2025
//! Jerry Liu), for Windows. Claude Code and Claude Desktop are in register.rs.
//!
//! Dayflow runs `codex mcp add` and hands Cursor and VS Code an install link. Daytrace edits each
//! tool's own MCP config file the way it edits Claude Desktop's: the file is copied to a
//! timestamped backup first, written through a temporary file and a rename, and only the entry
//! named "daytrace" is added, replaced or removed. A file that can't be parsed is left alone, and
//! so is an entry named "daytrace" that doesn't run daytrace-mcp (Dayflow's "custom MCP server"
//! conflict). Locations and formats, from each tool's documentation:
//!
//! - Codex: `$CODEX_HOME/config.toml`, by default `~/.codex/config.toml`, shared by the Codex
//!   CLI, IDE extension and desktop app; `[mcp_servers.daytrace]` with `command` and `args`.
//!   Edited with toml_edit, so comments, order and formatting of the rest of the file stay.
//! - Cursor: `~/.cursor/mcp.json` (global), `mcpServers.daytrace` with `type`, `command`, `args`.
//! - VS Code: the user profile's `mcp.json`, top-level `servers`, entry with `type`, `command`,
//!   `args`. Daytrace writes the default profile (`%APPDATA%\Code\User\mcp.json`); VS Code asks
//!   the user to trust the server when it first starts it.
//!
//! JSON files are rewritten with serde_json, which keeps every key and value but writes keys in
//! alphabetical order (as Dayflow's JSONSerialization with .sortedKeys does).

use super::register::{backup, mcp_exe, write_atomic, SERVER_NAME};
use crate::llm::cli::{CliBackend, CliTool};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Codex,
    Cursor,
    VsCode,
}

impl Client {
    pub const ALL: [Client; 3] = [Client::Codex, Client::Cursor, Client::VsCode];

    /// The id the UI sends ("codex", "cursor", "vsCode").
    pub fn id(self) -> &'static str {
        match self {
            Client::Codex => "codex",
            Client::Cursor => "cursor",
            Client::VsCode => "vsCode",
        }
    }

    pub fn from_id(id: &str) -> Option<Client> {
        Client::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code",
        }
    }

    /// The JSON object that holds the servers (JSON clients).
    fn servers_key(self) -> &'static str {
        match self {
            Client::VsCode => "servers",
            _ => "mcpServers",
        }
    }
}

// ---- where the files are ----

/// `$CODEX_HOME`, else `~/.codex`.
pub fn codex_home(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(h) = env("CODEX_HOME") {
        return Some(PathBuf::from(h));
    }
    home(env).map(|h| h.join(".codex"))
}

fn home(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env("USERPROFILE").or_else(|| env("HOME")).map(PathBuf::from)
}

/// VS Code's default profile folder: %APPDATA%\Code\User on Windows (the documented locations of
/// the user settings file on macOS and Linux elsewhere).
fn vscode_user_dir(env: &dyn Fn(&str) -> Option<String>, windows: bool) -> Option<PathBuf> {
    if windows {
        return env("APPDATA").map(|a| Path::new(&a).join("Code").join("User"));
    }
    let home = home(env)?;
    Some(if cfg!(target_os = "macos") { home.join("Library/Application Support/Code/User") } else { home.join(".config/Code/User") })
}

/// The config file Daytrace edits for `client`.
pub fn config_file(client: Client, env: &dyn Fn(&str) -> Option<String>, windows: bool) -> Option<PathBuf> {
    match client {
        Client::Codex => codex_home(env).map(|d| d.join("config.toml")),
        Client::Cursor => home(env).map(|h| h.join(".cursor").join("mcp.json")),
        Client::VsCode => vscode_user_dir(env, windows).map(|d| d.join("mcp.json")),
    }
}

/// The `codex` CLI to look for: the path set for the Codex CLI provider (main or fallback), else
/// a search.
pub fn codex_backend(settings: &crate::config::Settings) -> CliBackend {
    use crate::config::ProviderKind;
    let path = if settings.provider == ProviderKind::CodexCli {
        settings.base_url.clone()
    } else if settings.fallback_provider == Some(ProviderKind::CodexCli) {
        settings.fallback_base_url.clone()
    } else {
        String::new()
    };
    CliBackend::new(CliTool::Codex, &path, "")
}

// ---- what an entry is ----

/// The "daytrace" entry of a config file.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    None,
    /// Runs daytrace-mcp; the command as written. `disabled`: Codex's `enabled = false`.
    Ours { command: String, disabled: bool },
    /// Named "daytrace" but runs something else (or is a URL server): what it runs.
    Foreign(String),
}

/// Whether `command` starts a daytrace-mcp program (any folder, with or without .exe).
fn is_daytrace_mcp(command: &str) -> bool {
    let name = command.trim().trim_matches('"').rsplit(['\\', '/']).next().unwrap_or("").to_ascii_lowercase();
    name == "daytrace-mcp" || name == "daytrace-mcp.exe"
}

fn classify(command: Option<&str>, url: Option<&str>, disabled: bool) -> Entry {
    match (command, url) {
        (Some(c), _) if is_daytrace_mcp(c) => Entry::Ours { command: c.to_string(), disabled },
        (Some(c), _) => Entry::Foreign(c.to_string()),
        (None, Some(u)) => Entry::Foreign(u.to_string()),
        (None, None) => Entry::Foreign("an entry without a command".into()),
    }
}

// ---- JSON clients (pure) ----

fn parse_object(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        // VS Code allows comments in its JSON files; Daytrace doesn't rewrite those.
        _ => Err("isn't plain JSON (it may have comments)".into()),
    }
}

/// The entry a JSON client gets.
fn json_entry(exe: &str) -> Value {
    json!({"type": "stdio", "command": exe, "args": []})
}

pub fn json_recorded(text: &str, key: &str) -> Result<Entry, String> {
    let root = parse_object(text)?;
    let Some(servers) = root.get(key) else { return Ok(Entry::None) };
    let Some(servers) = servers.as_object() else { return Err(format!("has a \"{key}\" value that isn't an object")) };
    Ok(match servers.get(SERVER_NAME) {
        None => Entry::None,
        Some(e) => classify(e.get("command").and_then(Value::as_str), e.get("url").and_then(Value::as_str), false),
    })
}

/// The config text with the daytrace entry added or replaced; everything else kept.
pub fn json_with_entry(existing: Option<&str>, key: &str, exe: &str) -> Result<String, String> {
    let mut root = parse_object(existing.unwrap_or(""))?;
    let servers = root.entry(key).or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else { return Err(format!("has a \"{key}\" value that isn't an object")) };
    servers.insert(SERVER_NAME.into(), json_entry(exe));
    Ok(serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default() + "\n")
}

/// The config text without the daytrace entry, or None when there is none.
pub fn json_without_entry(existing: &str, key: &str) -> Result<Option<String>, String> {
    let mut root = parse_object(existing)?;
    let Some(servers) = root.get_mut(key).and_then(Value::as_object_mut) else { return Ok(None) };
    if servers.remove(SERVER_NAME).is_none() {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default() + "\n"))
}

// ---- Codex config.toml (pure) ----

fn parse_toml(text: &str) -> Result<toml_edit::DocumentMut, String> {
    text.parse::<toml_edit::DocumentMut>().map_err(|_| "isn't valid TOML".to_string())
}

const NOT_A_TABLE: &str = "has an \"mcp_servers\" value that isn't a table";

pub fn toml_recorded(text: &str) -> Result<Entry, String> {
    let doc = parse_toml(text)?;
    let Some(servers) = doc.get("mcp_servers") else { return Ok(Entry::None) };
    let Some(servers) = servers.as_table_like() else { return Err(NOT_A_TABLE.into()) };
    let Some(entry) = servers.get(SERVER_NAME) else { return Ok(Entry::None) };
    let Some(entry) = entry.as_table_like() else { return Ok(Entry::Foreign("a value that isn't a table".into())) };
    let s = |k: &str| entry.get(k).and_then(|v| v.as_str());
    let disabled = entry.get("enabled").and_then(|v| v.as_bool()) == Some(false);
    Ok(classify(s("command"), s("url"), disabled))
}

/// config.toml with `[mcp_servers.daytrace]` added or replaced (command and args only, so an
/// `enabled = false` goes too); the rest of the file stays as written.
pub fn toml_with_entry(existing: Option<&str>, exe: &str) -> Result<String, String> {
    let mut doc = parse_toml(existing.unwrap_or(""))?;
    let mut entry = toml_edit::Table::new();
    entry.insert("command", toml_edit::value(exe));
    entry.insert("args", toml_edit::value(toml_edit::Array::new()));
    if !doc.contains_key("mcp_servers") {
        // `[mcp_servers.daytrace]` alone, without an empty `[mcp_servers]` header above it.
        let mut servers = toml_edit::Table::new();
        servers.set_implicit(true);
        doc.insert("mcp_servers", toml_edit::Item::Table(servers));
    }
    let servers = doc.get_mut("mcp_servers").and_then(|s| s.as_table_like_mut()).ok_or_else(|| NOT_A_TABLE.to_string())?;
    servers.insert(SERVER_NAME, toml_edit::Item::Table(entry));
    let mut text = doc.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

/// config.toml without `[mcp_servers.daytrace]`, or None when there is none.
pub fn toml_without_entry(existing: &str) -> Result<Option<String>, String> {
    let mut doc = parse_toml(existing)?;
    let Some(servers) = doc.get_mut("mcp_servers") else { return Ok(None) };
    let Some(servers) = servers.as_table_like_mut() else { return Err(NOT_A_TABLE.into()) };
    if servers.remove(SERVER_NAME).is_none() {
        return Ok(None);
    }
    Ok(Some(doc.to_string()))
}

// ---- per client ----

fn recorded(client: Client, text: &str) -> Result<Entry, String> {
    match client {
        Client::Codex => toml_recorded(text),
        _ => json_recorded(text, client.servers_key()),
    }
}

fn with_entry(client: Client, existing: Option<&str>, exe: &str) -> Result<String, String> {
    match client {
        Client::Codex => toml_with_entry(existing, exe),
        _ => json_with_entry(existing, client.servers_key(), exe),
    }
}

fn without_entry(client: Client, existing: &str) -> Result<Option<String>, String> {
    match client {
        Client::Codex => toml_without_entry(existing),
        _ => json_without_entry(existing, client.servers_key()),
    }
}

// ---- status ----

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClientEntryStatus {
    /// "codex", "cursor" or "vsCode".
    pub id: &'static str,
    pub name: &'static str,
    /// Installed for this Windows user (Codex: its CLI was found or its folder exists).
    pub installed: bool,
    /// The config file Daytrace edits.
    pub config_path: Option<String>,
    /// The daytrace entry's command, when it runs daytrace-mcp.
    pub recorded_command: Option<String>,
    /// Codex: the entry is there with `enabled = false`.
    pub disabled: bool,
    /// An entry named "daytrace" that doesn't run daytrace-mcp: what it runs. Daytrace leaves it.
    pub foreign: Option<String>,
    /// Why the client can't be set up from here (not installed, a file Daytrace can't read).
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentClients {
    pub mcp_path: String,
    pub mcp_installed: bool,
    pub clients: Vec<ClientEntryStatus>,
}

/// Installed: Codex has its folder or a CLI; Cursor its `.cursor` folder; VS Code its default
/// profile folder.
fn installed(client: Client, file: Option<&Path>, codex_cli: bool) -> bool {
    let dir = file.and_then(Path::parent).is_some_and(Path::is_dir);
    match client {
        Client::Codex => codex_cli || dir,
        _ => dir,
    }
}

fn not_installed(client: Client) -> String {
    match client {
        Client::Codex => "Codex isn't installed for this Windows user. Install the Codex CLI (npm install -g @openai/codex), the IDE extension or the app, open it once, then check again.".into(),
        Client::Cursor => "Cursor isn't installed for this Windows user. Install it, open it once, then check again.".into(),
        Client::VsCode => "VS Code isn't installed for this Windows user. Install it, open it once, then check again.".into(),
    }
}

pub fn client_status(client: Client, file: Option<&Path>, codex_cli: bool) -> ClientEntryStatus {
    let mut s = ClientEntryStatus {
        id: client.id(),
        name: client.name(),
        installed: installed(client, file, codex_cli),
        config_path: file.map(|f| f.to_string_lossy().to_string()),
        recorded_command: None,
        disabled: false,
        foreign: None,
        problem: None,
    };
    if let Some(text) = file.and_then(|f| std::fs::read_to_string(f).ok()) {
        match recorded(client, &text) {
            Ok(Entry::Ours { command, disabled }) => {
                s.recorded_command = Some(command);
                s.disabled = disabled;
            }
            Ok(Entry::Foreign(what)) => s.foreign = Some(what),
            Ok(Entry::None) => {}
            Err(why) => s.problem = Some(format!("{} {why}, so Daytrace can't change it.", file.map(|f| f.display().to_string()).unwrap_or_default())),
        }
    }
    if !s.installed && s.problem.is_none() {
        s.problem = Some(not_installed(client));
    }
    s
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

pub fn status(settings: &crate::config::Settings) -> AgentClients {
    let exe = mcp_exe();
    let codex_cli = codex_backend(settings).executable().is_ok();
    let clients = Client::ALL
        .into_iter()
        .map(|c| client_status(c, config_file(c, &real_env, cfg!(windows)).as_deref(), codex_cli))
        .collect();
    AgentClients { mcp_path: exe.to_string_lossy().to_string(), mcp_installed: exe.is_file(), clients }
}

// ---- actions ----

fn after_add(client: Client, updated: bool) -> String {
    match (client, updated) {
        (Client::Codex, false) => "Added to Codex. New Codex sessions in the CLI, the IDE extension and the app can read your timeline.".into(),
        (Client::Codex, true) => "Updated the daytrace server in Codex. New Codex sessions use it.".into(),
        (Client::Cursor, false) => "Added to Cursor. If Cursor is open, restart it to load the server.".into(),
        (Client::Cursor, true) => "Updated the daytrace server in Cursor. If Cursor is open, restart it.".into(),
        (Client::VsCode, false) => "Added to VS Code (default profile). VS Code asks you to trust the server the first time it starts it.".into(),
        (Client::VsCode, true) => "Updated the daytrace server in VS Code. VS Code asks you to trust it again because its settings changed.".into(),
    }
}

/// Adds or replaces the daytrace entry in `file` (the config of `client`).
pub fn add(client: Client, file: Option<&Path>, installed: bool, exe: &Path) -> anyhow::Result<String> {
    if !exe.is_file() {
        anyhow::bail!("daytrace-mcp isn't next to Daytrace ({}). Install Daytrace again to get it.", exe.display());
    }
    let Some(file) = file.filter(|_| installed) else { anyhow::bail!(not_installed(client)) };
    let existing = std::fs::read_to_string(file).ok();
    let left_alone = |why: String| anyhow::anyhow!("{} {why}, so Daytrace left it alone. Fix it or add the entry yourself, then try again.", file.display());
    let before = match &existing {
        Some(text) => recorded(client, text).map_err(left_alone)?,
        None => Entry::None,
    };
    if let Entry::Foreign(what) = &before {
        anyhow::bail!(
            "{} already has a server named “daytrace” that runs {what}. Daytrace left it unchanged: rename or remove it in {}, then try again.",
            client.name(),
            file.display()
        );
    }
    let text = with_entry(client, existing.as_deref(), &exe.to_string_lossy()).map_err(left_alone)?;
    let saved = match &existing {
        Some(_) => Some(backup(file)?),
        None => None,
    };
    write_atomic(file, &text)?;
    let mut m = after_add(client, matches!(before, Entry::Ours { .. }));
    if let Some(b) = saved {
        m.push_str(&format!(" The previous file was copied to {}.", b.display()));
    }
    Ok(m)
}

/// Removes the daytrace entry from `file` when it is Daytrace's.
pub fn remove(client: Client, file: Option<&Path>) -> anyhow::Result<String> {
    let none = || format!("{} had no daytrace server.", client.name());
    let Some(file) = file else { return Ok(none()) };
    let Ok(existing) = std::fs::read_to_string(file) else { return Ok(none()) };
    let left_alone = |why: String| anyhow::anyhow!("{} {why}, so Daytrace left it alone.", file.display());
    match recorded(client, &existing).map_err(left_alone)? {
        Entry::None => return Ok(none()),
        Entry::Foreign(what) => anyhow::bail!("The server named “daytrace” in {} runs {what}, not daytrace-mcp, so Daytrace left it.", client.name()),
        Entry::Ours { .. } => {}
    }
    let Some(text) = without_entry(client, &existing).map_err(left_alone)? else { return Ok(none()) };
    let saved = backup(file)?;
    write_atomic(file, &text)?;
    let restart = match client {
        Client::Codex => " New Codex sessions no longer have it.",
        Client::Cursor => " Restart Cursor to apply it.",
        Client::VsCode => "",
    };
    Ok(format!("Removed from {}.{restart} The previous file was copied to {}.", client.name(), saved.display()))
}

/// The file and whether the client is installed, for the add/remove commands.
pub fn target(client: Client, settings: &crate::config::Settings) -> (Option<PathBuf>, bool) {
    let file = config_file(client, &real_env, cfg!(windows));
    let codex_cli = client == Client::Codex && codex_backend(settings).executable().is_ok();
    let ok = installed(client, file.as_deref(), codex_cli);
    (file, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\me\AppData\Local\Daytrace\daytrace-mcp.exe";

    #[test]
    fn locations_follow_the_docs() {
        let env = |k: &str| match k {
            "USERPROFILE" => Some(r"C:\Users\me".to_string()),
            "APPDATA" => Some(r"C:\Users\me\AppData\Roaming".to_string()),
            _ => None,
        };
        let p = |c| config_file(c, &env, true).unwrap();
        assert_eq!(p(Client::Codex), Path::new(r"C:\Users\me").join(".codex").join("config.toml"));
        assert_eq!(p(Client::Cursor), Path::new(r"C:\Users\me").join(".cursor").join("mcp.json"));
        assert_eq!(p(Client::VsCode), Path::new(r"C:\Users\me\AppData\Roaming").join("Code").join("User").join("mcp.json"));
        let with_home = |k: &str| if k == "CODEX_HOME" { Some(r"D:\codex".to_string()) } else { env(k) };
        assert_eq!(config_file(Client::Codex, &with_home, true).unwrap(), Path::new(r"D:\codex").join("config.toml"));
        assert_eq!(Client::from_id("vsCode"), Some(Client::VsCode));
        assert_eq!(Client::from_id("claude"), None);
    }

    #[test]
    fn toml_edit_keeps_the_rest_of_the_file() {
        let before = "# my Codex settings\nmodel = \"gpt-6-luna\"  # the default\n\n[mcp_servers.files]\ncommand = \"npx\"\nargs = [\"-y\", \"server-files\"]\n\n[profiles.fast]\nmodel_reasoning_effort = \"low\"\n";
        let after = toml_with_entry(Some(before), EXE).unwrap();
        // The new table goes after the other MCP servers; nothing else moves or changes.
        assert_eq!(
            after,
            "# my Codex settings\nmodel = \"gpt-6-luna\"  # the default\n\n[mcp_servers.files]\ncommand = \"npx\"\nargs = [\"-y\", \"server-files\"]\n\n\
             [mcp_servers.daytrace]\ncommand = 'C:\\Users\\me\\AppData\\Local\\Daytrace\\daytrace-mcp.exe'\nargs = []\n\n[profiles.fast]\nmodel_reasoning_effort = \"low\"\n"
        );
        assert_eq!(toml_recorded(&after).unwrap(), Entry::Ours { command: EXE.into(), disabled: false });
        // What Codex reads back.
        let v: toml_edit::DocumentMut = after.parse().unwrap();
        assert_eq!(v["mcp_servers"]["files"]["command"].as_str(), Some("npx"));
        assert_eq!(v["profiles"]["fast"]["model_reasoning_effort"].as_str(), Some("low"));
        // Again: replaced, not duplicated; an `enabled = false` goes with the old entry.
        let off = after.replace("args = []\n", "args = []\nenabled = false\n");
        assert_eq!(toml_recorded(&off).unwrap(), Entry::Ours { command: EXE.into(), disabled: true });
        let twice = toml_with_entry(Some(&off), r"D:\daytrace-mcp.exe").unwrap();
        assert_eq!(twice.matches("[mcp_servers.daytrace]").count(), 1);
        assert!(!twice.contains("enabled"), "{twice}");
        // A new file: only the daytrace table, no empty [mcp_servers] header.
        assert_eq!(toml_with_entry(None, "x").unwrap(), "[mcp_servers.daytrace]\ncommand = \"x\"\nargs = []\n");
        // An inline table stays inline.
        let inline = toml_with_entry(Some("mcp_servers = { other = { command = \"o\" } }\n"), "x").unwrap();
        let v: toml_edit::DocumentMut = inline.parse().unwrap();
        assert_eq!(v["mcp_servers"]["daytrace"]["command"].as_str(), Some("x"));
        assert_eq!(v["mcp_servers"]["other"]["command"].as_str(), Some("o"));
        // Files Daytrace must not touch.
        assert!(toml_with_entry(Some("model = "), "x").is_err());
        assert_eq!(toml_with_entry(Some("mcp_servers = 3\n"), "x").unwrap_err(), NOT_A_TABLE);

        // Removing keeps everything else, including the comments.
        let removed = toml_without_entry(&after).unwrap().unwrap();
        assert_eq!(removed.trim_end(), before.trim_end());
        assert_eq!(toml_without_entry(before).unwrap(), None);
        assert_eq!(toml_without_entry("").unwrap(), None);
    }

    #[test]
    fn foreign_entries_are_recognized() {
        assert_eq!(toml_recorded("[mcp_servers.daytrace]\nurl = \"https://example.com/mcp\"\n").unwrap(), Entry::Foreign("https://example.com/mcp".into()));
        assert_eq!(toml_recorded("[mcp_servers.daytrace]\ncommand = \"python\"\n").unwrap(), Entry::Foreign("python".into()));
        assert!(matches!(toml_recorded("[mcp_servers.daytrace]\ncommand = '/opt/daytrace/daytrace-mcp'\n").unwrap(), Entry::Ours { .. }));
        assert_eq!(json_recorded(r#"{"servers":{"daytrace":{"type":"http","url":"http://x"}}}"#, "servers").unwrap(), Entry::Foreign("http://x".into()));
        assert!(matches!(json_recorded(r#"{"mcpServers":{"daytrace":{"command":"D:\\DAYTRACE-MCP.EXE"}}}"#, "mcpServers").unwrap(), Entry::Ours { .. }));
        assert_eq!(json_recorded("{}", "servers").unwrap(), Entry::None);
        assert!(json_recorded("// comment\n{}", "servers").unwrap_err().contains("comments"));
        assert!(json_recorded(r#"{"servers":[]}"#, "servers").is_err());
    }

    #[test]
    fn json_merge_keeps_other_servers() {
        let vscode = r#"{"servers":{"github":{"type":"http","url":"https://api.githubcopilot.com/mcp/"}},"inputs":[{"id":"token","type":"promptString"}]}"#;
        let after = json_with_entry(Some(vscode), "servers", EXE).unwrap();
        let v: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(v["servers"]["github"]["url"], "https://api.githubcopilot.com/mcp/");
        assert_eq!(v["inputs"][0]["id"], "token");
        assert_eq!(v["servers"]["daytrace"], json!({"type": "stdio", "command": EXE, "args": []}));
        assert_eq!(serde_json::from_str::<Value>(&json_with_entry(None, "mcpServers", "x").unwrap()).unwrap(), json!({"mcpServers": {"daytrace": {"type": "stdio", "command": "x", "args": []}}}));
        let removed = json_without_entry(&after, "servers").unwrap().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&removed).unwrap(), serde_json::from_str::<Value>(vscode).unwrap());
        assert_eq!(json_without_entry(vscode, "servers").unwrap(), None);
    }

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("daytrace-mcp.exe");
        std::fs::write(&exe, b"").unwrap();
        (dir, exe)
    }

    fn backups(dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains(".daytrace-backup-")).collect();
        v.sort();
        v
    }

    #[test]
    fn codex_add_update_remove_with_backups() {
        let (dir, exe) = fixture();
        let home = dir.path().join(".codex");
        let file = home.join("config.toml");
        // Not installed: no folder, no CLI.
        let s = client_status(Client::Codex, Some(&file), false);
        assert!(!s.installed && s.problem.as_deref().unwrap().starts_with("Codex isn't installed"));
        assert!(add(Client::Codex, Some(&file), false, &exe).unwrap_err().to_string().starts_with("Codex isn't installed"));
        // The CLI alone counts as installed; the file and its folder are created.
        assert!(client_status(Client::Codex, Some(&file), true).installed);
        let m = add(Client::Codex, Some(&file), true, &exe).unwrap();
        assert!(m.starts_with("Added to Codex."), "{m}");
        assert!(!m.contains("copied"), "no backup of a file that didn't exist: {m}");
        let s = client_status(Client::Codex, Some(&file), false);
        assert!(s.installed);
        assert_eq!(s.recorded_command.as_deref(), Some(exe.to_string_lossy().as_ref()));

        // An existing file with the user's own settings: backed up, kept.
        std::fs::write(&file, "model = \"gpt-6-luna\"\n").unwrap();
        let m = add(Client::Codex, Some(&file), true, &exe).unwrap();
        assert!(m.contains(".daytrace-backup-"), "{m}");
        assert_eq!(std::fs::read_to_string(&backups(&home)[0]).unwrap(), "model = \"gpt-6-luna\"\n");
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("model = \"gpt-6-luna\"\n"));
        assert!(add(Client::Codex, Some(&file), true, &exe).unwrap().starts_with("Updated the daytrace server in Codex."));
        // No temporary files are left.
        assert!(!std::fs::read_dir(&home).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".tmp")));

        let m = remove(Client::Codex, Some(&file)).unwrap();
        assert!(m.starts_with("Removed from Codex."), "{m}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "model = \"gpt-6-luna\"\n");
        assert_eq!(remove(Client::Codex, Some(&file)).unwrap(), "Codex had no daytrace server.");

        // Someone else's "daytrace" server and a broken file are left alone.
        std::fs::write(&file, "[mcp_servers.daytrace]\ncommand = \"python\"\n").unwrap();
        assert_eq!(client_status(Client::Codex, Some(&file), false).foreign.as_deref(), Some("python"));
        assert!(add(Client::Codex, Some(&file), true, &exe).unwrap_err().to_string().contains("already has a server named “daytrace” that runs python"));
        assert!(remove(Client::Codex, Some(&file)).unwrap_err().to_string().contains("not daytrace-mcp"));
        std::fs::write(&file, "model = [").unwrap();
        assert!(add(Client::Codex, Some(&file), true, &exe).unwrap_err().to_string().contains("isn't valid TOML, so Daytrace left it alone"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "model = [");
        assert!(client_status(Client::Codex, Some(&file), true).problem.unwrap().contains("isn't valid TOML"));
    }

    #[test]
    fn cursor_and_vscode_files() {
        let (dir, exe) = fixture();
        let cursor = dir.path().join(".cursor").join("mcp.json");
        assert!(!client_status(Client::Cursor, Some(&cursor), true).installed, "the Codex CLI says nothing about Cursor");
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        assert!(add(Client::Cursor, Some(&cursor), true, &exe).unwrap().starts_with("Added to Cursor."));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["daytrace"]["command"], exe.to_string_lossy().as_ref());

        let vscode = dir.path().join("Code").join("User").join("mcp.json");
        std::fs::create_dir_all(vscode.parent().unwrap()).unwrap();
        std::fs::write(&vscode, r#"{"servers":{"github":{"type":"http","url":"https://api.githubcopilot.com/mcp/"}},"inputs":[]}"#).unwrap();
        let m = add(Client::VsCode, Some(&vscode), true, &exe).unwrap();
        assert!(m.starts_with("Added to VS Code (default profile)."), "{m}");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&vscode).unwrap()).unwrap();
        assert_eq!(v["servers"]["github"]["type"], "http");
        assert_eq!(v["servers"]["daytrace"]["type"], "stdio");
        assert_eq!(backups(vscode.parent().unwrap()).len(), 1);
        // A file with comments (JSONC) is not rewritten.
        std::fs::write(&vscode, "{\n  // mine\n  \"servers\": {}\n}\n").unwrap();
        assert!(add(Client::VsCode, Some(&vscode), true, &exe).unwrap_err().to_string().contains("isn't plain JSON (it may have comments), so Daytrace left it alone"));
        assert_eq!(std::fs::read_to_string(&vscode).unwrap(), "{\n  // mine\n  \"servers\": {}\n}\n");
        // daytrace-mcp missing.
        assert!(add(Client::Cursor, Some(&cursor), true, &dir.path().join("missing.exe")).unwrap_err().to_string().contains("isn't next to Daytrace"));
        assert!(remove(Client::Cursor, Some(&cursor)).unwrap().starts_with("Removed from Cursor. Restart Cursor"));
        assert_eq!(remove(Client::VsCode, None).unwrap(), "VS Code had no daytrace server.");
    }
}
