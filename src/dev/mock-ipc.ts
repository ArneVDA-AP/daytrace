/*
 * DEV ONLY: browser preview of the real UI without Tauri. `npm run dev`, open
 * http://localhost:1420/ in a browser. src/main.tsx installs this only when
 * import.meta.env.DEV is true and there is no Tauri runtime, so it is absent from production
 * builds (scripts/check-dist.mjs fails the build if the marker below shows up in dist).
 *
 * Every IPC command the UI calls is answered from SYNTHETIC fixtures (src/dev/fixtures.ts).
 * Variants, as query parameters BEFORE the hash (they survive hash navigation):
 *   ?status=recording|paused|paused-timed|locked|away|capture-error|starting   recorder
 *   ?status=writing|batch|offline|failing|nokey|queued                          analysis
 *     (combine with commas: ?status=offline,away)
 *   ?onboarding=1        first run (setup instead of the shell)
 *   ?empty=1             a fresh install without any cards
 *   ?appearance=light|dark|system   Settings > Appearance
 *   ?provider=anthropic|ollama|lm_studio|openai_compatible|claude_cli
 *   ?latency=400         delay every answer (ms) to see loading states
 *   ?chat=empty          no chat conversations yet (more Goals and Chat variants: src/dev/goalschat.ts)
 *   ?recap=today|edited|generating|failed|autoerror|auto|nohistory|exported|exportfail   Daily
 *   ?insights=error      Daily and Weekly numbers fail to load
 * Example: http://localhost:1420/?status=offline,away#/timeline
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { Card, ProviderKind, Settings, Shot, Status } from "@/api";
import type { StorageUsage } from "@/views/settings/storage-api";
import { at, shiftDay } from "@/lib/time";
import { allCards, DEFAULT_CATEGORIES, fmtClock, makeDataset } from "@/dev/fixtures";
import { computeDay, computeWeek, makeRecapMock } from "@/dev/insights";
import { screenSvg } from "@/dev/screens";
import { goalsChatHandlers } from "@/dev/goalschat";
import { timelineMock } from "@/dev/timeline";
import { applySettingsMock } from "@/dev/settings";
import { applyF4Mock } from "@/dev/f4";
import { applyF2Mock } from "@/dev/w4-f2";

export const DEV_MOCK_MARKER = "DAYTRACE_DEV_MOCK_IPC";

type Args = Record<string, unknown>;

function defaultSettings(provider: ProviderKind): Settings {
  const base: Record<ProviderKind, [string, string]> = {
    anthropic: ["https://api.anthropic.com", "claude-haiku-5-5"],
    ollama: ["http://localhost:11434", "qwen3-vl:4b"],
    lm_studio: ["http://localhost:1234", "qwen3-vl-4b-instruct"],
    openai_compatible: ["https://api.openai.com/v1", "gpt-5-mini"],
    claude_cli: ["", "haiku"],
    codex_cli: ["", "gpt-6-luna"],
  };
  return {
    recordingEnabled: true,
    captureIntervalSecs: 10,
    captureHeight: 720,
    jpegQuality: 70,
    retentionDays: 7,
    provider,
    baseUrl: base[provider][0],
    model: base[provider][1],
    pipeline: "cloud",
    categories: DEFAULT_CATEGORIES.map((c) => ({ ...c })),
    blockedApps: ["KeePassXC", "1Password", "Bitwarden"],
    outputLanguage: "",
    launchAtLogin: true,
    idlePauseSecs: 120,
    useBatchApi: false,
    reasoning: "low",
    recapExportDir: "",
    recapAuto: false,
    recapTime: "08:00",
    chatModel: "",
    fallbackProvider: null,
    fallbackBaseUrl: "",
    fallbackModel: "",
    promptOverrides: { detailedTitle: "", detailedSummary: "", lightweightTitle: "", lightweightSummary: "" },
    onboardingCompleted: true,
    pauseUntil: null,
    pausePresets: [15, 30, 60],
    appearance: "system",
    notifications: { recapReady: true, firstCards: true, recordingProblems: true, analysisProblems: true, missingApiKey: true, updates: true },
    checkUpdates: true,
    storageLimitGb: 0,
    autoInstallUpdates: true,
    recapModel: "",
    showGoalPrompt: true,
  };
}

export function installMockIpc() {
  const q = new URLSearchParams(location.search);
  const flags = new Set((q.get("status") ?? "").split(",").filter(Boolean));
  const latency = Number(q.get("latency") ?? 0) || 0;
  const now = () => Math.floor(Date.now() / 1000);
  const ds = makeDataset(now(), q.get("empty") === "1");

  const provider = (q.get("provider") as ProviderKind | null) ?? "anthropic";
  const settings: Settings = defaultSettings(provider);
  if (q.get("onboarding") === "1") settings.onboardingCompleted = false;
  const appearance = q.get("appearance");
  if (appearance === "light" || appearance === "dark" || appearance === "system") settings.appearance = appearance;
  if (flags.has("paused")) settings.recordingEnabled = false;
  if (flags.has("paused-timed")) {
    settings.recordingEnabled = false;
    settings.pauseUntil = now() + 45 * 60;
  }
  const hasApiKey = !flags.has("nokey");

  const recorder = (): Status["recorder"] => {
    if (!settings.recordingEnabled || flags.has("starting")) return { state: "off" };
    if (flags.has("locked")) return { state: "pausedLocked" };
    if (flags.has("away")) return { state: "pausedIdle" };
    if (flags.has("capture-error")) return { state: "error", detail: "Windows Graphics Capture returned no frame for the last 3 attempts" };
    return { state: "recording" };
  };

  const status = (): Status => {
    const t = now();
    const analysis: Status["analysis"] = { currentBatch: null, currentStep: null, lastError: null, lastSuccessAt: t - 11 * 60 };
    let pending = 0;
    if (flags.has("writing")) {
      analysis.currentBatch = 812;
      analysis.currentStep = "Reading screenshots";
      pending = 2;
    }
    if (flags.has("batch")) {
      analysis.currentBatch = 812;
      analysis.currentStep = "Reading screenshots (Batch API, can take a while)";
      pending = 1;
    }
    if (flags.has("queued")) pending = 2;
    if (flags.has("offline")) {
      analysis.lastError = "error sending request for url (https://api.anthropic.com/v1/messages): operation timed out";
      pending = 3;
    }
    if (flags.has("failing")) analysis.lastError = "The model's answer could not be read: expected a JSON list of cards, got plain text.";
    const r = recorder();
    return {
      recorder: r,
      analysis,
      pendingBatches: pending,
      lastCaptureAt: r.state === "recording" ? t - (t % 10) : t - 47 * 60,
      tokensToday: [182_400, 9_850],
      today: ds.today,
    };
  };

  const dataDir = "C:\\Users\\Preview\\AppData\\Roaming\\dev.daytrace.app";
  const cardsOf = (day: string) => ds.days.get(day) ?? [];
  const cardById = (id: number) => allCards(ds).find((c) => c.id === id) ?? null;

  // Screenshots: one row per 10 s of each card (ids derived from card id and index).
  const shotsFor = (c: Card, max: number): Shot[] => {
    const n = Math.max(1, Math.floor((c.endTs - c.startTs) / 10));
    const step = Math.max(1, Math.floor(n / max));
    const out: Shot[] = [];
    for (let i = 0; i < n && out.length < max; i += step) {
      const hidden = c.appSites?.secondary?.includes("bank") && i % 5 === 2 ? "KeePassXC" : null;
      out.push({
        id: c.id * 10_000 + i,
        capturedAt: c.startTs + i * 10,
        appName: hidden ? null : c.isIdleShortcut ? null : (c.appSites?.primary ?? "Microsoft Edge"),
        windowTitle: hidden ? null : c.isIdleShortcut ? null : `${c.title} - ${c.appSites?.primary ?? "Microsoft Edge"}`,
        hiddenApp: hidden,
        skipReason: null,
      });
    }
    return out;
  };
  const screenshot = (id: number) => {
    const c = cardById(Math.floor(id / 10_000));
    if (!c) return null;
    const ts = c.startTs + (id % 10_000) * 10;
    return screenSvg(c.isIdleShortcut ? null : (c.appSites?.primary ?? "Microsoft Edge"), `${c.title} - ${c.appSites?.primary ?? ""}`, fmtClock(ts), id % 97);
  };

  // Recaps (src/dev/insights.ts): written for earlier days, not yet for today; ?recap=… variants.
  const recaps = makeRecapMock({ today: ds.today, cardsOf, model: () => settings.model, q, now, sleep });

  const storage = (): StorageUsage => {
    const days = Array.from({ length: 7 }, (_, i) => shiftDay(ds.today, -i)).map((d, i) => ({ day: d, screenshots: i === 2 ? 0 : 2600 + i * 37, bytes: i === 2 ? 0 : 480_000_000 + i * 11_000_000 }));
    return {
      dataDir,
      screenshotsBytes: days.reduce((a, d) => a + d.bytes, 0),
      screenshotCount: days.reduce((a, d) => a + d.screenshots, 0),
      days,
      databaseBytes: 48_000_000,
      backupsBytes: 96_000_000,
      backups: [{ name: "daytrace-backup-1.db", bytes: 48_000_000, createdAt: now() - 86400 }],
      logsBytes: 3_400_000,
      bytesPerHour: 52_000_000,
      averageDayBytes: 495_000_000,
      health: { status: "ok", detail: null, restoredFrom: null, restoredBackupAt: null, movedTo: null, checkedAt: now() - 600 },
    };
  };

  const pauseUntilFor = (choice: string): number | null => {
    if (choice === "indefinite") return null;
    if (choice === "tomorrow") return at(shiftDay(ds.today, 1), 4, 0);
    const m = Number(choice);
    return Number.isFinite(m) ? now() + m * 60 : null;
  };

  // Handlers the area mocks override (settings, timeline extras, goals, chat) live in their files:
  // src/dev/settings.ts, timeline.ts and goalschat.ts, applied below.
  const handlers: Record<string, (a: Args) => unknown> = {
    // core
    provider_defaults: (a) => {
      const s = defaultSettings(a.provider as ProviderKind);
      return [s.baseUrl, s.model];
    },
    get_status: () => status(),
    set_recording: (a) => {
      settings.recordingEnabled = !!a.enabled;
      if (a.enabled) settings.pauseUntil = null;
    },
    get_day: (a) => structuredClone(cardsOf(a.day as string)),
    get_card_screenshots: (a) => {
      const c = cardById(a.cardId as number);
      return c ? shotsFor(c, (a.max as number) ?? 24) : [];
    },
    get_screenshot: (a) => screenshot(a.id as number),
    retry_card: () => null,
    analyze_now: () => null,
    export_markdown: (a) => {
      let n = 0;
      for (let d = a.fromDay as string; d <= (a.toDay as string); d = shiftDay(d, 1)) n += cardsOf(d).length;
      return n;
    },
    open_data_dir: () => null,
    provider_prompt_defaults: () => ({
      detailedTitle: "Write a short, specific title for what the person did.",
      detailedSummary: "Summarize the activity in two or three sentences.",
      lightweightTitle: "Write a short title.",
      lightweightSummary: "Summarize in one sentence.",
    }),
    provider_check_cli: () => ({ path: "C:\\Users\\Preview\\AppData\\Roaming\\npm\\claude.cmd", version: "2.1.0" }),
    // platform
    platform_pause: (a) => {
      settings.recordingEnabled = false;
      settings.pauseUntil = pauseUntilFor(a.choice as string);
      flags.delete("paused");
    },
    platform_resume: () => {
      settings.recordingEnabled = true;
      settings.pauseUntil = null;
    },
    platform_finish_onboarding: (a) => {
      settings.onboardingCompleted = true;
      settings.recordingEnabled = !!a.recording;
      return structuredClone(settings);
    },
    platform_take_navigation: () => null,
    platform_set_window_theme: () => null,
    platform_app_candidates: () => [
      { name: "KeePassXC", running: true, lastSeen: now() - 600, captures: 0 },
      { name: "Visual Studio Code", running: true, lastSeen: now(), captures: 1820 },
      { name: "Microsoft Edge", running: true, lastSeen: now() - 60, captures: 1210 },
      { name: "Microsoft Teams", running: false, lastSeen: now() - 7200, captures: 270 },
      { name: "Signal", running: true, lastSeen: now() - 1800, captures: 12 },
    ],
    platform_open_release: () => null,
    platform_open_screenshots: () => null,
    platform_test_notification: () => null,
    // timeline extras
    te_delete_card: (a) => {
      for (const [d, cards] of ds.days) ds.days.set(d, cards.filter((c) => c.id !== a.cardId));
      return true;
    },
    te_days_with_data: (a) => {
      const out = [];
      for (let d = a.fromDay as string; d <= (a.toDay as string); d = shiftDay(d, 1)) {
        const n = cardsOf(d).length;
        if (n) out.push({ day: d, cards: n });
      }
      return out;
    },
    te_timeline_text: (a) => cardsOf(a.day as string).map((c) => `${c.start} - ${c.end}  ${c.title}`).join("\n"),
    te_reprocess_card: () => 812,
    // insights
    insights_day: (a) => computeDay(a.day as string, cardsOf(a.day as string), settings.categories),
    insights_week: (a) => computeWeek(a.day as string, cardsOf, settings.categories),
    ...recaps,
    // storage
    storage_usage: () => storage(),
    storage_delete_older_than: () => ({ filesRemoved: 5400, bytesFreed: 980_000_000 }),
    storage_delete_day: () => ({ filesRemoved: 2600, bytesFreed: 480_000_000 }),
    storage_backup_now: () => ({ name: "daytrace-backup-2.db", bytes: 48_000_000, createdAt: now() }),
    storage_agent_setup: () => ({
      exePath: "C:\\Users\\Preview\\AppData\\Local\\Daytrace\\daytrace.exe",
      installed: false,
      claudeCodeCommand: "claude mcp add daytrace -- \"C:\\Users\\Preview\\AppData\\Local\\Daytrace\\daytrace.exe\" mcp",
      desktopConfig: "{\n  \"mcpServers\": {}\n}",
      desktopConfigPath: "C:\\Users\\Preview\\AppData\\Roaming\\Claude\\claude_desktop_config.json",
    }),
    // plugins
    "plugin:dialog|save": () => "C:\\Users\\Preview\\Documents\\daytrace-export.md",
    "plugin:dialog|open": () => "C:\\Users\\Preview\\OneDrive\\Daytrace",
  };

  // Goals and Chat: richer fixtures and variants (?goals=…, ?chat=…) from src/dev/goalschat.ts.
  Object.assign(handlers, goalsChatHandlers({ ds, settings, provider, q, now, cardsOf, hasApiKey: () => hasApiKey }));
  Object.assign(handlers, timelineMock(ds, now)); // Timeline fixtures and handlers (src/dev/timeline.ts)
  applySettingsMock(handlers, { q, flags, settings, now, dataDir });
  applyF4Mock(handlers, { q, now }); // update install, What's new, debug info (src/dev/f4.ts)
  applyF2Mock(handlers, { q, settings, today: ds.today }); // wave 4 views (src/dev/w4-f2.ts)

  mockWindows("main");
  mockIPC(
    async (cmd, payload) => {
      if (latency) await sleep(latency);
      const h = handlers[cmd];
      if (h) return h((payload ?? {}) as Args);
      if (cmd.startsWith("plugin:window|") || cmd.startsWith("plugin:webview|") || cmd.startsWith("plugin:opener|")) return null;
      console.warn(`[${DEV_MOCK_MARKER}] no mock for "${cmd}"`, payload);
      return null;
    },
    { shouldMockEvents: true },
  );
  console.info(`[${DEV_MOCK_MARKER}] browser preview with synthetic data. Variants: ?status=…, see src/dev/mock-ipc.ts`);
}

function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}
