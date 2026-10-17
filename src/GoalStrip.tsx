import { useEffect, useState } from "react";
import { formatDuration } from "./api";
import { GoalsView, journalApi } from "./views/journal-api";

/**
 * The day's targets next to the timeline (Dayflow's DayGoalHeader at the top of its timeline
 * panel): focus time against the target and the distraction budget left. Goals are set in the
 * Goals tab.
 */
export function GoalStrip({ day, refresh }: { day: string; refresh: unknown }) {
  const [view, setView] = useState<GoalsView | null>(null);

  useEffect(() => {
    let live = true;
    journalApi
      .get(day)
      .then((v) => live && setView(v))
      .catch(() => live && setView(null));
    return () => {
      live = false;
    };
  }, [day, refresh]);

  if (!view) return null;
  if (view.plan.isSkipped) return null;
  if (view.prompt) {
    return <p className="goalstrip-note muted">Set today’s goals in the Goals tab to track focus and distraction here.</p>;
  }
  const target = view.plan.focusTargetMinutes * 60;
  const limit = view.plan.distractionLimitMinutes * 60;
  const focus = view.progress.focusSecs;
  const used = view.progress.distractedSecs;
  const pct = (a: number, b: number) => (b > 0 ? Math.min(100, (a / b) * 100) : 0);
  return (
    <div className="goalstrip" aria-label={view.isToday ? "Today’s targets" : "Targets"}>
      <div className="goalstrip-item">
        <span>
          Focus <b>{formatDuration(focus)}</b> of {formatDuration(target)}
        </span>
        <div className="goalstrip-bar" role="meter" aria-label="Focus" aria-valuemin={0} aria-valuemax={target} aria-valuenow={Math.min(focus, target)}>
          <span style={{ width: `${pct(focus, target)}%` }} />
        </div>
      </div>
      <div className="goalstrip-item">
        <span>
          Distraction budget{" "}
          {used > limit ? (
            <>
              <b>{formatDuration(used - limit)}</b> over
            </>
          ) : (
            <>
              <b>{formatDuration(limit - used)}</b> left
            </>
          )}
        </span>
        <div className={`goalstrip-bar distraction${used > limit ? " over" : ""}`} role="meter" aria-label="Distraction budget used" aria-valuemin={0} aria-valuemax={limit} aria-valuenow={Math.min(used, limit)}>
          <span style={{ width: `${pct(used, limit)}%` }} />
        </div>
      </div>
    </div>
  );
}
