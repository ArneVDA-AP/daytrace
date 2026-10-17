import * as React from "react";
import type { Card, Category } from "@/api";
import { href } from "@/lib/router";
import { clock, dateOf, formatDayTitle, formatDuration, formatDurationShort, hourLabel, weekday } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { CategoryTotal, Kind, WeekStats } from "@/views/insights/api";
import { byCategory, durationOrDash, kindOf, percent, wallMinute } from "@/views/insights/format";
import { CategoryMark, Legend, SvgPatterns, useWidth, type LegendItem } from "@/views/insights/parts";

/*
 * Weekly charts, hand-drawn in SVG (no chart library): horizontal stacked bars for time per day,
 * diverging bars for focus and distraction, one lane per day for the week by hour, and a one-hue
 * heat map for distraction by hour. Each has a table with the same numbers (the page's Tables
 * switch, ?view=table). Rows that open a day are HTML links next to the SVG, so focus and
 * keyboard behave like every other link. Text stays in text colours; colour is only on marks.
 */

const AXIS_TEXT = "font-mono text-axis";
const axisStyle: React.CSSProperties = { fill: "var(--chart-axis)" };
const gridStyle: React.CSSProperties = { stroke: "var(--chart-grid)" };

/** Mon 12 */
export function dayShortLabel(day: string): string {
  return `${weekday(day)} ${dateOf(day).getDate()}`;
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
              <a
                href={href("daily", { day: r.day })}
                aria-label={`${dayShortLabel(r.day)}: ${r.future ? "not yet" : r.secs ? `${formatDuration(r.secs)} recorded, ${summary}` : "no recording"}. Open this day in Daily`}
                className="w-fit rounded-sm text-meta text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
              >
                {dayShortLabel(r.day)}
              </a>
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
                <a href={href("daily", { day: r.day })} className="rounded-sm underline-offset-2 hover:underline">
                  {dayShortLabel(r.day)}
                </a>
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
            Share distracted
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
            <td className="py-1.5 text-right text-muted-foreground tnum">{r.focus + r.distracted > 0 ? percent(r.distracted, r.focus + r.distracted) : ""}</td>
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

  return (
    <div className="flex flex-col gap-3">
      <Legend items={legend} />
      <div className="grid grid-cols-[4rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5">
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
        {lanes.map((l) => (
          <React.Fragment key={l.day}>
            <a
              href={href("timeline", { day: l.day })}
              aria-label={`${dayShortLabel(l.day)}: ${laneSummary(l)}. Open this day in the timeline`}
              className="w-fit rounded-sm text-meta text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
            >
              {dayShortLabel(l.day)}
            </a>
            <div className="relative min-w-0" style={{ height: LANE_H }}>
              {width > 0 && (
                <svg width={width} height={LANE_H} className="absolute inset-0 block" aria-hidden="true">
                  <SvgPatterns idleId={idleId} failedId={failedId} />
                  <rect x={0} y={0} width={width} height={LANE_H} rx={3} style={{ fill: "var(--surface-2)" }} />
                  {hours.map((h) => (
                    <line key={h} x1={Math.round(x(h * 60)) + 0.5} x2={Math.round(x(h * 60)) + 0.5} y1={0} y2={LANE_H} style={gridStyle} />
                  ))}
                  {l.cards.map((c) => {
                    const name = c.category.trim() || "Uncategorized";
                    const kind: Kind = c.isIdleShortcut ? "idle" : kindOf(name, categories);
                    const a = wallMinute(c.startTs);
                    const b = a + (c.endTs - c.startTs) / 60;
                    const x0 = Math.max(0, x(a));
                    const x1 = Math.min(width, x(b));
                    if (x1 <= x0) return null;
                    return (
                      <rect key={c.id} x={x0} y={3} width={Math.max(1.5, x1 - x0)} height={LANE_H - 6} rx={1.5} style={{ fill: markFill({ name, kind }, colorFor, idleId, failedId) }}>
                        <title>{`${clock(c.startTs)} – ${clock(c.endTs)}: ${kind === "system" ? "Not analyzed" : name}, ${c.title}`}</title>
                      </rect>
                    );
                  })}
                  {!l.cards.length && (
                    <text x={8} y={LANE_H / 2 + 4} className="text-caption" style={{ fill: "var(--subtle-foreground)" }}>
                      {l.future ? "Not yet" : "No recording"}
                    </text>
                  )}
                </svg>
              )}
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
                <a href={href("timeline", { day: l.day })} className="rounded-sm underline-offset-2 hover:underline">
                  {dayShortLabel(l.day)}
                </a>
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

// ---------------------------------------------------------------- distraction heat map

const HEAT_CELL_H = 22;

/** Hours (index since 4 AM) shown for the week: the activity window, at least 8 hours. */
export function heatHours(week: WeekStats): number[] {
  let first = Math.floor(week.workflow.startMinute / 60);
  let last = Math.min(23, Math.ceil(week.workflow.endMinute / 60) - 1);
  for (const r of week.distractionHours)
    r.secs.forEach((s, h) => {
      if (s > 0) {
        first = Math.min(first, h);
        last = Math.max(last, h);
      }
    });
  while (last - first < 7) {
    if (last < 23) last++;
    else first--;
  }
  return Array.from({ length: last - first + 1 }, (_, i) => first + i);
}

function heatStep(secs: number, max: number): number {
  if (secs <= 0 || max <= 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil((secs / max) * 4)));
}

export function DistractionHeatmap({ week, summary }: { week: WeekStats; summary: string }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const hours = heatHours(week);
  const max = Math.max(0, ...week.distractionHours.flatMap((r) => r.secs));
  const labelW = 40;
  const gap = 3;
  const top = 18;
  const cw = hours.length ? (width - labelW - gap * (hours.length - 1)) / hours.length : 0;
  const every = cw < 22 ? 3 : cw < 34 ? 2 : 1;
  const h = top + week.distractionHours.length * (HEAT_CELL_H + gap);

  return (
    <figure className="flex flex-col gap-2.5">
      <div ref={ref} className="w-full" style={{ height: h }}>
        {width > 0 && (
          <svg width={width} height={h} role="img" aria-label={`Distracted minutes per hour and day. ${summary} Switch to Tables for every hour.`} className="block">
            {hours.map((hr, i) =>
              (hr - hours[0]) % every === 0 ? (
                <text key={hr} x={labelW + i * (cw + gap)} y={11} className={AXIS_TEXT} style={axisStyle}>
                  {hourLabel(4 + hr)}
                </text>
              ) : null,
            )}
            {week.distractionHours.map((r, ri) => {
              const y = top + ri * (HEAT_CELL_H + gap);
              return (
                <g key={r.day}>
                  <text x={0} y={y + HEAT_CELL_H / 2 + 4} className="text-caption" style={{ fill: "var(--muted-foreground)" }}>
                    {weekday(r.day)}
                  </text>
                  {hours.map((hr, i) => {
                    const s = r.secs[hr] ?? 0;
                    return (
                      <rect key={hr} x={labelW + i * (cw + gap)} y={y} width={Math.max(1, cw)} height={HEAT_CELL_H} rx={3} style={{ fill: `var(--heat-${heatStep(s, max)})` }}>
                        <title>{`${formatDayTitle(r.day)}, ${hourLabel(4 + hr)} to ${hourLabel(5 + hr)}: ${s ? `${formatDuration(s)} distracted` : "no distraction"}`}</title>
                      </rect>
                    );
                  })}
                </g>
              );
            })}
          </svg>
        )}
      </div>
      <figcaption className="flex items-center gap-1.5 text-caption text-muted-foreground" style={{ paddingLeft: labelW }}>
        Less
        {[0, 1, 2, 3, 4].map((s) => (
          <span key={s} aria-hidden="true" data-mark="" className="h-2.5 w-4 rounded-[2px]" style={{ background: `var(--heat-${s})` }} />
        ))}
        More
        <span className="sr-only">. Darker squares hold more distracted minutes; the most in one hour is {formatDurationShort(max)}.</span>
      </figcaption>
    </figure>
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
