/*
 * Why the last analysis failed, in plain words, with the fix (UX2). The classification is a copy
 * of src-tauri/src/llm/failure.rs (Dayflow TimelineFailureClassifier.swift): same rules, same
 * order, the first match wins. The status DTO carries only the raw error text, so the UI sorts it
 * here. Keep both lists in step when one changes.
 *
 * What Daytrace does by itself (analysis.rs): every new batch of screenshots (about every 15
 * minutes while recording) is analyzed as it comes; once one succeeds, failed batches of the last
 * 24 hours are tried again, up to 3 times each. So nothing is lost while a problem lasts, but the
 * failed cards are only rewritten after a request gets through.
 *
 * No imports: this file runs under `node --test` (src/app/failure.test.mjs).
 */

export type FailureKind =
  | "loginExpired"
  | "cliMissing"
  | "cliOutdated"
  | "localOffline"
  | "localModelMissing"
  | "apiKey"
  | "usageLimit"
  | "noCredits"
  | "geoBlocked"
  | "accountBlocked"
  | "modelMissing"
  | "notConfigured"
  | "transient"
  | "unreadable"
  | "unknown";

export interface Classification {
  kind: FailureKind;
  /** Provider named by the error text itself ("Claude", "ChatGPT", ...). */
  provider: string | null;
}

/** failure.rs RULES, in order. */
const RULES: [FailureKind, string[]][] = [
  [
    "unreadable",
    [
      "minutes long", "empty title", "last segment", "segment end time", "segment out of bounds", "missing coverage",
      "gap detected", "do not cover all", "invalid duration", "first segment must start", "overlap detected",
      "no cards returned", "covers a genuine gap", "outside the supplied source", "activity cards overlap",
      "could not parse json", "no valid segments",
    ],
  ],
  [
    "loginExpired",
    [
      "could not be refreshed", "log out and sign in", "please run /login", "run /login", "not logged in", "oauth",
      "token has been revoked", "authentication token", "sso session", "aws sso login",
    ],
  ],
  ["cliMissing", ["cli not found", "enoent", "requires git-bash", "git-bash"]],
  ["cliOutdated", ["requires a newer version of codex", "newer version of codex"]],
  ["modelMissing", ["is not supported when using codex with a chatgpt account", "is not found for api version", "not_found_error", "model_not_found", "unknown model"]],
  ["localOffline", ["could not reach the model server"]],
  ["localModelMissing", ["ollama api request failed with status 404", "no models loaded", "context size has been exceeded"]],
  ["usageLimit", ["hit your usage limit", "hit your limit", "usage limit", "session limit", "weekly limit", "spend limit", "accountquotaexceeded", "usage quota"]],
  ["noCredits", ["prepayment credits", "insufficient balance", "spending cap", "dunning", "payment required", "out of credits", "credit balance"]],
  [
    "apiKey",
    ["api key not found", "api key expired", "has been suspended", "api key not valid", "invalid api key", "service account", "no anthropic api key", "authentication_error", "invalid x-api-key"],
  ],
  ["geoBlocked", ["location is not supported"]],
  ["accountBlocked", ["denied access", "organization has disabled", "account is on hold"]],
  ["notConfigured", ["no llm provider configured"]],
  [
    "transient",
    [
      "timed out", "reconnecting", "network connection", "internet connection", "tls error", "ssl error", "could not connect",
      "unable to connect", "connection refused", "econnreset", "enotfound", "socket", "hostname", "502", "503", "529",
      "upstream", "service unavailable", "internal error", "high demand", "at capacity", "quota exceeded",
      "too many requests", "overloaded", "stream disconnected", "message too long", "cancelled", "error sending request",
      "http 429", "rate limit",
    ],
  ],
  ["unreadable", ["failed to decode", "be read because", "failed to parse", "reading additional input", "failed to load", "no stdout to parse", "returned no answer", "unusable"]],
];

function kindFor(lower: string): FailureKind {
  const t = lower.trim();
  // Raw model output as the error text: an unreadable reply.
  if (!t || t.startsWith("[") || t.startsWith("{") || t.startsWith("]") || t.startsWith("`")) return "unreadable";
  // A rejected key over HTTP before the broad patterns.
  if (t.includes("http 401") || (t.includes("http 403") && t.includes("key"))) return "apiKey";
  return RULES.find(([, pats]) => pats.some((p) => lower.includes(p)))?.[0] ?? "unknown";
}

