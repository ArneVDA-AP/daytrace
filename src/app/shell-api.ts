import { invoke } from "@tauri-apps/api/core";

/*
 * The shell's own backend commands (src-tauri/src/features/shell.rs): the "new recap" badge on
 * Daily, the taskbar overlay dot that mirrors it, and the fixed help pages fix buttons open.
 */

export interface ShellBadges {
  /** Day of a recap created after Daily was last opened, else null. */
  recapDay: string | null;
}

export type HelpLink = "console_billing" | "console_keys";

export const shellApi = {
  badges: () => invoke<ShellBadges>("shell_badges"),
  /** Daily was opened: clears the recap badge. */
  recapOpened: () => invoke<void>("shell_recap_opened"),
  /** The dot on the taskbar button (Windows only). */
  setOverlay: (on: boolean) => invoke<void>("shell_set_overlay", { on }),
  openLink: (target: HelpLink) => invoke<void>("shell_open_link", { target }),
};

/**
 * Dispatched on window when the Timeline changed cards (edit, delete, retry), so the rail counts
 * today's "Not analyzed" cards again without waiting for its next check.
 */
export const CARDS_CHANGED_EVENT = "daytrace:cards-changed";

export function announceCardsChanged() {
  dispatchEvent(new Event(CARDS_CHANGED_EVENT));
}
