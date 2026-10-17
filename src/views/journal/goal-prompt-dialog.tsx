import * as React from "react";
import { useAppState } from "@/app/state";
import { savePatch } from "@/app/settings-writes";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { toast } from "@/components/ui/toast";
import { navigate } from "@/lib/router";
import { formatDayTitle, shiftDay } from "@/lib/time";
import { errorText } from "@/lib/utils";
import { journalApi, type GoalsView } from "@/views/journal-api";
import { GoalSetup, ReviewBody } from "@/views/journal/targets";

/*
 * The daily goal prompt (Dayflow's DayGoalFlowView): yesterday's review when yesterday had targets,
 * then "Where do you want to spend your time today?". Save targets and Skip today answer it like
 * on the Goals tab (5 skips in a row stop the prompt); Not now closes it without an answer.
 */

export default function GoalPromptDialog({ view, onClose }: { view: GoalsView; onClose: () => void }) {
  const app = useAppState();
  const [step, setStep] = React.useState<"review" | "setup">(view.review ? "review" : "setup");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const yesterday = shiftDay(view.day, -1);

  // The setup replaces the review: its first field takes the focus.
  React.useEffect(() => {
    if (step === "setup" && view.review) requestAnimationFrame(() => document.getElementById("prompt-focus-h")?.focus());
  }, [step, view.review]);

  const answer = async (f: () => Promise<GoalsView>, done: string) => {
    setBusy(true);
    setError(null);
    try {
      await f();
      onClose();
      toast(done, { tone: "success", action: { label: "Open Goals", onClick: () => navigate("journal", { day: view.day }) } });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const turnOff = async () => {
    setBusy(true);
    setError(null);
    try {
      app.setSettings(await savePatch({ showGoalPrompt: false }));
      onClose();
      toast("The daily goal prompt is off", {
        description: "Turn it on again in Settings > Notifications.",
        action: { label: "Undo", onClick: () => void savePatch({ showGoalPrompt: true }).then(app.setSettings, () => undefined) },
      });
    } catch (e) {
      setError(`Couldn’t turn it off: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="font-display text-title">{step === "review" ? "Yesterday’s review" : "Today’s goals"}</DialogTitle>
          <DialogDescription className="text-meta">
            {step === "review"
              ? `How ${formatDayTitle(yesterday)} went against its targets.`
              : "Where do you want to spend your time today? Daytrace tracks it on the Timeline and in Goals."}
          </DialogDescription>
        </DialogHeader>

        {error && (
          <Banner tone="error" title="Couldn’t save that">
            {error}
          </Banner>
        )}

        {step === "review" && view.review ? (
          <>
            <ReviewBody review={view.review} colorFor={app.colorFor} />
            <DialogFooter>
              <Button variant="ghost" onClick={onClose}>
                Not now
              </Button>
              <Button onClick={() => setStep("setup")}>Set today’s targets</Button>
            </DialogFooter>
          </>
        ) : (
          <GoalSetup
            idPrefix="prompt"
            plan={view.plan}
            progress={view.progress}
            reference={view.reference}
            isToday
            busy={busy}
            onSave={(f, dl) => void answer(() => journalApi.savePlan(view.day, f, dl), "Targets saved")}
            onSkip={() => void answer(() => journalApi.skip(view.day), "Goals skipped for today")}
            onCancel={onClose}
            cancelLabel="Not now"
            settingsLink={false}
          />
        )}

        <p className="border-t pt-3 text-caption text-muted-foreground">
          This opens on the first use of the window each day.{" "}
          <Button variant="link" size="xs" className="h-auto px-0 text-caption" onClick={() => void turnOff()} disabled={busy}>
            Don’t show it again
          </Button>
        </p>
      </DialogContent>
    </Dialog>
  );
}
