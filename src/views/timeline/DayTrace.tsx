import * as React from "react";
import { Switch } from "@/components/ui/switch";
import { clock, dayBounds, formatDuration, hourLabel } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { Card } from "./api";
import { useWidth } from "./data";
import {
  BREAK_PX,
  NIGHT_FROM_HOUR,
  NIGHT_TO_HOUR,
  fitAxis,
  gapSpans,
  hourTicks,
  isFailed,
  laneKeyOf,
  laneTotals,
  nightSpans,
  recordedSecs,
  scaleFor,
  wholeDayAxis,
  type Lane,
} from "./model";

/*
 * The day trace (designer-bespoke's hypnogram on shadcn's strip): the logical day, 4 AM to 4 AM,
 * as one lane per category with the day's total next to each lane. Bars are cards at their real
 * time; thin risers join cards that follow each other; idle is a hatch, a failed card red stripes
 * across all lanes, stretches between cards without recording a light hatch ("Not recorded");
 * night hours are
 * shaded and today has a now line. "Fit to recorded hours" (the design review's fix) cuts out
 * stretches of an hour or more without recording and marks the cut on the axis, so a night-owl or
 * evening-only day fills the width.
 *
 * One tab stop: ←/→ (or ↑/↓) move to the previous/next card and select it, Home/End jump to the
 * first/last. The card list below has a full-size row for every bar (WCAG 2.5.8 equivalent).
 */

const LANE_H = 18;
const BAR_H = 10;
const LABEL_CHAR_PX = 6.7;

function hourText(ts: number): string {
  return hourLabel(new Date(ts * 1000).getHours());
}

