import { invoke } from "@tauri-apps/api/core";
import type { Card } from "./api";

export type ReviewRating = "focused" | "neutral" | "distracted";

export interface Feedback {
  cardId: number;
  direction: "up" | "down";
  note: string;
  updatedAt: number;
}

export interface CardReview {
  cardId: number;
  rating: ReviewRating;
  coverage: number;
}

export interface ReviewSummary {
  focusedSecs: number;
  neutralSecs: number;
  distractedSecs: number;
  lastReviewedAt: number | null;
  unreviewedCards: number;
  firstUnreviewed: number | null;
}

export interface FailedGroup {
  cardIds: number[];
  batchIds: number[];
  startTs: number;
  endTs: number;
  retryable: boolean;
}

export interface DayExtras {
  day: string;
  edited: number[];
  feedback: Feedback[];
  reviews: CardReview[];
  reviewSummary: ReviewSummary;
  failedGroups: FailedGroup[];
}

export interface DayCount {
  day: string;
  cards: number;
}

export interface DayPlan {
  day: string;
  batchIds: number[];
  keptEditedCards: number;
  blocked: string | null;
  /** With the Batch API on: the requests are sent directly, at the regular price. */
  costNote?: string | null;
}

export interface BatchProgress {
  waiting: number;
  running: number;
  failed: number;
  step: string | null;
}

export type RetryState = "queued" | "running" | "done" | "failed" | "stopped" | "waiting";

export interface RetryItem {
  batchId: number;
  state: RetryState;
  position: number;
  total: number;
}

export const teApi = {
  dayExtras: (day: string) => invoke<DayExtras>("te_day_extras", { day }),
  updateCard: (cardId: number, edit: { title?: string; category?: string; subcategory?: string }) =>
    invoke<Card>("te_update_card", {
      cardId,
      title: edit.title ?? null,
      category: edit.category ?? null,
      subcategory: edit.subcategory ?? null,
    }),
  deleteCard: (cardId: number) => invoke<boolean>("te_delete_card", { cardId }),
  setFeedback: (cardId: number, direction: "up" | "down" | null, note: string) =>
    invoke<Feedback | null>("te_set_feedback", { cardId, direction, note }),
  rateCard: (cardId: number, rating: ReviewRating | null) => invoke<void>("te_rate_card", { cardId, rating }),
  daysWithData: (fromDay: string, toDay: string) => invoke<DayCount[]>("te_days_with_data", { fromDay, toDay }),
  timelineText: (day: string) => invoke<string>("te_timeline_text", { day }),
  reprocessDayPlan: (day: string) => invoke<DayPlan>("te_reprocess_day_plan", { day }),
  reprocessDay: (day: string) => invoke<DayPlan>("te_reprocess_day", { day }),
  reprocessCard: (cardId: number) => invoke<number>("te_reprocess_card", { cardId }),
  batchProgress: (batchIds: number[]) => invoke<BatchProgress>("te_batch_progress", { batchIds }),
  retryBatches: (batchIds: number[]) => invoke<number>("te_retry_batches", { batchIds }),
  retryStatus: () => invoke<RetryItem[]>("te_retry_status"),
};

/** Dayflow RetryCoordinator.statusLine for a group of batches. */
export function retryLine(items: RetryItem[], batchIds: number[]): string | null {
  const mine = items.filter((i) => batchIds.includes(i.batchId));
  if (mine.length === 0) return null;
  if (mine.some((i) => i.state === "running")) return "Analyzing again…";
  if (mine.some((i) => i.state === "waiting")) return "Waiting for an API key. Add one in Settings.";
  if (mine.some((i) => i.state === "failed")) return "Failed again. Retry stopped.";
  const queued = mine.filter((i) => i.state === "queued").map((i) => i.position);
  if (queued.length) {
    const total = mine[0].total;
    const lo = Math.min(...queued);
    const hi = Math.max(...queued);
    return lo === hi ? `Queued (${lo} of ${total})` : `Queued (${lo}–${hi} of ${total})`;
  }
  if (mine.some((i) => i.state === "stopped")) return "Stopped because an earlier batch failed.";
  return null;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
