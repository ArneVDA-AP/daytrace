import { useEffect, useState } from "react";
import { api, formatDayTitle } from "../api";
import type { SectionProps } from "./types";
import { AgentSetup, DbHealth, GB, PurgeResult, StorageUsage, formatBytes, formatWhen, storageApi } from "./storage-api";
import "./storage.css";

/** Size limits offered, in GB (Dayflow offers 1, 2, 3, 5, 10, 20 GB or unlimited). */
const LIMITS = [0, 1, 2, 3, 5, 10, 20, 50];

/**
 * Settings → Storage: disk usage, the screenshot size limit, deleting screenshots, database
 * backups, and the setup text for daytrace-mcp (AI tools). Ported from Dayflow's storage
 * settings tab and its MCP / CLI tab.
 */
export function StorageSection({ s, set }: SectionProps) {
  return (
    <>
      <StoragePanel s={s} set={set} />
      <AgentAccessPanel />
    </>
  );
}

function HealthNotice({ h }: { h: DbHealth }) {
  let text: string | null = null;
  if (h.status === "restored") {
    text =
      `At startup Daytrace found damage in its database and restored the backup from ${h.restoredBackupAt ? formatWhen(h.restoredBackupAt) : h.restoredFrom}. ` +
      `Activity recorded after that backup is missing. The damaged file was kept in the data folder as ${h.movedTo}.`;
  } else if (h.status === "reset") {
    text =
      "At startup Daytrace found damage in its database and no usable backup, so it started a new, empty database. " +
      `The damaged file was kept in the data folder as ${h.movedTo}.`;
  } else if (h.status === "damaged") {
    text =
      "Daytrace found damage in its database and could not move the file aside. Quit Daytrace, move daytrace.sqlite out of the data folder, " +
      `and start Daytrace again to restore the newest backup. Details: ${h.detail ?? "none"}`;
  }
  if (!text) return null;
  return (
    <p className="storage-notice" role="alert">
      {text}
    </p>
  );
}

