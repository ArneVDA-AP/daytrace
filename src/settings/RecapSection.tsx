import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { SectionProps } from "./types";

/** A full path: a drive (C:\), a network share (\\server) or, outside Windows, a root slash. */
function isAbsolutePath(p: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(p) || /^[\\/]{2}[^\\/]/.test(p) || p.startsWith("/");
}

/** Settings for the daily recap: automatic writing, its time, and the Markdown export folder. */
export function RecapSection({ s, set }: SectionProps) {
  const [pickError, setPickError] = useState<string | null>(null);
  const dir = s.recapExportDir.trim();

  async function chooseFolder() {
    setPickError(null);
    try {
      const dir = await open({ directory: true, multiple: false, defaultPath: s.recapExportDir || undefined });
      if (typeof dir === "string") set("recapExportDir", dir);
    } catch (e) {
      setPickError(String(e));
    }
  }

  return (
    <section>
      <h2>Daily recap</h2>
      <p className="hint">
        When this is on, Daytrace writes a standup each morning from your last day with at least 3 hours of activity: the highlights, the next task, and
        room for blockers. It uses the model above and costs one request a day. It does not run while recording is paused. Generate in the Daily view
        writes one by hand.
      </p>
      <label className="check">
        <input type="checkbox" checked={s.recapAuto} onChange={(e) => set("recapAuto", e.target.checked)} />
        Write the recap automatically every day
      </label>
      <label className="field">
        <span>Write it after</span>
        <input type="time" value={s.recapTime} disabled={!s.recapAuto} onChange={(e) => set("recapTime", e.target.value || "08:00")} />
      </label>
      <label className="field">
        <span>Save recaps as Markdown in</span>
        <div className="row">
          <input value={s.recapExportDir} placeholder="Not saved to a folder" onChange={(e) => set("recapExportDir", e.target.value)} spellCheck={false} />
          <button className="ghost" type="button" onClick={chooseFolder}>Choose folder</button>
          {s.recapExportDir && (
            <button className="link" type="button" onClick={() => set("recapExportDir", "")}>Stop saving</button>
          )}
        </div>
      </label>
      {dir && !isAbsolutePath(dir) && (
        <p className="bad">Use a full path for the recap folder, such as C:\Users\you\OneDrive, or select Choose folder.</p>
      )}
      {pickError && <p className="bad">Couldn’t open the folder picker: {pickError}</p>}
      <p className="hint">
        Each day gets one file, such as Daytrace\2026-10-16.md, with the recap, the day’s numbers and its cards. Pick a folder your phone syncs, such as
        one in OneDrive. With automatic recaps on, the file is written once a day after the time above. Export recap in the Daily view writes one by hand.
      </p>
    </section>
  );
}
