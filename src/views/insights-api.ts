import { invoke } from "@tauri-apps/api/core";

// Types mirror src-tauri/src/features/insights/*.rs (serde camelCase).

export type Kind = "focus" | "distraction" | "idle" | "system";

export interface CategoryTotal {
  key: string;
  name: string;
  color: string;
  kind: Kind;
  secs: number;
}

export interface GridCell {
  occupancy: number;
  title?: string;
  cardId?: number;
}

export interface GridRow {
  key: string;
  name: string;
  color: string;
  cells: GridCell[];
}

export interface Marker {
  startMinute: number;
  endMinute: number;
  title: string;
}

export interface DayGrid {
  /** Minutes since 4 AM. */
  startMinute: number;
  endMinute: number;
  slotMinutes: number;
  rows: GridRow[];
  dominant: (number | null)[];
  markers: Marker[];
}

export interface DistractionItem {
  startTs: number;
  endTs: number;
  secs: number;
  title: string;
  summary: string;
  withinCard: string | null;
  cardId: number;
}

export interface Block {
  startTs: number;
  endTs: number;
  secs: number;
}

export interface NamedSecs {
  name: string;
  secs: number;
}

export interface DayStats {
  day: string;
  dayStartTs: number;
  dayEndTs: number;
  cardCount: number;
  capturedSecs: number;
  focusSecs: number;
  distractedSecs: number;
  idleSecs: number;
  transitionSecs: number;
  contextSwitches: number;
  interruptions: number;
  distractedRatio: number;
  totals: CategoryTotal[];
  grid: DayGrid;
  distractions: DistractionItem[];
  focusBlocks: Block[];
  longestFocus: Block | null;
  programs: NamedSecs[];
}

export interface RecapEntry {
  targetDay: string;
  sourceDay: string;
  highlights: string[];
  tasks: string[];
  blockers: string[];
  provider: string;
  model: string;
  edited: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface RecapView {
  targetDay: string;
  entry: RecapEntry | null;
  sourceDay: string | null;
  generating: boolean;
  autoError: string | null;
  exportPath: string | null;
}

export interface WeekDaySummary {
  day: string;
  label: string;
  capturedSecs: number;
  focusSecs: number;
  distractedSecs: number;
  idleSecs: number;
  totals: CategoryTotal[];
  contextShifts: number;
  distractionCount: number;
  longestFocus: Block | null;
}

export interface WeekCell {
  key: string;
  name: string;
  color: string;
  occupancy: number;
  secs: number;
}

export interface WeekStats {
  weekStart: string;
  weekEnd: string;
  days: WeekDaySummary[];
  distribution: CategoryTotal[];
  capturedSecs: number;
  focusSecs: number;
  distractedSecs: number;
  idleSecs: number;
  workflow: {
    startMinute: number;
    endMinute: number;
    slotMinutes: number;
    rows: { day: string; label: string; cells: (WeekCell | null)[] }[];
    totals: CategoryTotal[];
  };
  heatmap: {
    startMinute: number;
    endMinute: number;
    bucketMinutes: number;
    rows: { day: string; label: string; values: number[] }[];
  };
  distractionHours: { day: string; label: string; secs: number[] }[];
  contextInsight: string;
  focusBlocks: { day: string; startTs: number; endTs: number; secs: number }[];
  appsByCategory: { key: string; name: string; color: string; secs: number; apps: AppUsage[] }[];
  topApps: AppUsage[];
  programs: NamedSecs[];
}

export interface AppUsage {
  name: string;
  secs: number;
  previousSecs: number;
}

export const insightsApi = {
  day: (day: string) => invoke<DayStats>("insights_day", { day }),
  week: (day: string) => invoke<WeekStats>("insights_week", { day }),
  recap: (day: string) => invoke<RecapView>("insights_recap", { day }),
  generateRecap: (day: string) => invoke<RecapView>("insights_generate_recap", { day }),
  saveRecap: (day: string, highlights: string[], tasks: string[], blockers: string[]) =>
    invoke<RecapView>("insights_save_recap", { day, highlights, tasks, blockers }),
  /** Writes the recap file of `day`; without a path it goes to the export folder. Returns the path. */
  exportRecap: (day: string, path?: string) => invoke<string>("insights_export_recap", { day, path: path ?? null }),
};

// ---- formatting helpers ----

/** Clock label for minutes since 4 AM, e.g. 300 → "9 AM". */
export function hourLabel(minuteSince4am: number): string {
  const h = (4 + Math.floor(minuteSince4am / 60)) % 24;
  if (h === 0) return "12 AM";
  if (h === 12) return "12 PM";
  return h < 12 ? `${h} AM` : `${h - 12} PM`;
}

/** Whole hours inside [start, end] (minutes since 4 AM), every `step` hours. */
export function hourTicks(start: number, end: number, step = 1): number[] {
  const out: number[] = [];
  for (let m = Math.ceil(start / 60) * 60; m <= end; m += 60 * step) out.push(m);
  return out;
}

export function percent(part: number, whole: number): string {
  return whole > 0 ? `${Math.round((part / whole) * 100)}%` : "0%";
}

/** "today", "yesterday", a weekday within the last week, else "Fri 9 Oct" (Dayflow's standupDayLabelText). */
export function relativeDayLabel(day: string, today: string): string {
  const d = new Date(`${day}T12:00:00`);
  const t = new Date(`${today}T12:00:00`);
  const diff = Math.round((t.getTime() - d.getTime()) / 86400000);
  if (diff === 0) return "today";
  if (diff === 1) return "yesterday";
  if (diff >= 2 && diff <= 6) return d.toLocaleDateString("en-GB", { weekday: "long" });
  return d.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
}

export function shortDate(day: string): string {
  return new Date(`${day}T12:00:00`).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
}

/** Cell background: the category colour mixed into the empty-cell colour by occupancy. */
export function fill(color: string, occupancy: number): string {
  if (occupancy <= 0) return "var(--soft)";
  const pct = Math.round(25 + Math.min(1, occupancy) * 75);
  return `color-mix(in srgb, ${color} ${pct}%, var(--soft))`;
}