export function DayTrace({
  day,
  cards,
  lanes,
  colorFor,
  selectedId,
  onSelect,
  now,
  muted,
  fit,
  onFitChange,
  error = false,
}: {
  day: string;
  /** null while loading: the frame (lanes, axis) is drawn so nothing moves when cards arrive. */
  cards: Card[] | null;
  lanes: Lane[];
  colorFor: (category: string) => string;
  selectedId: number | null;
  onSelect: (id: number) => void;
  /** Current time on today, else null. */
  now: number | null;
  muted: Set<string>;
  fit: boolean;
  onFitChange: (fit: boolean) => void;
  /** The day could not be loaded: totals read "–" instead of waiting. */
  error?: boolean;
}) {
  const [from, to] = dayBounds(day);
  const list = React.useMemo(() => (cards ?? []).filter((c) => c.endTs > from && c.startTs < to).sort((a, b) => a.startTs - b.startTs), [cards, from, to]);
  const axis = React.useMemo(() => (fit && list.length ? fitAxis(list, from, to, now) : wholeDayAxis(from, to)), [fit, list, from, to, now]);
  const [plotRef, width] = useWidth<HTMLDivElement>(640);
  const scale = scaleFor(axis, width);
  const plotH = Math.max(1, lanes.length) * LANE_H;
  const laneIndex = new Map(lanes.map((l, i) => [l.key, i]));
  const laneY = (c: Card) => (laneIndex.get(laneKeyOf(c, lanes)) ?? 0) * LANE_H + LANE_H / 2;
  const totals = React.useMemo(() => laneTotals(list, lanes, from, to), [list, lanes, from, to]);
  const recorded = React.useMemo(() => recordedSecs(list, from, to), [list, from, to]);
  const failedCount = list.filter(isFailed).length;
  const breaks = axis.windows.length - 1;
  const cut = axis.windows.slice(1).reduce((a, w, i) => a + w.start - axis.windows[i].end, 0);

  const [hover, setHover] = React.useState<number | null>(null);
  const [focused, setFocused] = React.useState<number | null>(null);
  const refs = React.useRef(new Map<number, HTMLButtonElement>());
  const tabbable = list.some((c) => c.id === selectedId) ? selectedId : (list[list.length - 1]?.id ?? null);

  const move = (fromId: number, key: string) => {
    const i = list.findIndex((c) => c.id === fromId);
    let next: Card | undefined;
    if (key === "ArrowRight" || key === "ArrowDown") next = list[i + 1];
    else if (key === "ArrowLeft" || key === "ArrowUp") next = list[i - 1];
    else if (key === "Home") next = list[0];
    else if (key === "End") next = list[list.length - 1];
    if (!next) return false;
    onSelect(next.id);
    refs.current.get(next.id)?.focus();
    return true;
  };

  // Hour labels: the smallest step that leaves room for a label; window ends are left out except
  // the last one, so the label after an axis break (where recording starts again) stays.
  const steps = [1, 2, 3, 4, 6, 12];
  const step = steps.find((s) => s * scale.pxPerHour >= 46) ?? 12;
  const ends = new Set(axis.windows.slice(0, -1).map((w) => w.end));
  const starts = new Set(axis.windows.slice(1).map((w) => w.start));
  const labels: { ts: number; x: number; align: "start" | "middle" | "end"; text: string }[] = [];
  let lastRight = -Infinity;
  for (const ts of hourTicks(axis, step)) {
    if (ends.has(ts)) continue;
    const x = scale.x(ts);
    if (x === null) continue;
    const text = hourText(ts);
    const w = text.length * LABEL_CHAR_PX;
    // Right after an axis break the label starts at the line, clear of the break mark.
    const align = x < w / 2 || starts.has(ts) ? "start" : x > width - w / 2 ? "end" : "middle";
    const left = align === "start" ? x : align === "end" ? x - w : x - w / 2;
    if (left < lastRight + 8) continue;
    lastRight = left + w;
    labels.push({ ts, x, align, text });
  }

  // Shaded and hatched spans, cut to the visible windows.
  const spansInWindows = (spans: [number, number][]) => {
    const out: { left: number; width: number; key: string }[] = [];
    for (const [s, e] of spans) {
      for (const w of axis.windows) {
        const a = Math.max(s, w.start);
        const b = Math.min(e, w.end);
        if (b <= a) continue;
        const x0 = scale.x(a);
        const x1 = scale.x(b);
        if (x0 === null || x1 === null) continue;
        out.push({ left: x0, width: x1 - x0, key: `${a}-${b}` });
      }
    }
    return out;
  };
  const night = spansInWindows(nightSpans(day));
  const gaps = spansInWindows(gapSpans(list));

  const tip = list.find((c) => c.id === (hover ?? focused)) ?? null;
  const tipX = tip ? (scale.x(Math.max(tip.startTs, from)) ?? 0) : 0;
  const nowX = now !== null && now > from && now < to ? scale.x(now) : null;

  return (
    <section aria-labelledby="trace-title" className="shrink-0 border-b px-(--page-x) pt-3 pb-2">
      <h2 id="trace-title" className="sr-only">
        Day overview
      </h2>
      <div className="flex gap-3">
        <ul aria-label="Time per category" className="w-[8.75rem] shrink-0">
          {lanes.map((l) => (
            <li key={l.key} className="flex items-center gap-1.5 text-caption" style={{ height: LANE_H }}>
              <span
                aria-hidden="true"
                data-mark=""
                className={cn("size-2.5 shrink-0 rounded-[2px]", l.isIdle && "hatch-idle border border-cat-idle bg-surface-1")}
                style={l.isIdle ? undefined : { background: colorFor(l.name) }}
              />
              <span className={cn("min-w-0 truncate", muted.has(l.key) ? "text-subtle-foreground line-through" : "text-foreground")}>{l.name}</span>
              <span className="ml-auto shrink-0 text-muted-foreground tnum">
                {cards === null ? (error ? "–" : "") : formatDuration(totals.get(l.key) ?? 0)}
              </span>
            </li>
          ))}
        </ul>

        <div ref={plotRef} className="relative min-w-0 flex-1">
          <div
            role="group"
            aria-label={
              list.length
                ? `${list.length} ${list.length === 1 ? "card" : "cards"} on the day trace${breaks ? ", hours without recording left out" : ""}. Arrow keys move between cards.`
                : "Day trace, no cards"
            }
            className="relative"
            style={{ height: plotH }}
            onKeyDown={(e) => {
              const id = Number((e.target as HTMLElement).dataset.cardId);
              if (!Number.isFinite(id)) return;
              if (["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp", "Home", "End"].includes(e.key) && move(id, e.key)) e.preventDefault();
            }}
          >
            {night.map((n) => (
              <div key={`n${n.key}`} aria-hidden="true" className="absolute inset-y-0 bg-surface-2" style={{ left: n.left, width: n.width }} />
            ))}
            {gaps.map((g) => (
              <div key={`g${g.key}`} aria-hidden="true" className="hatch-gap absolute inset-y-0" style={{ left: g.left, width: g.width }} />
            ))}
            {axis.windows.slice(0, -1).map((_, i) => (
              <div
                key={`b${i}`}
                aria-hidden="true"
                className="hatch-gap absolute inset-y-0 border-x border-dashed border-border-strong bg-surface-1"
                style={{ left: scale.breakX(i), width: BREAK_PX }}
              />
            ))}
            {labels.map((l) => (
              <span key={`t${l.ts}`} aria-hidden="true" className="absolute inset-y-0 w-px bg-(--trace-gridline)" style={{ left: Math.round(l.x) }} />
            ))}
            {lanes.map((l, i) => (
              <span
                key={`l${l.key}`}
                aria-hidden="true"
                className="absolute inset-x-0 border-t border-dotted border-border"
                style={{ top: i * LANE_H + LANE_H / 2 }}
              />
            ))}
            {list
              .filter((c) => c.id === selectedId)
              .map((c) => {
                const x0 = scale.x(Math.max(c.startTs, from));
                const x1 = scale.x(Math.min(c.endTs, to));
                if (x0 === null || x1 === null) return null;
                return <div key="sel" aria-hidden="true" className="absolute -inset-y-0.5 rounded-sm bg-surface-3" style={{ left: x0 - 2, width: Math.max(4, x1 - x0) + 4 }} />;
              })}
            {list.slice(1).map((c, i) => {
              const prev = list[i];
              if (Math.abs(c.startTs - prev.endTs) > 60 || isFailed(c) || isFailed(prev)) return null;
              const x = scale.x(c.startTs);
              if (x === null) return null;
              const y0 = laneY(prev);
              const y1 = laneY(c);
              if (y0 === y1) return null;
              return (
                <span
                  key={`r${c.id}`}
                  aria-hidden="true"
                  className="absolute w-px bg-border-strong"
                  style={{ left: Math.round(x) - 1, top: Math.min(y0, y1), height: Math.abs(y1 - y0) }}
                />
              );
            })}
            {list.map((c) => {
              const x0 = scale.x(Math.max(c.startTs, from));
              const x1 = scale.x(Math.min(c.endTs, to));
              if (x0 === null || x1 === null) return null;
              const failed = isFailed(c);
              const lane = lanes[laneIndex.get(laneKeyOf(c, lanes)) ?? 0];
              const idle = !failed && !!lane?.isIdle;
              const sel = c.id === selectedId;
              const dim = !failed && muted.has(laneKeyOf(c, lanes));
              const w = Math.max(3, x1 - x0 - 1);
              const y = laneY(c);
              return (
                <button
                  key={c.id}
                  ref={(el) => {
                    if (el) refs.current.set(c.id, el);
                    else refs.current.delete(c.id);
                  }}
                  type="button"
                  data-card-id={c.id}
                  tabIndex={c.id === tabbable ? 0 : -1}
                  aria-current={sel ? "true" : undefined}
                  aria-label={`${c.start} to ${c.end}, ${formatDuration(c.endTs - c.startTs)}. ${failed ? "Processing failed, not analyzed" : `${c.category}: ${c.title}`}`}
                  onClick={() => onSelect(c.id)}
                  onPointerEnter={() => setHover(c.id)}
                  onPointerLeave={() => setHover((h) => (h === c.id ? null : h))}
                  onFocus={() => setFocused(c.id)}
                  onBlur={() => setFocused((f) => (f === c.id ? null : f))}
                  className={cn("absolute z-[1] rounded-[3px] focus-visible:z-10 focus-visible:outline-offset-1", dim && "opacity-30")}
                  style={failed ? { left: x0, width: w, top: 0, height: plotH } : { left: x0, width: w, top: y - LANE_H / 2, height: LANE_H }}
                >
                  <span
                    aria-hidden="true"
                    data-mark=""
                    className={cn(
                      "absolute inset-x-0 rounded-[2px]",
                      failed && "stripes-failed inset-y-px border border-destructive bg-destructive-surface",
                      idle && "hatch-idle border border-cat-idle bg-surface-1",
                      sel && "outline-2 outline-offset-1 outline-foreground",
                    )}
                    style={failed ? undefined : { top: (LANE_H - BAR_H) / 2, height: BAR_H, ...(idle ? {} : { background: colorFor(c.category) }) }}
                  />
                </button>
              );
            })}
            {nowX !== null && nowX !== undefined && (
              <div aria-hidden="true" className="pointer-events-none absolute -top-1 z-20 w-0" style={{ left: nowX, height: plotH + 7 }}>
                {/* data-mark keeps the line in Contrast themes (CanvasText there, UX6). */}
                <span data-mark="" className="absolute inset-y-0 -left-px w-0.5 rounded-full bg-(--trace-now) forced-colors:outline-0" />
                <span data-mark="" className="absolute -bottom-0.5 -left-[3px] size-1.5 rounded-full bg-(--trace-now) forced-colors:outline-0" />
              </div>
            )}
          </div>

          <div aria-hidden="true" className="relative mt-1 h-4 font-mono text-axis text-subtle-foreground">
            {cards !== null &&
              labels.map((l) => (
                <span
                  key={l.ts}
                  className={cn(
                    "absolute top-0 whitespace-nowrap",
                    l.align === "middle" && "-translate-x-1/2",
                    l.align === "end" && "-translate-x-full",
                  )}
                  style={{ left: l.x }}
                >
                  {l.text}
                </span>
              ))}
            {cards !== null &&
              axis.windows.slice(0, -1).map((_, i) => (
                <span key={`bk${i}`} className="absolute top-0 flex h-3 items-center justify-center gap-[3px]" style={{ left: scale.breakX(i), width: BREAK_PX }}>
                  <span className="h-3 w-px rotate-[20deg] bg-muted-foreground" />
                  <span className="h-3 w-px rotate-[20deg] bg-muted-foreground" />
                </span>
              ))}
          </div>

          {tip && (
            <div
              role="presentation"
              className="pointer-events-none absolute z-30 w-max max-w-72 rounded-md border bg-popover px-2.5 py-1.5 text-caption text-popover-foreground shadow-md"
              style={{ top: plotH + 6, left: Math.min(Math.max(0, tipX - 40), Math.max(0, width - 288)) }}
            >
              <span className="block font-mono text-muted-foreground tnum">
                {tip.start} – {tip.end} · {formatDuration(tip.endTs - tip.startTs)}
              </span>
              <span className="block font-medium">{isFailed(tip) ? "Processing failed" : tip.title}</span>
              {!isFailed(tip) && <span className="block text-muted-foreground">{tip.category}</span>}
            </div>
          )}
        </div>
      </div>

      <div className="mt-1.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-caption text-muted-foreground">
        {night.length > 0 && (
          <span className="inline-flex items-center gap-1.5">
            <span aria-hidden="true" className="inline-block h-2.5 w-3.5 rounded-[2px] border border-border bg-surface-2" />
            Night, {hourLabel(NIGHT_FROM_HOUR)} to {hourLabel(NIGHT_TO_HOUR)}
          </span>
        )}
        {gaps.length > 0 && (
          <span className="inline-flex items-center gap-1.5">
            <span aria-hidden="true" className="hatch-gap inline-block h-2.5 w-3.5 rounded-[2px] border border-border" />
            Not recorded
          </span>
        )}
        {failedCount > 0 && (
          <span className="inline-flex items-center gap-1.5">
            <span aria-hidden="true" data-mark="" className="stripes-failed inline-block h-2.5 w-3.5 rounded-[2px] border border-destructive bg-destructive-surface" />
            Not analyzed
          </span>
        )}
        {breaks > 0 && (
          <span className="inline-flex items-center gap-1.5">
            <span aria-hidden="true" className="inline-flex h-3 w-3.5 items-center justify-center gap-[3px]">
              <span className="h-3 w-px rotate-[20deg] bg-muted-foreground" />
              <span className="h-3 w-px rotate-[20deg] bg-muted-foreground" />
            </span>
            {formatDuration(cut)} without recording left out
          </span>
        )}
        <span className="inline-flex items-center gap-1.5">
          {nowX !== null && nowX !== undefined && now !== null && (
            <>
              <span aria-hidden="true" data-mark="" className="inline-block h-3 w-0.5 rounded-full bg-(--trace-now) forced-colors:outline-0" />
              <span>Now {clock(now)}</span>
              <span aria-hidden="true">·</span>
            </>
          )}
          <span>
            Recorded <span className="tnum text-foreground">{cards === null ? (error ? "–" : "…") : formatDuration(recorded)}</span>
          </span>
        </span>
        {/* A view option that is on by default: a switch reads quieter than a pressed button. With
            no cards there is nothing to fit, so it is not shown (UX23). */}
        {list.length > 0 && (
          <label className="ml-auto inline-flex items-center gap-2 text-meta text-foreground">
            <Switch checked={fit} onCheckedChange={onFitChange} />
            Fit to recorded hours
          </label>
        )}
      </div>
    </section>
  );
}
