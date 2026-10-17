import { Target } from "lucide-react";
import { formatDuration } from "@/lib/time";
import { href } from "@/lib/router";
import { cn } from "@/lib/utils";
import type { GoalsLite } from "./api";

/*
 * The day's targets (Dayflow DayGoalHeader): focus time against the target and what is left of the
 * distraction budget. Targets are set in Goals. Used above the card list (compact) and in "Your
 * day so far".
 */

function Meter({ label, value, max, over, valueText }: { label: string; value: number; max: number; over?: boolean; valueText: string }) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={Math.min(value, max)}
      aria-valuetext={valueText}
      className="h-1.5 overflow-hidden rounded-full bg-surface-3"
    >
      <div
        data-mark=""
        className={cn("h-full rounded-full", over ? "bg-destructive" : "bg-foreground")}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function Targets({ goals, day, compact }: { goals: GoalsLite | null; day: string; compact?: boolean }) {
  if (!goals || goals.plan.isSkipped) return null;
  if (goals.prompt) {
    return (
      <p className={cn("flex items-center gap-2 text-meta text-muted-foreground", compact && "text-caption")}>
        <Target className="size-3.5 shrink-0" aria-hidden="true" />
        <span>
          No targets for today yet.{" "}
          <a href={href("journal", { day })} className="font-medium text-foreground underline underline-offset-2">
            Set today’s goals
          </a>{" "}
          to track focus and distraction here.
        </span>
      </p>
    );
  }
  const target = goals.plan.focusTargetMinutes * 60;
  const limit = goals.plan.distractionLimitMinutes * 60;
  const focus = goals.progress.focusSecs;
  const used = goals.progress.distractedSecs;
  const over = used > limit;
  return (
    <div className={cn("grid gap-x-5 gap-y-3", compact ? "grid-cols-2 items-end text-caption" : "grid-cols-1 text-meta")}>
      <div className="flex min-w-0 flex-col gap-1.5">
        <p className="text-muted-foreground">
          Focus <span className="font-medium text-foreground tnum">{formatDuration(focus)}</span> of {formatDuration(target)}
        </p>
        <Meter label="Focus against the target" value={focus} max={target} valueText={`${formatDuration(focus)} of ${formatDuration(target)}`} />
      </div>
      <div className="flex min-w-0 flex-col gap-1.5">
        <p className="text-muted-foreground">
          Distraction budget{" "}
          {over ? (
            <>
              <span className="font-medium text-destructive tnum">{formatDuration(used - limit)}</span> over
            </>
          ) : (
            <>
              <span className="font-medium text-foreground tnum">{formatDuration(limit - used)}</span> left
            </>
          )}
        </p>
        <Meter
          label="Distraction budget used"
          value={used}
          max={limit}
          over={over}
          valueText={over ? `${formatDuration(used - limit)} over the budget of ${formatDuration(limit)}` : `${formatDuration(used)} of ${formatDuration(limit)} used`}
        />
      </div>
    </div>
  );
}
