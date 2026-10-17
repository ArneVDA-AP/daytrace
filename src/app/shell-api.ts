import { invoke } from "@tauri-apps/api/core";

/*
 * The shell's own backend commands (src-tauri/src/features/shell.rs): the "new recap" badge on
 * Daily, the taskbar overlay dot that mirrors it, and the one link opener (named pages, or https
 * links to a few allowed sites; the backend refuses everything else).
 */

export interface ShellBadges {
  /** Day of a recap created after Daily was last opened, else null. */
  recapDay: string | null;
}

/** Named pages the backend opens by name. */
export type HelpLink = "console_billing" | "console_keys" | "releases" | "nodejs";

export const shellApi = {
  badges: () => invoke<ShellBadges>("shell_badges"),
  /** Daily was opened: clears the recap badge. */
  recapOpened: () => invoke<void>("shell_recap_opened"),
  /** The dot on the taskbar button (Windows only). */
  setOverlay: (on: boolean) => invoke<void>("shell_set_overlay", { on }),
  /** Opens a named page in the default browser. */
  openLink: (target: HelpLink) => invoke<void>("shell_open_link", { target }),
  /**
   * Opens an https link in the default browser when it points to Daytrace's repository or a help
   * site the backend allows (links in the release notes); rejects with the reason otherwise.
   */
  openUrl: (url: string) => invoke<void>("shell_open_link", { target: url }),
};

/**
 * Dispatched on window when the Timeline changed cards (edit, delete, retry), so the rail counts
 * today's "Not analyzed" cards again without waiting for its next check.
 */
export const CARDS_CHANGED_EVENT = "daytrace:cards-changed";

export function announceCardsChanged() {
  dispatchEvent(new Event(CARDS_CHANGED_EVENT));
}
