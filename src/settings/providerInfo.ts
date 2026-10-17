import type { ProviderKind } from "../api";

export interface ProviderInfo {
  id: ProviderKind;
  label: string;
  /** Short name for sentences ("Testing Claude Code CLI…"). */
  short: string;
  /** "required": an API key is needed; "optional": a key may be set; "none": no key field. */
  key: "required" | "optional" | "none";
  /** Runs a command-line tool on this PC; `baseUrl` holds the executable path. */
  cli: boolean;
}

export const PROVIDERS: ProviderInfo[] = [
  { id: "anthropic", label: "Claude (Anthropic API)", short: "Claude API", key: "required", cli: false },
  { id: "claude_cli", label: "Claude (Claude Code CLI, uses your Claude subscription)", short: "Claude Code CLI", key: "none", cli: true },
  { id: "ollama", label: "Ollama (on this PC)", short: "Ollama", key: "none", cli: false },
  { id: "lm_studio", label: "LM Studio (on this PC)", short: "LM Studio", key: "optional", cli: false },
  { id: "openai_compatible", label: "Other OpenAI-compatible API", short: "OpenAI-compatible API", key: "required", cli: false },
  { id: "codex_cli", label: "ChatGPT (Codex CLI, uses your ChatGPT account)", short: "Codex CLI", key: "none", cli: true },
];

export function providerInfo(id: ProviderKind): ProviderInfo {
  return PROVIDERS.find((p) => p.id === id) ?? PROVIDERS[0];
}

/** The executable name shown in hints. */
export function cliCommand(id: ProviderKind): string {
  return id === "codex_cli" ? "codex" : "claude";
}

/** How to sign in once, in a terminal. */
export function cliSignIn(id: ProviderKind): string {
  return id === "codex_cli" ? "run codex login in a terminal" : "run claude in a terminal and type /login";
}
