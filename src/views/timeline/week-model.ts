import type { Card } from "@/api";
// Relative imports with extensions, so `node --test` can load this file (model.test.mjs).
import { DAY_START_HOUR, dayBounds, shiftDay, weekStart } from "../../lib/time.ts";
import { GAP_MIN_SECS, NIGHT_FROM_HOUR, NIGHT_TO_HOUR, fitSpans, isFailed, wholeDayAxis, type Axis } from "./model.ts";

/*
 * Pure helpers for the Timeline's Week mode (Dayflow Views/UI/MainView/WeekTimelineGridView.swift,
 * TimelineActivityLoader.resolveDisplaySegments): the seven days of the week, each card placed on
 * one time scale shared by all days, in seconds since the day's 4 AM start on the wall clock (so
 * 9 AM is at the same height on every day, also on a DST change day), the shared axis ("Fit to
 * recorded hours" over the whole week), and the arrow-key moves between cards. No React here.
 */

/** Seconds in the time scale of a day column: 4 AM to 4 AM. */
export const DAY_SECS = 24 * 3600;
/** Dayflow failureGroupingGapTolerance: failed cards this close form one item. */
const FAILED_RUN_GAP = 60;

/** The seven days (Monday first) of the week that contains `day`. */
export function weekDays(day: string): string[] {
  const start = weekStart(day);
  return Array.from({ length: 7 }, (_, i) => shiftDay(start, i));
}

/** Seconds since 4 AM on the wall clock. */
export function wallOffset(ts: number): number {
  const d = new Date(ts * 1000);
  return ((d.getHours() - DAY_START_HOUR + 24) % 24) * 3600 + d.getMinutes() * 60 + d.getSeconds();
}

export interface WeekItem {
  /** The card a click opens: the first card of a run of failed cards. */
  card: Card;
  /** Cards shown by this item: more than one for a run of failed cards. */
  count: number;
  failed: boolean;
  /** Column, 0 = Monday. */
  col: number;
  /** The span shown, in seconds since 4 AM (start < end ≤ DAY_SECS). */
  start: number;
  end: number;
  /** The same span as timestamps (after overlaps were resolved). */
  startTs: number;
  endTs: number;
}

interface Seg {
  card: Card;
  count: number;
  failed: boolean;
  start: number;
  end: number;
}

/**
 * Dayflow resolveDisplaySegments for one day: runs of failed cards (≤ 60 s apart) become one
 * item; where two cards overlap the shorter one keeps its time and the longer one is trimmed
 * (to its larger remaining side), or dropped when nothing is left of it.
 */
export function displaySegments(cards: Card[], from: number, to: number): Seg[] {
  const sorted = cards
    .map((c) => ({ card: c, start: Math.max(c.startTs, from), end: Math.min(c.endTs, to) }))
    .filter((s) => s.end > s.start)
    .sort((a, b) => a.start - b.start || a.end - b.end || a.card.id - b.card.id);
  const segs: Seg[] = [];
  for (const s of sorted) {
    const failed = isFailed(s.card);
    const last = segs[segs.length - 1];
    if (failed && last?.failed && s.start - last.end <= FAILED_RUN_GAP) {
      last.end = Math.max(last.end, s.end);
      last.count += 1;
    } else {
      segs.push({ card: s.card, count: 1, failed, start: s.start, end: s.end });
    }
  }
  for (let pass = 0, changed = true; changed && pass < 8; pass++) {
    changed = false;
    for (let i = 0; i < segs.length; i++) {
      for (let j = i + 1; j < segs.length; j++) {
        const a = segs[i];
        const b = segs[j];
        if (b.start >= a.end) break;
        const small = a.end - a.start <= b.end - b.start ? i : j;
        const large = small === i ? j : i;
        const sm = segs[small];
        const lg = { ...segs[large] };
        if (lg.start < sm.start && sm.end < lg.end) {
          if (lg.end - sm.end >= sm.start - lg.start) lg.start = sm.end;
          else lg.end = sm.start;
        } else if (sm.start <= lg.start && lg.start < sm.end) lg.start = sm.end;
        else if (sm.start < lg.end && lg.end <= sm.end) lg.end = sm.start;
        if (lg.end <= lg.start) segs.splice(large, 1);
        else if (lg.start !== segs[large].start || lg.end !== segs[large].end) segs[large] = lg;
        else continue;
        segs.sort((x, y) => x.start - y.start || x.end - y.end);
        changed = true;
        j = i;
      }
    }
  }
  return segs;
}

