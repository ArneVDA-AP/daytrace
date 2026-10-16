import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, shiftDay } from "./api";

export function ExportPanel({ day, onClose }: { day: string; onClose: () => void }) {
  const [from, setFrom] = useState(shiftDay(day, -6));
  const [to, setTo] = useState(day);
  const [msg, setMsg] = useState<string | null>(null);

  async function run() {
    const path = await save({ defaultPath: `daytrace-${from}-to-${to}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
    if (!path) return;
    try {
      const n = await api.exportMarkdown(from, to, path);
      setMsg(`Exported ${n} cards to ${path}`);
    } catch (e) {
      setMsg(`Couldn’t export: ${e}`);
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
      <button className="link" onClick={onClose}>Close</button>
      {msg && <span className="muted">{msg}</span>}
    </div>
  );
}
