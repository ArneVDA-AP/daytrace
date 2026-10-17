import { useState } from "react";
import { platformApi, type AppCandidate } from "../views/platform-api";
import { formatDuration } from "../api";
import "../views/platform.css";

function seen(c: AppCandidate): string {
  if (c.running) return "open now";
  if (c.lastSeen === null) return "";
  const ago = Date.now() / 1000 - c.lastSeen;
  if (ago < 3600) return "seen in the last hour";
  if (ago < 86400) return `seen ${formatDuration(ago).replace(/ \d+ min$/, "")} ago`;
  const days = Math.round(ago / 86400);
  return days === 1 ? "seen yesterday" : `seen ${days} days ago`;
}

/** Lists programs with an open window and programs from recent screenshots; one click adds one to the private list. */
export function PrivateAppPicker({ blocked, onAdd }: { blocked: string[]; onAdd: (name: string) => void }) {
  const [apps, setApps] = useState<AppCandidate[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      setApps(await platformApi.appCandidates());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  const listed = new Set(blocked.map((b) => b.trim().toLowerCase()));
  const shown = (apps ?? []).filter((a) => !listed.has(a.name.toLowerCase()));

  return (
    <div className="apppicker">
      <button className="ghost" type="button" onClick={load} disabled={loading}>
        {loading ? "Looking for apps…" : apps ? "Refresh the list" : "Choose from open and recent apps"}
      </button>
      {error && <p className="bad">Couldn’t list apps: {error}</p>}
      {apps && shown.length === 0 && !error && <p className="hint">No other apps found. Apps show up here once Daytrace has seen them in front.</p>}
      {shown.length > 0 && (
        <>
          <p className="hint">Click an app to add it to the list. Remember to save.</p>
          <ul className="apppicker-list" aria-label="Apps you can add">
            {shown.map((a) => (
              <li key={a.name}>
                <button type="button" onClick={() => onAdd(a.name)} aria-label={`Add ${a.name}`}>
                  {a.name}
                  <small>{seen(a)}</small>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

/** Adds a program name to the private list unless it is already there. */
export function addPrivateApp(list: string[], name: string): string[] {
  const clean = list.map((l) => l.trim()).filter(Boolean);
  if (clean.some((l) => l.toLowerCase() === name.toLowerCase())) return clean;
  return [...clean, name];
}
