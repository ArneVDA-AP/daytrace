//! Agent access: the `daytrace-mcp` program (src/bin/daytrace-mcp.rs). With no command it is an
//! MCP server on stdio for Claude Code, Claude Desktop and other MCP clients; with `cli <command>`
//! it prints the timeline in the terminal. It opens the app's database read-only and works while
//! Daytrace is closed. Ported from Dayflow's tools/dayflow-cli and plugins/dayflow (MIT,
//! (c) 2025 Jerry Liu).
//!
//! It owns no tables and no Tauri commands; this code runs in its own process, never in the app.

pub mod cli;
pub mod mcp;
pub mod queries;

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
        let server = mcp::Server::new(source);
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
