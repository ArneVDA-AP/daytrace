import * as React from "react";
import { useAppState } from "@/app/state";
import { isTypingTarget } from "@/lib/utils";
import { journalApi, type GoalsView } from "@/views/journal-api";

/*
 * Dayflow's daily goal prompt (MainView/Lifecycle.requestDailyGoalPromptIfNeeded): yesterday's
 * review and today's targets, opened by itself. Dayflow asks after the 4 AM rollover; Daytrace
 * asks on the first use of the window after it (a click, a key, the window coming to the front),
 * because the PC can be on all night and 4 AM says nothing about when the day starts. The backend
 * decides whether it is due (Settings "Show daily goal prompt", 3 days with recording, fewer than
 * 5 skips in a row, no plan for today yet) and marks it shown, so it opens at most once a day.
 * The dialog loads on demand; the Goals tab keeps the same flow for later.
 */

const GoalPromptDialog = React.lazy(() => import("./goal-prompt-dialog"));

/** Asking again on the same day: the backend says no once shown, but a day can become due later. */
const RECHECK_MS = 30 * 60 * 1000;
let checked: { day: string; at: number } | null = null;

export function GoalPrompt() {
  const app = useAppState();
  const enabled = app.settings.settings.showGoalPrompt && app.settings.settings.onboardingCompleted;
  const [view, setView] = React.useState<GoalsView | null>(null);
  const live = React.useRef({ enabled, today: app.today, open: false });
  live.current = { enabled, today: app.today, open: view !== null };

  React.useEffect(() => {
    let asking = false;
    const ask = (e: Event) => {
      const { enabled: on, today, open } = live.current;
      if (!on || open || asking) return;
      // Typing in a field: wait for a click or the next time the window comes to the front.
      if (e.type === "keydown" && isTypingTarget(e.target)) return;
      if (e.type === "visibilitychange" && document.visibilityState !== "visible") return;
      const now = Date.now();
      if (checked && checked.day === today && now - checked.at < RECHECK_MS) return;
      // Another dialog is open (export, a confirmation): the next interaction asks again.
      if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
      checked = { day: today, at: now };
      asking = true;
      journalApi
        .takePrompt()
        .then((v) => {
          if (v && live.current.enabled) setView(v);
        })
        .catch(() => {
          // Not worth an error on screen: the Goals tab shows the same flow.
        })
        .finally(() => {
          asking = false;
        });
    };
    window.addEventListener("pointerdown", ask, true);
    window.addEventListener("keydown", ask, true);
    window.addEventListener("focus", ask);
    document.addEventListener("visibilitychange", ask);
    return () => {
      window.removeEventListener("pointerdown", ask, true);
      window.removeEventListener("keydown", ask, true);
      window.removeEventListener("focus", ask);
      document.removeEventListener("visibilitychange", ask);
    };
  }, []);

  if (!view) return null;
  return (
    <React.Suspense fallback={null}>
      <GoalPromptDialog view={view} onClose={() => setView(null)} />
    </React.Suspense>
  );
}
