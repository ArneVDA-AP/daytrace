import type { Card, Category } from "@/api";
import { DAY_START_HOUR, dateOf } from "@/lib/time";
import type { FailedGroup } from "./api";

/*
 * Pure helpers for the Timeline: lanes and category totals, the axis of the day trace (whole day
 * or fitted to the recorded hours), and the rows of the card list. No React here.
 */

/** Gaps of this length or more show as "Not recording" (list rows, hatched spans on the trace). */
export const GAP_MIN_SECS = 5 * 60;
/** In "Fit to recorded hours", stretches without recording of at least this length are cut out. */
export const BREAK_MIN_SECS = 60 * 60;
/** Width of an axis break on the trace, in px. */
export const BREAK_PX = 22;
/** Night hours shaded on the trace (a fixed convention, named in the legend). */
export const NIGHT_FROM_HOUR = 22;
export const NIGHT_TO_HOUR = 6;

export const keyOf = (name: string) => name.trim().toLowerCase();

export function isFailed(c: Card): boolean {
  return keyOf(c.category) === "system";
}

/** Idle cards: the away shortcut, or a card in the settings' idle category. */
export function isIdleCard(c: Card, idleKey: string | null): boolean {
  return c.isIdleShortcut || (idleKey !== null && keyOf(c.category) === idleKey);
}

export interface Lane {
  key: string;
  name: string;
  isIdle: boolean;
}

/**
 * Category lanes of the trace and the filter chips, in the settings order with the idle category
 * last (Dayflow TabFilterBar), then names that only appear on cards. Failed cards have no lane.
 */
export function lanesFor(categories: Category[], cards: Card[]): Lane[] {
  const out: Lane[] = [];
  const seen = new Set<string>();
  const add = (name: string, isIdle: boolean) => {
    const k = keyOf(name);
    if (!k || k === "system" || seen.has(k)) return;
    seen.add(k);
    out.push({ key: k, name: name.trim(), isIdle });
  };
  categories.filter((c) => !c.isIdle).forEach((c) => add(c.name, false));
  cards.forEach((c) => add(c.category, c.isIdleShortcut));
  categories.filter((c) => c.isIdle).forEach((c) => add(c.name, true));
  // An idle category that only appears on cards still goes last.
  return [...out.filter((l) => !l.isIdle), ...out.filter((l) => l.isIdle)];
}

/** Lane of a card: idle shortcut cards go to the idle lane. */
export function laneKeyOf(c: Card, lanes: Lane[]): string {
  if (c.isIdleShortcut) return lanes.find((l) => l.isIdle)?.key ?? keyOf(c.category);
  return keyOf(c.category);
}

interface Span {
  card: Card;
  start: number;
  end: number;
}

/**
 * Non-failed cards clipped to [from, to) with overlaps removed, so each second counts once
 * (Dayflow DaySummaryStats.removeOverlaps: sorted by start, longer first on equal starts).
 */
export function normalizedSpans(cards: Card[], from: number, to: number): Span[] {
  const spans = cards
    .filter((c) => !isFailed(c))
    .map((c) => ({ card: c, start: Math.max(c.startTs, from), end: Math.min(c.endTs, to) }))
    .filter((s) => s.end > s.start)
    .sort((a, b) => a.start - b.start || b.end - a.end || a.card.id - b.card.id);
  const out: Span[] = [];
  let covered = from;
  for (const s of spans) {
    const start = Math.max(s.start, covered);
    if (s.end <= start) continue;
    out.push({ card: s.card, start, end: s.end });
    covered = Math.max(covered, s.end);
  }
  return out;
}

/** Seconds per lane key, over the normalized spans. */
export function laneTotals(cards: Card[], lanes: Lane[], from: number, to: number): Map<string, number> {
  const m = new Map<string, number>();
  for (const s of normalizedSpans(cards, from, to)) {
    const k = laneKeyOf(s.card, lanes);
    m.set(k, (m.get(k) ?? 0) + (s.end - s.start));
  }
  return m;
}

export function recordedSecs(cards: Card[], from: number, to: number): number {
  return normalizedSpans(cards, from, to).reduce((a, s) => a + s.end - s.start, 0);
}

// ---------------------------------------------------------------- trace axis

export interface AxisWindow {
  start: number;
  end: number;
}

