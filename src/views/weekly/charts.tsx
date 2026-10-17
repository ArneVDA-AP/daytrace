import * as React from "react";
import type { Card, Category } from "@/api";
import { href } from "@/lib/router";
import { clock, dateOf, formatDayShort, formatDayTitle, formatDuration, formatDurationShort, hourLabel, weekday } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { CategoryTotal, Kind, WeekStats } from "@/views/insights/api";
import { byCategory, durationOrDash, kindOf, percent, wallMinute } from "@/views/insights/format";
import { CategoryMark, Legend, SvgPatterns, useWidth, type LegendItem } from "@/views/insights/parts";

/*
 * Weekly charts, hand-drawn in SVG (no chart library): horizontal stacked bars for time per day,
 * diverging bars for focus and distraction, one lane per day for the week by hour (every card a
 * link to it on the timeline), a one-hue heat map for distraction by hour and a diverging one for
 * focused against distracted by hour. Each has a table with the same numbers (the page's Tables
 * switch, ?view=table). Rows that open a day are HTML links next to the SVG, so focus and
 * keyboard behave like every other link; days still ahead are plain text. Text stays in text
 * colors; color is only on marks.
 */

const AXIS_TEXT = "font-mono text-axis";
const axisStyle: React.CSSProperties = { fill: "var(--chart-axis)" };
const gridStyle: React.CSSProperties = { stroke: "var(--chart-grid)" };

/** Mon 12 */
export function dayShortLabel(day: string): string {
  return `${weekday(day)} ${dateOf(day).getDate()}`;
}

const DAY_LINK = "w-fit rounded-sm text-meta text-muted-foreground underline-offset-2 hover:text-foreground hover:underline";

/** A row's day: a link to it, or plain text for a day still ahead (nothing to open yet). */
function DayLabel({ day, future, to, label, className = DAY_LINK }: { day: string; future: boolean; to: string; label?: string; className?: string }) {
  if (future)
    return (
      <span className="w-fit text-meta text-subtle-foreground">
        {dayShortLabel(day)}
        <span className="sr-only">, not yet</span>
      </span>
    );
  return (
    <a href={to} aria-label={label} className={className}>
      {dayShortLabel(day)}
    </a>
  );
}

/** A bar with square left corners and rounded right ones (the data end). */
function rightRounded(x: number, y: number, w: number, h: number, r: number): string {
  const rr = Math.max(0, Math.min(r, w / 2, h / 2));
  return `M${x},${y}H${x + w - rr}A${rr},${rr} 0 0 1 ${x + w},${y + rr}V${y + h - rr}A${rr},${rr} 0 0 1 ${x + w - rr},${y + h}H${x}Z`;
}

/** A vertical bar rounded at its top (up) or bottom (down) end. */
function endRounded(x: number, y: number, w: number, h: number, r: number, up: boolean): string {
  const rr = Math.max(0, Math.min(r, w / 2, h));
  if (up) return `M${x},${y + h}V${y + rr}A${rr},${rr} 0 0 1 ${x + rr},${y}H${x + w - rr}A${rr},${rr} 0 0 1 ${x + w},${y + rr}V${y + h}Z`;
  return `M${x},${y}H${x + w}V${y + h - rr}A${rr},${rr} 0 0 1 ${x + w - rr},${y + h}H${x + rr}A${rr},${rr} 0 0 1 ${x},${y + h - rr}Z`;
}

/** Tick step in hours for a scale of `maxHours`. */
function hourStep(maxHours: number): number {
  if (maxHours <= 4) return 1;
  if (maxHours <= 10) return 2;
  if (maxHours <= 20) return 4;
  return 6;
}

function markFill(t: { name: string; kind: Kind }, colorFor: (n: string) => string, idleId: string, failedId: string): string {
  if (t.kind === "idle") return `url(#${idleId})`;
  if (t.kind === "system") return `url(#${failedId})`;
  return colorFor(t.name);
}

export interface DayRow {
  day: string;
  totals: CategoryTotal[];
  secs: number;
  future: boolean;
}

/** Categories that occur in the week, in stack order (settings order, idle last). */
export function weekCategories(rows: DayRow[], categories: Category[]): { name: string; kind: Kind }[] {
  const m = new Map<string, { name: string; kind: Kind }>();
  for (const r of rows) for (const t of r.totals) if (t.secs > 0 && t.kind !== "system" && !m.has(t.key)) m.set(t.key, { name: t.name, kind: t.kind });
  return byCategory([...m.values()], categories);
}

