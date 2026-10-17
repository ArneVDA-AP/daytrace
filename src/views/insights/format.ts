import type { Category, CountsAs } from "@/api";
import { dateOf, formatDuration, hourLabel, shiftDay } from "@/lib/time";
import type { Kind } from "./api";

/*
 * Small pure helpers shared by Daily and Weekly.
 */

/** "35%" of a whole; "0%" for an empty whole. */
export function percent(part: number, whole: number): string {
  return whole > 0 ? `${Math.round((part / whole) * 100)}%` : "0%";
}

const longWeekday = new Intl.DateTimeFormat("en-GB", { weekday: "long" });
const shortDay = new Intl.DateTimeFormat("en-GB", { weekday: "short", day: "numeric", month: "short" });

/** "today", "yesterday", a weekday within the last week, else "Fri 9 Oct" (Dayflow's standupDayLabelText). */
export function relativeDayLabel(day: string, today: string): string {
  if (day === today) return "today";
  if (day === shiftDay(today, -1)) return "yesterday";
  const diff = Math.round((dateOf(today).getTime() - dateOf(day).getTime()) / 86_400_000);
  if (diff >= 2 && diff <= 6) return longWeekday.format(dateOf(day));
  return shortDay.format(dateOf(day));
}

/** "Wednesday" */
export function weekdayLong(day: string): string {
  return longWeekday.format(dateOf(day));
}

/** Hour label for minutes since 4 AM on the wall clock: 300 -> "9 AM". */
export function hourAt(minuteSince4: number): string {
  return hourLabel(4 + Math.floor(minuteSince4 / 60));
}

/** "+20 min on last week", "same as last week", or undefined when the app was not used before. */
export function changeOnLastWeek(secs: number, previous: number): string | undefined {
  if (previous <= 0) return undefined;
  const delta = Math.round((secs - previous) / 60) * 60;
  if (delta === 0) return "same as last week";
  return `${delta > 0 ? "+" : "−"}${formatDuration(Math.abs(delta))} on last week`;
}

/** What a category counts as: its choice in Settings, else by name (types.rs CountsAs::derived). */
export function countsAsOf(c: Pick<Category, "name" | "isIdle" | "countsAs">): CountsAs {
  if (c.isIdle) return "neither";
  if (c.countsAs) return c.countsAs;
  const key = c.name.trim().toLowerCase();
  return key === "distraction" || key === "distractions" ? "distraction" : "focus";
}

/** Kind of a card's category name, like the backend's cat_info. */
export function kindOf(name: string, categories: Category[]): Kind {
  const key = name.trim().toLowerCase();
  if (key === "system") return "system";
  const c = categories.find((x) => x.name.trim().toLowerCase() === key);
  if (c?.isIdle || (!c && key === "idle")) return "idle";
  return countsAsOf(c ?? { name, isIdle: false });
}

/** Names of the categories that count as `what`, in settings order. */
export function categoriesCounting(what: CountsAs, categories: Category[]): string[] {
  return categories.filter((c) => !c.isIdle && countsAsOf(c) === what).map((c) => c.name);
}

/** "Work", "Work and Personal", "Work, Personal and Study". */
export function nameList(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/**
 * Sort key for category names: settings order first, unknown names after them, Idle and failed
 * cards last. Stacks, legends and tables all use it, so colours line up in the same order.
 */
export function categoryRank(categories: Category[]): (name: string) => number {
  const order = new Map<string, number>();
  categories.forEach((c, i) => order.set(c.name.trim().toLowerCase(), c.isIdle ? 900 + i : i));
  return (name) => {
    const key = name.trim().toLowerCase();
    if (key === "system") return 1000;
    return order.get(key) ?? (key === "idle" ? 950 : 500);
  };
}

/** Sorts items with a `name` by categoryRank, then by name. */
export function byCategory<T extends { name: string }>(items: T[], categories: Category[]): T[] {
  const rank = categoryRank(categories);
  return [...items].sort((a, b) => rank(a.name) - rank(b.name) || a.name.localeCompare(b.name));
}

/** "6 h 10 min" or "–" for nothing. */
export function durationOrDash(secs: number): string {
  return secs > 0 ? formatDuration(secs) : "–";
}

/** Wall-clock minutes since 4 AM of a timestamp (0 ≤ m < 1440), like the backend's wall_offset. */
export function wallMinute(ts: number): number {
  const d = new Date(ts * 1000);
  const m = d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60 - 240;
  return m < 0 ? m + 1440 : m;
}
