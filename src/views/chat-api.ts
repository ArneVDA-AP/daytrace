import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Which model answers, and why chat cannot run (if it cannot). */
export interface ChatInfo {
  provider: "anthropic" | "ollama" | "lm_studio" | "openai_compatible";
  providerLabel: string;
  model: string;
  /** Chat-only model; empty means the timeline model. */
  chatModel: string;
  timelineModel: string;
  problem: string | null;
  busy: boolean;
  welcomePrompts: string[];
}

export interface Conversation {
  id: number;
  title: string;
  provider: string | null;
  model: string | null;
  createdAt: number;
  updatedAt: number;
  messageCount: number;
}

export interface ToolRun {
  name: string;
  label: string;
  input: unknown;
  summary: string;
  ok: boolean;
}

export interface Citation {
  /** The id the answer links to (`card:<id>`). */
  citedId: number;
  /** The card to open now; a rewritten card resolves to its replacement. */
  cardId: number | null;
  day: string;
  title: string;
  category: string;
  start: string;
  end: string;
}

export interface ChatMessage {
  id: number;
  role: "user" | "assistant" | "error";
  content: string;
  /** Technical detail for errors. */
  detail: string | null;
  tools: ToolRun[];
  citations: Citation[];
  suggestions: string[];
  provider: string | null;
  model: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  createdAt: number;
}

export interface ConversationDetail {
  conversation: Conversation;
  messages: ChatMessage[];
}

export interface ChatMemory {
  content: string;
  updatedAt: number | null;
  maxChars: number;
}

export type ChatProgress = { conversationId: number } & (
  | { stage: "thinking" }
  | { stage: "toolStarted"; name: string; label: string }
  | { stage: "toolFinished"; name: string; summary: string; ok: boolean }
);

export const chatApi = {
  info: () => invoke<ChatInfo>("chat_info"),
  setModel: (model: string) => invoke<ChatInfo>("chat_set_model", { model }),
  list: () => invoke<Conversation[]>("chat_list_conversations"),
  get: (id: number) => invoke<ConversationDetail>("chat_get_conversation", { id }),
  send: (conversationId: number | null, text: string) => invoke<ConversationDetail>("chat_send", { conversationId, text }),
  cancel: () => invoke<void>("chat_cancel"),
  remove: (id: number) => invoke<void>("chat_delete_conversation", { id }),
  getMemory: () => invoke<ChatMemory>("chat_get_memory"),
  saveMemory: (content: string) => invoke<ChatMemory>("chat_save_memory", { content }),
  openLink: (url: string) => invoke<void>("chat_open_link", { url }),
  onProgress: (cb: (p: ChatProgress) => void): Promise<UnlistenFn> => listen<ChatProgress>("chat-progress", (e) => cb(e.payload)),
};

export function providerName(id: string | null): string {
  switch (id) {
    case "anthropic":
      return "Anthropic";
    case "ollama":
      return "Ollama";
    case "lm_studio":
      return "LM Studio";
    case "openai_compatible":
      return "OpenAI-compatible";
    default:
      return "";
  }
}

/** Errors from invoke arrive as strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong.";
}