function StoragePanel({ s, set }: SectionProps) {
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /** Key of the delete waiting for confirmation: "older" or "day:<YYYY-MM-DD>". */
  const [confirm, setConfirm] = useState<string | null>(null);
  const [olderDays, setOlderDays] = useState(s.retentionDays);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  async function load() {
    setLoadError(null);
    try {
      setUsage(await storageApi.usage());
    } catch (e) {
      setLoadError(String(e));
    }
  }

  useEffect(() => {
    load();
  }, []);

  function purged(r: PurgeResult) {
    setMessage(
      r.filesRemoved === 0
        ? { ok: true, text: "Nothing to delete. Screenshots from the last hour and those still waiting for analysis are kept." }
        : { ok: true, text: `Deleted ${r.filesRemoved.toLocaleString("en-US")} screenshots and freed ${formatBytes(r.bytesFreed)}.` },
    );
  }

  async function run(key: string, action: () => Promise<void>) {
    setBusy(key);
    setConfirm(null);
    setMessage(null);
    try {
      await action();
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    } finally {
      setBusy(null);
      await load();
    }
  }

  const deleteOlder = () => run("older", async () => purged(await storageApi.deleteOlderThan(olderDays)));
  const deleteDay = (day: string) => run(`day:${day}`, async () => purged(await storageApi.deleteDay(day)));
  const backupNow = () =>
    run("backup", async () => {
      const b = await storageApi.backupNow();
      setMessage({ ok: true, text: `Backup saved as ${b.name} (${formatBytes(b.bytes)}).` });
    });

  const recordedBytes = usage ? usage.days.reduce((n, d) => n + d.bytes, 0) : 0;
  const limitOptions = LIMITS.includes(s.storageLimitGb) ? LIMITS : [...LIMITS, s.storageLimitGb].sort((a, b) => a - b);
  const overLimit = s.storageLimitGb > 0 && recordedBytes > s.storageLimitGb * GB;
  const newest = usage?.backups[0];

  return (
    <section aria-labelledby="storage-h">
      <h2 id="storage-h">Storage</h2>
      {usage && <HealthNotice h={usage.health} />}
      <p className="hint">
        Screenshots take most of the space. Deleting screenshots here keeps the timeline cards and the notes they were written from, so the timeline,
        chat and recaps keep working.
      </p>

      {!usage && !loadError && <p className="muted">Measuring disk usage…</p>}
      {loadError && (
        <p className="bad">
          Couldn’t read disk usage: {loadError}{" "}
          <button className="link" type="button" onClick={load}>
            Try again
          </button>
        </p>
      )}

      {usage && (
        <>
          <dl className="storage-usage">
            <dt>Screenshots</dt>
            <dd>
              {formatBytes(usage.screenshotsBytes)}
              <span className="muted">
                {" "}
                · {usage.screenshotCount.toLocaleString("en-US")} screenshots from {usage.days.length} {usage.days.length === 1 ? "day" : "days"}
              </span>
            </dd>
            <dt>Timeline database</dt>
            <dd>{formatBytes(usage.databaseBytes)}</dd>
            <dt>Database backups</dt>
            <dd>
              {formatBytes(usage.backupsBytes)}
              <span className="muted">
                {" "}
                · {newest ? `${usage.backups.length} kept, newest ${formatWhen(newest.createdAt)}` : "no backup yet"}
              </span>{" "}
              <button className="link" type="button" onClick={backupNow} disabled={busy !== null}>
                {busy === "backup" ? "Backing up…" : "Back up now"}
              </button>
            </dd>
            <dt>Log file</dt>
            <dd>{formatBytes(usage.logsBytes)}</dd>
          </dl>
          <p className="hint">
            Daytrace backs up its database once a day and keeps the last 7 backups. It checks the database at every start and restores the newest good
            backup if the file is damaged.
          </p>

          <p className="hint">
            {usage.bytesPerHour !== null && <>Recording takes about {formatBytes(usage.bytesPerHour)} per hour right now. </>}
            {usage.averageDayBytes !== null ? (
              <>
                At your average of {formatBytes(usage.averageDayBytes)} per recorded day, keeping screenshots for {s.retentionDays}{" "}
                {s.retentionDays === 1 ? "day" : "days"} takes about {formatBytes(usage.averageDayBytes * s.retentionDays)}.
              </>
            ) : (
              <>After the first full day of recording, Daytrace estimates how much space your retention period takes.</>
            )}{" "}
            Set how long screenshots are kept under Recording.
          </p>
        </>
      )}

      <label className="field">
        <span>Limit screenshots to</span>
        <select value={s.storageLimitGb} onChange={(e) => set("storageLimitGb", Number(e.target.value))}>
          {limitOptions.map((n) => (
            <option key={n} value={n}>
              {n === 0 ? "No size limit" : `${n} GB`}
            </option>
          ))}
        </select>
      </label>
      <p className="hint">
        When screenshots take more space than this, the oldest are deleted first, in addition to the {s.retentionDays}-day limit. Screenshots from the last
        hour and those still waiting for analysis are kept.
      </p>
      {overLimit && (
        <p className="storage-warn">
          Screenshots take {formatBytes(recordedBytes)} now. After you save, Daytrace deletes the oldest within 5 minutes until they fit in{" "}
          {s.storageLimitGb} GB.
        </p>
      )}

      <div className="field">
        <span id="storage-older-label">Delete screenshots older than</span>
        <div className="row storage-older">
          <input
            type="number"
            min={1}
            max={365}
            value={olderDays}
            aria-labelledby="storage-older-label"
            onChange={(e) => {
              setOlderDays(Math.max(1, Math.min(365, Number(e.target.value) || 1)));
              setConfirm(null);
            }}
          />
          <span>days</span>
          {confirm === "older" ? (
            <>
              <button className="storage-danger" type="button" onClick={deleteOlder}>
                Delete them
              </button>
              <button className="link" type="button" onClick={() => setConfirm(null)}>
                Cancel
              </button>
            </>
          ) : (
            <button className="ghost" type="button" onClick={() => setConfirm("older")} disabled={busy !== null}>
              {busy === "older" ? "Deleting…" : "Delete now"}
            </button>
          )}
        </div>
      </div>
      {confirm === "older" && <p className="storage-warn">This deletes the screenshot files for good. Timeline cards stay.</p>}

      {message && (
        <p className={message.ok ? "ok" : "bad"} role="status">
          {message.text}
        </p>
      )}

      {usage && usage.days.length > 0 && (
        <table className="storage-days">
          <caption>Screenshots per day</caption>
          <thead>
            <tr>
              <th scope="col">Day</th>
              <th scope="col">Screenshots</th>
              <th scope="col">Size</th>
              <th scope="col">
                <span className="storage-sr">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {usage.days.map((d) => {
              const key = `day:${d.day}`;
              return (
                <tr key={d.day}>
                  <th scope="row">{formatDayTitle(d.day)}</th>
                  <td>{d.screenshots.toLocaleString("en-US")}</td>
                  <td>{formatBytes(d.bytes)}</td>
                  <td>
                    {confirm === key ? (
                      <>
                        <button className="storage-danger" type="button" onClick={() => deleteDay(d.day)}>
                          Delete this day’s screenshots
                        </button>
                        <button className="link" type="button" onClick={() => setConfirm(null)}>
                          Cancel
                        </button>
                      </>
                    ) : (
                      <button className="link" type="button" onClick={() => setConfirm(key)} disabled={busy !== null} aria-label={`Delete screenshots of ${formatDayTitle(d.day)}`}>
                        {busy === key ? "Deleting…" : "Delete"}
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {usage && usage.days.length === 0 && <p className="muted">No screenshots are stored right now.</p>}

      <div className="row storage-actions">
        <button className="ghost" type="button" onClick={() => api.openDataDir()}>
          Open data folder
        </button>
        <button className="ghost" type="button" onClick={load} disabled={busy !== null}>
          Refresh
        </button>
      </div>
    </section>
  );
}

function CopyBlock({ label, text, multiline }: { label: string; text: string; multiline?: boolean }) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }
  }
  return (
    <div className="storage-copy">
      <div className="row">
        <span className="muted">{label}</span>
        <button className="link" type="button" onClick={copy}>
          Copy
        </button>
        {state === "copied" && (
          <span className="ok" role="status">
            Copied
          </span>
        )}
        {state === "failed" && (
          <span className="bad" role="status">
            Couldn’t copy. Select the text and copy it.
          </span>
        )}
      </div>
      {multiline ? <pre className="storage-code">{text}</pre> : <code className="storage-code">{text}</code>}
    </div>
  );
}

function AgentAccessPanel() {
  const [setup, setSetup] = useState<AgentSetup | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    storageApi.agentSetup().then(setSetup, (e) => setError(String(e)));
  }, []);

  return (
    <section aria-labelledby="agent-h">
      <h2 id="agent-h">Use your timeline in AI tools</h2>
      <p className="hint">
        daytrace-mcp lets Claude Code, Claude Desktop and other MCP clients read your timeline: activities, time per category, daily recaps and the
        notes behind each activity. It only reads, works while Daytrace is closed, and never sees screenshots or the names of private apps.
      </p>
      {error && <p className="bad">Couldn’t find daytrace-mcp: {error}</p>}
      {!setup && !error && <p className="muted">Loading…</p>}
      {setup && (
        <>
          {!setup.installed && (
            <p className="storage-warn">daytrace-mcp was not found next to Daytrace ({setup.exePath}). The Daytrace installer puts it there.</p>
          )}
          <CopyBlock label="Claude Code: run this in a terminal" text={setup.claudeCodeCommand} />
          <CopyBlock label={`Claude Desktop: add this to ${setup.desktopConfigPath}`} text={setup.desktopConfig} multiline />
          <p className="hint">If the file already has “mcpServers”, add only the “daytrace” entry to it. Then restart Claude Desktop.</p>
          <p className="hint">
            It also works in a terminal: <code>daytrace-mcp cli today</code> prints today’s timeline and <code>daytrace-mcp help</code> lists every
            command. The program is in the Daytrace folder: {setup.exePath}
          </p>
        </>
      )}
    </section>
  );
}
