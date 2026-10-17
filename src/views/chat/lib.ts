import { providerName, type Conversation } from "@/views/chat-api";

const clockFmt = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit" });
const dateFmt = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short" });

/** "2:14 PM" */
export function clockOf(ts: number): string {
  return clockFmt.format(new Date(ts * 1000));
}

/** "14 Oct, 9:12 AM" */
export function dateTimeOf(ts: number): string {
  return `${dateFmt.format(new Date(ts * 1000))}, ${clockOf(ts)}`;
}

/** "2:14 PM" today, "14 Oct, 2:14 PM" before. */
export function whenOf(ts: number, nowMs = Date.now()): string {
  return new Date(ts * 1000).toDateString() === new Date(nowMs).toDateString() ? clockOf(ts) : dateTimeOf(ts);
}

/** Dayflow's history groups: today, yesterday, this week, earlier (calendar days, like Dayflow). */
export function groupConversations(list: Conversation[], nowMs = Date.now()): { title: string; items: Conversation[] }[] {
  const now = new Date(nowMs);
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000;
  const startYesterday = startToday - 86400;
  const startWeek = startToday - 6 * 86400;
  const groups = [
    { title: "Today", items: [] as Conversation[] },
    { title: "Yesterday", items: [] as Conversation[] },
    { title: "This week", items: [] as Conversation[] },
    { title: "Earlier", items: [] as Conversation[] },
  ];
  for (const c of list) {
    const t = c.updatedAt;
    groups[t >= startToday ? 0 : t >= startYesterday ? 1 : t >= startWeek ? 2 : 3].items.push(c);
  }
  return groups.filter((g) => g.items.length);
}

/** "2:14 PM · claude-haiku-5-5" for today, "14 Oct · Ollama" before. */
export function conversationWhen(c: Conversation, nowMs = Date.now()): string {
  const d = new Date(c.updatedAt * 1000);
  const sameDay = d.toDateString() === new Date(nowMs).toDateString();
  const when = sameDay ? clockOf(c.updatedAt) : dateFmt.format(d);
  const who = c.model || providerName(c.provider);
  return who ? `${when} · ${who}` : when;
}

export function formatTokens(n: number): string {
  return n.toLocaleString("en-US");
}

// Same link shape as the renderer; the label stops at the next bracket, so this stays linear.
const CARD_LINK_RE = /\[([^[\]\n]+)\]\(card:\/{0,2}\d+\)/g;

/** Answer text for the clipboard: card links become their words. */
export function plainAnswer(text: string): string {
  return text.replace(CARD_LINK_RE, "$1");
}
