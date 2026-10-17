import type { Pipeline, ProviderKind } from "@/api";

/*
 * The one list of model providers, used by Settings (main and fallback provider) and by the
 * first-run setup. Moved from src/settings/providerInfo.ts; the setup's one-line hints live here
 * too, so both screens describe a provider the same way.
 */

export interface ProviderInfo {
  id: ProviderKind;
  /** Full name, for select lists ("Claude (Claude Code CLI, uses your Claude subscription)"). */
  label: string;
  /** Name on a choice card ("Claude Code CLI"). */
  title: string;
  /** Short name for sentences ("Testing Claude Code CLI…"). */
  short: string;
  /** One line on cost and on where the screenshots go. */
  note: string;
  /** The same, shorter, for the setup's choice cards. */
  brief: string;
  /** "required": an API key is needed; "optional": a key may be set; "none": no key field. */
  key: "required" | "optional" | "none";
  /** Runs a command-line tool on this PC; `baseUrl` holds the executable path. */
  cli: boolean;
  /** The model runs on this PC: nothing leaves it. */
  local: boolean;
}

export const PROVIDERS: ProviderInfo[] = [
  {
    id: "anthropic",
    label: "Claude (Anthropic API)",
    title: "Claude API",
    short: "Claude API",
    note: "Pay per use with your own API key. Screenshots are sent to Anthropic. Writes the most detailed cards.",
    brief: "Pay per use with an API key. Screenshots go to Anthropic.",
    key: "required",
    cli: false,
    local: false,
  },
  {
    id: "claude_cli",
    label: "Claude (Claude Code CLI, uses your Claude subscription)",
    title: "Claude Code CLI",
    short: "Claude Code CLI",
    note: "Uses your Claude subscription through Claude Code on this PC. Screenshots are sent to Anthropic under that account. No API key.",
    brief: "Uses your Claude subscription. Screenshots go to Anthropic.",
    key: "none",
    cli: true,
    local: false,
  },
  {
    id: "codex_cli",
    label: "ChatGPT (Codex CLI, uses your ChatGPT account)",
    title: "Codex CLI",
    short: "Codex CLI",
    note: "Uses your ChatGPT account through the Codex CLI on this PC. Screenshots are sent to OpenAI under that account. No API key.",
    brief: "Uses your ChatGPT account. Screenshots go to OpenAI.",
    key: "none",
    cli: true,
    local: false,
  },
  {
    id: "ollama",
    label: "Ollama (on this PC)",
    title: "Ollama",
    short: "Ollama",
    note: "Free and private: the model runs on this PC and nothing leaves it. Needs a vision model such as qwen3-vl:4b.",
    brief: "Free, runs on this PC. Nothing leaves it.",
    key: "none",
    cli: false,
    local: true,
  },
  {
    id: "lm_studio",
    label: "LM Studio (on this PC)",
    title: "LM Studio",
    short: "LM Studio",
    note: "Free and private, like Ollama: the model runs on this PC and nothing leaves it.",
    brief: "Free, runs on this PC. Nothing leaves it.",
    key: "optional",
    cli: false,
    local: true,
  },
  {
    id: "openai_compatible",
    label: "Other OpenAI-compatible API",
    title: "Other OpenAI-compatible server",
    short: "OpenAI-compatible API",
    note: "OpenAI, OpenRouter, Gemini’s OpenAI endpoint and similar. Screenshots are sent to that server under your API key.",
    brief: "OpenAI, OpenRouter and similar. Screenshots go to that server.",
    key: "required",
    cli: false,
    local: false,
  },
];

export function providerInfo(id: ProviderKind): ProviderInfo {
  return PROVIDERS.find((p) => p.id === id) ?? PROVIDERS[0];
}

/** Ollama and LM Studio use the Lightweight analysis; everything else the Detailed one. */
export function defaultPipeline(id: ProviderKind): Pipeline {
  return id === "ollama" || id === "lm_studio" ? "local" : "cloud";
}

/** The executable name shown in hints. */
export function cliCommand(id: ProviderKind): string {
  return id === "codex_cli" ? "codex" : "claude";
}

/** The product to install for a CLI provider. */
export function cliProduct(id: ProviderKind): string {
  return id === "codex_cli" ? "the Codex CLI" : "Claude Code";
}

/** How to sign in once, in a terminal. */
export function cliSignIn(id: ProviderKind): string {
  return id === "codex_cli" ? "run codex login in a terminal" : "run claude in a terminal and type /login";
}

/** The company that receives the screenshots of a CLI provider. */
export function cliCompany(id: ProviderKind): string {
  return id === "codex_cli" ? "OpenAI" : "Anthropic";
}

/** Where to get an API key, for the key field's hint. */
export function keySource(id: ProviderKind): string | null {
  if (id === "anthropic") return "Create one in the Claude Console (console.anthropic.com) under API keys.";
  if (id === "openai_compatible") return "Use the key your provider gives you, for example from platform.openai.com or openrouter.ai.";
  if (id === "lm_studio") return "Only needed when LM Studio’s server requires one.";
  return null;
}
