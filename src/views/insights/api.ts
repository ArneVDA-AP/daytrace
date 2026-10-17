import { invoke } from "@tauri-apps/api/core";

/*
 * Daily and Weekly: the insights commands. Types mirror src-tauri/src/features/insights/*.rs
 * (serde camelCase). The backend also sends a `color` hex on categories (the stored settings hex
 * or an old fallback palette); it is left out of these types on purpose: views colour every
 * category mark with colorFor(name), which follows the theme and Windows Contrast themes.
 */

/** "neither": a category set to count as neither focus nor distraction (still active time). */
export type Kind = "focus" | "distraction" | "neither" | "idle" | "system";

export interface CategoryTotal {
  key: string;
  name: string;
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
  cells: GridCell[];
}

export interface Marker {
  startMinute: number;
  endMinute: number;
  title: string;
}

/** Dayflow's per-category grid (unused by the views, which build the grid from the cards). */
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
  /** Title of the card the distraction was noted in; null for a Distraction-category card. */
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
  /** All non-System time, overlaps counted once ("Recorded"). */
  capturedSecs: number;
  /** Recorded minus idle: the denominator of every percentage of the day. */
  activeSecs: number;
  focusSecs: number;
  distractedSecs: number;
  idleSecs: number;
  /** Gaps between cards. */
  transitionSecs: number;
  contextSwitches: number;
  /** Cards with distractions noted inside them. */
  interruptions: number;
  /** Distracted time as a share of active time. */
  distractedRatio: number;
  totals: CategoryTotal[];
  grid: DayGrid;
  distractions: DistractionItem[];
  /** Focus time merged across gaps under 5 minutes, in time order. */
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
  /** The day the standup is for. */
  targetDay: string;
  entry: RecapEntry | null;
  /** The entry's source day, or the day a new recap would be written from. */
  sourceDay: string | null;
  /** A generation for this day is running (the automatic recap or another window). */
  generating: boolean;
  /** The last automatic generation error for this day. */
  autoError: string | null;
  /** The export file of the source day, when it exists. */
  exportPath: string | null;
  /** Set for today while there is no recap and automatic recaps are on. */
  auto: AutoPlan | null;
}

/** What the automatic recap will do (it needs 3 hours of activity on the day it is written from). */
export interface AutoPlan {
  /** "HH:MM" after which it runs. */
  after: string;
  /** The day it would be written from; null when none of the previous 3 days has 3 hours. */
  sourceDay: string | null;
  /** Why it will not run now, in plain words. */
  blocked: string | null;
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
  occupancy: number;
  secs: number;
}

export interface AppUsage {
  name: string;
  secs: number;
  /** The same app the week before (in `appsByCategory`: in the same category); 0 when unused. */
  previousSecs: number;
}

export interface AppCategory {
  key: string;
  name: string;
  secs: number;
  apps: AppUsage[];
}

export interface FocusHourRow {
  day: string;
  label: string;
  /** -1 focused to 1 distracted; null for an hour without focus or distraction. */
  scores: (number | null)[];
  focusSecs: number[];
  distractedSecs: number[];
}

export interface WeekStats {
  weekStart: string;
  weekEnd: string;
  days: WeekDaySummary[];
  /** Top four categories plus Other, without Idle and System. */
  distribution: CategoryTotal[];
  capturedSecs: number;
  focusSecs: number;
  distractedSecs: number;
  idleSecs: number;
  /** Dayflow's weekly grid; the views use its window (minutes since 4 AM) for the hour charts. */
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
  /** Distracted seconds per clock hour, index 0 = 4 AM. */
  distractionHours: { day: string; label: string; secs: number[] }[];
  /** Dayflow's focus heat map by clock hour (index 0 = 4 AM). */
  focusHours: FocusHourRow[];
  contextInsight: string;
  focusBlocks: { day: string; startTs: number; endTs: number; secs: number }[];
  appsByCategory: AppCategory[];
  topApps: AppUsage[];
  programs: NamedSecs[];
}

export const insightsApi = {
  day: (day: string) => invoke<DayStats>("insights_day", { day }),
  week: (day: string) => invoke<WeekStats>("insights_week", { day }),
  recap: (day: string) => invoke<RecapView>("insights_recap", { day }),
  generateRecap: (day: string) => invoke<RecapView>("insights_generate_recap", { day }),
  saveRecap: (day: string, highlights: string[], tasks: string[], blockers: string[]) =>
    invoke<RecapView>("insights_save_recap", { day, highlights, tasks, blockers }),
  /** Writes the recap file of `day` (a source day); without a path it goes to the export folder. Returns the path. */
  exportRecap: (day: string, path?: string) => invoke<string>("insights_export_recap", { day, path: path ?? null }),
  /** Writes a chart image (PNG as base64, without the data: prefix) to a path from the save dialog. */
  saveImage: (path: string, pngBase64: string) => invoke<void>("insights_save_image", { path, pngBase64 }),
};
