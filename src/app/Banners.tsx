import * as React from "react";
import { KeyRound, PlugZap, ScreenShareOff } from "lucide-react";
import { api } from "@/api";
import { useAppState } from "@/app/state";
import { providerLabel } from "@/app/status";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { toast } from "@/components/ui/toast";
import { navigate } from "@/lib/router";
import { errorText } from "@/lib/utils";
import { teApi } from "@/timeline-extras-api";

/**
 * Problems that stop the timeline from filling, shown once, under the page header of every
 * section, each with its fix as a button. Routine status lives in the rail; these are only for
 * the three blocking problems: no API key, provider unreachable, screen capture failing.
 */
export function GlobalBanners() {
  const app = useAppState();
  const { problems, status, settings, today } = app;
  const [busy, setBusy] = React.useState<string | null>(null);

  const run = async (key: string, fn: () => Promise<void>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      toast("That didn’t work", { description: errorText(e), tone: "error" });
    } finally {
      setBusy(null);
      void app.reloadStatus();
    }
  };

  const items: React.ReactNode[] = [];
  const s = settings.settings;

  if (problems.captureError) {
    items.push(
      <Banner
        key="capture"
        tone="error"
        icon={<ScreenShareOff aria-hidden />}
        title="Can’t capture the screen"
        actions={
          <Button
            size="sm"
            variant="outline"
            disabled={busy === "capture"}
            onClick={() =>
              run("capture", async () => {
                await api.setRecording(true);
                toast("Trying to capture again", { description: "The message goes away with the next screenshot." });
              })
            }
          >
            Try now
          </Button>
        }
      >
        {sentence(problems.captureError)} This happens on the Windows sign-in screen, in administrator prompts and with
        protected video. Daytrace tries again every {s.captureIntervalSecs}&nbsp;s by itself.
      </Banner>,
    );
  }

  if (problems.noKey) {
    items.push(
      <Banner
        key="key"
        tone="warn"
        icon={<KeyRound aria-hidden />}
        title="Add a Claude API key to get timeline cards"
        actions={
          <Button size="sm" onClick={() => navigate("settings", { section: "provider" })}>
            Add API key
          </Button>
        }
      >
        Daytrace is recording, but it can’t describe your screen without a key. Screenshots are kept on this PC and
        analyzed once the key is added.
      </Banner>,
    );
  } else if (problems.unreachable) {
    const n = status.pendingBatches;
    items.push(
      <Banner
        key="net"
        tone="error"
        icon={<PlugZap aria-hidden />}
        title={`Can’t reach ${providerLabel(settings)}`}
        actions={
          <>
            <Button
              size="sm"
              variant="outline"
              disabled={busy === "net"}
              onClick={() =>
                run("net", async () => {
                  const extras = await teApi.dayExtras(today);
                  const ids = extras.failedGroups.filter((g) => g.retryable).flatMap((g) => g.batchIds);
                  if (ids.length) await teApi.retryBatches(ids);
                  else await api.analyzeNow();
                  toast("Trying again", { description: "This message goes away once a request gets through." });
                })
              }
            >
              Try now
            </Button>
            <Button size="sm" variant="ghost" onClick={() => navigate("settings", { section: "provider" })}>
              Provider settings
            </Button>
          </>
        }
      >
        {n > 0 ? `${n} ${n === 1 ? "batch is" : "batches are"} waiting. ` : ""}
        {s.recordingEnabled
          ? "Daytrace tries again when the next batch of screenshots is ready (about every 15 minutes), then rewrites the cards that failed. "
          : "Daytrace tries again when recording resumes and the next batch is ready. "}
        Screenshots stay on this PC.
      </Banner>,
    );
  }

  if (!items.length) return null;
  return <div className="flex shrink-0 flex-col gap-2 border-b bg-surface-1 px-(--page-x) py-3">{items}</div>;
}

function sentence(s: string): string {
  const t = s.trim();
  return /[.!?]$/.test(t) ? t : `${t}.`;
}
