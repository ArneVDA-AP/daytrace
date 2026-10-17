import * as React from "react";
import { useAppState } from "@/app/state";
import { whatsNewSettled } from "@/app/WhatsNew";
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
 *
 * It never opens over something else: the check runs after the click or key is done (pointerup,
 * keyup), and an answer that arrives while a dialog, a menu or a popover is open, or while "What's
 * new" is still loading or open, waits until the window is clear. The backend has already marked
 * it shown by then, so it is kept, not dropped.
 */

const GoalPromptDialog = React.lazy(() => import("./goal-prompt-dialog"));

/** Asking again on the same day: the backend says no once shown, but a day can become due later. */
const RECHECK_MS = 30 * 60 * 1000;
/** How often a waiting prompt looks whether the window is clear. */
const WAIT_MS = 250;
let checked: { day: string; at: number } | null = null;
/** A prompt the backend handed out that is not on screen yet (kept across a remount). */
let pending: GoalsView | null = null;

const BUSY = '[role="dialog"], [role="alertdialog"], [role="menu"]';
const busy = () => document.querySelector(BUSY) !== null;

export function GoalPrompt() {
  const app = useAppState();
  const enabled = app.settings.settings.showGoalPrompt && app.settings.settings.onboardingCompleted;
  const [view, setView] = React.useState<GoalsView | null>(null);
  const [waiting, setWaiting] = React.useState(() => pending !== null);
  const live = React.useRef({ enabled, today: app.today, open: false });
  live.current = { enabled, today: app.today, open: view !== null || waiting };

  React.useEffect(() => {
    let asking = false;
    const ask = (e: Event) => {
      const { enabled: on, today, open } = live.current;
      if (!on || open || asking) return;
      // Typing in a field: wait for a click or the next time the window comes to the front.
      if (e.type === "keyup" && isTypingTarget(e.target)) return;
      if (e.type === "visibilitychange" && document.visibilityState !== "visible") return;
      const now = Date.now();
      if (checked && checked.day === today && now - checked.at < RECHECK_MS) return;
      checked = { day: today, at: now };
      asking = true;
      journalApi
        .takePrompt()
        .then((v) => {
          if (!v) return;
          pending = v;
          setWaiting(true);
        })
        .catch(() => {
          // Not worth an error on screen: the Goals tab shows the same flow.
        })
        .finally(() => {
          asking = false;
        });
    };
    window.addEventListener("pointerup", ask, true);
    window.addEventListener("keyup", ask, true);
    window.addEventListener("focus", ask);
    document.addEventListener("visibilitychange", ask);
    return () => {
      window.removeEventListener("pointerup", ask, true);
      window.removeEventListener("keyup", ask, true);
      window.removeEventListener("focus", ask);
      document.removeEventListener("visibilitychange", ask);
    };
  }, []);

  // A prompt waits for "What's new" and for a window without dialogs or menus, two checks in a row
  // (a dialog that a click opens renders right after the pointerup).
  React.useEffect(() => {
    if (!waiting) return;
    let stop = false;
    let timer = 0;
    let clearOnce = false;
    const tick = () => {
      if (stop) return;
      if (busy()) clearOnce = false;
      else if (clearOnce) {
        const v = pending;
        pending = null;
        setWaiting(false);
        if (v && live.current.enabled) setView(v);
        return;
      } else clearOnce = true;
      timer = window.setTimeout(tick, WAIT_MS);
    };
    void whatsNewSettled.then(() => {
      if (!stop) timer = window.setTimeout(tick, WAIT_MS);
    });
    return () => {
      stop = true;
      window.clearTimeout(timer);
    };
  }, [waiting]);

  if (!view) return null;
  return (
    <React.Suspense fallback={null}>
      <GoalPromptDialog view={view} onClose={() => setView(null)} />
    </React.Suspense>
  );
}
