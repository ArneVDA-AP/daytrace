import * as React from "react";
import { Copy, CopyCheck } from "lucide-react";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { AgentClientsGroup } from "./agent-clients";
import { AgentConnect } from "./agent-connect";
import { AgentEdits } from "./agent-edits";
import { ErrorBanner, FieldError, Group, Note, Row } from "./fields";
import { storageApi, type AgentSetup } from "./storage-api";

/** Settings > AI tools: the setup text for daytrace-mcp (Dayflow's MCP / CLI tab). */
export function AiToolsSection() {
  const [setup, setSetup] = React.useState<AgentSetup | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const load = React.useCallback(() => {
    setError(null);
    storageApi.agentSetup().then(setSetup, (e) => setError(errorText(e)));
  }, []);
  React.useEffect(load, [load]);
  const showSkeleton = useDelayedLoading(!setup && !error);

  return (
    <>
      <Note>
        daytrace-mcp lets Claude Code, Claude Desktop and other MCP clients read your timeline: activities, time per category, daily recaps and the notes
        behind each activity. Reading works while Daytrace is closed and never sees screenshots or the names of private apps. Changes are off unless you
        allow them below.
      </Note>
      <AgentConnect />
      <AgentClientsGroup />
      <AgentEdits />
      {error && (
        <ErrorBanner
          title="Couldn’t find daytrace-mcp"
          message={error}
          action={
            <Button size="sm" variant="outline" onClick={load}>
              Try again
            </Button>
          }
        />
      )}
      {!setup && !error && (
        <div className="flex flex-col gap-3" aria-busy="true">
          {showSkeleton && (
            <>
              <Skeleton className="h-24 w-full rounded-lg" />
              <Skeleton className="h-40 w-full rounded-lg" />
            </>
          )}
        </div>
      )}
      {setup && (
        <>
          {!setup.installed && (
            <Banner tone="warn" title="daytrace-mcp is missing">
              It was not found next to Daytrace ({setup.exePath}). The Daytrace installer puts it there; install Daytrace again to get it.
            </Banner>
          )}
          {/* The manual setup under one heading, so it does not read as a second list of the
              one-click rows above (U6). One-click rows for more tools go above this group. */}
          <Group
            title="Set it up yourself"
            description="The same setup as the buttons above, to copy into a tool yourself, and what any other MCP client needs."
          >
            <Row label="Claude Code: run this in a terminal" stack>
              <CopyBlock label="Command for Claude Code" text={setup.claudeCodeCommand} />
            </Row>
            <Row
              label={
                <>
                  Claude Desktop: add this to <span className="font-mono text-meta font-normal [overflow-wrap:anywhere]">{setup.desktopConfigPath}</span>
                </>
              }
              description="If the file already has “mcpServers”, add only the “daytrace” entry to it. Then restart Claude Desktop."
              stack
            >
              <CopyBlock label="Configuration for Claude Desktop" text={setup.desktopConfig} multiline />
            </Row>
            <Row
              label="Other MCP clients"
              description="Add a local (stdio) server named daytrace that runs this program, with no arguments."
              stack
            >
              <CopyBlock label="Program for other MCP clients" text={setup.exePath} />
            </Row>
          </Group>
          <Group title="In a terminal">
            <Row
              label="Read your timeline as text"
              description={
                <>
                  <p>
                    <code className="font-mono text-caption">daytrace-mcp cli today</code> prints today’s timeline and{" "}
                    <code className="font-mono text-caption">daytrace-mcp help</code> lists every command.
                  </p>
                  <p>
                    The program is in the Daytrace folder: <span className="font-mono text-caption [overflow-wrap:anywhere]">{setup.exePath}</span>
                  </p>
                </>
              }
            />
          </Group>
        </>
      )}
    </>
  );
}

function CopyBlock({ label, text, multiline }: { label: string; text: string; multiline?: boolean }) {
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
    <div className="flex w-full flex-col gap-2">
      <div className="relative">
        <pre
          aria-label={label}
          tabIndex={0}
          className="scroll-thin max-h-72 overflow-auto rounded-md border bg-surface-2 py-2.5 pr-24 pl-3 font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere] select-text focus-visible:-outline-offset-2"
        >
          {multiline ? text : text.trim()}
        </pre>
        <Button variant="outline" size="sm" className="absolute top-1.5 right-1.5" onClick={() => void copy()} aria-label={`Copy: ${label}`}>
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
