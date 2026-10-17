//! Agent access: the `daytrace-mcp` program (src/bin/daytrace-mcp.rs) and the app side of it.
//! Ported from Dayflow's tools/dayflow-cli, plugins/dayflow and Core/AgentAccess (MIT,
//! (c) 2025 Jerry Liu).
//!
//! `daytrace-mcp` with no command is an MCP server on stdio for Claude Code, Claude Desktop and
//! other MCP clients; with `cli <command>` it prints the timeline in the terminal. Reads open the
//! app's database read-only and work while Daytrace is closed. Writes (categories, activities,
//! day goals) go to the running app over the bridge (bridge.rs, writes.rs), only when "Allow
//! edits" is on.
//!
//! In the app: the bridge server (`spawn`) and the Settings > AI tools commands (PATH, one-click
//! setup of Claude Code and Claude Desktop).

pub mod bridge;
pub mod cli;
pub mod cli_write;
pub mod mcp;
pub mod path;
pub mod queries;
pub mod register;
pub mod writes;

use crate::state::AppState;
use std::sync::Arc;

/// Tables owned by this feature: none.
pub const SCHEMA: &str = "";

/// Entry point of `daytrace-mcp`; returns the process exit code.
pub fn main_entry(args: &[String]) -> i32 {
    use std::io::IsTerminal;
    let inv = match cli::parse(args) {
        Ok(i) => i,
        Err(m) => {
            eprintln!("{m}");
            return 2;
        }
    };
    let env_dir = std::env::var("DAYTRACE_DATA_DIR").ok();
    let source = queries::Source::resolve(inv.db.as_deref(), env_dir.as_deref());
    if inv.command == cli::Command::Serve {
        let stdin = std::io::stdin();
        if stdin.is_terminal() {
            // stdout is reserved for protocol frames.
            eprintln!("daytrace-mcp is an MCP server and waits for JSON-RPC on stdin. Run `daytrace-mcp help` for the command line.");
        }
        let mut server = mcp::Server::new(source);
        server.chat = inv.chat;
        return match server.serve(stdin.lock(), std::io::stdout().lock()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("daytrace-mcp: {e}");
                1
            }
        };
    }
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    cli::run(&inv, &source, &mut stdout.lock(), &mut stderr.lock())
}

// ---- in the app ----

/// Starts the write bridge. It runs while the app runs and refuses everything while "Allow
/// edits" is off.
pub fn spawn(app: tauri::AppHandle, state: Arc<AppState>) {
    use tauri::Emitter;
    let root = state.paths.root.clone();
    let handler: bridge::Handler = Arc::new(move |op, args| {
        let (data, changed) = writes::handle(&state, op, args)?;
        if changed == writes::Changed::Settings {
            // The window reloads the settings (categories) on this event.
            let _ = app.emit("settings-changed", ());
        }
        Ok(data)
    });
    bridge::spawn(root, handler);
}

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// Settings > AI tools: what is set up where.
#[tauri::command]
pub async fn agent_access_status(state: S<'_>) -> CmdResult<register::AgentAccess> {
    let settings = state.settings();
    tauri::async_runtime::spawn_blocking(move || register::status(&settings)).await.map_err(err)
}

#[tauri::command]
pub async fn agent_add_to_claude_code(state: S<'_>) -> CmdResult<String> {
    let backend = register::claude_backend(&state.settings());
    register::add_to_claude_code(&backend, &register::mcp_exe()).await.map_err(err)
}

#[tauri::command]
pub async fn agent_remove_from_claude_code(state: S<'_>) -> CmdResult<String> {
    let backend = register::claude_backend(&state.settings());
    register::remove_from_claude_code(&backend).await.map_err(err)
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

#[tauri::command]
pub async fn agent_add_to_claude_desktop() -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(|| register::add_to_claude_desktop(&register::desktop_targets(&real_env), &register::mcp_exe()))
        .await
        .map_err(err)?
        .map_err(err)
}

#[tauri::command]
pub async fn agent_remove_from_claude_desktop() -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(|| register::remove_from_claude_desktop(&register::desktop_targets(&real_env))).await.map_err(err)?.map_err(err)
}

/// Adds (`add`) or removes the Daytrace folder from the user's PATH.
#[tauri::command]
pub async fn agent_set_on_path(add: bool) -> CmdResult<String> {
    let dir = path::install_dir().ok_or("Couldn't find the Daytrace folder.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let change = if add { path::link(&dir) } else { path::unlink(&dir) }.map_err(err)?;
        Ok(match change {
            path::Change::Added => "Added to PATH. Terminals and programs you open from now on find daytrace-mcp.".into(),
            path::Change::Removed => "Removed from PATH.".into(),
            path::Change::Unchanged if add => "The Daytrace folder is already on PATH.".into(),
            path::Change::Unchanged => "The Daytrace folder wasn't on PATH.".into(),
        })
    })
    .await
    .map_err(err)?
}
