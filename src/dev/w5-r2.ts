/*
 * DEV ONLY. Chat answers with inline charts (wave 5, gap 19), for the browser preview. The person,
 * the numbers and the questions are invented. Variant (a query parameter before the hash):
 *   ?chat=charts   two more chats: #/chat?c=91 has every chart type, #/chat?c=92 has blocks
 *                  that can't be drawn (lengths that differ, an unknown type, too many points,
 *                  invalid JSON) next to two that can. A question asked in either gets a chart.
 * src/dev/mock-ipc.ts applies it after src/dev/goalschat.ts.
 */
import type { ChatMessage, ConversationDetail } from "@/views/chat-api";

type Args = Record<string, unknown>;
type Handlers = Record<string, (a: Args) => unknown>;

const fence = (type: string, json: unknown) => ["```chart type=" + type, typeof json === "string" ? json : JSON.stringify(json), "```"].join("\n");

const WEEK = ["Mon", "Tue", "Wed", "Thu", "Fri"];

const ALL_TYPES = [
  "Pulled this week's cards. Most of the week went to work, with the longest blocks in the late morning.",
  "",
  fence("stacked_bar", {
    title: "Time by category per day (hours)",
    x: WEEK,
    series: [
      { name: "Work", values: [4.2, 3.7, 4.9, 4.3, 3.1], color: "#407BC5" },
      { name: "Personal", values: [1.1, 0.8, 1.4, 2.2, 0.9], color: "#287C42" },
      { name: "Distraction", values: [0.6, 0.4, 0.3, 0.9, 0.5], color: "#73176F" },
      { name: "Idle", values: [0.5, 0.7, 0.4, 0.6, 0.8], color: "#878D94" },
    ],
  }),
  "",
  "- **Thursday** was the longest day, mostly the command menu and the calendar.",
  "- Distraction stayed under an hour every day.",
  "",
  fence("line", { title: "Focus time by day (hours)", x: WEEK, y: [4.2, 3.7, 4.9, 4.3, 3.1], color: "#1F6FEB" }),
  "",
  fence("bar", {
    title: "Time by activity today (minutes)",
    x: ["Activity table migration", "Pull request review", "Profiling the slow start", "Email", "Keyboard video"],
    y: [75, 50, 70, 25, 15],
    color: "#F96E00",
  }),
  "",
  fence("donut", {
    title: "Time split (today)",
    labels: ["Work", "Personal", "Distraction", "Idle"],
    values: [5.2, 1.3, 0.4, 0.6],
    colors: ["#407BC5", "#287C42", "#73176F", "#878D94"],
  }),
  "",
  fence("heatmap", {
    title: "Focus by daypart (hours)",
    x: WEEK,
    y: ["Morning", "Afternoon", "Evening"],
    values: [
      [1.8, 1.5, 2.1, 1.2, 1.6],
      [1.6, 1.4, 1.9, 1.1, 1.0],
      [0.8, 0.8, 0.9, 2.0, 0],
    ],
    color: "#1F6FEB",
  }),
  "",
  fence("gantt", {
    title: "Focus blocks (today)",
    items: [
      { label: "Migration", start: 9.25, end: 10.5 },
      { label: "Break", start: 10.5, end: 10.75, color: "#F96E00" },
      { label: "Profiling", start: 11.0, end: 12.2 },
      { label: "Review", start: 13.5, end: 14.4 },
      { label: "Migration", start: 15.0, end: 16.1 },
    ],
  }),
  "",
  "Mornings are your most reliable focus time; Thursday evening was the exception.",
].join("\n");

const days = Array.from({ length: 61 }, (_, i) => `Day ${i + 1}`);

const BROKEN = [
  "Here is what I could put together from the last month.",
  "",
  fence("bar", '{ "title": "Time by app (hours)", "x": ["Code", "Edge", "Teams"], "y": [12.5, 8] }'),
  "",
  fence("scatter", '{ "title": "Context switches against focus", "x": [1, 2], "y": [3, 4] }'),
  "",
  fence("line", { title: "Focus by day", x: days, y: days.map((_, i) => (i % 7) / 2) }),
  "",
  fence("donut", "{ title: 'Split', labels: ['Work'], values: [1] }"),
  "",
  "These two could be drawn:",
  "",
  fence("bar", { title: "Change on last week (minutes)", x: ["Work", "Personal", "Distraction"], y: [35, -20, -45] }),
  "",
  fence("bar", { title: "Time by app (hours)", x: ["Code", "Edge", "Teams"], y: [12.5, 8, 3.25], color: "FFE0B0" }),
].join("\n");