export function categoryLegend(cats: { name: string; kind: Kind }[], colorFor: (n: string) => string): LegendItem[] {
  return cats.map((c) => ({ key: c.name, label: c.name, mark: <CategoryMark name={c.name} kind={c.kind} colorFor={colorFor} /> }));
}

// ---------------------------------------------------------------- time per day

const ROW_H = 30;
const BAR_H = 16;

export function TimePerDayChart({ rows, categories, colorFor }: { rows: DayRow[]; categories: Category[]; colorFor: (n: string) => string }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const idleId = React.useId().replace(/:/g, "");
  const cats = weekCategories(rows, categories);
  const rank = new Map(cats.map((c, i) => [c.name, i]));
  const maxH = Math.max(1, ...rows.map((r) => r.secs / 3600));
  const step = hourStep(maxH);
  const scale = Math.ceil(maxH / step) * step;
  const ticks = Array.from({ length: scale / step + 1 }, (_, i) => i * step);
  const w = Math.max(0, width);
  const x = (h: number) => (h / scale) * (w - 1);

  return (
    <div className="flex flex-col gap-3">
      <Legend items={categoryLegend(cats, colorFor)} />
      <div className="grid grid-cols-[4rem_minmax(0,1fr)_5.75rem] items-center gap-x-3">
        {rows.map((r) => {
          const segs = [...r.totals].filter((t) => t.secs > 0 && t.kind !== "system").sort((a, b) => (rank.get(a.name) ?? 99) - (rank.get(b.name) ?? 99));
          const summary = segs.map((t) => `${t.name} ${formatDuration(t.secs)}`).join(", ");
          let cx = 0;
          return (
            <React.Fragment key={r.day}>
              <DayLabel
                day={r.day}
                future={r.future}
                to={href("daily", { day: r.day })}
                label={`${dayShortLabel(r.day)}: ${r.secs ? `${formatDuration(r.secs)} recorded, ${summary}` : "no recording"}. Open this day in Daily`}
              />
              <div ref={r === rows[0] ? ref : undefined} className="relative h-[30px] min-w-0">
                {w > 0 && (
                  <svg width={w} height={ROW_H} className="absolute inset-0 block overflow-visible" aria-hidden="true">
                    <SvgPatterns idleId={idleId} />
                    {ticks.map((t) => (
                      <line key={t} x1={x(t) + 0.5} x2={x(t) + 0.5} y1={0} y2={ROW_H} style={gridStyle} />
                    ))}
                    {segs.map((t, i) => {
                      const sw = (t.secs / 3600 / scale) * (w - 1);
                      const last = i === segs.length - 1;
                      const draw = Math.max(1, last ? sw : sw - 2);
                      const y = (ROW_H - BAR_H) / 2;
                      const fill = markFill(t, colorFor, idleId, idleId);
                      const el = last ? (
                        <path key={t.key} d={rightRounded(cx, y, draw, BAR_H, 4)} style={{ fill }}>
                          <title>{`${dayShortLabel(r.day)}: ${t.name} ${formatDuration(t.secs)}`}</title>
                        </path>
                      ) : (
                        <rect key={t.key} x={cx} y={y} width={draw} height={BAR_H} style={{ fill }}>
                          <title>{`${dayShortLabel(r.day)}: ${t.name} ${formatDuration(t.secs)}`}</title>
                        </rect>
                      );
                      cx += sw;
                      return el;
                    })}
                    {r.secs === 0 && (
                      <text x={4} y={ROW_H / 2 + 4} className="text-caption" style={{ fill: "var(--subtle-foreground)" }}>
                        {r.future ? "Not yet" : "No recording"}
                      </text>
                    )}
                  </svg>
                )}
              </div>
              <span className="text-right text-meta whitespace-nowrap tnum">{r.secs ? formatDuration(r.secs) : ""}</span>
            </React.Fragment>
          );
        })}
        <span />
        <div className="relative h-4 min-w-0" aria-hidden="true">
          {w > 0 && (
            <svg width={w} height={16} className="absolute inset-0 block overflow-visible">
              {ticks.map((t, i) => (
                <text key={t} x={x(t)} y={12} textAnchor={i === 0 ? "start" : i === ticks.length - 1 ? "end" : "middle"} className={AXIS_TEXT} style={axisStyle}>
                  {t} h
                </text>
              ))}
            </svg>
          )}
        </div>
        <span />
      </div>
    </div>
  );
}