function providerIn(lower: string): string | null {
  if (lower.includes("codex") || lower.includes("chatgpt") || lower.includes("could not be refreshed")) return "ChatGPT";
  if (lower.includes("aws sso") || lower.includes("sso session")) return "AWS";
  if (lower.includes("claude") || lower.includes("/login") || lower.includes("anthropic")) return "Claude";
  if (lower.includes("ollama") || lower.includes("lmstudio") || lower.includes("lm studio")) return "Ollama/LM Studio";
  if (lower.includes("gemini") || lower.includes("generativelanguage")) return "Gemini";
  return null;
}

export function classifyFailure(message: string): Classification {
  const lower = message.toLowerCase();
  return { kind: kindFor(lower), provider: providerIn(lower) };
}

// Transient failures split into "can't reach" (the existing network banner) and "busy".
const NETWORK = /error sending request|could not reach|could not connect|unable to connect|connection|connect error|timed out|timeout|dns|resolve|unreachable|network|no route|refused|offline|failed to fetch|econnreset|enotfound|socket|hostname|tls error|ssl error/i;

const isCli = (p?: ProviderId) => p === "claude_cli" || p === "codex_cli";

/**
 * The provider could not be reached (network, DNS, a local server that is not running). A CLI
 * provider that times out is slow, not unreachable: that one gets the failure banner.
 */
export function isUnreachable(message: string | null | undefined, provider?: ProviderId): boolean {
  if (!message) return false;
  const c = classifyFailure(message);
  if (c.kind === "localOffline") return true;
  if (isCli(provider)) return false;
  return (c.kind === "transient" || c.kind === "unknown") && NETWORK.test(message);
}

// ---------------------------------------------------------------- what the UI says

export type ProviderId = "anthropic" | "ollama" | "lm_studio" | "openai_compatible" | "claude_cli" | "codex_cli";

export type FixAction =
  /** Settings → Model (provider section). */
  | { kind: "settings"; label: string }
  /** One of the fixed pages shell_open_link opens. */
  | { kind: "link"; label: string; target: "console_billing" | "console_keys" }
  /** Retry today's failed batches now (or analyze the queue). */
  | { kind: "retry"; label: string };

export interface FailureView {
  kind: FailureKind;
  /** Short line for the rail ("API key refused"). */
  label: string;
  /** The cause and what Daytrace does by itself, in a few plain sentences (banner body). */
  body: string;
  /** Second line in the rail: what happens next. */
  next: string;
  /** Buttons, the first one is the main fix. */
  fixes: FixAction[];
  /** Daytrace gets past this by itself (busy servers, an unreadable answer). */
  selfHealing: boolean;
}

/** "Claude", "Claude Code", "Ollama", ... for the configured provider. */
export function providerName(p: ProviderId, host?: string | null): string {
  switch (p) {
    case "anthropic":
      return "Claude";
    case "claude_cli":
      return "Claude Code";
    case "codex_cli":
      return "Codex";
    case "ollama":
      return "Ollama";
    case "lm_studio":
      return "LM Studio";
    case "openai_compatible":
      return host || "your AI provider";
  }
}

const RETRY: FixAction = { kind: "retry", label: "Try again" };
const MODEL: FixAction = { kind: "settings", label: "Provider settings" };

/**
 * The failure of the last batch as the rail and the banner show it. `recording`: recording is on
 * (new batches keep coming), so the text can promise the next try.
 */
