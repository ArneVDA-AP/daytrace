import { invoke } from "@tauri-apps/api/core";

export interface Plan {
  day: string;
  focusTargetMinutes: number;
  distractionLimitMinutes: number;
  isSkipped: boolean;
  /** The day has its own saved plan (confirmed or skipped). */
  saved: boolean;
  /** When not saved: the earlier day the targets come from. */
  carriedFrom: string | null;
}

export interface CategorySecs {
  name: string;
  color: string;
  secs: number;
}

export interface Progress {
  focusSecs: number;
  distractedSecs: number;
  focusCategories: CategorySecs[];
  distractionCategories: string[];
}

export interface Review {
  plan: Plan;
  progress: Progress;
  focusMet: boolean;
  distractionMet: boolean;
}

export interface Reference {
  yesterdayFocusMinutes: number;
  lastWeekFocusMinutes: number;
  yesterdayDistractionMinutes: number;
  lastWeekDistractionMinutes: number;
}

export interface EvidenceCard {
  cardId: number;
  title: string;
  start: string;
  end: string;
  secs: number;
  matched: string[];
}

export interface Evidence {
  totalSecs: number;
  cardCount: number;
  cards: EvidenceCard[];
}

export interface GoalItem {
  id: number;
  text: string;
  done: boolean;
  evidence: Evidence;
}

export interface GoalsView {
  day: string;
  isToday: boolean;
  plan: Plan;
  progress: Progress;
  review: Review | null;
  reference: Reference;
  /** Ask for today's goals (morning flow). */
  prompt: boolean;
  items: GoalItem[];
}

export const journalApi = {
  get: (day: string) => invoke<GoalsView>("journal_get", { day }),
  savePlan: (day: string, focusTargetMinutes: number, distractionLimitMinutes: number) =>
    invoke<GoalsView>("journal_save_plan", { day, focusTargetMinutes, distractionLimitMinutes }),
  skip: (day: string) => invoke<GoalsView>("journal_skip", { day }),
  addItem: (day: string, text: string) => invoke<GoalsView>("journal_add_item", { day, text }),
  updateItem: (id: number, patch: { text?: string; done?: boolean }) =>
    invoke<GoalsView>("journal_update_item", { id, text: patch.text ?? null, done: patch.done ?? null }),
  deleteItem: (id: number, day: string) => invoke<GoalsView>("journal_delete_item", { id, day }),
};
