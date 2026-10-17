// Logical days run 4 AM to 4 AM, matching the backend (timeutil.rs logical_day).
// Days are "YYYY-MM-DD" strings; timestamps are Unix seconds. Formatting goes through Intl so a
// 24-hour Windows region can be honoured later by passing the region locale from Rust.

export const LOCALE = "en-US";
const clockFmt = new Intl.DateTimeFormat(LOCALE, { hour: "numeric", minute: "2-digit" });
const dayTitleFmt = new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long" });
const dayShortFmt = new Intl.DateTimeFormat("en-GB", { weekday: "short", day: "numeric", month: "short" });
const weekdayFmt = new Intl.DateTimeFormat("en-GB", { weekday: "short" });
const monthDayFmt = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long" });
const monthYearFmt = new Intl.DateTimeFormat("en-GB", { month: "long", year: "numeric" });

/** Hour at which a logical day starts. */
export const DAY_START_HOUR = 4;

/** Local midnight of the calendar date of `day`. */
export function dateOf(day: string): Date {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** "YYYY-MM-DD" of a local calendar date. */
export function dayKey(dt: Date): string {
  return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
}

export function shiftDay(day: string, delta: number): string {
  const dt = dateOf(day);
  dt.setDate(dt.getDate() + delta);
  return dayKey(dt);
}

/** The logical day a moment belongs to (before 4 AM counts as the previous day). */
export function logicalDay(ts: number): string {
  const dt = new Date(ts * 1000);
  if (dt.getHours() < DAY_START_HOUR) dt.setDate(dt.getDate() - 1);
  return dayKey(dt);
}

/** Unix seconds for a wall-clock time on a logical day; hours before 4 belong to the next date. */
export function at(day: string, h: number, m = 0): number {
  const dt = dateOf(day);
  const date = h < DAY_START_HOUR ? dt.getDate() + 1 : dt.getDate();
  return new Date(dt.getFullYear(), dt.getMonth(), date, h, m).getTime() / 1000;
}

/** [start, end) of a logical day in Unix seconds. DST-safe: 23 or 25 hours on change days. */
export function dayBounds(day: string): [number, number] {
  const dt = dateOf(day);
  const y = dt.getFullYear();
  const mo = dt.getMonth();
  const d = dt.getDate();
  return [new Date(y, mo, d, DAY_START_HOUR).getTime() / 1000, new Date(y, mo, d + 1, DAY_START_HOUR).getTime() / 1000];
}

/** Whole days from `a` to `b` (positive when b is later). */
export function daysBetween(a: string, b: string): number {
  return Math.round((dateOf(b).getTime() - dateOf(a).getTime()) / 86_400_000);
}

/** "2:34 AM" */
export function clock(ts: number): string {
  return clockFmt.format(new Date(ts * 1000));
}

/** "12 AM", "4 PM": hour labels for axes. */
export function hourLabel(h: number): string {
  const hh = ((h % 24) + 24) % 24;
  if (hh === 0) return "12 AM";
  if (hh === 12) return "12 PM";
  return hh < 12 ? `${hh} AM` : `${hh - 12} PM`;
}

/** "Friday 16 October" */
export function formatDayTitle(day: string): string {
  return dayTitleFmt.format(dateOf(day));
}
/** "Fri 16 Oct" */
export function formatDayShort(day: string): string {
  return dayShortFmt.format(dateOf(day));
}
/** "Fri" */
export function weekday(day: string): string {
  return weekdayFmt.format(dateOf(day));
}
/** "16 October" */
export function monthDay(day: string): string {
  return monthDayFmt.format(dateOf(day));
}
/** "October 2026" */
export function monthYear(dt: Date): string {
  return monthYearFmt.format(dt);
}

/** "Today", "Yesterday", or the day title. */
export function relativeDayTitle(day: string, today: string): string {
  if (day === today) return "Today";
  if (day === shiftDay(today, -1)) return "Yesterday";
  return formatDayTitle(day);
}

/** Monday of the week that contains `day`. */
export function weekStart(day: string): string {
  const dow = (dateOf(day).getDay() + 6) % 7;
  return shiftDay(day, -dow);
}

/** "12–18 October 2026", "28 September – 4 October 2026" */
export function formatWeekRange(start: string): string {
  const a = dateOf(start);
  const b = dateOf(shiftDay(start, 6));
  const left = a.getMonth() === b.getMonth() ? String(a.getDate()) : `${monthDayFmt.format(a)} `;
  return `${left}–${a.getMonth() === b.getMonth() ? "" : " "}${monthDayFmt.format(b)} ${b.getFullYear()}`;
}

/** "1 h 30 min", "45 min", "2 h". Units joined with no-break spaces. */
export function formatDuration(seconds: number): string {
  const mins = Math.max(0, Math.round(seconds / 60));
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** Compact form for chart labels and tight cells: "1h 30m". */
export function formatDurationShort(seconds: number): string {
  const mins = Math.max(0, Math.round(seconds / 60));
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h === 0) return `${m}m`;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}
