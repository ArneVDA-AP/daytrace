import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type NotificationPrefs } from "../api";
import type { SectionProps } from "./types";
import { applyAppearance, OPEN_ONBOARDING_EVENT, platformApi, type Appearance, type UpdateStatus } from "../views/platform-api";
import { DEFAULT_PRESETS, minutesLabel } from "../StatusPill";
import "../views/platform.css";

/** Dayflow's PausePresets.minuteOptions. */
const MINUTE_OPTIONS = [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 480];

const NOTIFICATIONS: { key: keyof NotificationPrefs; label: string }[] = [
  { key: "recapReady", label: "The daily recap is ready" },
  { key: "firstCards", label: "The first timeline cards after setup are ready" },
  { key: "recordingProblems", label: "Daytrace can’t capture the screen" },
  { key: "analysisProblems", label: "Cards can’t be made (API key, credits, local model offline, repeated failures)" },
  { key: "missingApiKey", label: "Screenshots are waiting for an API key" },
  { key: "updates", label: "A new version is available" },
];

function updateText(u: UpdateStatus): { text: string; tone: "ok" | "bad" | "muted" } {
  // A failed check keeps the release found before.
  if (u.available && u.latestVersion) return { text: `Version ${u.latestVersion} available${u.error ? `. The last check failed: ${u.error}` : ""}`, tone: "ok" };
  if (u.error) return { text: u.error, tone: "bad" };
  if (u.noReleases) return { text: "No release has been published yet.", tone: "muted" };
  if (u.checkedAt) return { text: "Daytrace is up to date.", tone: "muted" };
  return { text: "Not checked yet.", tone: "muted" };
}

/** Settings for appearance, pause lengths, notifications, updates and the first-run setup. */
export function PlatformSection({ s, set }: SectionProps) {
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    platformApi.info().then((i) => setUpdate(i.update)).catch(() => {});
    const un = listen<UpdateStatus>("platform-update", (e) => setUpdate(e.payload));
    return () => {
      un.then((f) => f());
      // Leaving Settings without saving drops the appearance preview.
      api.getSettings().then((d) => applyAppearance(d.settings.appearance)).catch(() => {});
    };
  }, []);

  function setAppearance(a: Appearance) {
    set("appearance", a);
    applyAppearance(a);
  }

  const presets = s.pausePresets.length === 3 ? s.pausePresets : DEFAULT_PRESETS;
  function setPreset(i: number, minutes: number) {
    set("pausePresets", presets.map((m, j) => (j === i ? minutes : m)));
  }

  async function checkNow() {
    setChecking(true);
    try {
      setUpdate(await platformApi.checkUpdates());
    } catch (e) {
      setUpdate((u) => (u ? { ...u, error: String(e) } : u));
    } finally {
      setChecking(false);
    }
  }

  const u = update ? updateText(update) : null;

  return (
    <>
      <section>
        <h2>Appearance</h2>
        <fieldset className="field">
          <span>Theme</span>
          <div className="platform-radios" role="radiogroup" aria-label="Theme">
            {(["system", "light", "dark"] as Appearance[]).map((a) => (
              <label key={a} className="check">
                <input type="radio" name="appearance" checked={s.appearance === a} onChange={() => setAppearance(a)} />
                {a === "system" ? "Same as Windows" : a === "light" ? "Light" : "Dark"}
              </label>
            ))}
          </div>
        </fieldset>
      </section>

      <section>
        <h2>Pausing</h2>
        <p className="hint">
          The tray menu and the status button offer three pause lengths, plus until tomorrow (4 AM) and until you resume. Recording resumes on its own when a
          timed pause ends, also after a restart.
        </p>
        <fieldset className="field">
          <span>Pause lengths</span>
          <div className="presets">
            {presets.map((m, i) => (
              <select key={i} value={m} aria-label={`Pause length ${i + 1}`} onChange={(e) => setPreset(i, Number(e.target.value))}>
                {MINUTE_OPTIONS.map((o) => (
                  <option key={o} value={o}>
                    {minutesLabel(o)}
                  </option>
                ))}
              </select>
            ))}
          </div>
        </fieldset>
      </section>

      <section>
        <h2>Notifications</h2>
        <p className="hint">Each kind is shown at most once a day, and not while the Daytrace window is in front.</p>
        <div className="notif-list">
          {NOTIFICATIONS.map((n) => (
            <label key={n.key} className="check">
              <input type="checkbox" checked={s.notifications[n.key]} onChange={(e) => set("notifications", { ...s.notifications, [n.key]: e.target.checked })} />
              {n.label}
            </label>
          ))}
        </div>
        <div className="row" style={{ marginTop: 10 }}>
          <button
            className="ghost"
            type="button"
            onClick={() =>
              platformApi
                .testNotification()
                .then(() => setNotice("Sent. If nothing appears, check Notifications in Windows Settings and Focus."))
                .catch((e) => setNotice(String(e)))
            }
          >
            Send a test notification
          </button>
          {notice && <span className="muted">{notice}</span>}
        </div>
      </section>

      <section>
        <h2>Updates</h2>
        <p className="hint">
          Daytrace {update?.currentVersion ?? ""} looks for a new release on GitHub. It never downloads or installs anything; the release page opens in your
          browser.
        </p>
        <label className="check">
          <input type="checkbox" checked={s.checkUpdates} onChange={(e) => set("checkUpdates", e.target.checked)} />
          Check for updates once a day
        </label>
        <div className="row" style={{ marginTop: 10 }}>
          <button className="ghost" type="button" onClick={checkNow} disabled={checking}>
            {checking ? "Checking…" : "Check now"}
          </button>
          {u && <span className={u.tone}>{u.text}</span>}
          {update?.available && (
            <button className="primary" type="button" onClick={() => platformApi.openRelease().catch((e) => setNotice(String(e)))}>
              Open the release page
            </button>
          )}
        </div>
      </section>

      <section>
        <h2>Setup</h2>
        <p className="hint">Go through the first-run steps again: what Daytrace sends where, the model, private apps and starting with Windows.</p>
        <button className="ghost" type="button" onClick={() => window.dispatchEvent(new CustomEvent(OPEN_ONBOARDING_EVENT))}>
          Run setup again
        </button>
      </section>
    </>
  );
}
