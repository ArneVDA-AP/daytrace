import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, formatDayTitle, shiftDay } from "./api";
import { teApi } from "./timeline-extras-api";
import "./timeline-extras.css";

export function ExportPanel({ day, onClose }: { day: string; onClose: () => void }) {
  const [from, setFrom] = useState(shiftDay(day, -6));
  const [to, setTo] = useState(day);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  async function run() {
    const path = await save({ defaultPath: `daytrace-${from}-to-${to}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
    if (!path) return;
    try {
      const n = await api.exportMarkdown(from, to, path);
      setMsg({ ok: true, text: `Exported ${n} cards to ${path}` });
    } catch (e) {
      setMsg({ ok: false, text: `Couldn’t export: ${e}` });
    }
  }

  async function copyDay() {
    try {
      await navigator.clipboard.writeText(await teApi.timelineText(day));
      setMsg({ ok: true, text: `Copied ${formatDayTitle(day)} to the clipboard` });
    } catch (e) {
      setMsg({ ok: false, text: `Couldn’t copy: ${e}` });
    }
  }

  return (
    <div className="exportbar">
      <label>
        From <input type="date" value={from} max={to} onChange={(e) => setFrom(e.target.value)} />
      </label>
      <label>
        to <input type="date" value={to} min={from} onChange={(e) => setTo(e.target.value)} />
      </label>
      <button className="primary" onClick={run}>Export as Markdown</button>
      <button className="ghost" onClick={copyDay}>Copy this day as text</button>
      <button className="link" onClick={onClose}>Close</button>
      <span className="muted export-hint">
        Each card comes with its category and subcategory, summary, step-by-step log and interruptions.
      </span>
      {msg && <span className={msg.ok ? "muted" : "bad"}>{msg.text}</span>}
    </div>
  );
}
