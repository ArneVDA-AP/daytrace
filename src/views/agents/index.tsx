import * as React from "react";
import { ExternalLink, Play, RefreshCw, RotateCcw, Square } from "lucide-react";
import { shellApi } from "@/app/shell-api";
import { Banner } from "@/components/banner";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { agentsApi, frameUrl, type AgentsStatus } from "@/views/agents/api";
import { AgentsFrame, agentsFrame } from "@/views/agents/frame-host";

/*
 * Agents: Dayflow's Agents tab (Views/UI/Agents/AgentPlaybackView.swift). Start runs the
 * `agentplayback` npm package on this PC (features/agents.rs) and the section shows its local
 * dashboard of Claude Code and Codex sessions in a frame. The process belongs to the backend, so
 * it keeps running while you switch sections; it stops with Stop or when Daytrace quits.
 *
 * The frame holds a page from http://127.0.0.1:<port>, another origin than Daytrace's. Tauri
 * rejects IPC calls from it (remote origin, no capability for it), and the sandbox keeps it from
 * navigating Daytrace itself. Its links that open a new window go to the backend's new-window
 * handler, which opens web links in the default browser (tray.rs, features/agents.rs). The frame
 * itself lives in the shell (frame-host.tsx), so switching sections does not reload it.
 */

const POLL_STARTING_MS = 1000;
const POLL_RUNNING_MS = 5000;

type Busy = "start" | "stop" | "recheck" | "open" | null;

function same(a: AgentsStatus | null, b: AgentsStatus): boolean {
  return a !== null && JSON.stringify(a) === JSON.stringify(b);
}

export default function AgentsSection() {
  const [status, setStatus] = React.useState<AgentsStatus | null>(null);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState<Busy>(null);
  const [actionError, setActionError] = React.useState<{ title: string; message: string } | null>(null);

  const apply = React.useCallback((next: AgentsStatus) => {
    setStatus((prev) => (same(prev, next) ? prev : next));
    setLoadError(null);
  }, []);

  const load = React.useCallback(async () => {
    try {
      apply(await agentsApi.status());
    } catch (e) {
      setLoadError(errorText(e));
    }
  }, [apply]);

  React.useEffect(() => {
    void load();
  }, [load]);

  // While it starts, look every second for the URL; while it runs, notice when it stops.
  const state = status?.state;
  React.useEffect(() => {
    if (state !== "starting" && state !== "running") return;
    const t = setInterval(() => void load(), state === "starting" ? POLL_STARTING_MS : POLL_RUNNING_MS);
    return () => clearInterval(t);
  }, [state, load]);

  const act = async (kind: Exclude<Busy, null>, run: () => Promise<AgentsStatus | void>, failTitle: string) => {
    setBusy(kind);
    setActionError(null);
    try {
      const next = await run();
      if (next) apply(next);
    } catch (e) {
      setActionError({ title: failTitle, message: errorText(e) });
    } finally {
      setBusy(null);
    }
  };
  const start = () => act("start", agentsApi.start, "AgentPlayback didn’t start");
  const stop = () => act("stop", agentsApi.stop, "AgentPlayback didn’t stop");
  const recheck = () => act("recheck", agentsApi.recheck, "Daytrace couldn’t look for Node.js");
  const openInBrowser = () => act("open", agentsApi.openInBrowser, "The browser didn’t open");

  const showSkeleton = useDelayedLoading(status === null && loadError === null);
  const running = status?.state === "running" ? status : null;

  // The frame stays loaded across section switches while the dashboard runs; once it is known
  // to have stopped, it goes.
  React.useEffect(() => {
    if (status && status.state !== "running") agentsFrame.drop();
  }, [status]);

  return (
    <>
      <PageHeader
        title="Agents"
        badge={running ? <TitleBadge>Running</TitleBadge> : undefined}
        actions={
          running && (
            <>
              <Button variant="outline" size="sm" onClick={openInBrowser} disabled={busy !== null}>
                <ExternalLink aria-hidden="true" /> Open in browser
              </Button>
              <Button variant="outline" size="sm" onClick={stop} disabled={busy !== null}>
                <Square aria-hidden="true" /> Stop
              </Button>
            </>
          )
        }
      />
      <Announcer status={status} />
      {actionError && (
        <div className="px-(--page-x) pt-4">
          <Banner tone="error" title={actionError.title}>
            {actionError.message}
          </Banner>
        </div>
      )}
      {loadError ? (
        <div className="px-(--page-x) py-5">
          <Banner tone="error" title="Daytrace couldn’t read the state of AgentPlayback" details={loadError} actions={<Button onClick={() => void load()}>Try again</Button>}>
            The section can’t show whether the dashboard is running.
          </Banner>
        </div>
      ) : status === null ? (
        showSkeleton ? <IntroSkeleton /> : null
      ) : running ? (
        <Dashboard key={running.url} url={running.url} />
      ) : (
        <Intro status={status} busy={busy} onStart={start} onStop={stop} onRecheck={recheck} />
      )}
    </>
  );
}

/** Says the state out loud when it changes by itself (starting → running); failures are alerts. */
function Announcer({ status }: { status: AgentsStatus | null }) {
  const text =
    status?.state === "starting"
      ? "Starting AgentPlayback…"
      : status?.state === "running"
        ? "AgentPlayback is running."
        : "";
  return (
    <p role="status" className="sr-only">
      {text}
    </p>
  );
}

function Dashboard({ url }: { url: string }) {
  if (!frameUrl(url)) {
    return (
      <div className="px-(--page-x) py-5">
        <Banner tone="error" title="Daytrace won’t show this address" details={url}>
          The dashboard reported an address that isn’t on this PC. Stop it and start it again.
        </Banner>
      </div>
    );
  }
  return <AgentsFrame url={url} />;
}

