import { invoke } from "@tauri-apps/api/core";
import { api, type Card, type Shot } from "@/api";
import { teApi } from "@/timeline-extras-api";

/*
 * Every backend call the Timeline makes, in one place. The commands are shared with other
 * sections (insights_day with Daily, journal_get with Goals); the types below mirror only the
 * fields the Timeline reads, so it does not depend on those sections' modules.
 */

export type { Card, Shot };
export type {
  BatchProgress,
  CardReview,
  DayExtras,
  DayPlan,
  FailedGroup,
  Feedback,
  RetryItem,
  ReviewRating,
  ReviewSummary,
} from "@/timeline-extras-api";

/** insights_day (src-tauri/src/features/insights/stats.rs), the fields used here. */
export interface DayStatsLite {
  day: string;
  capturedSecs: number;
  focusSecs: number;
  distractedSecs: number;
  idleSecs: number;
  interruptions: number;
  distractedRatio: number;
  longestFocus: { startTs: number; endTs: number; secs: number } | null;
  focusBlocks: { startTs: number; endTs: number; secs: number }[];
  distractions: { startTs: number; endTs: number; secs: number; title: string; cardId: number }[];
}

/** insights_week, the fields used here. */
export interface WeekStatsLite {
  weekStart: string;
  weekEnd: string;
  capturedSecs: number;
}

/** journal_get (src-tauri/src/features/journal.rs), the fields used here. */
export interface GoalsLite {
  day: string;
  isToday: boolean;
  plan: { focusTargetMinutes: number; distractionLimitMinutes: number; isSkipped: boolean; saved: boolean; carriedFrom: string | null };
  progress: { focusSecs: number; distractedSecs: number };
  /** The morning flow asks for today's goals first. */
  prompt: boolean;
}

export const timelineApi = {
  getDay: (day: string) => api.getDay(day),
  cardScreenshots: (cardId: number, max: number) => api.getCardScreenshots(cardId, max),
  screenshot: (id: number, thumb: boolean) => api.getScreenshot(id, thumb),
  /** Shows the screenshot file selected in File Explorer. The path stays in the backend. */
  revealScreenshot: (id: number) => invoke<void>("reveal_screenshot", { id }),
  retryCard: (cardId: number) => api.retryCard(cardId),
  exportMarkdown: (fromDay: string, toDay: string, path: string) => api.exportMarkdown(fromDay, toDay, path),
  dayStats: (day: string) => invoke<DayStatsLite>("insights_day", { day }),
  weekStats: (day: string) => invoke<WeekStatsLite>("insights_week", { day }),
  goals: (day: string) => invoke<GoalsLite>("journal_get", { day }),
  ...teApi,
};
