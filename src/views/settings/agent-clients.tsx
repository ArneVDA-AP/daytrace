import * as React from "react";
import { Plug, Unplug } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { samePath, StatusLine } from "./agent-connect";
import { agentClientsApi, type AgentClientId, type AgentClients, type AgentClientStatus } from "./agent-clients-api";
import { ErrorBanner, Group, OutcomeLine, Row, type Outcome } from "./fields";

const SPINNER = {
  "aria-hidden": true,
  role: undefined,
  "aria-label": undefined,
} as const;

/** What to do when the tool isn't installed. */
const INSTALL: Record<AgentClientId, string> = {
  codex: "Install the Codex CLI (npm install -g @openai/codex), the IDE extension or the app, open it once, then open this page again.",
  cursor: "Install Cursor and open it once, then open this page again.",
  vsCode: "Install VS Code and open it once, then open this page again.",
};

/** What each row says about the file it changes. */
const ABOUT: Record<AgentClientId, string> = {
  codex: "Adds a “daytrace” server to Codex’s config.toml, which the Codex CLI, the IDE extension and the app share. The rest of the file stays as it is.",
  cursor: "Adds a “daytrace” server to Cursor’s global mcp.json. Restart Cursor afterwards.",
  vsCode: "Adds a “daytrace” server to the mcp.json of VS Code’s default profile. VS Code asks you to trust the server the first time it starts it.",
};

/**
 * Settings > AI tools: one-click setup of daytrace-mcp in Codex, Cursor and VS Code (Dayflow's
 * Settings > MCP / CLI "Connect" rows for them). Each one writes the tool's own settings file
 * after a backup copy (features/agent_bridge/clients.rs).
 */