export function TimePerDayTable({ rows, categories, colorFor }: { rows: DayRow[]; categories: Category[]; colorFor: (n: string) => string }) {
  const cats = weekCategories(rows, categories);
  const secsOf = (r: DayRow, name: string) => r.totals.find((t) => t.name === name)?.secs ?? 0;
  return (
    <div className="scroll-thin overflow-x-auto">
      <table className="w-full min-w-[32rem] text-meta">
        <caption className="sr-only">Time per day by category</caption>
        <thead>
          <tr className="border-b text-left text-muted-foreground">
            <th scope="col" className="py-2 pr-3 font-normal">
              Day
            </th>
            {cats.map((c) => (
              <th key={c.name} scope="col" className="py-2 pl-3 text-right font-normal">
                <span className="inline-flex items-center gap-1.5">
                  <CategoryMark name={c.name} kind={c.kind} colorFor={colorFor} />
                  {c.name}
                </span>
              </th>
            ))}
            <th scope="col" className="py-2 pl-3 text-right font-normal">
              Total
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.day} className="border-b last:border-0">
              <th scope="row" className="py-1.5 pr-3 text-left font-normal">
                <DayLabel day={r.day} future={r.future} to={href("daily", { day: r.day })} className="rounded-sm underline-offset-2 hover:underline" />
              </th>
              {cats.map((c) => (
                <td key={c.name} className={cn("py-1.5 pl-3 text-right whitespace-nowrap tnum", !secsOf(r, c.name) && "text-subtle-foreground")}>
                  {durationOrDash(secsOf(r, c.name))}
                </td>
              ))}
              <td className={cn("py-1.5 pl-3 text-right font-medium whitespace-nowrap tnum", !r.secs && "font-normal text-subtle-foreground")}>
                {r.future ? "Not yet" : durationOrDash(r.secs)}
              </td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="border-t">
            <th scope="row" className="pt-2 pr-3 text-left font-normal text-muted-foreground">
              Week
            </th>
            {cats.map((c) => (
              <td key={c.name} className="pt-2 pl-3 text-right whitespace-nowrap tnum">
                {durationOrDash(rows.reduce((a, r) => a + secsOf(r, c.name), 0))}
              </td>
            ))}
            <td className="pt-2 pl-3 text-right font-medium whitespace-nowrap tnum">{durationOrDash(rows.reduce((a, r) => a + r.secs, 0))}</td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------- focus and distraction

export interface FocusRow {
  day: string;
  focus: number;
  distracted: number;
  /** Recorded minus idle: the denominator of the percentages. */
  active: number;
  future: boolean;
}

const FOCUS_H = 176;

export function FocusChart({ rows, distractionColor }: { rows: FocusRow[]; distractionColor: string }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const up = Math.max(0, ...rows.map((r) => r.focus / 3600));
  const down = Math.max(0, ...rows.map((r) => r.distracted / 3600));
  const step = hourStep(up + down);
  const top = Math.max(step, Math.ceil(up / step) * step);
  // Distraction is usually small: give it its own room instead of a whole tick step.
  const bottom = down > 0 ? Math.max(down * 1.15, step * 0.3) : 0;
  const axisW = 36;
  const labelsH = 20;
  const plotH = FOCUS_H - labelsH - 8;
  const per = plotH / (top + bottom);
  const zero = 8 + top * per;
  const plotW = Math.max(0, width - axisW);
  const col = plotW / rows.length;
  const barW = Math.min(28, Math.max(8, col * 0.45));
  const ticks: number[] = [];
  for (let t = -Math.floor(bottom / step) * step; t <= top; t += step) ticks.push(t);

  return (
    <div className="flex flex-col gap-3">
      <Legend
        items={[
          { key: "f", label: "Focused", mark: <span aria-hidden="true" className="size-2.5 rounded-[2px]" style={{ background: "var(--chart-5)" }} /> },
          { key: "d", label: "Distracted", mark: <span aria-hidden="true" className="size-2.5 rounded-[2px]" style={{ background: distractionColor }} /> },
        ]}
      />
      <div ref={ref} className="h-44 w-full">
        {width > 0 && (
          <svg width={width} height={FOCUS_H} role="img" aria-label="Focused time above the line and distracted time below it, per day. Switch to Tables for the numbers." className="block overflow-visible">
            {ticks.map((t) => {
              const y = zero - t * per;
              return (
                <g key={t}>
                  <line x1={axisW} x2={width} y1={Math.round(y) + 0.5} y2={Math.round(y) + 0.5} style={t === 0 ? { stroke: "var(--border-strong)" } : gridStyle} />
                  <text x={axisW - 6} y={y + 4} textAnchor="end" className={AXIS_TEXT} style={axisStyle}>
                    {Math.abs(t)} h
                  </text>
                </g>
              );
            })}
            {rows.map((r, i) => {
              const cx = axisW + col * i + col / 2;
              const fh = (r.focus / 3600) * per;
              const dh = (r.distracted / 3600) * per;
              return (
                <g key={r.day}>
                  {fh > 0 && (
                    <path d={endRounded(cx - barW / 2, zero - fh, barW, fh, 4, true)} style={{ fill: "var(--chart-5)" }}>
                      <title>{`${dayShortLabel(r.day)}: focused ${formatDuration(r.focus)}`}</title>
                    </path>
                  )}
                  {dh > 0 && (
                    <path d={endRounded(cx - barW / 2, zero + 1, barW, dh, 4, false)} style={{ fill: distractionColor }}>
                      <title>{`${dayShortLabel(r.day)}: distracted ${formatDuration(r.distracted)}`}</title>
                    </path>
                  )}
                  <text x={cx} y={FOCUS_H - 4} textAnchor="middle" className="text-caption" style={{ fill: "var(--muted-foreground)" }}>
                    {weekday(r.day)}
                  </text>
                </g>
              );
            })}
          </svg>
        )}
      </div>
    </div>
  );
}

export function FocusTable({ rows }: { rows: FocusRow[] }) {
  return (
    <table className="w-full text-meta">
      <caption className="sr-only">Focused and distracted time per day</caption>
      <thead>
        <tr className="border-b text-left text-muted-foreground">
          <th scope="col" className="py-2 font-normal">
            Day
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Focused
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Distracted
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Distracted, of active time
          </th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.day} className="border-b last:border-0">
            <th scope="row" className="py-1.5 text-left font-normal">
              {dayShortLabel(r.day)}
            </th>
            <td className="py-1.5 text-right tnum">{r.future ? "Not yet" : durationOrDash(r.focus)}</td>
            <td className="py-1.5 text-right tnum">{r.future ? "" : durationOrDash(r.distracted)}</td>
            <td className="py-1.5 text-right text-muted-foreground tnum">{r.active > 0 ? percent(r.distracted, r.active) : ""}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ---------------------------------------------------------------- week by hour (lanes)

const LANE_H = 24;

export interface Lane {
  day: string;
  cards: Card[];
  future: boolean;
}

function laneSummary(l: Lane): string {
  const real = l.cards.filter((c) => c.category.trim().toLowerCase() !== "system");
  if (!l.cards.length) return l.future ? "not yet" : "no recording";
  const first = Math.min(...l.cards.map((c) => c.startTs));
  const last = Math.max(...l.cards.map((c) => c.endTs));
  return `${clock(first)} to ${clock(last)}, ${real.length} ${real.length === 1 ? "card" : "cards"}`;
}

interface LaneCard {
  card: Card;
  name: string;
  kind: Kind;
  x0: number;
  x1: number;
}

/** "Mon 12 Oct, 9:00 AM to 10:15 AM: Work, Spec review" */
function cardLabel(day: string, c: LaneCard): string {
  const what = c.kind === "system" ? "Not analyzed" : c.name;
  return `${formatDayShort(day)}, ${clock(c.card.startTs)} to ${clock(c.card.endTs)}: ${what}, ${c.card.title || "Untitled card"}`;
}

export function WeekLanes({
  lanes,
  range,
  categories,
  colorFor,
}: {
  lanes: Lane[];
  /** Minutes since 4 AM: the week's activity window. */
  range: [number, number];
  categories: Category[];
  colorFor: (n: string) => string;
}) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const ids = React.useId().replace(/:/g, "");
  const idleId = `${ids}i`;
  const failedId = `${ids}f`;
  const groupRef = React.useRef<HTMLDivElement>(null);
  const [ws, we] = range;
  const span = Math.max(60, we - ws);
  const x = (m: number) => ((m - ws) / span) * width;
  const step = span > 14 * 60 ? 3 : span > 8 * 60 ? 2 : 1;
  const firstHour = Math.ceil(ws / 60);
  const hours: number[] = [];
  for (let h = firstHour; h * 60 <= we; h += step) hours.push(h);

  const present = new Map<string, { name: string; kind: Kind }>();
  for (const l of lanes)
    for (const c of l.cards) {
      const name = c.category.trim() || "Uncategorized";
      const kind = c.isIdleShortcut ? "idle" : kindOf(name, categories);
      if (!present.has(name.toLowerCase())) present.set(name.toLowerCase(), { name, kind });
    }
  const legendCats = byCategory([...present.values()].filter((c) => c.kind !== "system"), categories);
  const legend = categoryLegend(legendCats, colorFor);
  if ([...present.values()].some((c) => c.kind === "system")) legend.push({ key: "system", label: "Not analyzed", mark: <CategoryMark name="System" kind="system" colorFor={colorFor} /> });

  // Every card's place in its lane, in time order (cards outside the window are left out).
  const placed: LaneCard[][] = lanes.map((l) =>
    [...l.cards]
      .sort((a, b) => a.startTs - b.startTs)
      .map((c) => {
        const name = c.category.trim() || "Uncategorized";
        const kind: Kind = c.isIdleShortcut ? "idle" : kindOf(name, categories);
        const a = wallMinute(c.startTs);
        const b = a + (c.endTs - c.startTs) / 60;
        return { card: c, name, kind, x0: Math.max(0, x(a)), x1: Math.min(width, x(b)) };
      })
      .filter((c) => c.x1 > c.x0),
  );

  // One tab stop for all cards: arrows move inside a day (Left/Right) and between days (Up/Down,
  // to the card nearest in time), Home/End to the day's first and last card, Enter opens it.
  const first = placed.findIndex((p) => p.length > 0);
  const [active, setActive] = React.useState<{ lane: number; i: number } | null>(null);
  const cur = active && placed[active.lane]?.[active.i] ? active : first >= 0 ? { lane: first, i: 0 } : null;
  const focusCard = (lane: number, i: number) => {
    setActive({ lane, i });
    groupRef.current?.querySelector<HTMLElement>(`[data-lane="${lane}"][data-card-index="${i}"]`)?.focus();
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!cur || !(e.target as HTMLElement).dataset?.cardIndex) return;
    const row = placed[cur.lane];
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      focusCard(cur.lane, Math.max(0, Math.min(row.length - 1, cur.i + (e.key === "ArrowRight" ? 1 : -1))));
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      focusCard(cur.lane, e.key === "Home" ? 0 : row.length - 1);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const dir = e.key === "ArrowDown" ? 1 : -1;
      const mid = (row[cur.i].x0 + row[cur.i].x1) / 2;
      for (let l = cur.lane + dir; l >= 0 && l < placed.length; l += dir) {
        if (!placed[l].length) continue;
        let best = 0;
        placed[l].forEach((c, i) => {
          if (Math.abs((c.x0 + c.x1) / 2 - mid) < Math.abs((placed[l][best].x0 + placed[l][best].x1) / 2 - mid)) best = i;
        });
        focusCard(l, best);
        return;
      }
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <Legend items={legend} />
      <div
        ref={groupRef}
        role="group"
        aria-label="Cards by time of day, one row per day. Arrow keys move between cards; Enter opens a card in the timeline."
        className="grid grid-cols-[4rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5"
        onKeyDown={onKeyDown}
      >
        <span />
        <div ref={ref} className="relative h-4 min-w-0" aria-hidden="true">
          {width > 0 && (
            <svg width={width} height={16} className="absolute inset-0 block overflow-visible">
              {hours.map((h, i) => (
                <text
                  key={h}
                  x={x(h * 60)}
                  y={11}
                  textAnchor={i === 0 && x(h * 60) < 20 ? "start" : x(h * 60) > width - 24 ? "end" : "middle"}
                  className={AXIS_TEXT}
                  style={axisStyle}
                >
                  {hourLabel(4 + h)}
                </text>
              ))}
            </svg>
          )}
        </div>
        {lanes.map((l, li) => (
          <React.Fragment key={l.day}>
            <DayLabel day={l.day} future={l.future} to={href("timeline", { day: l.day })} label={`${dayShortLabel(l.day)}: ${laneSummary(l)}. Open this day in the timeline`} />
            <div className="relative min-w-0" style={{ height: LANE_H }}>
              {width > 0 && (
                <svg width={width} height={LANE_H} className="absolute inset-0 block" aria-hidden="true">
                  <SvgPatterns idleId={idleId} failedId={failedId} />
                  <rect x={0} y={0} width={width} height={LANE_H} rx={3} style={{ fill: "var(--surface-2)" }} />
                  {hours.map((h) => (
                    <line key={h} x1={Math.round(x(h * 60)) + 0.5} x2={Math.round(x(h * 60)) + 0.5} y1={0} y2={LANE_H} style={gridStyle} />
                  ))}
                  {placed[li].map((c) => (
                    <rect
                      key={c.card.id}
                      x={c.x0}
                      y={3}
                      width={Math.max(1.5, c.x1 - c.x0)}
                      height={LANE_H - 6}
                      rx={1.5}
                      style={{ fill: markFill({ name: c.name, kind: c.kind }, colorFor, idleId, failedId) }}
                    />
                  ))}
                  {!l.cards.length && (
                    <text x={8} y={LANE_H / 2 + 4} className="text-caption" style={{ fill: "var(--subtle-foreground)" }}>
                      {l.future ? "Not yet" : "No recording"}
                    </text>
                  )}
                </svg>
              )}
              {/* Each card is a link to it on the timeline, over its mark (not part of a saved image). */}
              {width > 0 &&
                placed[li].map((c, i) => {
                  const on = cur?.lane === li && cur.i === i;
                  const w = Math.max(4, c.x1 - c.x0);
                  return (
                    <a
                      key={c.card.id}
                      href={href("timeline", { day: l.day, card: c.card.id })}
                      data-lane={li}
                      data-card-index={i}
                      data-export-skip=""
                      tabIndex={on ? 0 : -1}
                      aria-label={cardLabel(l.day, c)}
                      title={cardLabel(l.day, c)}
                      onFocus={() => setActive({ lane: li, i })}
                      className="absolute top-[3px] rounded-[2px] hover:ring-2 hover:ring-foreground/70"
                      style={{ left: Math.min(c.x0, width - w), width: w, height: LANE_H - 6 }}
                    />
                  );
                })}
            </div>
          </React.Fragment>
        ))}
      </div>
    </div>
  );
}

