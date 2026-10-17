import { invoke } from "@tauri-apps/api/core";

/** One AI tool daytrace-mcp can be set up in (features/agent_bridge/register.rs ClientStatus). */
export interface ClientStatus {
  /** Claude Code: its CLI was found. Claude Desktop: it is installed for this user. */
  installed: boolean;
  /** Where the CLI or the config file is. */
  location: string | null;
  /** The command of the daytrace entry, when one is set up. */
  recordedCommand: string | null;
  /** Why it can't be set up from here. */
  problem: string | null;
}

/** Settings > AI tools: what is set up where (agent_access_status). */
export interface AgentAccess {
  mcpPath: string;
  mcpInstalled: boolean;
  /** The folder that goes on PATH. */
  installDir: string;
  onPath: boolean;
  /** PATH can be changed from here (Windows only). */
  pathSupported: boolean;
  claudeCode: ClientStatus;
  claudeDesktop: ClientStatus;
  pluginMarketplaceCommand: string;
  pluginInstallCommand: string;
}

/** Each action resolves to a plain-words result and rejects with the error text. */
export const agentAccessApi = {
  status: () => invoke<AgentAccess>("agent_access_status"),
  addToClaudeCode: () => invoke<string>("agent_add_to_claude_code"),
  removeFromClaudeCode: () => invoke<string>("agent_remove_from_claude_code"),
  addToClaudeDesktop: () => invoke<string>("agent_add_to_claude_desktop"),
  removeFromClaudeDesktop: () => invoke<string>("agent_remove_from_claude_desktop"),
  setOnPath: (add: boolean) => invoke<string>("agent_set_on_path", { add }),
};
