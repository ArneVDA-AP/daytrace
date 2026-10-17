//! `daytrace-mcp`: Daytrace's MCP server (stdio) and command line. Console program; it reads the
//! app's database read-only and works while Daytrace is closed. See features/agent_bridge.rs.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(daytrace_lib::features::agent_bridge::main_entry(&args));
}