function IntroSkeleton() {
  return (
    <div className="flex max-w-2xl flex-col gap-4 px-(--page-x) py-5" aria-hidden="true">
      <Skeleton className="h-5 w-4/5" />
      <Skeleton className="h-5 w-3/5" />
      <Skeleton className="h-8 w-24" />
      <Skeleton className="mt-4 h-24 w-full" />
    </div>
  );
}

function Intro({
  status,
  busy,
  onStart,
  onStop,
  onRecheck,
}: {
  status: AgentsStatus;
  busy: Busy;
  onStart: () => void;
  onStop: () => void;
  onRecheck: () => void;
}) {
  return (
    <div className="scroll-thin min-h-0 flex-1 overflow-y-auto">
      <div className="flex max-w-2xl flex-col gap-6 px-(--page-x) py-5">
        {status.state === "notInstalled" && (
          <Banner
            tone="warn"
            title="Node.js is needed"
            actions={
              <>
                <Button onClick={onRecheck} disabled={busy !== null}>
                  {busy === "recheck" ? <Spinner aria-hidden="true" role={undefined} aria-label={undefined} /> : <RefreshCw aria-hidden="true" />} Re-check
                </Button>
                <Button variant="ghost" onClick={() => void shellApi.openLink("nodejs").catch(() => undefined)}>
                  <ExternalLink aria-hidden="true" /> Open nodejs.org
                </Button>
              </>
            }
          >
            <p>{status.reason}</p>
            <p className="mt-1">
              Install the LTS version of Node.js from <span translate="no">nodejs.org</span> (it includes npm and npx), then select Re-check.
            </p>
          </Banner>
        )}
        {status.state === "failed" && (
          <Banner
            tone="error"
            title={status.title}
            details={status.log.length ? status.log.join("\n") : null}
            actions={
              <Button onClick={onStart} disabled={busy !== null}>
                <RotateCcw aria-hidden="true" /> Try again
              </Button>
            }
          >
            {status.message}
          </Banner>
        )}

        <div className="flex flex-col gap-3">
          <p className="text-lead text-pretty">
            AgentPlayback shows your Claude Code and Codex sessions on a 24-hour dial: when each one ran, when it waited for you, which project it
            belonged to, and what it would have cost at API prices. It runs on this PC and doesn’t call a model.
          </p>
          {status.state === "starting" ? (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
              <p className="inline-flex items-center gap-2 font-semibold">
                <Spinner aria-hidden="true" role={undefined} aria-label={undefined} /> Starting AgentPlayback…
              </p>
              <Button variant="outline" size="sm" onClick={onStop} disabled={busy !== null}>
                Cancel
              </Button>
              <p className="basis-full text-meta text-muted-foreground">
                The first start downloads AgentPlayback with npx, which can take a minute. You can switch sections in the meantime.
              </p>
            </div>
          ) : (
            status.state === "stopped" && (
              <div>
                <Button onClick={onStart} disabled={busy !== null}>
                  {busy === "start" ? <Spinner aria-hidden="true" role={undefined} aria-label={undefined} /> : <Play aria-hidden="true" />} Start
                </Button>
              </div>
            )
          )}
        </div>

        <section aria-labelledby="agents-reads" className="flex flex-col gap-2">
          <h2 id="agents-reads" className="text-body font-semibold">
            What it reads
          </h2>
          <p className="text-meta text-muted-foreground">The session logs that Claude Code and Codex keep on this PC:</p>
          <ul className="flex flex-col divide-y rounded-lg border">
            {status.sources.map((s) => (
              <li key={`${s.agent}:${s.path}`} className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 px-3 py-2">
                <span className="w-24 shrink-0 font-semibold" translate="no">
                  {s.agent}
                </span>
                <span className="min-w-0 flex-1 font-mono text-meta [overflow-wrap:anywhere]" translate="no">
                  {s.path}
                </span>
                <span className="text-meta text-muted-foreground">{s.exists ? "Found" : "Not found"}</span>
              </li>
            ))}
          </ul>
        </section>

        <section aria-labelledby="agents-network" className="flex flex-col gap-2">
          <h2 id="agents-network" className="text-body font-semibold">
            What it downloads and keeps
          </h2>
          <ul className="flex list-disc flex-col gap-1.5 pl-5 text-meta text-pretty">
            <li>The first start downloads AgentPlayback {status.version} from npm (about 11 MB), with the npx that comes with Node.js.</li>
            <li>
              For the cost estimates it downloads a price list from GitHub (<span translate="no">raw.githubusercontent.com</span>). Your sessions
              aren’t sent anywhere.
            </li>
            <li>
              Its scan results and the price list stay in{" "}
              <span className="font-mono [overflow-wrap:anywhere]" translate="no">
                {status.dataDir}
              </span>
              .
            </li>
            <li>The dashboard is served on this PC only (127.0.0.1). Daytrace starts it when you select Start and stops it when you quit.</li>
          </ul>
        </section>

        <section aria-labelledby="agents-version" className="flex flex-col gap-2">
          <h2 id="agents-version" className="text-body font-semibold">
            Version
          </h2>
          <p className="text-meta text-pretty">
            Daytrace runs AgentPlayback {status.version}, the version it was checked with. A newer version comes with a Daytrace update, so
            nothing new runs on this PC without a review.
          </p>
          {status.node && (
            <p className="text-meta text-muted-foreground">
              Node.js {status.node.version},{" "}
              <span className="font-mono [overflow-wrap:anywhere]" translate="no">
                {status.node.npxPath}
              </span>
            </p>
          )}
          <p className="text-meta text-muted-foreground">
            AgentPlayback is made by the Dayflow developer and published under the MIT license.
          </p>
        </section>
      </div>
    </div>
  );
}