export interface Axis {
  windows: AxisWindow[];
  /** Total seconds shown (sum of the windows). */
  shown: number;
}

function floorHour(ts: number): number {
  const d = new Date(ts * 1000);
  d.setMinutes(0, 0, 0);
  return d.getTime() / 1000;
}

function ceilHour(ts: number): number {
  const f = floorHour(ts);
  return f === ts ? ts : floorHour(ts + 3600);
}

/** The whole logical day as one window. */
export function wholeDayAxis(from: number, to: number): Axis {
  return { windows: [{ start: from, end: to }], shown: to - from };
}

/**
 * "Fit to recorded hours": the hours that have cards (and now, on today), padded to whole hours;
 * stretches of an hour or more without anything are cut out and drawn as axis breaks.
 */
export function fitAxis(cards: Card[], from: number, to: number, now: number | null): Axis {
  const spans: [number, number][] = cards
    .map((c) => [Math.max(c.startTs, from), Math.min(c.endTs, to)] as [number, number])
    .filter(([s, e]) => e > s);
  if (now !== null && now > from && now < to) spans.push([now, now]);
  if (spans.length === 0) return wholeDayAxis(from, to);
  spans.sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const [s, e] of spans) {
    const last = merged[merged.length - 1];
    if (last && s - last[1] < BREAK_MIN_SECS) last[1] = Math.max(last[1], e);
    else merged.push([s, e]);
  }
  const windows: AxisWindow[] = [];
  for (const [s, e] of merged) {
    const start = Math.max(from, floorHour(s));
    const end = Math.min(to, Math.max(ceilHour(e), start + 3600));
    const last = windows[windows.length - 1];
    if (last && start - last.end < BREAK_MIN_SECS) last.end = Math.max(last.end, end);
    else windows.push({ start, end });
  }
  // Nothing to cut: show the plain day.
  const shown = windows.reduce((a, w) => a + w.end - w.start, 0);
  if (windows.length === 1 && shown >= to - from - 3600) return wholeDayAxis(from, to);
  return { windows, shown };
}

/** Maps a timestamp to an x position in px for a plot `width` px wide (null when cut out). */
export function scaleFor(axis: Axis, width: number) {
  const breaks = axis.windows.length - 1;
  const usable = Math.max(1, width - breaks * BREAK_PX);
  const offsets: number[] = [];
  let acc = 0;
  for (const w of axis.windows) {
    offsets.push(acc);
    acc += w.end - w.start;
  }
  const x = (ts: number): number | null => {
    for (let i = 0; i < axis.windows.length; i++) {
      const w = axis.windows[i];
      if (ts >= w.start && ts <= w.end) return i * BREAK_PX + ((offsets[i] + ts - w.start) / axis.shown) * usable;
    }
    return null;
  };
  /** Left edge of break i (between window i and i + 1). */
  const breakX = (i: number) => i * BREAK_PX + ((offsets[i] + axis.windows[i].end - axis.windows[i].start) / axis.shown) * usable;
  return { x, breakX, pxPerHour: (usable / axis.shown) * 3600 };
}

/** Whole hours inside the windows, `step` hours apart (aligned to the 4 AM day start). */
export function hourTicks(axis: Axis, step: number): number[] {
  const out = new Set<number>();
  for (const w of axis.windows) {
    const d = new Date(ceilHour(w.start) * 1000);
    while (d.getTime() / 1000 <= w.end) {
      const h = d.getHours();
      if ((((h - DAY_START_HOUR) % step) + step) % step === 0) out.add(d.getTime() / 1000);
      d.setHours(h + 1);
    }
  }
  return [...out].sort((a, b) => a - b);
}

/** Night spans [start, end) of the logical day: 4 AM to 6 AM and 10 PM to 4 AM. */
export function nightSpans(day: string): [number, number][] {
  const d = dateOf(day);
  const t = (dd: number, h: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + dd, h).getTime() / 1000;
  return [
    [t(0, DAY_START_HOUR), t(0, NIGHT_TO_HOUR)],
    [t(0, NIGHT_FROM_HOUR), t(1, DAY_START_HOUR)],
  ];
}

