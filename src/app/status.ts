import type { SettingsDto, Status } from "@/api";
import { clock } from "@/lib/time";

/*
 * What the rail status block, the banners and the command menu say about recording and analysis.
 * Pure functions over api.getStatus() and the settings, so every state can be checked in isolation
 * and the wording stays in one place (same words as the tray where both exist).
 */

export type RecorderTone = "live" | "paused" | "error";

export interface RecorderView {
  tone: RecorderTone;
  /** "Recording", "Paused until 3:15 PM", "Paused while locked", ... */
  label: string;
  /** Second line, e.g. "Every 10 s, last at 2:34 AM". */
  detail: string;
  /** Recording is switched off by the user (Resume applies), as opposed to paused by Windows state. */
  userPaused: boolean;
}

export type AnalysisTone = "ok" | "busy" | "waiting" | "warn" | "error";

export interface AnalysisView {
  tone: AnalysisTone;
  label: string;
  detail: string;
}

export interface Problems {
  /** The provider needs an API key and none is stored. */
  noKey: boolean;
  /** The last analysis failed because the provider could not be reached. */
  unreachable: boolean;
  /** The recorder reports that Windows returned no image. */
  captureError: string | null;
}

/** "Claude API", "Ollama at localhost:11434", ... */
export function providerLabel(dto: SettingsDto): string {
  const s = dto.settings;
  switch (s.provider) {
    case "anthropic":
      return "the Claude API";
    case "claude_cli":
      return "Claude Code";
    case "codex_cli":
      return "Codex";
    case "ollama":
      return `Ollama at ${host(s.baseUrl) ?? "localhost"}`;
    case "lm_studio":
      return `LM Studio at ${host(s.baseUrl) ?? "localhost"}`;
    case "openai_compatible":
      return host(s.baseUrl) ?? "the model provider";
  }
}

function host(url: string): string | null {
  try {
    return new URL(url).host || null;
  } catch {
    return null;
  }
}

const NETWORK_ERROR = /error sending request|connect|connection|timed out|timeout|dns|resolve|unreachable|network|no route|refused|offline|failed to fetch/i;

/** Whether an analysis error means the provider could not be reached (vs. a refused key, a bad answer...). */
export function isNetworkError(message: string | null | undefined): boolean {
  return !!message && NETWORK_ERROR.test(message);
}

export function problems(status: Status, dto: SettingsDto): Problems {
  return {
    noKey: dto.settings.provider === "anthropic" && !dto.hasApiKey,
    unreachable: isNetworkError(status.analysis.lastError),
    captureError: status.recorder.state === "error" ? status.recorder.detail || "Windows returned no image." : null,
  };
}

function minutes(secs: number): string {
  const m = Math.round(secs / 60);
  return m === 1 ? "1 min" : `${m} min`;
}

export function describeRecorder(status: Status, dto: SettingsDto): RecorderView {
  const s = dto.settings;
  if (!s.recordingEnabled) {
    return {
      tone: "paused",
      label: s.pauseUntil ? `Paused until ${clock(s.pauseUntil)}` : "Paused",
      detail: s.pauseUntil ? "Resumes by itself, or resume now" : "Resume here or from the tray icon",
      userPaused: true,
    };
  }
  const r = status.recorder;
  switch (r.state) {
    case "recording":
      return {
        tone: "live",
        label: "Recording",
        detail: status.lastCaptureAt
          ? `Every ${s.captureIntervalSecs} s, last at ${clock(status.lastCaptureAt)}`
          : `A screenshot every ${s.captureIntervalSecs} s`,
        userPaused: false,
      };
    case "pausedLocked":
      return { tone: "paused", label: "Paused while locked", detail: "Resumes when you unlock Windows", userPaused: false };
    case "pausedIdle":
      return {
        tone: "paused",
        label: "Paused, you’re away",
        detail: s.idlePauseSecs > 0 ? `No input for ${minutes(s.idlePauseSecs)}. Resumes when you’re back` : "Resumes when you’re back",
        userPaused: false,
      };
    case "error":
      return {
        tone: "error",
        label: "Can’t capture the screen",
        detail: `Daytrace tries again every ${s.captureIntervalSecs} s`,
        userPaused: false,
      };
    case "off":
      return { tone: "paused", label: "Starting…", detail: "The first screenshot is on its way", userPaused: false };
  }
}

export function describeAnalysis(status: Status, dto: SettingsDto): AnalysisView {
  const p = problems(status, dto);
  const a = status.analysis;
  const queued = status.pendingBatches;
  const step = a.currentStep;
  if (step) {
    const batchApi = /batch api/i.test(step);
    const more = queued > 1 ? `${queued - 1} more ${queued - 1 === 1 ? "batch" : "batches"} in the queue` : "";
    return batchApi
      ? { tone: "waiting", label: "Waiting for Batch API results…", detail: more || "Usually a few minutes, at most an hour" }
      : { tone: "busy", label: "Writing timeline cards…", detail: more || stepText(step) };
  }
  if (p.noKey) {
    return { tone: "warn", label: "Waiting for an API key", detail: "Screenshots are kept until a key is added" };
  }
  if (a.lastError) {
    if (p.unreachable) {
      return {
        tone: "error",
        label: `Can’t reach ${providerLabel(dto)}`,
        detail: queued > 0 ? `${queued} ${queued === 1 ? "batch" : "batches"} waiting. Tries again with the next batch` : "Tries again with the next batch",
      };
    }
    return { tone: "error", label: "The last batch failed", detail: firstLine(a.lastError) };
  }
  if (queued > 0) {
    return { tone: "busy", label: `${queued} ${queued === 1 ? "batch" : "batches"} waiting`, detail: "Analysis runs within a minute" };
  }
  return {
    tone: "ok",
    label: "Timeline up to date",
    detail: a.lastSuccessAt ? `Last card written at ${clock(a.lastSuccessAt)}` : "Cards follow about 15 minutes after recording",
  };
}

function stepText(step: string): string {
  return step.replace(/\s*\(.*\)\s*$/, "");
}

function firstLine(msg: string): string {
  const line = msg.split("\n")[0].trim();
  return line.length > 120 ? `${line.slice(0, 117)}…` : line;
}
