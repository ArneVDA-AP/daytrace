import { invoke } from "@tauri-apps/api/core";

export type AgentClientId = "codex" | "cursor" | "vsCode";

/** One of Codex, Cursor and VS Code (features/agent_bridge/clients.rs ClientEntryStatus). */
export interface AgentClientStatus {
  id: AgentClientId;
  /** "Codex", "Cursor" or "VS Code". */
  name: string;
  /** Installed for this Windows user (Codex: its CLI was found or its folder exists). */
  installed: boolean;
  /** The settings file Daytrace edits. */
  configPath: string | null;
  /** The command of the daytrace entry, when it runs daytrace-mcp. */
  recordedCommand: string | null;
  /** Codex: the entry is there but turned off (`enabled = false`). */
  disabled: boolean;
  /** An entry named "daytrace" that runs something else: what it runs. Daytrace leaves it. */
  foreign: string | null;
  /** Why it can't be set up from here (not installed, a file Daytrace can't read). */
  problem: string | null;
}

/** Settings > AI tools: Codex, Cursor and VS Code (agent_clients_status). */
export interface AgentClients {
  mcpPath: string;
  mcpInstalled: boolean;
  clients: AgentClientStatus[];
}

/** Each action resolves to a plain-words result and rejects with the error text. */
export const agentClientsApi = {
  status: () => invoke<AgentClients>("agent_clients_status"),
  add: (client: AgentClientId) => invoke<string>("agent_client_add", { client }),
  remove: (client: AgentClientId) => invoke<string>("agent_client_remove", { client }),
};
