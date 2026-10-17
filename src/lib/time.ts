// Logical days run 4 AM to 4 AM, matching the backend (timeutil.rs logical_day).
// Days are "YYYY-MM-DD" strings; timestamps are Unix seconds. Formatting goes through Intl so a
// 24-hour Windows region can be honored later by passing the region locale from Rust.

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

// ---------------------------------------------------------------- typed days (Ctrl+K)

const MONTHS = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
// Dutch names too: the owner's region writes dates that way ("12 okt", "3 mei").
const MONTHS_NL = ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"];
const WEEKDAYS = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
const WEEKDAYS_NL = ["zondag", "maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag"];

function prefixIndex(word: string, names: string[]): number {
  if (word.length < 3) return -1;
  return names.findIndex((n) => n.startsWith(word));
}

function monthOf(word: string): number | null {
  if (word === "mrt") return 2;
  const i = prefixIndex(word, MONTHS);
  if (i >= 0) return i;
  const j = prefixIndex(word, MONTHS_NL);
  return j >= 0 ? j : null;
}

function weekdayOf(word: string): number | null {
  const i = prefixIndex(word, WEEKDAYS);
  if (i >= 0) return i;
  const j = prefixIndex(word, WEEKDAYS_NL);
  return j >= 0 ? j : null;
}

/** A real date as a day key, or null (31 February is not one). */
function validDay(y: number, m: number, d: number): string | null {
  if (!Number.isInteger(y) || !Number.isInteger(m) || !Number.isInteger(d)) return null;
  const dt = new Date(y, m, d);
  if (dt.getFullYear() !== y || dt.getMonth() !== m || dt.getDate() !== d) return null;
  return dayKey(dt);
}

/** A date without a year: this year, or last year when it would be after today. */
function inPast(m: number, d: number, today: string, year?: number): string | null {
  const ty = dateOf(today).getFullYear();
  if (year !== undefined) return validDay(year, m, d);
  const k = validDay(ty, m, d);
  if (k && k <= today) return k;
  return validDay(ty - 1, m, d);
}

/**
 * A day typed in the command menu, never after today: "today", "yesterday", "monday" (the most
 * recent one, today included), "last monday" (before today), "12 oct", "oct 12", "12 october
 * 2025", "12/10" (day/month), "2026-10-12", or a day of the month ("12"). Dutch month and weekday
 * names work as well. Returns null when the text is not a day.
 */
export function parseDayQuery(input: string, today: string): string | null {
  const q = input.trim().toLowerCase().replace(/\s+/g, " ");
  if (!q) return null;
  if (q === "today" || q === "vandaag") return today;
  if (q === "yesterday" || q === "gisteren") return shiftDay(today, -1);

  const check = (k: string | null) => (k && k <= today ? k : null);

  let m = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(q);
  if (m) return check(validDay(Number(m[1]), Number(m[2]) - 1, Number(m[3])));

  m = /^(\d{1,2})[/.-](\d{1,2})(?:[/.-](\d{2}|\d{4}))?$/.exec(q);
  if (m) {
    const y = m[3] ? (m[3].length === 2 ? 2000 + Number(m[3]) : Number(m[3])) : undefined;
    return check(inPast(Number(m[2]) - 1, Number(m[1]), today, y));
  }

  const words = q.split(/[\s,]+/).filter((w) => w && w !== "the" && w !== "of" && w !== "on");
  let last = false;
  let weekday: number | null = null;
  let month: number | null = null;
  let dayNum: number | null = null;
  let year: number | undefined;
  for (const raw of words) {
    const w = raw.replace(/(st|nd|rd|th)$/, (s) => (/^\d/.test(raw) ? "" : s)).replace(/\.$/, "");
    if (w === "last" || w === "vorige" || w === "vorig") {
      last = true;
      continue;
    }
    if (/^\d{4}$/.test(w)) {
      year = Number(w);
      continue;
    }
    if (/^\d{1,2}$/.test(w)) {
      if (dayNum !== null) return null;
      dayNum = Number(w);
      continue;
    }
    const mo = monthOf(w);
    if (mo !== null && month === null) {
      month = mo;
      continue;
    }
    const wd = weekdayOf(w);
    if (wd !== null && weekday === null) {
      weekday = wd;
      continue;
    }
    return null;
  }

  if (dayNum !== null && month !== null) return check(inPast(month, dayNum, today, year));
  if (dayNum !== null && month === null && weekday === null && year === undefined) {
    // "12": the 12th of this month, or of last month when that is still to come.
    const t = dateOf(today);
    const k = validDay(t.getFullYear(), t.getMonth(), dayNum);
    if (k && k <= today) return k;
    const prev = new Date(t.getFullYear(), t.getMonth() - 1, 1);
    return check(validDay(prev.getFullYear(), prev.getMonth(), dayNum));
  }
  if (weekday !== null && dayNum === null && month === null && year === undefined) {
    const dow = dateOf(today).getDay();
    let back = (dow - weekday + 7) % 7;
    if (last && back === 0) back = 7;
    return shiftDay(today, -back);
  }
  return null;
}

/** Compact form for chart labels and tight cells: "1h 30m". */
export function formatDurationShort(seconds: number): string {
  const mins = Math.max(0, Math.round(seconds / 60));
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h === 0) return `${m}m`;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}