export function LanesTable({ lanes }: { lanes: Lane[] }) {
  return (
    <table className="w-full text-meta">
      <caption className="sr-only">First and last card of each day</caption>
      <thead>
        <tr className="border-b text-left text-muted-foreground">
          <th scope="col" className="py-2 font-normal">
            Day
          </th>
          <th scope="col" className="py-2 font-normal">
            First card
          </th>
          <th scope="col" className="py-2 font-normal">
            Last card ends
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Cards
          </th>
        </tr>
      </thead>
      <tbody>
        {lanes.map((l) => {
          const has = l.cards.length > 0;
          return (
            <tr key={l.day} className="border-b last:border-0">
              <th scope="row" className="py-1.5 text-left font-normal">
                <DayLabel day={l.day} future={l.future} to={href("timeline", { day: l.day })} className="rounded-sm underline-offset-2 hover:underline" />
              </th>
              <td className="py-1.5 font-mono tnum">{has ? clock(Math.min(...l.cards.map((c) => c.startTs))) : <span className="font-sans text-subtle-foreground">{l.future ? "Not yet" : "No recording"}</span>}</td>
              <td className="py-1.5 font-mono tnum">{has ? clock(Math.max(...l.cards.map((c) => c.endTs))) : ""}</td>
              <td className="py-1.5 text-right tnum">{has ? l.cards.length : ""}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

// ---------------------------------------------------------------- heat maps by hour

const HEAT_CELL_H = 22;

/** Hours (index since 4 AM) shown for the week: the activity window, at least 8 hours. */
export function heatHours(week: WeekStats): number[] {
  let first = Math.floor(week.workflow.startMinute / 60);
  let last = Math.min(23, Math.ceil(week.workflow.endMinute / 60) - 1);
  const widen = (h: number) => {
    first = Math.min(first, h);
    last = Math.max(last, h);
  };
  for (const r of week.distractionHours) r.secs.forEach((s, h) => s > 0 && widen(h));
  for (const r of week.focusHours ?? []) r.scores.forEach((s, h) => s !== null && widen(h));
  while (last - first < 7) {
    if (last < 23) last++;
    else first--;
  }
  return Array.from({ length: last - first + 1 }, (_, i) => first + i);
}

/**
 * Days by clock hours, one colored cell per hour: the distraction heat map and the focus heat map.
 * `cell` gives each cell's fill and its words (the cell's tooltip; the table has the same).
 */
function HourHeatmap({
  rows,
  hours,
  cell,
  label,
  legend,
}: {
  rows: { day: string }[];
  hours: number[];
  cell: (row: number, hour: number) => { fill: string; text: string };
  label: string;
  legend: React.ReactNode;
}) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const labelW = 40;
  const gap = 3;
  const top = 18;
  const cw = hours.length ? (width - labelW - gap * (hours.length - 1)) / hours.length : 0;
  const every = cw < 22 ? 3 : cw < 34 ? 2 : 1;
  const h = top + rows.length * (HEAT_CELL_H + gap);

  return (
    <figure className="flex flex-col gap-2.5">
      <div ref={ref} className="w-full" style={{ height: h }}>
        {width > 0 && (
          <svg width={width} height={h} role="img" aria-label={label} className="block">
            {hours.map((hr, i) =>
              (hr - hours[0]) % every === 0 ? (
                <text key={hr} x={labelW + i * (cw + gap)} y={11} className={AXIS_TEXT} style={axisStyle}>
                  {hourLabel(4 + hr)}
                </text>
              ) : null,
            )}
            {rows.map((r, ri) => {
              const y = top + ri * (HEAT_CELL_H + gap);
              return (
                <g key={r.day}>
                  <text x={0} y={y + HEAT_CELL_H / 2 + 4} className="text-caption" style={{ fill: "var(--muted-foreground)" }}>
                    {weekday(r.day)}
                  </text>
                  {hours.map((hr, i) => {
                    const c = cell(ri, hr);
                    return (
                      <rect key={hr} x={labelW + i * (cw + gap)} y={y} width={Math.max(1, cw)} height={HEAT_CELL_H} rx={3} style={{ fill: c.fill }}>
                        <title>{`${formatDayTitle(r.day)}, ${hourLabel(4 + hr)} to ${hourLabel(5 + hr)}: ${c.text}`}</title>
                      </rect>
                    );
                  })}
                </g>
              );
            })}
          </svg>
        )}
      </div>
      <figcaption className="flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground" style={{ paddingLeft: labelW }}>
        {legend}
      </figcaption>
    </figure>
  );
}

function heatStep(secs: number, max: number): number {
  if (secs <= 0 || max <= 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil((secs / max) * 4)));
}

function HeatKey({ fill }: { fill: string }) {
  return <span aria-hidden="true" data-mark="" className="h-2.5 w-4 rounded-[2px]" style={{ background: fill }} />;
}

export function DistractionHeatmap({ week, summary }: { week: WeekStats; summary: string }) {
  const max = Math.max(0, ...week.distractionHours.flatMap((r) => r.secs));
  return (
    <HourHeatmap
      rows={week.distractionHours}
      hours={heatHours(week)}
      label={`Distracted minutes per hour and day. ${summary} Switch to Tables for every hour.`}
      cell={(ri, hr) => {
        const s = week.distractionHours[ri].secs[hr] ?? 0;
        return { fill: `var(--heat-${heatStep(s, max)})`, text: s ? `${formatDuration(s)} distracted` : "no distraction" };
      }}
      legend={
        <>
          Less
          {[0, 1, 2, 3, 4].map((s) => (
            <HeatKey key={s} fill={`var(--heat-${s})`} />
          ))}
          More
          <span className="sr-only">. Darker squares hold more distracted minutes; the most in one hour is {formatDurationShort(max)}.</span>
        </>
      }
    />
  );
}

// ---------------------------------------------------------------- focus heat map (Dayflow's)

/** Dayflow's neutral band (WeeklyFocusHeatmapSection DesignColor.neutralThreshold). */
const NEUTRAL = 0.045;
/** How much of the full color each step of the scale takes (over the content layer). */
const MIX = [30, 55, 78, 100];

/**
 * A focus score (-1 focused to 1 distracted) as a step of the diverging scale: -4…-1 focused,
 * 0 mixed, 1…4 distracted; null for an hour without focus or distraction. Intensity follows
 * Dayflow's colorProgress (|score|^0.72).
 */
export function focusStep(score: number | null): number | null {
  if (score === null) return null;
  const i = Math.min(1, Math.abs(score));
  if (i < NEUTRAL) return 0;
  const step = Math.min(4, Math.max(1, Math.ceil(Math.pow(i, 0.72) * 4)));
  return score < 0 ? -step : step;
}

/** Fill of a step: focus in the first chart color, distraction in the second, both mixed into the
 *  content layer; the strongest step is the full color (≥ 3:1 on the content layer in both themes). */
export function focusFill(step: number | null): string {
  if (step === null) return "var(--heat-0)";
  if (step === 0) return "color-mix(in oklab, var(--chart-5) 40%, var(--surface-1))";
  const color = step < 0 ? "var(--chart-1)" : "var(--chart-2)";
  const p = MIX[Math.abs(step) - 1];
  return p === 100 ? color : `color-mix(in oklab, ${color} ${p}%, var(--surface-1))`;
}

/** "Mostly focused" / "Mostly distracted" / "Mixed" for a score. */
export function leaning(score: number | null): string {
  const step = focusStep(score);
  if (step === null) return "No focus or distraction";
  if (step === 0) return "Mixed";
  const strong = Math.abs(step) >= 3;
  return step < 0 ? (strong ? "Focused" : "Mostly focused") : strong ? "Distracted" : "Mostly distracted";
}

export function FocusHeatmap({ week, summary }: { week: WeekStats; summary: string }) {
  const rows = week.focusHours ?? [];
  return (
    <HourHeatmap
      rows={rows}
      hours={heatHours(week)}
      label={`Focused against distracted time per hour and day. ${summary} Switch to Tables for every hour.`}
      cell={(ri, hr) => {
        const r = rows[ri];
        const score = r.scores[hr] ?? null;
        const f = r.focusSecs[hr] ?? 0;
        const d = r.distractedSecs[hr] ?? 0;
        const text = score === null ? "no focus or distraction" : `${leaning(score).toLowerCase()}, ${formatDuration(f)} focused, ${formatDuration(d)} distracted`;
        return { fill: focusFill(focusStep(score)), text };
      }}
      legend={
        <>
          Focused
          {[-4, -3, -2, -1, 0, 1, 2, 3, 4].map((s) => (
            <HeatKey key={s} fill={focusFill(s)} />
          ))}
          Distracted
          <span className="ml-3 inline-flex items-center gap-1.5">
            <HeatKey fill={focusFill(null)} /> No focus or distraction
          </span>
          <span className="sr-only">. Blue squares lean to focus, plum squares to distraction, gray ones are mixed; the stronger the color, the clearer the hour.</span>
        </>
      }
    />
  );
}

export function FocusHourTable({ week }: { week: WeekStats }) {
  const rows = (week.focusHours ?? []).flatMap((r) =>
    r.scores.map((score, h) => ({ day: r.day, h, score, f: r.focusSecs[h] ?? 0, d: r.distractedSecs[h] ?? 0 })).filter((x) => x.score !== null),
  );
  if (!rows.length) return <p className="text-meta text-muted-foreground">No focus or distraction this week.</p>;
  return (
    <table className="w-full text-meta">
      <caption className="sr-only">Focused and distracted time per hour, hours with any</caption>
      <thead>
        <tr className="border-b text-left text-muted-foreground">
          <th scope="col" className="py-2 font-normal">
            Day
          </th>
          <th scope="col" className="py-2 font-normal">
            Hour
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Focused
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Distracted
          </th>
          <th scope="col" className="py-2 pl-3 font-normal">
            The hour leans
          </th>
        </tr>
      </thead>
      <tbody>
        {rows.map((x) => (
          <tr key={`${x.day}-${x.h}`} className="border-b last:border-0">
            <th scope="row" className="py-1.5 text-left font-normal">
              {dayShortLabel(x.day)}
            </th>
            <td className="py-1.5 font-mono tnum">
              {hourLabel(4 + x.h)} – {hourLabel(5 + x.h)}
            </td>
            <td className="py-1.5 text-right tnum">{durationOrDash(x.f)}</td>
            <td className="py-1.5 text-right tnum">{durationOrDash(x.d)}</td>
            <td className="py-1.5 pl-3">
              <span className="inline-flex items-center gap-1.5">
                <HeatKey fill={focusFill(focusStep(x.score))} />
                {leaning(x.score)}
              </span>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function HeatTable({ week }: { week: WeekStats }) {
  const rows = week.distractionHours.flatMap((r) => r.secs.map((s, h) => ({ day: r.day, h, s })).filter((x) => x.s > 0));
  if (!rows.length) return <p className="text-meta text-muted-foreground">No distractions noted this week.</p>;
  return (
    <table className="w-full text-meta">
      <caption className="sr-only">Distracted time per hour, hours with any distraction</caption>
      <thead>
        <tr className="border-b text-left text-muted-foreground">
          <th scope="col" className="py-2 font-normal">
            Day
          </th>
          <th scope="col" className="py-2 font-normal">
            Hour
          </th>
          <th scope="col" className="py-2 text-right font-normal">
            Distracted
          </th>
        </tr>
      </thead>
      <tbody>
        {rows.map((x) => (
          <tr key={`${x.day}-${x.h}`} className="border-b last:border-0">
            <th scope="row" className="py-1.5 text-left font-normal">
              {dayShortLabel(x.day)}
            </th>
            <td className="py-1.5 font-mono tnum">
              {hourLabel(4 + x.h)} – {hourLabel(5 + x.h)}
            </td>
            <td className="py-1.5 text-right tnum">{formatDuration(x.s)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
