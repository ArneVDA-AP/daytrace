/*
 * DEV ONLY (browser preview): wave 5 cleanup (R4). get_status's `cause` for the preview's error
 * texts (?fail=…, ?status=offline|failing), taken from src/dev/failure-causes.json: that file is
 * written and checked by a Rust test (llm/failure/cause.rs preview_causes_match), so the
 * previews show the backend's real wording. Applied last by src/dev/mock-ipc.ts.
 *
 * window.__daytraceFrameKey("Escape" | "KeyK" | "Digit1" …) sends the event the backend sends for
 * a shell key pressed while the WebView has focus (features/agents.rs watch_frame_keys, Windows),
 * to try the Agents frame's keys in the preview (?agents=running).
 */
import { emit } from "@tauri-apps/api/event";
import type { FailureCause, Settings, Status } from "@/api";
import causes from "@/dev/failure-causes.json";

type Args = Record<string, unknown>;
type Handlers = Record<string, (a: Args) => unknown>;

type Table = Record<string, FailureCause>;
const TABLES = causes as unknown as Record<string, { recording: Table; paused: Table } | undefined>;

export function applyR4Mock(handlers: Handlers, ctx: { settings: Settings }) {
  (window as unknown as { __daytraceFrameKey?: (code: string) => void }).__daytraceFrameKey = (code) => void emit("agents-frame-key", code);

  const base = handlers.get_status;
  handlers.get_status = (a) => {
    const s = base(a) as Status;
    const err = s.analysis.lastError;
    const tables = TABLES[ctx.settings.provider] ?? TABLES.anthropic!;
    const table = ctx.settings.recordingEnabled ? tables.recording : tables.paused;
    // An error text the file does not list: the generic cause ("The last batch failed").
    s.cause = err ? (table[err] ?? table["Something unexpected: the batch row was locked"] ?? null) : null;
    return s;
  };
}
