import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Settings, Status } from "./api";
import { platformApi, pausedUntilLabel } from "./views/platform-api";
import "./views/platform.css";

/** Dayflow's PausePresets.defaultMinutes. */
export const DEFAULT_PRESETS = [15, 30, 60];

/** "15 minutes", "1 hour", "1 hour 30 minutes" (the same labels as the tray). */
export function minutesLabel(m: number): string {
  const h = Math.floor(m / 60);
  const rest = m % 60;
  const hours = h === 0 ? "" : h === 1 ? "1 hour" : `${h} hours`;
  const mins = rest === 0 ? "" : rest === 1 ? "1 minute" : `${rest} minutes`;
  return [hours, mins].filter(Boolean).join(" ");
}

/** The pause choices in menu order, the same as the tray's "Pause for" menu. */
export function pauseOptions(presets: number[]): { id: string; label: string }[] {
  const valid = presets.length === 3 && presets.every((m) => Number.isInteger(m) && m >= 1 && m <= 1440);
  return [
    ...(valid ? presets : DEFAULT_PRESETS).map((m) => ({ id: String(m), label: minutesLabel(m) })),
    { id: "tomorrow", label: "Until tomorrow (4 AM)" },
    { id: "indefinite", label: "Until I resume" },
  ];
}

export function StatusPill({ status, settings, onChanged }: { status: Status; settings: Settings; onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const r = status.recorder;
  const active = settings.recordingEnabled;

  let label = "Recording";
  let tone = "live";
  if (!active) {
    label = settings.pauseUntil ? pausedUntilLabel(settings.pauseUntil) : "Recording paused";
    tone = "off";
  } else if (r.state === "off") {
    label = "Starting…";
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
    error ??
    (r.state === "error"
      ? r.detail
      : status.analysis.lastError
        ? `Last analysis error: ${status.analysis.lastError}`
        : `${status.tokensToday[0].toLocaleString()} input and ${status.tokensToday[1].toLocaleString()} output tokens today`);

  // Close on a click outside or Escape; focus the first item when the menu opens.
  useEffect(() => {
    if (!open) return;
    menu.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const onDown = (e: PointerEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", onDown);
    return () => window.removeEventListener("pointerdown", onDown);
  }, [open]);

  async function run(action: () => Promise<void>) {
    setOpen(false);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    }
    onChanged();
  }

  function onMenuKey(e: KeyboardEvent) {
    const items = Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      setOpen(false);
      wrap.current?.querySelector<HTMLButtonElement>(".pill")?.focus();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      items[(i + 1) % items.length]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      items[(i - 1 + items.length) % items.length]?.focus();
    }
  }

  return (
    <div className="statuswrap pausewrap" title={title} ref={wrap}>
      {step && (
        <span className="working">
          {step}
          {queue > 1 ? ` (${queue} waiting)` : ""}
        </span>
      )}
      {active ? (
        <button className={`pill ${tone}`} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
          <span className="dot" />
          {label}
          <span className="pillaction">Pause</span>
        </button>
      ) : (
        <button className={`pill ${tone}`} onClick={() => run(platformApi.resume)} aria-label={`${label}. Resume recording`}>
          <span className="dot" />
          {label}
          <span className="pillaction">Resume</span>
        </button>
      )}
      {open && (
        <div className="pausemenu" role="menu" aria-label="Pause recording for" ref={menu} onKeyDown={onMenuKey}>
          <div className="pausemenu-head" aria-hidden="true">Pause recording for</div>
          {pauseOptions(settings.pausePresets).map((o) => (
            <button key={o.id} role="menuitem" onClick={() => run(() => platformApi.pause(o.id))}>
              {o.label}
            </button>
          ))}
        </div>
      )}
      {error && <span className="bad pill-error">{error}</span>}
    </div>
  );
}
