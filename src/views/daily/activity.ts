import type { Card, Category } from "@/api";
import { at, dayBounds } from "@/lib/time";
import type { DistractionItem, Kind } from "@/views/insights/api";
import { categoryRank, kindOf } from "@/views/insights/format";

/*
 * The Daily activity grid: 96 quarter hours from 4 AM to 4 AM. Each quarter hour keeps the
 * seconds of every category that filled it (its per-category share), not one dominant color,
 * so a quarter hour that was 10 minutes of Work and 5 of Distraction shows both. Failed cards
 * ("System") are a share too, drawn as "Not analyzed"; what is left is not recorded, or not yet
 * when it is still ahead today. On the night clocks go forward, the quarter hours of the hour that
 * does not exist stay empty (`skipped`) instead of repeating the hour after it.
 */

export const SLOTS = 96;

export interface Share {
  /** Category name as on the card; "System" for failed cards. */
  name: string;
  kind: Kind;
  secs: number;
}

export interface Slot {
  index: number;
  /** Wall-clock label of the quarter hour ("2:15 AM"), also when the clock skipped it. */
  label: string;
  start: number;
  end: number;
  /** Starts after now: the time is still ahead. */
  future: boolean;
  /** The clock skipped this quarter hour (the change to daylight saving time): zero length. */
  skipped: boolean;
  /** In category order; the seconds never add up to more than the slot. */
  shares: Share[];
  /** Seconds covered by any card. */
  filled: number;
  /** The card that fills most of the quarter hour (opened on click). */
  top: { id: number; title: string } | null;
  /** Distractions noted inside cards during the quarter hour. */
  noted: string[];
}

function wallTime(i: number): { h: number; m: number } {
  return { h: (4 + Math.floor(i / 4)) % 24, m: (i % 4) * 15 };
}

/** "2:15 AM" for quarter hour `i`, from the wall clock (the clock() of a skipped time is wrong). */
export function slotLabel(i: number): string {
  const { h, m } = wallTime(i);
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${String(m).padStart(2, "0")} ${h < 12 ? "AM" : "PM"}`;
}

/** Start of quarter hour `i` on the wall clock (DST days keep their wall-clock labels). */
function slotStart(day: string, i: number): number {
  const { h, m } = wallTime(i);
  return at(day, h, m);
}

/** The wall-clock time of quarter hour `i` does not exist on `day` (clocks went forward). */
function isSkipped(day: string, i: number): boolean {
  const { h, m } = wallTime(i);
  const d = new Date(slotStart(day, i) * 1000);
  return d.getHours() !== h || d.getMinutes() !== m;
}

export function quarterSlots(day: string, cards: Card[], categories: Category[], distractions: DistractionItem[], now?: number): Slot[] {
  const [, dayEnd] = dayBounds(day);
  const rank = categoryRank(categories);
  const noted = distractions.filter((d) => d.withinCard);
  const slots: Slot[] = [];
  for (let i = 0; i < SLOTS; i++) {
    const skipped = isSkipped(day, i);
    const start = slotStart(day, i);
    const end = skipped ? start : i === SLOTS - 1 ? dayEnd : slotStart(day, i + 1);
    const len = Math.max(0, end - start);
    const byName = new Map<string, Share>();
    let best: { secs: number; card: Card } | null = null;
    for (const c of cards) {
      const o = Math.min(end, c.endTs) - Math.max(start, c.startTs);
      if (o <= 0) continue;
      const name = c.category.trim() || "Uncategorized";
      const key = name.toLowerCase();
      const share = byName.get(key) ?? { name, kind: c.isIdleShortcut ? "idle" : kindOf(name, categories), secs: 0 };
      share.secs += o;
      byName.set(key, share);
      if (!best || o > best.secs) best = { secs: o, card: c };
    }
    let shares = [...byName.values()];
    const total = shares.reduce((a, s) => a + s.secs, 0);
    // Overlapping cards: scale down so the quarter hour is never more than full.
    if (total > len && total > 0) shares = shares.map((s) => ({ ...s, secs: (s.secs * len) / total }));
    shares.sort((a, b) => rank(a.name) - rank(b.name) || a.name.localeCompare(b.name));
    slots.push({
      index: i,
      label: slotLabel(i),
      start,
      end,
      future: now !== undefined && start >= now,
      skipped,
      shares,
      filled: Math.min(total, len),
      top: best ? { id: best.card.id, title: best.card.title } : null,
      noted: noted.filter((d) => d.startTs < end && d.endTs > start).map((d) => d.title || "Untitled distraction"),
    });
  }
  return slots;
}

/** Seconds of failed ("Not analyzed") cards in the day, clipped to its bounds. */
export function failedSecs(day: string, cards: Card[]): number {
  const [s, e] = dayBounds(day);
  return cards
    .filter((c) => c.category.trim().toLowerCase() === "system")
    .reduce((a, c) => a + Math.max(0, Math.min(e, c.endTs) - Math.max(s, c.startTs)), 0);
}
