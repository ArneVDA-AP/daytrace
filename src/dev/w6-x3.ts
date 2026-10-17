/*
 * DEV ONLY: mocks for Settings > AI tools, Codex, Cursor and VS Code (agent_clients_status,
 * agent_client_add, agent_client_remove; w6-x3). Synthetic paths for an invented user "Preview".
 * Variants, as a query parameter before the hash:
 *   ?clients=mixed   Codex added but turned off, Cursor has someone else's "daytrace" server,
 *                    VS Code's mcp.json has comments (Daytrace can't change it)
 *   ?clients=none    none of the three installed
 *   ?clients=fail    every action fails
 *   ?clients=error   the status itself can't be read
 * Default: Codex found and not connected, Cursor connected, VS Code installed and not connected.
 */
import type { AgentClientId, AgentClients, AgentClientStatus } from "@/views/settings/agent-clients-api";

type Args = Record<string, unknown>;

const MCP = "C:\\Users\\Preview\\AppData\\Local\\Daytrace\\daytrace-mcp.exe";
const FILES: Record<AgentClientId, string> = {
  codex: "C:\\Users\\Preview\\.codex\\config.toml",
  cursor: "C:\\Users\\Preview\\.cursor\\mcp.json",
  vsCode: "C:\\Users\\Preview\\AppData\\Roaming\\Code\\User\\mcp.json",
};
const NAMES: Record<AgentClientId, string> = { codex: "Codex", cursor: "Cursor", vsCode: "VS Code" };
const BACKUP = "daytrace-backup-20261016-174200";

export function applyAgentClientsMock(handlers: Record<string, (a: Args) => unknown>, ctx: { q: URLSearchParams }) {
  const v = ctx.q.get("clients") ?? "";
  const fail = v === "fail";
  const none = v === "none";
  const mixed = v === "mixed";
  const state: Record<AgentClientId, { command: string | null; disabled: boolean }> = {
    codex: { command: mixed ? MCP : null, disabled: mixed },
    cursor: { command: !mixed && !none ? MCP : null, disabled: false },
    vsCode: { command: null, disabled: false },
  };

  const one = (id: AgentClientId): AgentClientStatus => {
    const installed = !none;
    const foreign = mixed && id === "cursor" ? "npx -y daytrace-community-server" : null;
    const problem = !installed
      ? id === "codex"
        ? "Codex isn't installed for this Windows user. Install the Codex CLI (npm install -g @openai/codex), the IDE extension or the app, open it once, then try again."
        : `${NAMES[id]} isn't installed for this Windows user. Install it, open it once, then try again.`
      : mixed && id === "vsCode"
        ? `${FILES.vsCode} isn't plain JSON (it may have comments), so Daytrace can't change it.`
        : null;
    return {
      id,
      name: NAMES[id],
      installed,
      configPath: FILES[id],
      recordedCommand: state[id].command,
      disabled: state[id].disabled,
      foreign,
      problem,
    };
  };

  const status = (): AgentClients => {
    if (v === "error") throw `Access is denied. (os error 5) while reading ${FILES.codex}`;
    return { mcpPath: MCP, mcpInstalled: true, clients: (["codex", "cursor", "vsCode"] as const).map(one) };
  };

  const added = (id: AgentClientId, updated: boolean) => {
    const copy = ` The previous file was copied to ${FILES[id]}.${BACKUP}.`;
    switch (id) {
      case "codex":
        return updated
          ? `Updated the daytrace server in Codex. New Codex sessions use it.${copy}`
          : `Added to Codex. New Codex sessions in the CLI, the IDE extension and the app can read your timeline.${copy}`;
      case "cursor":
        return updated ? `Updated the daytrace server in Cursor. If Cursor is open, restart it.${copy}` : `Added to Cursor. If Cursor is open, restart it to load the server.${copy}`;
      case "vsCode":
        return updated
          ? `Updated the daytrace server in VS Code. VS Code asks you to trust it again because its settings changed.${copy}`
          : `Added to VS Code (default profile). VS Code asks you to trust the server the first time it starts it.${copy}`;
    }
  };

  Object.assign(handlers, {
    agent_clients_status: () => status(),
    agent_client_add: (a: Args) => {
      const id = a.client as AgentClientId;
      if (fail) throw `${FILES[id]} isn't valid ${id === "codex" ? "TOML" : "JSON"}, so Daytrace left it alone. Fix it or add the entry yourself, then try again.`;
      const updated = state[id].command !== null;
      state[id] = { command: MCP, disabled: false };
      return added(id, updated);
    },
    agent_client_remove: (a: Args) => {
      const id = a.client as AgentClientId;
      if (fail) throw `Access is denied. (os error 5) while writing ${FILES[id]}`;
      state[id] = { command: null, disabled: false };
      const restart = id === "codex" ? " New Codex sessions no longer have it." : id === "cursor" ? " Restart Cursor to apply it." : "";
      return `Removed from ${NAMES[id]}.${restart} The previous file was copied to ${FILES[id]}.${BACKUP}.`;
    },
  });
}
