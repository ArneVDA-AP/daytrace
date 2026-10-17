import { useCallback, useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { clock, formatDuration, shiftDay } from "../api";
import type { ViewProps } from "./types";
import { DayStats, RecapView, insightsApi, percent, relativeDayLabel, shortDate } from "./insights-api";
import { Axis, Bars, CellRow, Panel, Stat, Swatch, minuteLabel } from "./insights-parts";
import "./insights.css";

const NO_HISTORY = "Not enough captured activity in the previous 3 days to generate a standup.";

/**
 * Daily: the standup for the selected day (Dayflow's Daily view: highlights from the last active
 * day, tasks for this day, blockers) and that day's numbers without a model: time per category,
 * the 15-minute activity grid, distractions and focus blocks.
 */
export function DailyView({ day, today, settings, openTimeline }: ViewProps) {
  const [stats, setStats] = useState<DayStats | null>(null);
  const [recap, setRecap] = useState<RecapView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"generate" | "save" | "export" | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [editing, setEditing] = useState<{ highlights: string; tasks: string; blockers: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const dayRef = useRef(day);
  dayRef.current = day;

  const load = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([insightsApi.day(day), insightsApi.recap(day)]);
      if (dayRef.current !== day) return;
      setStats(s);
      setRecap(r);
      setLoadError(null);
    } catch (e) {
      setLoadError(String(e));
    }
  }, [day]);

  useEffect(() => {
    setStats(null);
    setRecap(null);
    setEditing(null);
    setMessage(null);
    load();
    const t = setInterval(load, 60000);
    return () => clearInterval(t);
  }, [load]);

  const entry = recap?.entry ?? null;
  const sourceDay = entry?.sourceDay ?? recap?.sourceDay ?? null;
  const yesterday = shiftDay(today, -1);
  const highlightsTitle = sourceDay ? `Highlights from ${relativeDayLabel(sourceDay, today)}` : "Recent highlights";
  const tasksTitle = `Tasks for ${relativeDayLabel(day, today)}`;
  const generating = busy === "generate" || !!recap?.generating;
  const s = settings.settings;

  async function generate() {
    setBusy("generate");
    setMessage(null);
    try {
      const r = await insightsApi.generateRecap(day);
      if (r.targetDay === dayRef.current) {
        setRecap(r);
        setEditing(null);
      }
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    } finally {
      setBusy(null);
    }
  }

  async function saveEdits() {
    if (!editing) return;
    setBusy("save");
    const lines = (t: string) => t.split("\n").map((l) => l.trim()).filter(Boolean);
    try {
      const r = await insightsApi.saveRecap(day, lines(editing.highlights), lines(editing.tasks), lines(editing.blockers));
      setRecap(r);
      setEditing(null);
      setMessage(null);
    } catch (e) {
      setMessage({ ok: false, text: `Couldn’t save the standup: ${e}` });
    } finally {
      setBusy(null);
    }
  }

  async function copyStandup() {
    // DailyView+Standup.swift standupClipboardText
    const list = (items: string[]) => (items.length ? items.map((i) => `- ${i}`) : ["- None right now"]);
    const text = [
      highlightsTitle,
      ...list(entry?.highlights ?? []),
      "",
      tasksTitle,
      ...list(entry?.tasks ?? []),
      "",
      "Blockers",
      ...list(entry?.blockers ?? []),
    ].join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      setMessage({ ok: false, text: `Couldn’t copy: ${e}` });
    }
  }

  async function exportRecap() {
    if (!sourceDay) return;
    setBusy("export");
    setMessage(null);
    try {
      let path: string | undefined;
      if (!s.recapExportDir.trim()) {
        const p = await save({ defaultPath: `${sourceDay}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
        if (!p) return;
        path = p;
      }
      const written = await insightsApi.exportRecap(sourceDay, path);
      setMessage({ ok: true, text: `Saved to ${written}` });
      load();
    } catch (e) {
      setMessage({ ok: false, text: `Couldn’t export: ${e}` });
    } finally {
      setBusy(null);
    }
  }

  const standupHeading = `Standup for ${day === today ? "today" : shortDate(day)}`;
  const workflowHeading = day === today ? "Today so far" : day === yesterday ? "Your workflow yesterday" : `Your workflow on ${shortDate(day)}`;
  const totalsTitle = day === today ? "Today’s total so far" : day === yesterday ? "Yesterday’s total" : `Total for ${shortDate(day)}`;

  return (
    <div className="ins-view">
      {loadError && <p className="ins-error">Couldn’t load this day: {loadError}</p>}

      <section className="ins-section" aria-labelledby="ins-standup">
        <div className="ins-head">
          <h2 id="ins-standup">{standupHeading}</h2>
          <div className="ins-actions">
            {entry && !editing && (
              <button className="ghost" onClick={copyStandup}>{copied ? "Copied" : "Copy standup update"}</button>
            )}
            {!editing && (
              <button className="ghost" onClick={() => setEditing({ highlights: (entry?.highlights ?? []).join("\n"), tasks: (entry?.tasks ?? []).join("\n"), blockers: (entry?.blockers ?? []).join("\n") })}>
                Edit
              </button>
            )}
            {!editing && (
              <button className="ghost" onClick={generate} disabled={generating || !sourceDay} title={sourceDay ? undefined : NO_HISTORY}>
                {generating ? "Writing…" : entry ? "Regenerate" : "Generate"}
              </button>
            )}
            {!editing && sourceDay && (
              <button className="ghost" onClick={exportRecap} disabled={busy !== null} title={`Save the recap, numbers and cards of ${shortDate(sourceDay)} as Markdown`}>
                {busy === "export" ? "Exporting…" : "Export recap"}
              </button>
            )}
          </div>
        </div>

        {message && <p className={message.ok ? "ins-ok" : "ins-error"}>{message.text}</p>}
        {!entry && recap?.autoError && !message && <p className="ins-error">The automatic recap failed: {recap.autoError}</p>}

        {editing ? (
          <div className="ins-standup">
            <label className="ins-card">
              <span className="ins-card-title">{highlightsTitle}</span>
              <textarea rows={6} value={editing.highlights} onChange={(e) => setEditing({ ...editing, highlights: e.target.value })} />
            </label>
            <label className="ins-card">
              <span className="ins-card-title">{tasksTitle}</span>
              <textarea rows={6} value={editing.tasks} onChange={(e) => setEditing({ ...editing, tasks: e.target.value })} />
            </label>
            <label className="ins-card">
              <span className="ins-card-title">Blockers</span>
              <textarea rows={6} value={editing.blockers} onChange={(e) => setEditing({ ...editing, blockers: e.target.value })} />
            </label>
            <p className="ins-hint ins-wide">One item per line.</p>
            <div className="ins-actions ins-wide">
              <button className="primary" onClick={saveEdits} disabled={busy !== null}>{busy === "save" ? "Saving…" : "Save standup"}</button>
              <button className="ghost" onClick={() => setEditing(null)}>Cancel</button>
            </div>
          </div>
        ) : entry ? (
          <>
            <div className="ins-standup">
              <StandupList title={highlightsTitle} items={entry.highlights} />
              <StandupList title={tasksTitle} items={entry.tasks} />
              <StandupList title="Blockers" items={entry.blockers} />
            </div>
            <p className="ins-hint">
              {entry.model ? `Written by ${entry.model} from the cards of ${shortDate(entry.sourceDay)}` : `From ${shortDate(entry.sourceDay)}`}
              {entry.edited ? ", edited by you." : "."}
              {recap?.exportPath ? ` Saved as ${recap.exportPath}.` : ""}
            </p>
          </>
        ) : (
          <div className="ins-placeholder">
            {!recap ? (
              <p>Loading…</p>
            ) : !sourceDay ? (
              <p>{NO_HISTORY}</p>
            ) : (
              <p>
                No standup yet. Select Generate to write one from the cards of {shortDate(sourceDay)}.
                {day === today && s.recapAuto ? ` Daytrace also writes it automatically after ${s.recapTime}.` : ""}
              </p>
            )}
          </div>
        )}
      </section>

      <section className="ins-section" aria-labelledby="ins-workflow">
        <div className="ins-head">
          <h2 id="ins-workflow">{workflowHeading}</h2>
          <button className="link" onClick={() => openTimeline(day)}>Open timeline</button>
        </div>
        {!stats ? (
          !loadError && <p className="ins-empty">Loading…</p>
        ) : stats.cardCount === 0 ? (
          <p className="ins-empty">No cards for this day yet.</p>
        ) : (
          <DayDetails stats={stats} day={day} totalsTitle={totalsTitle} isToday={day === today} openTimeline={openTimeline} />
        )}
      </section>
    </div>
  );
}

function StandupList({ title, items }: { title: string; items: string[] }) {
  return (
    <article className="ins-card">
      <h3 className="ins-card-title">{title}</h3>
      {items.length ? (
        <ul>
          {items.map((i, n) => (
            <li key={n}>{i}</li>
          ))}
        </ul>
      ) : (
        <p className="ins-empty">None</p>
      )}
    </article>
  );
}

function DayDetails({
  stats,
  day,
  totalsTitle,
  isToday,
  openTimeline,
}: {
  stats: DayStats;
  day: string;
  totalsTitle: string;
  isToday: boolean;
  openTimeline: ViewProps["openTimeline"];
}) {
  const g = stats.grid;
  const span = Math.max(1, g.endMinute - g.startMinute);
  const slotTitle = (i: number) => `${minuteLabel(g.startMinute + i * g.slotMinutes)} – ${minuteLabel(Math.min(g.endMinute, g.startMinute + (i + 1) * g.slotMinutes))}`;
  return (
    <>
      <dl className="ins-stats">
        <Stat label="Context switched" value={`${stats.contextSwitches} times`} />
        <Stat label="Interrupted" value={`${stats.interruptions} times`} />
        <Stat label="Focused for" value={formatDuration(stats.focusSecs)} />
        <Stat label="Distracted for" value={formatDuration(stats.distractedSecs)} tone="bad" />
        <Stat label="Transitioning time" value={formatDuration(stats.transitionSecs)} />
      </dl>

      <div className="ins-grid" aria-label="Activity in 15-minute slots">
        <CellRow
          label="Main activity"
          cells={g.dominant.map((ri, i) => {
            const row = ri === null ? null : g.rows[ri];
            const cell = row ? row.cells[i] : null;
            return {
              color: row?.color ?? null,
              occupancy: row ? 1 : 0,
              title: row ? `${slotTitle(i)}: ${row.name}${cell?.title ? `, ${cell.title}` : ""}` : slotTitle(i),
              onClick: cell?.cardId ? () => openTimeline(day, cell.cardId) : undefined,
            };
          })}
        />
        {g.rows.map((row) => (
          <CellRow
            key={row.key}
            label={
              <>
                <Swatch color={row.color} />
                {row.name}
              </>
            }
            cells={row.cells.map((c, i) => ({
              color: row.color,
              occupancy: c.occupancy,
              title: c.title ? `${slotTitle(i)}: ${c.title} (${Math.round(c.occupancy * 100)}% of the slot)` : slotTitle(i),
              onClick: c.cardId ? () => openTimeline(day, c.cardId) : undefined,
            }))}
          />
        ))}
        <div className="ins-row">
          <span className="ins-row-label">Distractions</span>
          <div className="ins-track ins-marker-track">
            {g.markers.map((m, i) => (
              <span
                key={i}
                className="ins-marker"
                style={{ left: `${((m.startMinute - g.startMinute) / span) * 100}%`, width: `${Math.max(0.5, ((m.endMinute - m.startMinute) / span) * 100)}%` }}
                title={`${minuteLabel(m.startMinute)} – ${minuteLabel(m.endMinute)}: ${m.title}`}
              />
            ))}
          </div>
        </div>
        <Axis start={g.startMinute} end={g.endMinute} />
      </div>

      <div className="ins-panels">
        <Panel title={totalsTitle}>
          <Bars items={stats.totals.map((t) => ({ key: t.key, name: t.name, secs: t.secs, color: t.color }))} empty="No time recorded." />
        </Panel>

        <Panel title={isToday ? "Distractions so far" : "Distractions"}>
          <dl className="ins-pair">
            <Stat label="Total time captured" value={formatDuration(stats.capturedSecs)} />
            <Stat label="Total time distracted" value={`${formatDuration(stats.distractedSecs)} (${percent(stats.distractedSecs, stats.capturedSecs)})`} tone="bad" />
          </dl>
          {stats.distractions.length ? (
            <ul className="ins-list">
              {stats.distractions.map((d, i) => (
                <li key={i}>
                  <span className="ins-time">
                    {clock(d.startTs)} – {clock(d.endTs)}
                  </span>{" "}
                  <button className="link" onClick={() => openTimeline(day, d.cardId)}>{d.title || "Untitled"}</button>{" "}
                  <span className="ins-note">
                    {formatDuration(d.secs)}
                    {d.withinCard ? `, during “${d.withinCard}”` : ""}
                  </span>
                  {d.summary && <div className="ins-sub">{d.summary}</div>}
                </li>
              ))}
            </ul>
          ) : (
            <p className="ins-empty">No distractions noted.</p>
          )}
        </Panel>

        <Panel title="Focus">
          {stats.longestFocus ? (
            <>
              <dl className="ins-pair">
                <Stat label="Longest focus block" value={formatDuration(stats.longestFocus.secs)} />
              </dl>
              <p className="ins-hint">
                From {clock(stats.longestFocus.startTs)} to {clock(stats.longestFocus.endTs)}. All focus blocks:
              </p>
              <ul className="ins-list">
                {stats.focusBlocks.map((b, i) => (
                  <li key={i}>
                    <span className="ins-time">
                      {clock(b.startTs)} – {clock(b.endTs)}
                    </span>{" "}
                    {formatDuration(b.secs)}
                  </li>
                ))}
              </ul>
              <p className="ins-hint">Focus counts every category except Distraction and Idle. Breaks under 5 minutes do not end a block.</p>
            </>
          ) : (
            <p className="ins-empty">No focus time on this day.</p>
          )}
        </Panel>

        <Panel title="Programs in front">
          <Bars items={stats.programs.map((p) => ({ key: p.name, name: p.name, secs: p.secs }))} empty="No program data for this day." />
          {stats.programs.length > 0 && <p className="ins-hint">From the window in front at each screenshot.</p>}
        </Panel>
      </div>
    </>
  );
}
