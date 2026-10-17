import * as React from "react";
import { CircleAlert, CircleCheck, CircleDashed, Copy, CopyCheck, Plug, Unplug } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { useDelayedLoading } from "@/lib/use-delayed";
import { cn, errorText } from "@/lib/utils";
import { agentAccessApi, type AgentAccess, type ClientStatus } from "./agent-access-api";
import { ErrorBanner, FieldError, Group, OutcomeLine, Row, type Outcome } from "./fields";

const SPINNER = {
  "aria-hidden": true,
  role: undefined,
  "aria-label": undefined,
} as const;

type Action = "code-add" | "code-remove" | "desktop-add" | "desktop-remove" | "path-add" | "path-remove";

/**
 * Settings > AI tools: one-click setup of daytrace-mcp (Dayflow's Settings > MCP / CLI "Connect"
 * buttons and terminal command install): the Claude Code plugin, Claude Code, Claude Desktop and
 * the PATH entry. The manual setup below it stays for other tools.
 */
export function AgentConnect() {
  const [access, setAccess] = React.useState<AgentAccess | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState<Action | null>(null);
  const [outcomes, setOutcomes] = React.useState<Partial<Record<"code" | "desktop" | "path", Outcome>>>({});

  const load = React.useCallback(async () => {
    try {
      setAccess(await agentAccessApi.status());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  React.useEffect(() => {
    void load();
  }, [load]);
  const showSkeleton = useDelayedLoading(!access && !error);

  const rows = React.useRef<HTMLDivElement>(null);

  async function run(action: Action, row: "code" | "desktop" | "path", call: () => Promise<string>) {
    // The button pressed is disabled while it runs and may be replaced (Add → Remove); focus
    // goes back to the row's first button afterwards.
    const focusBack = () =>
      requestAnimationFrame(() => {
        if (document.activeElement && document.activeElement !== document.body) return;
        rows.current?.querySelector<HTMLButtonElement>(`#agent-row-${row} button:not(:disabled)`)?.focus();
      });
    setBusy(action);
    setOutcomes((o) => ({ ...o, [row]: undefined }));
    try {
      const text = await call();
      setOutcomes((o) => ({ ...o, [row]: { ok: true, text } }));
    } catch (e) {
      setOutcomes((o) => ({ ...o, [row]: { ok: false, text: errorText(e) } }));
    } finally {
      setBusy(null);
      await load();
      focusBack();
    }
  }

  if (error && !access) {
    return (
      <ErrorBanner
        title="Couldn’t check which AI tools are set up"
        message={error}
        action={
          <Button size="sm" variant="outline" onClick={() => void load()}>
            Try again
          </Button>
        }
      />
    );
  }
  if (!access) {
    return <div aria-busy="true">{showSkeleton && <Skeleton className="h-64 w-full rounded-lg" />}</div>;
  }

  const code = access.claudeCode;
  const desktop = access.claudeDesktop;
  const pending = (a: Action) => busy === a;
  const disabled = busy !== null || !access.mcpInstalled;

  return (
    <>
      <Group
        title="Claude Code plugin"
        description="The easiest way for Claude Code: Daytrace’s tools plus skills for a standup, a weekly review and questions about your day."
      >
        <Row
          label="Run these two commands in Claude Code"
          description="The plugin starts daytrace-mcp from your PATH (see Terminal command below), so open Claude Code after Daytrace is installed."
          stack
        >
          <CopyLine label="Add the Daytrace marketplace" text={access.pluginMarketplaceCommand} />
          <CopyLine label="Install the plugin" text={access.pluginInstallCommand} />
        </Row>
      </Group>

      <div ref={rows} className="contents">
        <Group
          title="Connect with one click"
          description="Adds daytrace-mcp to each tool’s own settings. Only the entry named “daytrace” is ever changed. To set it up yourself, copy the setup further down."
        >
          <Row
            id="agent-row-code"
            label="Claude Code"
            description={
              <>
                <ClientLine client={code} connected="Connected for all your projects" missing="Claude Code wasn’t found on this PC" mcpPath={access.mcpPath} />
                {!code.installed && code.problem && <p>{code.problem}</p>}
                <p>Runs “claude mcp add --scope user”. Not needed when you installed the plugin.</p>
              </>
            }
          >
            {code.recordedCommand ? (
              <>
                {!samePath(code.recordedCommand, access.mcpPath) && (
                  <Button
                    variant="outline"
                    aria-label="Update Claude Code to this daytrace-mcp"
                    disabled={disabled || !code.installed}
                    onClick={() => void run("code-add", "code", agentAccessApi.addToClaudeCode)}
                  >
                    {pending("code-add") ? <Spinner {...SPINNER} /> : <Plug aria-hidden />}
                    {pending("code-add") ? "Updating…" : "Update"}
                  </Button>
                )}
                <Button
                  variant="outline"
                  disabled={busy !== null || !code.installed}
                  onClick={() => void run("code-remove", "code", agentAccessApi.removeFromClaudeCode)}
                >
                  {pending("code-remove") ? <Spinner {...SPINNER} /> : <Unplug aria-hidden />}
                  {pending("code-remove") ? "Removing…" : "Remove from Claude Code"}
                </Button>
              </>
            ) : (
              <Button variant="outline" disabled={disabled || !code.installed} onClick={() => void run("code-add", "code", agentAccessApi.addToClaudeCode)}>
                {pending("code-add") ? <Spinner {...SPINNER} /> : <Plug aria-hidden />}
                {pending("code-add") ? "Adding…" : "Add to Claude Code"}
              </Button>
            )}
          </Row>
          <RowOutcome outcome={outcomes.code} />

          <Row
            id="agent-row-desktop"
            label="Claude Desktop"
            description={
              <>
                <ClientLine client={desktop} connected="Connected" missing="Claude Desktop isn’t installed for this Windows user" mcpPath={access.mcpPath} />
                <p>Adds a “daytrace” entry to Claude Desktop’s settings file and keeps a copy of the old file next to it. Restart Claude Desktop afterwards.</p>
              </>
            }
          >
            {desktop.recordedCommand ? (
              <>
                {!samePath(desktop.recordedCommand, access.mcpPath) && (
                  <Button
                    variant="outline"
                    aria-label="Update Claude Desktop to this daytrace-mcp"
                    disabled={disabled || !desktop.installed}
                    onClick={() => void run("desktop-add", "desktop", agentAccessApi.addToClaudeDesktop)}
                  >
                    {pending("desktop-add") ? <Spinner {...SPINNER} /> : <Plug aria-hidden />}
                    {pending("desktop-add") ? "Updating…" : "Update"}
                  </Button>
                )}
                <Button
                  variant="outline"
                  disabled={busy !== null}
                  onClick={() => void run("desktop-remove", "desktop", agentAccessApi.removeFromClaudeDesktop)}
                >
                  {pending("desktop-remove") ? <Spinner {...SPINNER} /> : <Unplug aria-hidden />}
                  {pending("desktop-remove") ? "Removing…" : "Remove from Claude Desktop"}
                </Button>
              </>
            ) : (
              <Button
                variant="outline"
                disabled={disabled || !desktop.installed}
                onClick={() => void run("desktop-add", "desktop", agentAccessApi.addToClaudeDesktop)}
              >
                {pending("desktop-add") ? <Spinner {...SPINNER} /> : <Plug aria-hidden />}
                {pending("desktop-add") ? "Adding…" : "Add to Claude Desktop"}
              </Button>
            )}
          </Row>
          <RowOutcome outcome={outcomes.desktop} />

          <Row
            id="agent-row-path"
            label="Terminal command"
            description={
              <>
                <StatusLine
                  tone={access.onPath ? "ok" : "off"}
                  text={access.onPath ? "daytrace-mcp is on your PATH" : access.pathSupported ? "daytrace-mcp isn’t on your PATH" : "Only on Windows"}
                />
                <p>
                  With the Daytrace folder on your PATH, <code className="font-mono text-caption">daytrace-mcp cli today</code> works in any terminal and the
                  plugin can start daytrace-mcp. The installer adds it; terminals opened afterwards pick it up.
                </p>
              </>
            }
          >
            {access.pathSupported &&
              (access.onPath ? (
                <Button variant="outline" disabled={busy !== null} onClick={() => void run("path-remove", "path", () => agentAccessApi.setOnPath(false))}>
                  {pending("path-remove") && <Spinner {...SPINNER} />}
                  {pending("path-remove") ? "Removing…" : "Remove from PATH"}
                </Button>
              ) : (
                <Button variant="outline" disabled={disabled} onClick={() => void run("path-add", "path", () => agentAccessApi.setOnPath(true))}>
                  {pending("path-add") && <Spinner {...SPINNER} />}
                  {pending("path-add") ? "Adding…" : "Add to PATH"}
                </Button>
              ))}
          </Row>
          <RowOutcome outcome={outcomes.path} />
        </Group>
      </div>
    </>
  );
}

function samePath(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

function RowOutcome({ outcome }: { outcome: Outcome | undefined }) {
  return (
    <div aria-live="polite" className="empty:hidden">
      {outcome && <OutcomeLine outcome={outcome} className="mx-4 my-3" />}
    </div>
  );
}

function StatusLine({ tone, text, detail }: { tone: "ok" | "off" | "warn"; text: string; detail?: React.ReactNode }) {
  const Icon = tone === "ok" ? CircleCheck : tone === "warn" ? CircleAlert : CircleDashed;
  return (
    <p className="flex items-start gap-1.5 text-foreground">
      <Icon
        className={cn("mt-0.5 size-3.5 shrink-0", tone === "ok" ? "text-live-foreground" : tone === "warn" ? "text-warn-foreground" : "text-muted-foreground")}
        aria-hidden
      />
      <span className="min-w-0 [overflow-wrap:anywhere]">
        {text}
        {detail}
      </span>
    </p>
  );
}

function ClientLine({ client, connected, missing, mcpPath }: { client: ClientStatus; connected: string; missing: string; mcpPath: string }) {
  if (client.recordedCommand) {
    const other = !samePath(client.recordedCommand, mcpPath);
    return (
      <StatusLine
        tone={other ? "warn" : "ok"}
        text={other ? "Connected to another copy of daytrace-mcp: " : connected}
        detail={
          other ? (
            <span className="font-mono text-caption" translate="no">
              {client.recordedCommand}
            </span>
          ) : undefined
        }
      />
    );
  }
  if (!client.installed) return <StatusLine tone="off" text={missing} />;
  return <StatusLine tone="off" text="Not connected" />;
}

/** One command with a Copy button. */
function CopyLine({ label, text }: { label: string; text: string }) {
  const [state, setState] = React.useState<"idle" | "copied" | "failed">("idle");
  React.useEffect(() => {
    if (state !== "copied") return;
    const t = setTimeout(() => setState("idle"), 2500);
    return () => clearTimeout(t);
  }, [state]);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }
  }
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <code
          aria-label={label}
          className="min-w-0 flex-1 rounded-md border bg-surface-2 px-3 py-1.5 font-mono text-caption [overflow-wrap:anywhere] select-text"
          translate="no"
        >
          {text}
        </code>
        <Button variant="outline" size="sm" onClick={() => void copy()} aria-label={`Copy: ${label}`}>
          {state === "copied" ? <CopyCheck aria-hidden /> : <Copy aria-hidden />}
          {state === "copied" ? "Copied" : "Copy"}
        </Button>
      </div>
      <div aria-live="polite" className="empty:hidden">
        {state === "copied" && <span className="sr-only">Copied to the clipboard</span>}
        {state === "failed" && <FieldError>Couldn’t copy. Select the text and press Ctrl+C.</FieldError>}
      </div>
    </div>
  );
}
