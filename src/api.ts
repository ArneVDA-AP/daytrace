import { invoke } from "@tauri-apps/api/core";

export type ProviderKind = "anthropic" | "ollama" | "lm_studio" | "openai_compatible";
export type Pipeline = "local" | "cloud";

export interface Category {
  name: string;
  color: string;
  description: string;
  isIdle: boolean;
}

export interface Settings {
  recordingEnabled: boolean;
  captureIntervalSecs: number;
  captureHeight: number;
  jpegQuality: number;
  retentionDays: number;
  provider: ProviderKind;
  baseUrl: string;
  model: string;
  pipeline: Pipeline;
  categories: Category[];
  blockedApps: string[];
  outputLanguage: string;
  launchAtLogin: boolean;
  idlePauseSecs: number;
  useBatchApi: boolean;
  reasoning: "off" | "low" | "default";
  recapExportDir: string;
  recapAuto: boolean;
  recapTime: string;
  chatModel: string;
  // platform_extras
  onboardingCompleted: boolean;
  /** End of a timed pause (Unix seconds); null with recording off means paused until resumed. */
  pauseUntil: number | null;
  pausePresets: number[];
  appearance: "system" | "light" | "dark";
  notifications: NotificationPrefs;
  checkUpdates: boolean;
}

export interface NotificationPrefs {
  recapReady: boolean;
  firstCards: boolean;
  recordingProblems: boolean;
  analysisProblems: boolean;
  missingApiKey: boolean;
  updates: boolean;
}

export interface SettingsDto {
  settings: Settings;
  hasApiKey: boolean;
  dataDir: string;
}

export type RecorderStatus =
  | { state: "off" }
  | { state: "recording" }
  | { state: "pausedLocked" }
  | { state: "pausedIdle" }
  | { state: "error"; detail: string };

export interface Status {
  recorder: RecorderStatus;
  analysis: { currentBatch: number | null; currentStep: string | null; lastError: string | null; lastSuccessAt: number | null };
  pendingBatches: number;
  lastCaptureAt: number | null;
  tokensToday: [number, number];
  today: string;
}

export interface Distraction {
  startTime: string;
  endTime: string;
  title: string;
  summary: string;
}

export interface Card {
  id: number;
  batchId: number | null;
  start: string;
  end: string;
  startTs: number;
  endTs: number;
  day: string;
  title: string;
  summary: string;
  category: string;
  subcategory: string;
  detailedSummary: string;
  distractions: Distraction[];
  appSites: { primary: string | null; secondary: string | null } | null;
  isIdleShortcut: boolean;
}

export interface Shot {
  id: number;
  capturedAt: number;
  appName: string | null;
  windowTitle: string | null;
  hiddenApp: string | null;
  skipReason: string | null;
}

export const api = {
  getSettings: () => invoke<SettingsDto>("get_settings"),
  saveSettings: (settings: Settings, apiKey?: string) => invoke<SettingsDto>("save_settings", { settings, apiKey: apiKey ?? null }),
  providerDefaults: (provider: ProviderKind) => invoke<[string, string]>("provider_defaults", { provider }),
  getStatus: () => invoke<Status>("get_status"),
  setRecording: (enabled: boolean) => invoke<void>("set_recording", { enabled }),
  getDay: (day: string) => invoke<Card[]>("get_day", { day }),
  getCardScreenshots: (cardId: number, max = 24) => invoke<Shot[]>("get_card_screenshots", { cardId, max }),
  getScreenshot: (id: number, thumb: boolean) => invoke<string | null>("get_screenshot", { id, thumb }),
  retryCard: (cardId: number) => invoke<void>("retry_card", { cardId }),
  analyzeNow: () => invoke<void>("analyze_now"),
  listModels: (provider: ProviderKind, baseUrl: string, apiKey?: string) =>
    invoke<string[]>("list_models", { provider, baseUrl, apiKey: apiKey || null }),
  testProvider: (provider: ProviderKind, baseUrl: string, model: string, apiKey?: string) =>
    invoke<string>("test_provider", { provider, baseUrl, model, apiKey: apiKey || null }),
  exportMarkdown: (fromDay: string, toDay: string, path: string) => invoke<number>("export_markdown", { fromDay, toDay, path }),
  openDataDir: () => invoke<void>("open_data_dir"),
};

// ---- date helpers (logical days start at 4 AM, matching the backend) ----

export function shiftDay(day: string, delta: number): string {
  const [y, m, d] = day.split("-").map(Number);
  const dt = new Date(y, m - 1, d + delta);
  return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
}

export function dayBounds(day: string): [number, number] {
  const [y, m, d] = day.split("-").map(Number);
  return [new Date(y, m - 1, d, 4).getTime() / 1000, new Date(y, m - 1, d + 1, 4).getTime() / 1000];
}

export function formatDayTitle(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
}

export function formatDuration(seconds: number): string {
  const mins = Math.max(0, Math.round(seconds / 60));
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

export function clock(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}
