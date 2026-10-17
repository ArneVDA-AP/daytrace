/*
 * DEV ONLY. Synthetic Goals (journal_*) and Chat (chat_*) answers for the browser preview. The
 * person, their goals and their questions are invented. src/dev/mock-ipc.ts merges these handlers
 * over its basic ones.
 *
 * Variants (query parameters before the hash):
 *   ?goals=prompt     today has no plan yet: yesterday's review and the setup form
 *   ?goals=skipped    today's goals were skipped
 *   ?goals=defaults   today uses the targets carried over from an earlier day
 *   ?goals=over       a small distraction budget that today's cards go over
 *   ?goals=none       no checklist items on any day (variants combine: ?goals=defaults,none)
 *   ?chat=empty       no conversations yet
 *   ?chat=error       the newest conversation ends in a rate-limit error (Try again)
 *   ?chat=busy        another window is still answering a question
 *   ?chat=slow        answers take about four times longer (to see the progress steps)
 *   ?chat=sendfail    sending is refused before anything is stored
 *   ?provider=claude_cli, ?status=nokey   chat cannot run (the composer explains why)
 */
import { emit } from "@tauri-apps/api/event";
import type { Card, ProviderKind, Settings } from "@/api";
import type { ChatInfo, ChatMemory, ChatMessage, Citation, Conversation, ConversationDetail, ToolRun } from "@/views/chat-api";
import type { Evidence, GoalItem, GoalsView, Plan, Progress } from "@/views/journal-api";
import { formatDayShort, shiftDay } from "@/lib/time";
import type { Dataset } from "@/dev/fixtures";
import { computeDay } from "@/dev/insights";
import { countsAsOf } from "@/views/insights/format";

type Args = Record<string, unknown>;

export interface GoalsChatContext {
  ds: Dataset;
  settings: Settings;
  provider: ProviderKind;
  q: URLSearchParams;
  now: () => number;
  cardsOf: (day: string) => Card[];
  hasApiKey: () => boolean;
}

// ---- Goals ----

const STOP = new Set([
  "the",
  "a",
  "an",
  "and",
  "or",
  "of",
  "for",
  "to",
  "in",
  "on",
  "at",
  "with",
  "about",
  "my",
  "this",
  "that",
  "it",
  "be",
  "do",
  "get",
  "up",
  "from",
  "by",
  "into",
]);

function stem(w: string): string {
  for (const suf of ["ing", "ed", "es", "s"]) if (w.length > suf.length + 3 && w.endsWith(suf)) return w.slice(0, -suf.length);
  return w;
}
function words(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((w) => w.length > 1 && !STOP.has(w));
}
/** Same rule as the backend (features/journal.rs): one keyword for short goals, two for longer ones. */
function evidenceFor(goal: string, cards: Card[]): Evidence {
  const keys = [...new Set(words(goal).map(stem))];
  if (!keys.length) return { totalSecs: 0, cardCount: 0, cards: [] };
  const need = keys.length <= 2 ? 1 : 2;
  const hits = cards
    .filter((c) => c.category !== "System" && !c.isIdleShortcut)
    .map((c) => {
      const ws = new Set(words(`${c.title} ${c.summary} ${c.subcategory} ${c.appSites?.primary ?? ""}`).map(stem));
      const matched = keys.filter((k) => ws.has(k));
      return { c, matched };
    })
    .filter((h) => h.matched.length >= need)
    .map(({ c, matched }) => ({ cardId: c.id, title: c.title, start: c.start, end: c.end, secs: c.endTs - c.startTs, matched }));
  const totalSecs = hits.reduce((a, h) => a + h.secs, 0);
  hits.sort((a, b) => b.matched.length - a.matched.length || b.secs - a.secs);
  return { totalSecs, cardCount: hits.length, cards: hits.slice(0, 5) };
}

const TODAY_GOALS: [string, boolean][] = [
  ["Finish the activity table migration", true],
  ["Review the export pull requests", true],
  ["Fix the sync conflict regression", false],
  ["Write the weekly status update", false],
  ["Call the bank about the new card", false],
];
const YESTERDAY_GOALS: [string, boolean][] = [
  ["Ship the command menu", true],
  ["Calendar keyboard navigation", true],
  ["Draft the timeline design notes", false],
];