const FOLLOW_UP = (q: string) =>
  [
    `A preview answer from synthetic data for “${q.length > 60 ? `${q.slice(0, 57)}…` : q}”.`,
    "",
    fence("bar", { title: "Time by category today (hours)", x: ["Work", "Personal", "Distraction"], y: [5.2, 1.3, 0.4] }),
  ].join("\n");

export function applyChatChartsMock(handlers: Handlers, { q, now }: { q: URLSearchParams; now: () => number }) {
  if (q.get("chat") !== "charts") return;
  const t0 = now();
  const msg = (m: Partial<ChatMessage> & Pick<ChatMessage, "id" | "role" | "content" | "createdAt">): ChatMessage => ({
    detail: null,
    tools: [],
    citations: [],
    suggestions: [],
    provider: "anthropic",
    model: "claude-haiku-5-5",
    inputTokens: null,
    outputTokens: null,
    ...m,
  });
  const tool = (label: string, summary: string) => ({ name: "timeByCategory", label, input: {}, summary, ok: true });
  const conv = (id: number, title: string, at: number, question: string, answer: string, tokens: [number, number]): ConversationDetail => ({
    conversation: { id, title, provider: "anthropic", model: "claude-haiku-5-5", createdAt: at, updatedAt: at + 38, messageCount: 2 },
    messages: [
      msg({ id: id * 10 + 1, role: "user", content: question, createdAt: at, provider: null, model: null }),
      msg({
        id: id * 10 + 2,
        role: "assistant",
        content: answer,
        createdAt: at + 38,
        tools: [tool("Adding up time per category for Oct 12 to Oct 16", "Added up 31 h 40 min of tracked time for Oct 12 to Oct 16.")],
        suggestions: ["Which day had the most context switches?", "When did distractions start on Thursday?"],
        inputTokens: tokens[0],
        outputTokens: tokens[1],
      }),
    ],
  });
  const mine: ConversationDetail[] = [
    conv(91, "Show my week as charts", t0 - 1800, "Show my week as charts", ALL_TYPES, [7410, 912]),
    conv(92, "Chart my month by app", t0 - 3 * 3600, "Chart my month by app", BROKEN, [5120, 644]),
  ];
  const find = (id: unknown) => mine.find((c) => c.conversation.id === id);

  const list = handlers.chat_list_conversations;
  const get = handlers.chat_get_conversation;
  const send = handlers.chat_send;
  const del = handlers.chat_delete_conversation;
  handlers.chat_list_conversations = (a) =>
    [...mine.map((c) => ({ ...c.conversation })), ...((list(a) as ConversationDetail["conversation"][]) ?? [])].sort((x, y) => y.updatedAt - x.updatedAt);
  handlers.chat_get_conversation = (a) => {
    const c = find(a.id);
    return c ? structuredClone(c) : get(a);
  };
  handlers.chat_delete_conversation = (a) => {
    const i = mine.findIndex((c) => c.conversation.id === a.id);
    if (i >= 0) mine.splice(i, 1);
    else return del(a);
  };
  handlers.chat_send = async (a) => {
    const c = find(a.conversationId);
    if (!c) return send(a);
    const text = String(a.text ?? "").trim();
    if (!text) throw "Type a question first.";
    const next = () => Math.max(...c.messages.map((m) => m.id)) + 1;
    c.messages.push(msg({ id: next(), role: "user", content: text, createdAt: now(), provider: null, model: null }));
    await new Promise((r) => setTimeout(r, 900));
    c.messages.push(msg({ id: next(), role: "assistant", content: FOLLOW_UP(text), createdAt: now(), inputTokens: 3880, outputTokens: 120 }));
    c.conversation.messageCount = c.messages.length;
    c.conversation.updatedAt = now();
    return structuredClone(c);
  };
}
