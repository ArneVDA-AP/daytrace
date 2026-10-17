/*
 * DEV ONLY (browser preview): mocks for the shell commands of wave 4 (F1) and variants for the
 * shell and timeline states. Synthetic data only. Applied by src/dev/mock-ipc.ts.
 *
 *   ?fail=apikey|credits|limit|busy|flaky|login|cli|slow|model|unknown   the last batch failed
 *     for that cause (the rail line and the "Timeline cards can't be written" banner)
 *   ?badge=recap      today's recap is new (dot on Daily, taskbar overlay) until Daily is opened
 *   ?gen=off          cards without `generatedBy` (an older backend); default: every card says
 *                     which model wrote it and every fourth card came from the fallback provider
 */
import type { Card } from "@/api";

type Args = Record<string, unknown>;
type Handlers = Record<string, (a: Args) => unknown>;

/** Raw error texts as the backend reports them (shapes from the providers' real messages). */
const FAILURES: Record<string, string> = {
  apikey: 'HTTP 401: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}',
  credits:
    'HTTP 400: {"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits."}}',
  limit: "Claude Code CLI: You've hit your limit · resets 9pm (Europe/Brussels)",
  busy: 'HTTP 529: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}',
  flaky: "The model's answer could not be read: failed to parse the card list (expected a JSON array, got plain text)",
  login: "Claude Code CLI: Not logged in · Please run /login",
  cli: "Claude Code CLI not found. Install it (in PowerShell: irm https://claude.ai/install.ps1 | iex), sign in once in a terminal, or set its path in Settings.",
  slow: "CLI process timed out after 300 seconds",
  model: 'HTTP 404: {"type":"error","error":{"type":"not_found_error","message":"model: claude-haiku-9"}}',
  unknown: "Something unexpected: the batch row was locked",
};

export function applyF1Mock(handlers: Handlers, ctx: { q: URLSearchParams; today: string; now: () => number }) {
  const { q } = ctx;

  // A failed last batch, by cause.
  const fail = q.get("fail");
  const failure = fail ? FAILURES[fail] : undefined;
  if (failure) {
    const base = handlers.get_status;
    handlers.get_status = (a) => {
      const s = base(a) as { analysis: { lastError: string | null; lastSuccessAt: number | null }; pendingBatches: number };
      s.analysis.lastError = failure;
      s.pendingBatches = Math.max(s.pendingBatches, 1);
      return s;
    };
  }

  // The recap badge.
  let recapNew = q.get("badge") === "recap";
  handlers.shell_badges = () => ({ recapDay: recapNew ? ctx.today : null });
  handlers.shell_recap_opened = () => {
    recapNew = false;
    return null;
  };
  handlers.shell_set_overlay = () => null;
  handlers.shell_open_link = () => null;

  // Which model wrote each card (F4 fills it in the backend).
  if (q.get("gen") !== "off") {
    const base = handlers.get_day;
    handlers.get_day = (a) =>
      (base(a) as Card[]).map((c) =>
        c.category === "System" || c.isIdleShortcut
          ? { ...c, generatedBy: null }
          : c.id % 4 === 0
            ? { ...c, generatedBy: { provider: "Ollama", model: "qwen3-vl:4b", fallback: true } }
            : { ...c, generatedBy: { provider: "Claude API", model: "claude-haiku-5-5", fallback: false } },
      );
  }
}