export function goalsChatHandlers(ctx: GoalsChatContext): Record<string, (a: Args) => unknown> {
  const { ds, settings, q, now, cardsOf } = ctx;
  const goalsVariant = new Set((q.get("goals") ?? "").split(",").filter(Boolean));
  const chatVariant = q.get("chat");

  // ---- goals state ----
  let itemSeq = 1;
  const items = new Map<string, { id: number; text: string; done: boolean }[]>();
  const plans = new Map<string, Plan>();
  const itemsOf = (day: string) => {
    if (!items.has(day)) {
      const seed = goalsVariant.has("none")
        ? []
        : day === ds.today
          ? TODAY_GOALS
          : day === shiftDay(ds.today, -1)
            ? YESTERDAY_GOALS
            : cardsOf(day).length
              ? [["Clear the inbox", true] as [string, boolean]]
              : [];
      items.set(
        day,
        seed.map(([text, done]) => ({ id: itemSeq++, text, done })),
      );
    }
    return items.get(day)!;
  };
  const defaultPlan = (day: string): Plan => ({
    day,
    focusTargetMinutes: 270,
    distractionLimitMinutes: 120,
    isSkipped: false,
    saved: false,
    carriedFrom: null,
  });
  // Saved plans: every earlier day with cards; today depends on the variant.
  for (let i = 1; i <= 13; i++) {
    const d = shiftDay(ds.today, -i);
    if (cardsOf(d).length) plans.set(d, { day: d, focusTargetMinutes: 300, distractionLimitMinutes: 45, isSkipped: false, saved: true, carriedFrom: null });
  }
  if (goalsVariant.has("skipped")) plans.set(ds.today, { ...defaultPlan(ds.today), isSkipped: true, saved: true });
  else if (goalsVariant.has("over")) plans.set(ds.today, { ...defaultPlan(ds.today), focusTargetMinutes: 240, distractionLimitMinutes: 20, saved: true });
  else if (!goalsVariant.has("prompt") && !goalsVariant.has("defaults"))
    plans.set(ds.today, { ...defaultPlan(ds.today), focusTargetMinutes: 300, distractionLimitMinutes: 45, saved: true });

  const planFor = (day: string): Plan => {
    const own = plans.get(day);
    if (own) return { ...own };
    // Carried forward from the latest earlier plan (Dayflow DayGoalPlan).
    for (let i = 1; i <= 30; i++) {
      const p = plans.get(shiftDay(day, -i));
      if (p && !p.isSkipped) return { ...p, day, saved: false, carriedFrom: p.day, isSkipped: false };
    }
    return defaultPlan(day);
  };
  const progressFor = (day: string): Progress => {
    const st = computeDay(day, cardsOf(day), settings.categories);
    return {
      focusSecs: st.focusSecs,
      distractedSecs: st.distractedSecs,
      // Like the backend: every focus category, also those without time yet.
      focusCategories: settings.categories
        .filter((c) => countsAsOf(c) === "focus")
        .map((c) => ({ name: c.name, color: c.color, secs: st.totals.find((t) => t.name === c.name)?.secs ?? 0 })),
      distractionCategories: settings.categories.filter((c) => countsAsOf(c) === "distraction").map((c) => c.name),
    };
  };
  const goals = (day: string): GoalsView => {
    const plan = planFor(day);
    const progress = progressFor(day);
    const y = shiftDay(day, -1);
    const prompt = day === ds.today && !plans.has(day) && goalsVariant.has("prompt");
    const yPlan = plans.get(y);
    const yProgress = progressFor(y);
    return {
      day,
      isToday: day === ds.today,
      plan,
      progress,
      review:
        prompt && yPlan && !yPlan.isSkipped
          ? {
              plan: yPlan,
              progress: yProgress,
              focusMet: yProgress.focusSecs >= yPlan.focusTargetMinutes * 60,
              distractionMet: yProgress.distractedSecs <= yPlan.distractionLimitMinutes * 60,
            }
          : null,
      reference: {
        yesterdayFocusMinutes: Math.round(yProgress.focusSecs / 60),
        lastWeekFocusMinutes: 241,
        yesterdayDistractionMinutes: Math.round(yProgress.distractedSecs / 60),
        lastWeekDistractionMinutes: 49,
      },
      prompt,
      items: itemsOf(day).map((it): GoalItem => ({ ...it, evidence: evidenceFor(it.text, cardsOf(day)) })),
    };
  };
  const dayOfItem = (id: number): string | null => {
    for (const [d, list] of items) if (list.some((x) => x.id === id)) return d;
    return null;
  };
  const cleanText = (raw: unknown): string => {
    const t = String(raw ?? "")
      .split(/\s+/)
      .filter(Boolean)
      .join(" ");
    if (!t) throw "Write what you want to get done.";
    return t.slice(0, 200);
  };

  // ---- chat state ----
  const t0 = now();
  const y = shiftDay(ds.today, -1);
  const yCards = cardsOf(y);
  const tCards = cardsOf(ds.today);
  const byTitle = (cards: Card[], part: string) => cards.find((c) => c.title.includes(part));
  const cite = (c: Card | undefined): Citation[] =>
    c ? [{ citedId: c.id, cardId: c.id, day: c.day, title: c.title, category: c.category, start: c.start, end: c.end }] : [];
  const link = (label: string, c: Card | undefined) => (c ? `[${label}](card:${c.id})` : label);
  const msg = (m: Partial<ChatMessage> & Pick<ChatMessage, "id" | "role" | "content" | "createdAt">): ChatMessage => ({
    detail: null,
    tools: [],
    citations: [],
    suggestions: [],
    provider: null,
    model: null,
    inputTokens: null,
    outputTokens: null,
    ...m,
  });
  const tool = (label: string, summary: string, ok = true): ToolRun => ({ name: "fetchTimeline", label, input: {}, summary, ok });
  const model = () => settings.chatModel || settings.model;

  const convs: ConversationDetail[] = [];
  if (chatVariant !== "empty") {
    // 1. Today: standup notes from yesterday's cards.
    const menu = byTitle(yCards, "command menu");
    const cal = byTitle(yCards, "day calendar");
    const notes = byTitle(yCards, "design notes");
    const tA = t0 - 2 * 3600;
    convs.push({
      conversation: {
        id: 4,
        title: "Standup notes for yesterday",
        provider: "anthropic",
        model: "claude-haiku-5-5",
        createdAt: tA,
        updatedAt: tA + 41,
        messageCount: 2,
      },
      messages: [
        msg({ id: 41, role: "user", content: "Generate standup notes for yesterday", createdAt: tA }),
        msg({
          id: 42,
          role: "assistant",
          content: [
            "Here are your standup notes for yesterday. Most of the work happened in the evening, between 9 PM and 2:40 AM.",
            "",
            "**Yesterday**",
            "",
            `- Built the ${link("command menu for the desktop app", menu)}, with sections, recent days and Ctrl+K to open it.`,
            `- Made the day calendar one tab stop with arrow keys inside and fixed focus after changing months. ${link("Keyboard navigation", cal)} now has tests for both.`,
            `- Wrote ${link("design notes for the timeline", notes)} on how to show gaps, idle time and failed cards.`,
            "",
            "**Today**",
            "",
            "- Finish the persistence part of the retry scheduler.",
            "",
            "**Blockers**",
            "",
            "- None that the cards show.",
          ].join("\n"),
          tools: [tool("Fetching timeline for Thu, Oct 15", `Fetched ${yCards.length} timeline cards for ${formatDayShort(y)}.`)],
          citations: [...cite(menu), ...cite(cal), ...cite(notes)],
          suggestions: ["What did I get done last week?", "When was I most focused this week"],
          provider: "anthropic",
          model: "claude-haiku-5-5",
          inputTokens: 6412,
          outputTokens: 241,
          createdAt: tA + 41,
        }),
      ],
    });

    // 2. Yesterday: focus this week, with a table, two citations in one sentence and a web link.
    const mig = byTitle(tCards, "migration");
    const prof = byTitle(tCards, "Profiling");
    const tB = t0 - 26 * 3600;
    convs.push({
      conversation: {
        id: 3,
        title: "When was I most focused this week",
        provider: "anthropic",
        model: "claude-haiku-5-5",
        createdAt: tB,
        updatedAt: tB + 52,
        messageCount: 2,
      },
      messages: [
        msg({ id: 31, role: "user", content: "When was I most focused this week", createdAt: tB }),
        msg({
          id: 32,
          role: "assistant",
          content: [
            "Your longest focus blocks were in the late morning and in the evening.",
            "",
            `On Friday you spent 1 h 15 min on ${link("the activity table migration", mig)} without a break, and later 1 h 10 min ${link("profiling the slow start", prof)}. On Thursday evening you worked 1 h 25 min on ${link("the command menu", menu)} and almost 2 h on ${link("calendar keyboard navigation", cal)}.`,
            "",
            "| Day | Focus | Longest block |",
            "| --- | --- | --- |",
            "| Mon | 4 h 10 min | 1 h 05 min |",
            "| Tue | 3 h 40 min | 50 min |",
            "| Wed | 4 h 55 min | 1 h 30 min |",
            "| Thu | 4 h 15 min | 1 h 55 min |",
            "",
            "Mornings between 10 and 12 are your most reliable focus time. If you want to read more about why the app starts slowly, the profiling notes point to the [WebView2 startup guide](https://learn.example.com/webview2/startup-performance).",
          ].join("\n"),
          tools: [
            tool("Adding up time per category for Oct 12 to Oct 16", "Added up 21 h 0 min of tracked time for Oct 12 to Oct 16."),
            tool("Searching cards for Oct 12 to Oct 16", "Found 9 matching cards for Oct 12 to Oct 16."),
          ],
          citations: [...cite(mig), ...cite(prof), ...cite(menu), ...cite(cal)],
          suggestions: ["Compare this week to last week", "What distracted me most?"],
          provider: "anthropic",
          model: "claude-haiku-5-5",
          inputTokens: 9120,
          outputTokens: 318,
          createdAt: tB + 52,
        }),
      ],
    });

    // 3. Earlier this week: a short exchange.
    const tC = t0 - 3 * 86400;
    convs.push({
      conversation: {
        id: 2,
        title: "Compare this week to last week",
        provider: "ollama",
        model: "qwen3:8b",
        createdAt: tC,
        updatedAt: tC + 70,
        messageCount: 2,
      },
      messages: [
        msg({ id: 21, role: "user", content: "Compare this week to last week", createdAt: tC }),
        msg({
          id: 22,
          role: "assistant",
          content:
            "So far this week you recorded 21 h, against 26 h by the same day last week. Work is about the same share (64 % against 61 %). Distraction went down from 3 h 20 min to 2 h 5 min, mostly fewer video breaks in the afternoon.",
          tools: [tool("Adding up time per category for Oct 5 to Oct 16", "Added up 47 h 10 min of tracked time for Oct 5 to Oct 16.")],
          provider: "ollama",
          model: "qwen3:8b",
          inputTokens: 3010,
          outputTokens: 96,
          createdAt: tC + 70,
        }),
      ],
    });

    // 4. Earlier.
    const tD = t0 - 9 * 86400;
    convs.push({
      conversation: {
        id: 1,
        title: "How long did the export review take?",
        provider: "anthropic",
        model: "claude-haiku-5-5",
        createdAt: tD,
        updatedAt: tD + 30,
        messageCount: 2,
      },
      messages: [
        msg({ id: 11, role: "user", content: "How long did the export review take?", createdAt: tD }),
        msg({
          id: 12,
          role: "assistant",
          content: "About 45 minutes, in one block after lunch.",
          provider: "anthropic",
          model: "claude-haiku-5-5",
          inputTokens: 2200,
          outputTokens: 24,
          createdAt: tD + 30,
        }),
      ],
    });

    if (chatVariant === "error") {
      const c = convs[0];
      const tE = t0 - 600;
      c.messages.push(
        msg({ id: 43, role: "user", content: "And what about the afternoon before?", createdAt: tE }),
        msg({
          id: 44,
          role: "error",
          content: "The model provider is rate limiting requests. Wait a minute and try again.",
          detail:
            'HTTP 429 Too Many Requests: {"type":"error","error":{"type":"rate_limit_error","message":"Number of request tokens has exceeded your per-minute rate limit"}}',
          tools: [tool("Fetching timeline for Thu, Oct 15", `Fetched ${yCards.length} timeline cards for ${formatDayShort(y)}.`)],
          provider: "anthropic",
          model: "claude-haiku-5-5",
          createdAt: tE + 5,
        }),
      );
      c.conversation.updatedAt = tE + 5;
      c.conversation.messageCount = c.messages.length;
    }
  }

  let busy = chatVariant === "busy";
  let memory: ChatMemory = {
    content: "Profile: works on a desktop sync app, usually in the evening.\nStyle: short standup notes as bullets, no greetings.",
    updatedAt: t0 - 86400 * 3,
    maxChars: 10000,
  };
  const slow = chatVariant === "slow" ? 4 : 1;
  let cancelled = false;
  const pause = async (ms: number) => {
    await sleep(ms * slow);
    if (cancelled) throw new Stopped();
  };

  const chatInfo = (): ChatInfo => {
    const label = (
      {
        anthropic: "Anthropic",
        ollama: "Ollama",
        lm_studio: "LM Studio",
        openai_compatible: "api.openai.com",
        claude_cli: "Claude Code CLI",
        codex_cli: "Codex CLI",
      } as const
    )[ctx.provider];
    // Both CLIs can chat (features/chat/cli_chat.rs, codex_chat.rs).
    const problem = ctx.provider === "anthropic" && !ctx.hasApiKey() ? "Chat needs an API key for Anthropic. Add one in Settings." : null;
    return {
      provider: ctx.provider as ChatInfo["provider"],
      providerLabel: label,
      model: model(),
      chatModel: settings.chatModel,
      timelineModel: settings.model,
      problem,
      busy,
      busyConversationId: busy ? (convs[0]?.conversation.id ?? null) : null,
      welcomePrompts: [
        "Generate standup notes for yesterday",
        "What did I get done last week?",
        "When was I most focused this week",
        "Compare this week to last week",
      ],
    };
  };

  const finish = (c: ConversationDetail, m: ChatMessage): ConversationDetail => {
    c.messages.push(m);
    c.conversation.messageCount = c.messages.length;
    c.conversation.updatedAt = now();
    convs.sort((a, b) => b.conversation.updatedAt - a.conversation.updatedAt);
    return structuredClone(c);
  };

  const chatSend = async (conversationId: number | null, raw: string): Promise<ConversationDetail> => {
    const text = raw.trim();
    if (!text) throw "Type a question first.";
    const info = chatInfo();
    if (info.problem) throw info.problem;
    if (chatVariant === "sendfail") throw "Could not save the question: the database is locked by another program. Try again in a moment.";
    if (busy) throw "Chat is still answering the previous question.";
    busy = true;
    cancelled = false;
    let conv: ConversationDetail | undefined;
    try {
      const t = now();
      const found = convs.find((x) => x.conversation.id === conversationId);
      if (conversationId !== null && !found) throw "This conversation no longer exists.";
      const c: ConversationDetail = found ?? {
        conversation: {
          id: Math.max(0, ...convs.map((x) => x.conversation.id)) + 1,
          title: text.length > 60 ? `${text.slice(0, 57)}…` : text,
          provider: ctx.provider,
          model: model(),
          createdAt: t,
          updatedAt: t,
          messageCount: 0,
        },
        messages: [],
      };
      if (!found) convs.unshift(c);
      conv = c;
      const id = c.conversation.id;
      const nextId = () => Math.max(0, ...c.messages.map((m) => m.id)) + 1;
      c.messages.push(msg({ id: nextId(), role: "user", content: text, createdAt: t }));
      const steps: [string, string][] = [
        ["Fetching timeline for Fri, Oct 16", `Fetched ${tCards.length} timeline cards for ${formatDayShort(ds.today)}.`],
        ["Searching cards for Oct 12 to Oct 16", "Found 6 matching cards for Oct 12 to Oct 16."],
      ];
      await emit("chat-progress", { conversationId: id, stage: "thinking" });
      await pause(600);
      for (const [label, summary] of steps) {
        await emit("chat-progress", { conversationId: id, stage: "toolStarted", name: "fetchTimeline", label });
        await pause(700);
        await emit("chat-progress", { conversationId: id, stage: "toolFinished", name: "fetchTimeline", summary, ok: true });
        await pause(200);
      }
      await emit("chat-progress", { conversationId: id, stage: "thinking" });
      await pause(500);
      const mig = byTitle(tCards, "migration");
      const rev = byTitle(tCards, "Reviewing pull requests");
      const kb = byTitle(tCards, "keyboard review");
      const answer = msg({
        id: nextId(),
        role: "assistant",
        content: `This is a preview answer from synthetic data. Today you spent 1 h 15 min on ${link("the activity table migration", mig)}, your longest focus block. After lunch you ${link("reviewed two export pull requests", rev)} and asked for a test around empty days. The only break in the morning was ${link("a keyboard review video", kb)} at 10:05 AM.`,
        tools: steps.map(([label, summary]) => tool(label, summary)),
        citations: [...cite(mig), ...cite(rev), ...cite(kb)],
        suggestions: ["What did I get done last week?", "When was I most distracted today?"],
        provider: ctx.provider,
        model: model(),
        inputTokens: 4870,
        outputTokens: 132,
        createdAt: now(),
      });
      return finish(c, answer);
    } catch (e) {
      if (!(e instanceof Stopped) || !conv) throw e;
      const c = conv;
      return finish(
        c,
        msg({
          id: Math.max(0, ...c.messages.map((m) => m.id)) + 1,
          role: "error",
          content: "Stopped before the answer was finished.",
          provider: ctx.provider,
          model: model(),
          createdAt: now(),
        }),
      );
    } finally {
      busy = chatVariant === "busy";
    }
  };

  return {
    // goals
    journal_get: (a) => goals(a.day as string),
    journal_save_plan: (a) => {
      const day = a.day as string;
      plans.set(day, {
        day,
        focusTargetMinutes: a.focusTargetMinutes as number,
        distractionLimitMinutes: a.distractionLimitMinutes as number,
        isSkipped: false,
        saved: true,
        carriedFrom: null,
      });
      return goals(day);
    },
    journal_skip: (a) => {
      const day = a.day as string;
      plans.set(day, { ...planFor(day), isSkipped: true, saved: true, carriedFrom: null });
      return goals(day);
    },
    journal_add_item: (a) => {
      const day = a.day as string;
      const text = cleanText(a.text);
      if (itemsOf(day).length >= 30) throw "A day can have at most 30 goals.";
      itemsOf(day).push({ id: itemSeq++, text, done: false });
      return goals(day);
    },
    journal_update_item: (a) => {
      const day = dayOfItem(a.id as number);
      if (!day) throw "This goal no longer exists.";
      const it = itemsOf(day).find((x) => x.id === a.id)!;
      if (typeof a.text === "string") it.text = cleanText(a.text);
      if (typeof a.done === "boolean") it.done = a.done;
      return goals(day);
    },
    journal_delete_item: (a) => {
      const day = a.day as string;
      items.set(
        day,
        itemsOf(day).filter((x) => x.id !== a.id),
      );
      return goals(day);
    },
    // chat
    chat_info: () => chatInfo(),
    chat_set_model: (a) => {
      settings.chatModel = String(a.model ?? "").trim();
      return chatInfo();
    },
    chat_list_conversations: (): Conversation[] => convs.map((c) => ({ ...c.conversation })),
    chat_get_conversation: (a) => {
      const c = convs.find((x) => x.conversation.id === a.id);
      if (!c) throw "This conversation no longer exists.";
      return structuredClone(c);
    },
    chat_send: (a) => chatSend((a.conversationId as number | null) ?? null, String(a.text ?? "")),
    chat_cancel: () => {
      if (busy) cancelled = true;
    },
    chat_delete_conversation: (a) => {
      if (busy && convs[0]?.conversation.id === a.id) throw "This chat is still being answered.";
      const i = convs.findIndex((c) => c.conversation.id === a.id);
      if (i >= 0) convs.splice(i, 1);
    },
    chat_get_memory: () => ({ ...memory }),
    chat_save_memory: (a) => {
      memory = { ...memory, content: String(a.content ?? "").slice(0, memory.maxChars), updatedAt: now() };
      return { ...memory };
    },
    chat_open_link: () => null,
  };
}

class Stopped extends Error {}

function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}