export function AgentClientsGroup() {
  const [data, setData] = React.useState<AgentClients | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState<{ id: AgentClientId; action: "add" | "remove" } | null>(null);
  const [outcomes, setOutcomes] = React.useState<Partial<Record<AgentClientId, Outcome>>>({});
  const rows = React.useRef<HTMLDivElement>(null);

  const load = React.useCallback(async () => {
    try {
      setData(await agentClientsApi.status());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  React.useEffect(() => {
    void load();
  }, [load]);
  const showSkeleton = useDelayedLoading(!data && !error);

  async function run(id: AgentClientId, action: "add" | "remove") {
    // The pressed button is disabled while it runs and may be replaced (Add → Remove); focus goes
    // back to the row's first button afterwards.
    const focusBack = () =>
      requestAnimationFrame(() => {
        if (document.activeElement && document.activeElement !== document.body) return;
        rows.current?.querySelector<HTMLButtonElement>(`#agent-client-${id} button:not(:disabled)`)?.focus();
      });
    setBusy({ id, action });
    setOutcomes((o) => ({ ...o, [id]: undefined }));
    try {
      const text = await (action === "add" ? agentClientsApi.add(id) : agentClientsApi.remove(id));
      setOutcomes((o) => ({ ...o, [id]: { ok: true, text } }));
    } catch (e) {
      setOutcomes((o) => ({ ...o, [id]: { ok: false, text: errorText(e) } }));
    } finally {
      setBusy(null);
      await load();
      focusBack();
    }
  }

  if (error && !data) {
    return (
      <ErrorBanner
        title="Couldn’t check Codex, Cursor and VS Code"
        message={error}
        action={
          <Button size="sm" variant="outline" onClick={() => void load()}>
            Try again
          </Button>
        }
      />
    );
  }
  if (!data) {
    return <div aria-busy="true">{showSkeleton && <Skeleton className="h-56 w-full rounded-lg" />}</div>;
  }

  return (
    <div ref={rows} className="contents">
      <Group
        title="Codex, Cursor and VS Code"
        description="One click adds daytrace-mcp to the tool’s settings file. Daytrace keeps a copy of the old file next to it and changes only the entry named “daytrace”."
      >
        {data.clients.map((c) => (
          <React.Fragment key={c.id}>
            <ClientRow
              client={c}
              mcpPath={data.mcpPath}
              mcpInstalled={data.mcpInstalled}
              busy={busy}
              onAdd={() => void run(c.id, "add")}
              onRemove={() => void run(c.id, "remove")}
            />
            <div aria-live="polite" className="empty:hidden">
              {outcomes[c.id] && <OutcomeLine outcome={outcomes[c.id] ?? null} className="mx-4 my-3" />}
            </div>
          </React.Fragment>
        ))}
      </Group>
    </div>
  );
}

function ClientRow({
  client: c,
  mcpPath,
  mcpInstalled,
  busy,
  onAdd,
  onRemove,
}: {
  client: AgentClientStatus;
  mcpPath: string;
  mcpInstalled: boolean;
  busy: { id: AgentClientId; action: "add" | "remove" } | null;
  onAdd: () => void;
  onRemove: () => void;
}) {
  const pending = (action: "add" | "remove") => busy?.id === c.id && busy.action === action;
  const blocked = busy !== null;
  // A file Daytrace can't parse, or someone else's "daytrace" server: nothing to press.
  const unchangeable = Boolean(c.foreign) || (c.installed && Boolean(c.problem));
  const canAdd = mcpInstalled && c.installed && !unchangeable;
  const stale = c.recordedCommand !== null && !samePath(c.recordedCommand, mcpPath);

  return (
    <Row
      id={`agent-client-${c.id}`}
      label={c.name}
      description={
        <>
          <ClientState client={c} stale={stale} />
          {!c.installed ? <p>{INSTALL[c.id]}</p> : c.problem && <p>{c.problem}</p>}
          {c.foreign && <p>Rename or remove that entry in the file below to set up Daytrace’s own.</p>}
          <p>{ABOUT[c.id]}</p>
          {c.configPath && c.installed && (
            <p>
              File:{" "}
              <span className="font-mono text-caption [overflow-wrap:anywhere]" translate="no">
                {c.configPath}
              </span>
            </p>
          )}
        </>
      }
    >
      {c.recordedCommand ? (
        <>
          {(stale || c.disabled) && (
            <Button
              variant="outline"
              aria-label={c.disabled ? `Turn daytrace back on in ${c.name}` : `Update ${c.name} to this daytrace-mcp`}
              disabled={blocked || !canAdd}
              onClick={onAdd}
            >
              {pending("add") ? <Spinner {...SPINNER} /> : <Plug aria-hidden />}
              {pending("add") ? "Updating…" : c.disabled ? "Turn on" : "Update"}
            </Button>
          )}
          <Button variant="outline" disabled={blocked || unchangeable} onClick={onRemove}>
            {pending("remove") ? <Spinner {...SPINNER} /> : <Unplug aria-hidden />}
            {pending("remove") ? "Removing…" : `Remove from ${c.name}`}
          </Button>
        </>
      ) : (
        <Button variant="outline" disabled={blocked || !canAdd} onClick={onAdd}>
          {pending("add") ? <Spinner {...SPINNER} /> : <Plug aria-hidden />}
          {pending("add") ? "Adding…" : `Add to ${c.name}`}
        </Button>
      )}
    </Row>
  );
}

function ClientState({ client: c, stale }: { client: AgentClientStatus; stale: boolean }) {
  if (c.foreign) {
    return (
      <StatusLine
        tone="warn"
        text="Has another server named “daytrace”: "
        detail={
          <span className="font-mono text-caption" translate="no">
            {c.foreign}
          </span>
        }
      />
    );
  }
  if (c.recordedCommand) {
    if (c.disabled) return <StatusLine tone="warn" text={`Added, but turned off in ${c.name}’s settings`} />;
    if (stale) {
      return (
        <StatusLine
          tone="warn"
          text="Connected to another copy of daytrace-mcp: "
          detail={
            <span className="font-mono text-caption" translate="no">
              {c.recordedCommand}
            </span>
          }
        />
      );
    }
    return <StatusLine tone="ok" text="Connected" />;
  }
  if (!c.installed) return <StatusLine tone="off" text={`${c.name} isn’t installed for this Windows user`} />;
  if (c.problem) return <StatusLine tone="warn" text="Daytrace can’t change its settings file" />;
  return <StatusLine tone="off" text="Not connected" />;
}
