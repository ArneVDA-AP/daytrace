/*
 * DEV ONLY: mocks for Settings > AI tools one-click setup (agent_* commands, w4-f3). Synthetic
 * paths for an invented user "Preview". Variants, as a query parameter before the hash:
 *   ?agent=connected   Claude Code and Claude Desktop connected, daytrace-mcp on PATH, edits on
 *   ?agent=stale       both connected to another copy of daytrace-mcp
 *   ?agent=none        Claude Code not found, Claude Desktop not installed, not on PATH
 *   ?agent=fail        every action fails (Claude Code refuses, a broken config file)
 *   ?agent=error       the status itself can't be read
 * Default: nothing connected yet, Claude Code found, Claude Desktop installed, on PATH.
 */
import type { Settings } from "@/api";
import type { AgentAccess } from "@/views/settings/agent-access-api";

type Args = Record<string, unknown>;

const DIR = "C:\\Users\\Preview\\AppData\\Local\\Daytrace";
const MCP = `${DIR}\\daytrace-mcp.exe`;

export function applyAgentAccessMock(handlers: Record<string, (a: Args) => unknown>, ctx: { q: URLSearchParams; settings: Settings }) {
  const v = ctx.q.get("agent") ?? "";
  const fail = v === "fail";
  const state = {
    code: v === "connected" ? MCP : v === "stale" ? "D:\\Old\\Daytrace\\daytrace-mcp.exe" : null,
    desktop: v === "connected" ? MCP : v === "stale" ? "D:\\Old\\Daytrace\\daytrace-mcp.exe" : null,
    onPath: v !== "none",
  };
  if (v === "connected") ctx.settings.allowAgentEdits = true;
  const codeFound = v !== "none";
  const desktopInstalled = v !== "none";

  const status = (): AgentAccess => {
    if (v === "error") throw "Access is denied. (os error 5) while reading C:\\Users\\Preview\\.claude.json";
    return {
      mcpPath: MCP,
      mcpInstalled: true,
      installDir: DIR,
      onPath: state.onPath,
      pathSupported: true,
      claudeCode: {
        installed: codeFound,
        location: codeFound ? "C:\\Users\\Preview\\.local\\bin\\claude.exe" : null,
        recordedCommand: state.code,
        problem: codeFound
          ? null
          : "Claude Code CLI not found. Install it (in PowerShell: irm https://claude.ai/install.ps1 | iex), sign in once in a terminal, or set its path in Settings.",
      },
      claudeDesktop: {
        installed: desktopInstalled,
        location: desktopInstalled ? "C:\\Users\\Preview\\AppData\\Roaming\\Claude\\claude_desktop_config.json" : null,
        recordedCommand: state.desktop,
        problem: desktopInstalled ? null : "Claude Desktop isn't installed for this Windows user.",
      },
      pluginMarketplaceCommand: "/plugin marketplace add ArneVDA-AP/daytrace",
      pluginInstallCommand: "/plugin install daytrace@daytrace",
    };
  };

  // The manual setup text as the backend writes it (features/storage.rs agent_setup_for).
  const setup = handlers.storage_agent_setup;
  handlers.storage_agent_setup = (a) => ({
    ...(setup(a) as object),
    exePath: MCP,
    claudeCodeCommand: `claude mcp add --scope user --transport stdio daytrace -- "${MCP}"`,
    desktopConfig: JSON.stringify({ mcpServers: { daytrace: { command: MCP, args: [] } } }, null, 2),
  });

  Object.assign(handlers, {
    agent_access_status: () => status(),
    agent_add_to_claude_code: () => {
      if (fail) throw "Claude Code didn't add the server: error: unknown option '--transport'";
      const updated = state.code !== null;
      state.code = MCP;
      return updated
        ? "Updated the daytrace server in Claude Code. New Claude Code sessions use it."
        : "Added to Claude Code for all your projects. New Claude Code sessions can read your timeline.";
    },
    agent_remove_from_claude_code: () => {
      state.code = null;
      return "Removed from Claude Code.";
    },
    agent_add_to_claude_desktop: () => {
      if (fail)
        throw "C:\\Users\\Preview\\AppData\\Roaming\\Claude\\claude_desktop_config.json isn't valid JSON, so Daytrace left it alone. Fix or remove it in Claude Desktop (Settings > Developer > Edit Config), then try again.";
      state.desktop = MCP;
      return "Added to Claude Desktop. Quit Claude Desktop from its tray icon and open it again to load it. The previous file was copied to C:\\Users\\Preview\\AppData\\Roaming\\Claude\\claude_desktop_config.json.daytrace-backup-20261016-174200.";
    },
    agent_remove_from_claude_desktop: () => {
      state.desktop = null;
      return "Removed from Claude Desktop. Restart Claude Desktop to apply it.";
    },
    agent_set_on_path: (a: Args) => {
      state.onPath = Boolean(a.add);
      return a.add ? "Added to PATH. Terminals and programs you open from now on find daytrace-mcp." : "Removed from PATH.";
    },
  });
}
