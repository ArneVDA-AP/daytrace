import { Status } from "./api";

export function StatusPill({ status, enabled, onToggle }: { status: Status; enabled: boolean; onToggle: () => void }) {
  const r = status.recorder;
  let label = "Recording";
  let tone = "live";
  if (!enabled || r.state === "off") {
    label = "Recording paused";
    tone = "off";
  } else if (r.state === "pausedLocked") {
    label = "Paused while locked";
    tone = "off";
  } else if (r.state === "pausedIdle") {
    label = "Paused, you’re away";
    tone = "off";
  } else if (r.state === "error") {
    label = "Can’t capture the screen";
    tone = "bad";
  }
  const step = status.analysis.currentStep;
  const queue = status.pendingBatches;
  const title =
    r.state === "error"
      ? r.detail
      : status.analysis.lastError
        ? `Last analysis error: ${status.analysis.lastError}`
        : `${status.tokensToday[0].toLocaleString()} input and ${status.tokensToday[1].toLocaleString()} output tokens today`;
  return (
    <div className="statuswrap" title={title}>
      {step && (
        <span className="working">
          {step}
          {queue > 1 ? ` (${queue} waiting)` : ""}
        </span>
      )}
      <button className={`pill ${tone}`} onClick={onToggle} aria-label={enabled ? "Pause recording" : "Resume recording"}>
        <span className="dot" />
        {label}
        <span className="pillaction">{enabled ? "Pause" : "Resume"}</span>
      </button>
    </div>
  );
}
