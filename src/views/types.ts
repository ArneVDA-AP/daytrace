import type { Category, SettingsDto, Status } from "../api";

/** Props every top-level view receives from App. */
export interface ViewProps {
  /** Logical day (YYYY-MM-DD, days start at 4 AM) currently selected in the header. */
  day: string;
  setDay: (day: string) => void;
  today: string;
  settings: SettingsDto;
  status: Status;
  categories: Category[];
  colorFor: (category: string) => string;
  /** Jump to the timeline for a day, optionally selecting a card. */
  openTimeline: (day: string, cardId?: number) => void;
}