/** The items of each day column, in time order. `cardsByDay[i]` are the cards of `days[i]`. */
export function weekColumns(days: string[], cardsByDay: (Card[] | null)[]): WeekItem[][] {
  return days.map((day, col) => {
    const [from, to] = dayBounds(day);
    return displaySegments(cardsByDay[col] ?? [], from, to).map((s) => {
      const start = s.start === from ? 0 : wallOffset(s.start);
      return {
        card: s.card,
        count: s.count,
        failed: s.failed,
        col,
        start,
        end: Math.min(DAY_SECS, start + (s.end - s.start)),
        startTs: s.start,
        endTs: s.end,
      };
    });
  });
}

const floorH = (t: number) => Math.floor(t / 3600) * 3600;
const ceilH = (t: number) => Math.ceil(t / 3600) * 3600;

/**
 * The shared axis of the week: the whole day, or "Fit to recorded hours" over every card of the
 * week (and now, when the week holds today), with stretches of an hour or more without any card
 * on any day cut out.
 */
export function weekAxis(cols: WeekItem[][], now: number | null, fit: boolean): Axis {
  if (!fit) return wholeDayAxis(0, DAY_SECS);
  const spans: [number, number][] = cols.flat().map((it) => [it.start, it.end]);
  if (spans.length === 0) return wholeDayAxis(0, DAY_SECS);
  if (now !== null) spans.push([now, now]);
  return fitSpans(spans, 0, DAY_SECS, floorH, ceilH);
}

/** Whole hours (seconds since 4 AM) inside the windows, `step` hours apart from 4 AM. */
export function offsetTicks(axis: Axis, step: number): number[] {
  const out = new Set<number>();
  for (const w of axis.windows) {
    for (let t = ceilH(w.start); t <= w.end; t += 3600) if ((t / 3600) % step === 0) out.add(t);
  }
  return [...out].sort((a, b) => a - b);
}

/** Wall-clock hour (0–23) of a tick. */
export function tickHour(t: number): number {
  return (DAY_START_HOUR + Math.round(t / 3600)) % 24;
}

/** Night hours in seconds since 4 AM: 4 to 6 AM and 10 PM to 4 AM. */
export function nightOffsets(): [number, number][] {
  return [
    [0, (NIGHT_TO_HOUR - DAY_START_HOUR) * 3600],
    [(NIGHT_FROM_HOUR - DAY_START_HOUR) * 3600, DAY_SECS],
  ];
}

/** Stretches of 5 minutes or more between the items of one column ("Not recorded"). */
export function columnGaps(items: WeekItem[]): [number, number][] {
  const out: [number, number][] = [];
  let end: number | null = null;
  for (const it of items) {
    if (end !== null && it.start - end >= GAP_MIN_SECS) out.push([end, it.start]);
    end = end === null ? it.end : Math.max(end, it.end);
  }
  return out;
}

/** Recorded seconds of a column: the items that are not failed. */
export function columnRecorded(items: WeekItem[]): number {
  return items.filter((it) => !it.failed).reduce((a, it) => a + it.endTs - it.startTs, 0);
}

export interface GridPos {
  col: number;
  i: number;
}

/**
 * The arrow-key move from `at`: ↑/↓ the previous/next card of the day, Home/End its first/last,
 * ←/→ the card nearest in time on the closest day before/after that has cards. null: no move.
 */
export function moveInWeek(cols: WeekItem[][], at: GridPos, key: string): GridPos | null {
  const col = cols[at.col];
  const cur = col?.[at.i];
  if (!cur) return null;
  if (key === "ArrowDown") return at.i + 1 < col.length ? { col: at.col, i: at.i + 1 } : null;
  if (key === "ArrowUp") return at.i > 0 ? { col: at.col, i: at.i - 1 } : null;
  if (key === "Home") return at.i > 0 ? { col: at.col, i: 0 } : null;
  if (key === "End") return at.i < col.length - 1 ? { col: at.col, i: col.length - 1 } : null;
  if (key !== "ArrowLeft" && key !== "ArrowRight") return null;
  const dir = key === "ArrowRight" ? 1 : -1;
  const mid = (cur.start + cur.end) / 2;
  for (let c = at.col + dir; c >= 0 && c < cols.length; c += dir) {
    const items = cols[c];
    if (!items.length) continue;
    let best = 0;
    items.forEach((it, i) => {
      if (Math.abs((it.start + it.end) / 2 - mid) < Math.abs((items[best].start + items[best].end) / 2 - mid)) best = i;
    });
    return { col: c, i: best };
  }
  return null;
}

/** Where the week's one tab stop starts: the first card of `day`, else of the nearest day with cards. */
export function firstStop(cols: WeekItem[][], days: string[], day: string): GridPos | null {
  const at = Math.max(0, days.indexOf(day));
  for (let d = 0; d < cols.length; d++) {
    for (const c of [at - d, at + d]) if (c >= 0 && c < cols.length && cols[c].length) return { col: c, i: 0 };
  }
  return null;
}
