import { invoke } from "@tauri-apps/api/core";
import { api, type Settings } from "../api";
import { applyAppearance as applyTheme } from "../lib/theme";

export type Appearance = Settings["appearance"];

export interface UpdateStatus {
  currentVersion: string;
  latestVersion: string | null;
  releaseUrl: string | null;
  available: boolean;
  noReleases: boolean;
  /** The last check that reached GitHub. */
  checkedAt: number | null;
  error: string | null;
  /** The last check, also a failed one. */
  attemptedAt: number | null;
}

export interface PauseOption {
  /** "15", "tomorrow" or "indefinite" */
  id: string;
  label: string;
}

export interface PlatformInfo {
  version: string;
  pauseOptions: PauseOption[];
  minuteOptions: number[];
  update: UpdateStatus;
  /** Set when settings.json could not be read at startup: where that file was kept. */
  damagedSettingsCopy: string | null;
}

export interface AppCandidate {
  name: string;
  running: boolean;
  lastSeen: number | null;
  captures: number;
}

/** Where a click on a notification leads. */
export type NavTarget = "timeline" | "daily" | "settings" | "release";

export const platformApi = {
  info: () => invoke<PlatformInfo>("platform_info"),
  pause: (choice: string) => invoke<void>("platform_pause", { choice }),
  resume: () => invoke<void>("platform_resume"),
  finishOnboarding: (recording: boolean) => invoke<Settings>("platform_finish_onboarding", { recording }),
  takeNavigation: () => invoke<NavTarget | null>("platform_take_navigation"),
  setWindowTheme: (appearance: Appearance) => invoke<void>("platform_set_window_theme", { appearance }),
  appCandidates: () => invoke<AppCandidate[]>("platform_app_candidates"),
  checkUpdates: () => invoke<UpdateStatus>("platform_check_updates"),
  openRelease: () => invoke<void>("platform_open_release"),
  openScreenshots: () => invoke<void>("platform_open_screenshots"),
  testNotification: () => invoke<void>("platform_test_notification"),
};

/**
 * The settings form never edits the recording state, but its copy may be minutes old: take
 * recording on/off, the pause end and the setup flag from the backend so saving the form does
 * not undo a pause or resume made from the tray meanwhile.
 */
export async function withLiveRecordingState(s: Settings): Promise<Settings> {
  try {
    const live = (await api.getSettings()).settings;
    return { ...s, recordingEnabled: live.recordingEnabled, pauseUntil: live.pauseUntil, onboardingCompleted: live.onboardingCompleted };
  } catch {
    return s;
  }
}

/** Dayflow's PausePresets.defaultMinutes. */
export const DEFAULT_PRESETS = [15, 30, 60];

/** "15 minutes", "1 hour", "1 hour 30 minutes" (the same labels as the tray). */
export function minutesLabel(m: number): string {
  const h = Math.floor(m / 60);
  const rest = m % 60;
  const hours = h === 0 ? "" : h === 1 ? "1 hour" : `${h} hours`;
  const mins = rest === 0 ? "" : rest === 1 ? "1 minute" : `${rest} minutes`;
  return [hours, mins].filter(Boolean).join(" ");
}

/** The pause choices in menu order, the same as the tray's "Pause for" menu. */
export function pauseOptions(presets: number[]): PauseOption[] {
  const valid = presets.length === 3 && presets.every((m) => Number.isInteger(m) && m >= 1 && m <= 1440);
  return [
    ...(valid ? presets : DEFAULT_PRESETS).map((m) => ({ id: String(m), label: minutesLabel(m) })),
    { id: "tomorrow", label: "Until tomorrow (4 AM)" },
    { id: "indefinite", label: "Until I resume" },
  ];
}

/** Applies the light/dark override to the page and the native window (see lib/theme.ts). */
export function applyAppearance(a: Appearance) {
  applyTheme(a);
}

/** Event a settings section dispatches to open the first-run setup again. */
export const OPEN_ONBOARDING_EVENT = "daytrace:open-onboarding";

/** "3:15 PM" for a Unix time. */
export function clockLabel(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

/** "Paused until 3:15 PM" (same wording as the tray). */
export function pausedUntilLabel(ts: number): string {
  return `Paused until ${clockLabel(ts)}`;
}
