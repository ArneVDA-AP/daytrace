import { invoke } from "@tauri-apps/api/core";
import { api, type Settings } from "../api";

export type Appearance = Settings["appearance"];

export interface UpdateStatus {
  currentVersion: string;
  latestVersion: string | null;
  releaseUrl: string | null;
  available: boolean;
  noReleases: boolean;
  checkedAt: number | null;
  error: string | null;
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

/** Applies the light/dark override to the page (data-theme on <html>) and the window title bar. */
export function applyAppearance(a: Appearance) {
  const root = document.documentElement;
  if (a === "system") delete root.dataset.theme;
  else root.dataset.theme = a;
  platformApi.setWindowTheme(a).catch(() => {});
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