/** Stretches between cards (≥ 5 min) where nothing was recorded. */
export function gapSpans(cards: Card[]): [number, number][] {
  const sorted = [...cards].sort((a, b) => a.startTs - b.startTs);
  const out: [number, number][] = [];
  let end: number | null = null;
  for (const c of sorted) {
    if (end !== null && c.startTs - end >= GAP_MIN_SECS) out.push([end, c.startTs]);
    end = end === null ? c.endTs : Math.max(end, c.endTs);
  }
  return out;
}

// ---------------------------------------------------------------- list rows

export type Row =
  | { kind: "card"; id: number; card: Card }
  | { kind: "run"; id: number; group: FailedGroup; cards: Card[] }
  | { kind: "gap"; id: string; start: number; end: number };

/**
 * Rows of the card list: cards in time order, runs of two or more failed cards collapsed into one
 * row (Dayflow TimelineActivityLoader), and "Not recording" rows for gaps of 5 minutes or more.
 */
export function buildRows(cards: Card[], groups: FailedGroup[]): Row[] {
  const groupOf = new Map<number, FailedGroup>();
  groups.filter((g) => g.cardIds.length > 1).forEach((g) => g.cardIds.forEach((id) => groupOf.set(id, g)));
  const shown = new Set<FailedGroup>();
  const rows: Row[] = [];
  let lastEnd: number | null = null;
  for (const c of cards) {
    const g = groupOf.get(c.id);
    if (g && shown.has(g)) continue;
    const start = g ? g.startTs : c.startTs;
    const end = g ? g.endTs : c.endTs;
    if (lastEnd !== null && start - lastEnd >= GAP_MIN_SECS) rows.push({ kind: "gap", id: `gap-${c.id}`, start: lastEnd, end: start });
    lastEnd = Math.max(lastEnd ?? end, end);
    if (g) {
      shown.add(g);
      rows.push({ kind: "run", id: g.cardIds[0], group: g, cards: cards.filter((x) => g.cardIds.includes(x.id)) });
    } else {
      rows.push({ kind: "card", id: c.id, card: c });
    }
  }
  return rows;
}

/** Ids that J/K and the arrow keys step through: one per card row, one per collapsed run. */
export function navIds(rows: Row[]): number[] {
  return rows.filter((r): r is Exclude<Row, { kind: "gap" }> => r.kind !== "gap").map((r) => r.id);
}

/** Index in navIds of the row that holds `cardId`. */
export function navIndexOf(rows: Row[], cardId: number | null): number {
  if (cardId === null) return -1;
  const items = rows.filter((r) => r.kind !== "gap");
  return items.findIndex((r) => (r.kind === "run" ? r.group.cardIds.includes(cardId) : r.id === cardId));
}

// ---------------------------------------------------------------- step-by-step log

const CLOCK = String.raw`\d{1,2}:\d{2}\s?[AP]M`;
const LOG_RE = new RegExp(String.raw`^(${CLOCK})(?:\s*[-–]\s*(${CLOCK}))?\s*[:\-–]?\s*(.*)$`, "i");

/** "9:20 AM - 9:35 AM: Wrote the scheduler" → time and text. Lines without a time keep their text. */
export function parseLogLine(line: string): { time: string | null; text: string } {
  const l = line.trim().replace(/^[-•*]\s*/, "");
  const m = l.match(LOG_RE);
  if (m && m[3]) return { time: m[1].toUpperCase().replace(/\s?([AP]M)$/, " $1"), text: m[3] };
  return { time: null, text: l };
}

/** A card as plain text (the detail's "Copy card as text"). */
export function cardText(c: Card): string {
  const lines = [`${c.start} – ${c.end}  ${c.title}`, isFailed(c) ? "Not analyzed" : [c.category, c.subcategory].filter(Boolean).join(" / ")];
  if (c.summary) lines.push("", c.summary);
  const log = c.detailedSummary.split("\n").map((l) => l.trim()).filter(Boolean);
  if (log.length && !isFailed(c)) lines.push("", ...log.map((l) => `- ${l.replace(/^[-•*]\s*/, "")}`));
  if (c.distractions.length) {
    lines.push("", "Interruptions:");
    c.distractions.forEach((d) => lines.push(`- ${d.startTime} – ${d.endTime} ${d.title}${d.summary ? `: ${d.summary}` : ""}`));
  }
  return lines.join("\n");
}