export function describeFailure(message: string, ctx: { provider: ProviderId; host?: string | null; recording: boolean }): FailureView {
  const c = classifyFailure(message);
  const named = c.provider === "Ollama/LM Studio" ? null : c.provider;
  const configured = providerName(ctx.provider, ctx.host);
  // The CLI providers' error texts name the service ("Claude"); the copy names the tool.
  const who = ctx.provider === "claude_cli" || ctx.provider === "codex_cli" ? configured : (named ?? configured);
  const nextBatch = ctx.recording ? "with the next batch of screenshots (about every 15 minutes)" : "once recording resumes and the next batch is ready";
  const afterFix = "Screenshots are kept: once it works again, Daytrace analyzes the next batch and then rewrites the failed cards by itself.";
  const selfHeal = `Daytrace tries again by itself ${nextBatch} and rewrites the failed cards once a request gets through.`;
  const view = (label: string, body: string, fixes: FixAction[], selfHealing = false): FailureView => ({
    kind: c.kind,
    label,
    body,
    next: selfHealing ? (ctx.recording ? "Tries again with the next batch" : "Tries again when recording resumes") : "Cards wait until this is fixed",
    fixes,
    selfHealing,
  });

  switch (c.kind) {
    case "apiKey":
      return view(
        `${who} refused the API key`,
        `It may be revoked, expired or mistyped. Add a new key under Settings, Model provider. ${afterFix}`,
        [{ kind: "settings", label: "Replace API key" }, ...(ctx.provider === "anthropic" ? [{ kind: "link", label: "Open Claude Console", target: "console_keys" } as FixAction] : [])],
      );
    case "noCredits":
      return view(
        `Your ${who} account is out of credits`,
        `${ctx.provider === "anthropic" ? "Top it up in the Claude Console" : "Top it up"}, or switch providers under Settings, Model provider. ${afterFix}`,
        ctx.provider === "anthropic" ? [{ kind: "link", label: "Open Claude Console", target: "console_billing" }, MODEL] : [MODEL],
      );
    case "usageLimit":
      return view(
        `${who} usage limit reached`,
        "Daytrace catches up by itself once the limit resets. To keep cards coming meanwhile, set a fallback provider under Settings, Model provider.",
        [MODEL],
      );
    case "loginExpired":
      return view(
        `${who} is signed out`,
        who === "Codex" || named === "ChatGPT"
          ? `Run “codex login” in PowerShell to sign in again, then choose Try again. ${afterFix}`
          : named === "AWS"
            ? `Run “aws sso login” in PowerShell, then choose Try again. ${afterFix}`
            : `Open PowerShell, run “claude”, type “/login” to sign in again, then choose Try again. ${afterFix}`,
        [RETRY, MODEL],
      );
    case "cliMissing":
      return view(
        `${who} not found`,
        `Install it, or set its path under Settings, Model provider.${ctx.provider === "claude_cli" ? " Claude Code on Windows also needs Git for Windows." : ""} ${afterFix}`,
        [MODEL],
      );
    case "cliOutdated":
      return view("Codex needs an update", `Run “npm install -g @openai/codex@latest” in PowerShell, then choose Try again. ${afterFix}`, [RETRY, MODEL]);
    case "localOffline":
      return view(`Can’t reach ${configured}`, `Start it on this PC, then choose Try again. ${afterFix}`, [RETRY, MODEL]);
    case "localModelMissing":
      return view("The local model isn’t loaded", `Open ${configured} and load the model you picked in Daytrace, then choose Try again. ${afterFix}`, [RETRY, MODEL]);
    case "modelMissing":
      return view(
        "The model isn’t available",
        `Your ${who} account can’t use the model Daytrace is set to. Pick another one under Settings, Model provider. ${afterFix}`,
        [{ kind: "settings", label: "Choose a model" }],
      );
    case "geoBlocked":
      return view(`${who} isn’t available here`, `It isn’t offered in your region. Switch to another provider under Settings, Model provider. ${afterFix}`, [MODEL]);
    case "accountBlocked":
      return view(`${who} blocked this account`, `A new API key or another provider under Settings, Model provider usually fixes it. ${afterFix}`, [MODEL]);
    case "notConfigured":
      return view("No AI provider set up", `Set one up under Settings, Model provider. ${afterFix}`, [{ kind: "settings", label: "Set up a provider" }]);
    case "transient":
      if (/timed out|timeout/i.test(message)) {
        return view(`${who} took too long to answer`, selfHeal, [RETRY], true);
      }
      return view(`${who} is busy`, `Its servers are overloaded or the rate limit was reached. ${selfHeal}`, [RETRY], true);
    case "unreadable":
      return view("The model’s answer couldn’t be read", `This usually sorts itself out. ${selfHeal}`, [RETRY], true);
    case "unknown":
      return view("The last batch failed", selfHeal, [RETRY], true);
  }
}
