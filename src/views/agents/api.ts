import { invoke } from "@tauri-apps/api/core";

/** Where node and npx were found (features/agents.rs NodeInfo). */
export interface NodeInfo {
  /** As `node --version` prints it ("v22.11.0"). */
  version: string;
  nodePath: string;
  npxPath: string;
}

/** A folder of session logs the dashboard reads. */
export interface AgentSource {
  /** "Claude Code" or "Codex". */
  agent: string;
  path: string;
  exists: boolean;
}

export type AgentsRun =
  | { state: "notInstalled"; reason: string }
  | { state: "stopped" }
  | { state: "starting"; since: number }
  | { state: "running"; url: string; since: number }
  | { state: "failed"; title: string; message: string; log: string[] };

export type AgentsStatus = AgentsRun & {
  node: NodeInfo | null;
  /** The pinned AgentPlayback version. */
  version: string;
  /** AGENTPLAYBACK_HOME: scan results, scan cache and the price list. */
  dataDir: string;
  sources: AgentSource[];
};

export const agentsApi = {
  /** The section's state; looks for Node.js the first time. */
  status: () => invoke<AgentsStatus>("agents_status"),
  /** Looks for Node.js again. */
  recheck: () => invoke<AgentsStatus>("agents_recheck"),
  /** Starts the dashboard; returns at once with "starting" (or "failed"). */
  start: () => invoke<AgentsStatus>("agents_start"),
  stop: () => invoke<AgentsStatus>("agents_stop"),
  openInBrowser: () => invoke<void>("agents_open_in_browser"),
};

/**
 * Only the loopback dashboard is ever put in the frame: http://127.0.0.1:<port>/. (The browser
 * preview's placeholder is served by the dev server itself.)
 */
export function frameUrl(url: string): string | null {
  try {
    const u = new URL(url);
    if (u.protocol !== "http:" || !u.port) return null;
    if (u.hostname === "127.0.0.1" && u.pathname === "/") return u.href;
    if (import.meta.env.DEV && u.host === location.host) return u.href;
    return null;
  } catch {
    return null;
  }
}
