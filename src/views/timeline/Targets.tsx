import { Target } from "lucide-react";
import { formatDuration } from "@/lib/time";
import { href } from "@/lib/router";
import { cn } from "@/lib/utils";
import type { GoalsLite } from "./api";

/*
 * The day's targets (Dayflow DayGoalHeader): focus time against the focus target and what is left
 * of the distraction limit, above the card list (shown once on the Timeline; set in Goals). The
 * meters are drawn like Goals' (journal/targets.tsx): focus as the focus categories' own colors,
 * the distraction limit in the distraction color with the part over it in red, a tick where the
 * target or limit sits once it is passed.
 */

const pct = (part: number, whole: number) => (whole > 0 ? Math.max(0, Math.min(100, (part / whole) * 100)) : 0);

function Meter({
  label,
  valueText,
  max,
  now,
  scale,
  tick,
  segments,
}: {
  label: string;
  valueText: string;
  max: number;
  now: number;
  scale: number;
  tick: number | null;
  segments: { key: string; secs: number; color?: string; className?: string }[];
}) {
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={Math.min(now, max)}
      aria-valuetext={valueText}
      className="relative h-2 w-full rounded-full bg-surface-3"
    >
      <div className="flex h-full w-full gap-px overflow-hidden rounded-full">
        {segments
          .filter((s) => s.secs > 0)
          .map((s) => (
            <span key={s.key} data-mark="" className={cn("block h-full shrink-0", s.className)} style={{ width: `${pct(s.secs, scale)}%`, ...(s.color ? { background: s.color } : {}) }} />
          ))}
      </div>
      {tick !== null && (
        <span aria-hidden="true" data-mark="" className="absolute -top-1 -bottom-1 w-0.5 rounded-full bg-foreground" style={{ left: `calc(${pct(tick, scale)}% - 1px)` }} />
      )}
    </div>
  );
}

export function Targets({ goals, day, colorFor, compact }: { goals: GoalsLite | null; day: string; colorFor: (category: string) => string; compact?: boolean }) {
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
  // Older backends send only the totals: one segment in the ink color then.
  const focusCats = (goals.progress.focusCategories ?? []).filter((c) => c.secs > 0);
  const focusSegments = focusCats.length ? focusCats.map((c) => ({ key: c.name, secs: c.secs, color: colorFor(c.name) })) : [{ key: "focus", secs: focus, className: "bg-foreground" }];
  const distractionColor = colorFor(goals.progress.distractionCategories?.[0] ?? "Distraction");

  return (
    <div className={cn("grid gap-x-5 gap-y-3", compact ? "grid-cols-2 items-end text-caption" : "grid-cols-1 text-meta")}>
      <div className="flex min-w-0 flex-col gap-1.5">
        <p className="text-muted-foreground">
          Focus target <span className="font-medium text-foreground tnum">{formatDuration(focus)}</span> of {formatDuration(target)}
        </p>
        <Meter
          label="Focus target"
          valueText={`${formatDuration(focus)} of ${formatDuration(target)}`}
          max={target}
          now={focus}
          scale={Math.max(target, focus)}
          tick={focus > target && target > 0 ? target : null}
          segments={focusSegments}
        />
      </div>
      <div className="flex min-w-0 flex-col gap-1.5">
        <p className="text-muted-foreground">
          Distraction limit{" "}
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
          label="Distraction limit used"
          valueText={`${formatDuration(used)} of ${formatDuration(limit)} used${over ? `, ${formatDuration(used - limit)} over` : ""}`}
          max={limit}
          now={used}
          scale={Math.max(limit, used)}
          tick={over && limit > 0 ? limit : null}
          segments={[
            { key: "used", secs: Math.min(used, limit), color: distractionColor },
            { key: "over", secs: over ? used - limit : 0, className: "bg-destructive" },
          ]}
        />
      </div>
    </div>
  );
}
