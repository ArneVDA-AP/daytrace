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
 * Example: http://localhost:1420/?status=offline,away#/timeline
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Card, ProviderKind, Settings, SettingsDto, Shot, Status } from "@/api";
import type { ChatInfo, ChatMessage, Conversation, ConversationDetail } from "@/views/chat-api";
import type { GoalItem, GoalsView } from "@/views/journal-api";
import type { RecapView } from "@/views/insights-api";
import type { DayExtras } from "@/timeline-extras-api";
import type { StorageUsage } from "@/settings/storage-api";
import { at, dayBounds, shiftDay } from "@/lib/time";
import { allCards, DEFAULT_CATEGORIES, fmtClock, makeDataset } from "@/dev/fixtures";
import { computeDay, computeWeek } from "@/dev/insights";
import { screenSvg } from "@/dev/screens";
import { goalsChatHandlers } from "@/dev/goalschat";

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
  let hasApiKey = !flags.has("nokey");

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

  const dto = (): SettingsDto => ({ settings: structuredClone(settings), hasApiKey: provider === "anthropic" ? hasApiKey : true, dataDir: "C:\\Users\\Preview\\AppData\\Roaming\\dev.daytrace.app" });
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

  const extras = (day: string): DayExtras => {
    const cards = cardsOf(day);
    const failed = cards.filter((c) => c.category === "System");
    return {
      day,
      edited: [],
      feedback: [],
      reviews: cards.filter((c) => c.category === "Work").slice(0, 3).map((c) => ({ cardId: c.id, rating: "focused", coverage: 1 })),
      reviewSummary: { focusedSecs: 3 * 3600, neutralSecs: 0, distractedSecs: 0, lastReviewedAt: now() - 3600, unreviewedCards: Math.max(0, cards.length - 3), firstUnreviewed: cards[3]?.id ?? null },
      failedGroups: failed.map((c) => ({ cardIds: [c.id], batchIds: [c.batchId ?? 0], startTs: c.startTs, endTs: c.endTs, retryable: true })),
    };
  };

  // Recaps: written for earlier days, not yet for today.
  const recaps = new Map<string, RecapView["entry"]>();
  const recapFor = (day: string): RecapView => {
    const cards = cardsOf(day).filter((c) => c.category !== "System" && !c.isIdleShortcut);
    let entry = recaps.get(day) ?? null;
    if (!entry && day < ds.today && cards.length) {
      entry = {
        targetDay: shiftDay(day, 1),
        sourceDay: day,
        highlights: cards.filter((c) => c.category === "Work").slice(0, 3).map((c) => c.title),
        tasks: ["Finish the retry scheduler persistence", "Write the test for empty days in the export"],
        blockers: day === shiftDay(ds.today, -1) ? ["Waiting for review on the migration"] : [],
        provider: "anthropic",
        model: "claude-haiku-5-5",
        edited: false,
        createdAt: dayBounds(day)[1],
        updatedAt: dayBounds(day)[1],
      };
    }
    return { targetDay: day, entry, sourceDay: entry?.sourceDay ?? (cards.length ? day : null), generating: false, autoError: null, exportPath: null, auto: null };
  };

  // Goals (journal) items per day.
  let itemSeq = 1;
  const items = new Map<string, GoalItem[]>();
  const goalItems = (day: string): GoalItem[] => {
    if (!items.has(day)) {
      const cards = cardsOf(day);
      const ev = (word: string) => {
        const m = cards.filter((c) => c.title.toLowerCase().includes(word));
        return { totalSecs: m.reduce((a, c) => a + c.endTs - c.startTs, 0), cardCount: m.length, cards: m.map((c) => ({ cardId: c.id, title: c.title, start: c.start, end: c.end, secs: c.endTs - c.startTs, matched: [word] })) };
      };
      items.set(day, cards.length ? [
        { id: itemSeq++, text: "Finish the activity table migration", done: true, evidence: ev("migration") },
        { id: itemSeq++, text: "Review the export pull requests", done: false, evidence: ev("review") },
        { id: itemSeq++, text: "Write the weekly status update", done: false, evidence: ev("status") },
      ] : []);
    }
    return items.get(day)!;
  };
  const goals = (day: string): GoalsView => {
    const st = computeDay(day, cardsOf(day), settings.categories);
    const plan = { day, focusTargetMinutes: 300, distractionLimitMinutes: 45, isSkipped: false, saved: day <= ds.today, carriedFrom: null };
    const progress = {
      focusSecs: st.focusSecs,
      distractedSecs: st.distractedSecs,
      focusCategories: st.totals.filter((t) => t.kind === "focus").map((t) => ({ name: t.name, color: t.color, secs: t.secs })),
      distractionCategories: ["Distraction"],
    };
    return {
      day,
      isToday: day === ds.today,
      plan,
      progress,
      review: day < ds.today ? { plan, progress, focusMet: st.focusSecs >= 300 * 60, distractionMet: st.distractedSecs <= 45 * 60 } : null,
      reference: { yesterdayFocusMinutes: 262, lastWeekFocusMinutes: 241, yesterdayDistractionMinutes: 58, lastWeekDistractionMinutes: 49 },
      prompt: false,
      items: goalItems(day),
    };
  };

  // Chat.
  const convs: ConversationDetail[] = [];
  if (q.get("chat") !== "empty") {
    const y = shiftDay(ds.today, -1);
    const cited = cardsOf(y).filter((c) => c.category === "Work").slice(0, 2);
    const t0 = now() - 2 * 3600;
    convs.push({
      conversation: { id: 1, title: "Standup notes for yesterday", provider: "anthropic", model: "claude-haiku-5-5", createdAt: t0, updatedAt: t0 + 40, messageCount: 2 },
      messages: [
        { id: 1, role: "user", content: "Generate standup notes for yesterday", detail: null, tools: [], citations: [], suggestions: [], provider: null, model: null, inputTokens: null, outputTokens: null, createdAt: t0 },
        {
          id: 2,
          role: "assistant",
          content: `**Yesterday**\n\n- Built the command menu for the desktop app [card:${cited[0]?.id ?? 0}]\n- Fixed keyboard navigation in the day calendar [card:${cited[1]?.id ?? 0}]\n\n**Today**\n\n- Finish the retry scheduler persistence\n\n**Blockers**\n\n- None`,
          detail: null,
          tools: [{ name: "get_day", label: `Read ${cardsOf(y).length} cards from yesterday`, input: { day: y }, summary: `${cardsOf(y).length} cards`, ok: true }],
          citations: cited.map((c) => ({ citedId: c.id, cardId: c.id, day: y, title: c.title, category: c.category, start: c.start, end: c.end })),
          suggestions: ["What did I get done last week?", "When was I most focused this week"],
          provider: "anthropic",
          model: "claude-haiku-5-5",
          inputTokens: 2140,
          outputTokens: 180,
          createdAt: t0 + 40,
        },
      ],
    });
  }
  const chatInfo = (): ChatInfo => ({
    provider: provider === "claude_cli" || provider === "codex_cli" ? "anthropic" : provider,
    providerLabel: provider === "anthropic" ? "Claude API" : provider,
    model: settings.chatModel || settings.model,
    chatModel: settings.chatModel,
    timelineModel: settings.model,
    problem: hasApiKey ? null : "Add an API key in Settings to use chat.",
    busy: false,
    busyConversationId: null,
    welcomePrompts: ["Generate standup notes for yesterday", "What did I get done last week?", "When was I most focused this week", "Compare this week to last week"],
  });
  const chatSend = async (conversationId: number | null, text: string): Promise<ConversationDetail> => {
    let c = convs.find((x) => x.conversation.id === conversationId);
    const t = now();
    if (!c) {
      c = { conversation: { id: convs.length + 1, title: text.slice(0, 60), provider: "anthropic", model: settings.model, createdAt: t, updatedAt: t, messageCount: 0 }, messages: [] };
      convs.unshift(c);
    }
    const id = c.conversation.id;
    const msgId = () => c!.messages.length + 1;
    c.messages.push({ id: msgId(), role: "user", content: text, detail: null, tools: [], citations: [], suggestions: [], provider: null, model: null, inputTokens: null, outputTokens: null, createdAt: t });
    await emit("chat-progress", { conversationId: id, stage: "thinking" });
    await sleep(500);
    await emit("chat-progress", { conversationId: id, stage: "toolStarted", name: "search_cards", label: "Searching cards from this week" });
    await sleep(700);
    await emit("chat-progress", { conversationId: id, stage: "toolFinished", name: "search_cards", summary: "14 cards", ok: true });
    await sleep(400);
    const today = cardsOf(ds.today).filter((x) => x.category === "Work");
    const answer: ChatMessage = {
      id: msgId(),
      role: "assistant",
      content: `This is a preview answer from synthetic data. Your longest focus block today was the migration work [card:${today[2]?.id ?? 0}].`,
      detail: null,
      tools: [{ name: "search_cards", label: "Searched cards from this week", input: { query: text }, summary: "14 cards", ok: true }],
      citations: today[2] ? [{ citedId: today[2].id, cardId: today[2].id, day: ds.today, title: today[2].title, category: today[2].category, start: today[2].start, end: today[2].end }] : [],
      suggestions: [],
      provider: "anthropic",
      model: settings.model,
      inputTokens: 1800,
      outputTokens: 120,
      createdAt: now(),
    };
    c.messages.push(answer);
    c.conversation.messageCount = c.messages.length;
    c.conversation.updatedAt = now();
    return structuredClone(c);
  };

  const storage = (): StorageUsage => {
    const days = Array.from({ length: 7 }, (_, i) => shiftDay(ds.today, -i)).map((d, i) => ({ day: d, screenshots: i === 2 ? 0 : 2600 + i * 37, bytes: i === 2 ? 0 : 480_000_000 + i * 11_000_000 }));
    return {
      dataDir: "C:\\Users\\Preview\\AppData\\Roaming\\dev.daytrace.app",
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

  const handlers: Record<string, (a: Args) => unknown> = {
    // core
    get_settings: () => dto(),
    save_settings: (a) => {
      Object.assign(settings, a.settings as Settings);
      if (typeof a.apiKey === "string" && a.apiKey.trim()) hasApiKey = true;
      return dto();
    },
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
    list_models: () => ["claude-haiku-5-5", "claude-sonnet-5", "claude-opus-5-5"],
    test_provider: () => "Connected. claude-haiku-5-5 answered in 0.8 s.",
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
    provider_key_status: () => hasApiKey,
    provider_save_key: () => {
      hasApiKey = true;
      return true;
    },
    provider_check_cli: () => ({ path: "C:\\Users\\Preview\\AppData\\Roaming\\npm\\claude.cmd", version: "2.1.0" }),
    // platform
    platform_info: () => ({
      version: "0.2.1",
      pauseOptions: [],
      minuteOptions: [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 480],
      update: { currentVersion: "0.2.1", latestVersion: "0.2.1", releaseUrl: null, available: false, noReleases: false, checkedAt: now() - 3600, error: null },
    }),
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
    platform_check_updates: () => ({ currentVersion: "0.2.1", latestVersion: "0.2.1", releaseUrl: null, available: false, noReleases: false, checkedAt: now(), error: null }),
    platform_open_release: () => null,
    platform_open_screenshots: () => null,
    platform_test_notification: () => null,
    // timeline extras
    te_day_extras: (a) => extras(a.day as string),
    te_update_card: (a) => {
      const c = cardById(a.cardId as number);
      if (!c) throw "card not found";
      if (a.title) c.title = a.title as string;
      if (a.category) c.category = a.category as string;
      if (a.subcategory) c.subcategory = a.subcategory as string;
      return structuredClone(c);
    },
    te_delete_card: (a) => {
      for (const [d, cards] of ds.days) ds.days.set(d, cards.filter((c) => c.id !== a.cardId));
      return true;
    },
    te_set_feedback: (a) => (a.direction ? { cardId: a.cardId, direction: a.direction, note: a.note ?? "", updatedAt: now() } : null),
    te_rate_card: () => null,
    te_days_with_data: (a) => {
      const out = [];
      for (let d = a.fromDay as string; d <= (a.toDay as string); d = shiftDay(d, 1)) {
        const n = cardsOf(d).length;
        if (n) out.push({ day: d, cards: n });
      }
      return out;
    },
    te_timeline_text: (a) => cardsOf(a.day as string).map((c) => `${c.start} - ${c.end}  ${c.title}`).join("\n"),
    te_reprocess_day_plan: (a) => ({ day: a.day, batchIds: cardsOf(a.day as string).map((c) => c.batchId ?? 0), keptEditedCards: 0, blocked: null }),
    te_reprocess_day: (a) => ({ day: a.day, batchIds: [], keptEditedCards: 0, blocked: null }),
    te_reprocess_card: () => 812,
    te_batch_progress: () => ({ waiting: 0, running: 0, failed: 0, step: null }),
    te_retry_batches: (a) => (a.batchIds as number[]).length,
    te_retry_status: () => [],
    // insights
    insights_day: (a) => computeDay(a.day as string, cardsOf(a.day as string), settings.categories),
    insights_week: (a) => computeWeek(a.day as string, cardsOf, settings.categories),
    insights_recap: (a) => recapFor(a.day as string),
    insights_generate_recap: async (a) => {
      await sleep(900);
      const day = a.day as string;
      const cards = cardsOf(day).filter((c) => c.category === "Work");
      recaps.set(day, {
        targetDay: day, sourceDay: day,
        highlights: cards.slice(0, 3).map((c) => c.title),
        tasks: ["Finish the retry scheduler persistence"],
        blockers: [],
        provider: "anthropic", model: settings.model, edited: false, createdAt: now(), updatedAt: now(),
      });
      return recapFor(day);
    },
    insights_save_recap: (a) => {
      const day = a.day as string;
      const cur = recapFor(day).entry;
      if (cur) recaps.set(day, { ...cur, highlights: a.highlights as string[], tasks: a.tasks as string[], blockers: a.blockers as string[], edited: true, updatedAt: now() });
      return recapFor(day);
    },
    insights_export_recap: (a) => `C:\\Users\\Preview\\OneDrive\\Daytrace\\${a.day}.md`,
    // goals
    journal_get: (a) => goals(a.day as string),
    journal_save_plan: (a) => goals(a.day as string),
    journal_skip: (a) => goals(a.day as string),
    journal_add_item: (a) => {
      goalItems(a.day as string).push({ id: itemSeq++, text: a.text as string, done: false, evidence: { totalSecs: 0, cardCount: 0, cards: [] } });
      return goals(a.day as string);
    },
    journal_update_item: (a) => {
      for (const [d, list] of items) {
        const it = list.find((x) => x.id === a.id);
        if (it) {
          if (typeof a.text === "string") it.text = a.text;
          if (typeof a.done === "boolean") it.done = a.done;
          return goals(d);
        }
      }
      return goals(ds.today);
    },
    journal_delete_item: (a) => {
      const day = a.day as string;
      items.set(day, goalItems(day).filter((x) => x.id !== a.id));
      return goals(day);
    },
    // chat
    chat_info: () => chatInfo(),
    chat_set_model: (a) => {
      settings.chatModel = a.model as string;
      return chatInfo();
    },
    chat_list_conversations: (): Conversation[] => convs.map((c) => c.conversation),
    chat_get_conversation: (a) => structuredClone(convs.find((c) => c.conversation.id === a.id) ?? convs[0]),
    chat_send: (a) => chatSend(a.conversationId as number | null, a.text as string),
    chat_cancel: () => null,
    chat_delete_conversation: (a) => {
      const i = convs.findIndex((c) => c.conversation.id === a.id);
      if (i >= 0) convs.splice(i, 1);
    },
    chat_get_memory: () => ({ content: "Works on a desktop app. Prefers short standup notes.", updatedAt: now() - 86400 * 3, maxChars: 2000 }),
    chat_save_memory: (a) => ({ content: a.content, updatedAt: now(), maxChars: 2000 }),
    chat_open_link: () => null,
    // storage
    storage_usage: () => storage(),
    storage_delete_older_than: () => ({ filesRemoved: 5400, bytesFreed: 980_000_000 }),
    storage_delete_day: () => ({ filesRemoved: 2600, bytesFreed: 480_000_000 }),
    storage_backup_now: () => ({ name: "daytrace-backup-2.db", bytes: 48_000_000, createdAt: now() }),
    storage_health: () => storage().health,
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
