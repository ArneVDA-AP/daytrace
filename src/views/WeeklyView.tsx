import { useEffect, useState } from "react";
import { clock, formatDuration, shiftDay } from "../api";
import type { ViewProps } from "./types";
import { WeekStats, insightsApi, percent, shortDate } from "./insights-api";
import { Axis, Bars, CellRow, Panel, Stat, Swatch, minuteLabel } from "./insights-parts";
import "./insights.css";

/** Dayflow's weekly data gate: patterns need about 15 hours of a week. */
const ENOUGH_SECS = 15 * 3600;

/**
 * Weekly review (Dayflow's Weekly view, no model): distribution, time per day, the weekly
 * workflow grid, the focus heat map, distractions by hour, context shifts, longest focus blocks,
 * and the apps and programs used. Weeks run Monday 4 AM to Monday 4 AM.
 */
export function WeeklyView({ day, setDay, today, openTimeline }: ViewProps) {
  const [week, setWeek] = useState<WeekStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setWeek(null);
    const load = () =>
      insightsApi
        .week(day)
        .then((w) => {
          if (live) {
            setWeek(w);
            setError(null);
          }
        })
        .catch((e) => live && setError(String(e)));
    load();
    const t = setInterval(load, 120000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [day]);

  const isCurrent = !!week && today >= week.weekStart && today <= week.weekEnd;
  return (
    <div className="ins-view">
      <div className="ins-head">
        <div className="ins-weeknav">
          <button className="icon" aria-label="Previous week" onClick={() => setDay(shiftDay(day, -7))}>‹</button>
          <h2>{week ? `Week of ${shortDate(week.weekStart)} – ${shortDate(week.weekEnd)}` : "Week"}</h2>
          <button className="icon" aria-label="Next week" disabled={isCurrent || !week} onClick={() => setDay(shiftDay(day, 7) > today ? today : shiftDay(day, 7))}>›</button>
          {!isCurrent && week && <button className="link" onClick={() => setDay(today)}>This week</button>}
        </div>
      </div>
      {error && <p className="ins-error">Couldn’t load this week: {error}</p>}
      {!week ? (
        !error && <p className="ins-empty">Loading…</p>
      ) : week.capturedSecs === 0 ? (
        <p className="ins-empty">No cards for this week yet.</p>
      ) : (
        <WeekBody week={week} openTimeline={openTimeline} />
      )}
    </div>
  );
}

function WeekBody({ week, openTimeline }: { week: WeekStats; openTimeline: ViewProps["openTimeline"] }) {
  const active = week.capturedSecs - week.idleSecs;
  return (
    <>
      <dl className="ins-stats">
        <Stat label="Time captured" value={formatDuration(week.capturedSecs)} />
        <Stat label="Focused" value={formatDuration(week.focusSecs)} />
        <Stat label="Distracted" value={`${formatDuration(week.distractedSecs)} (${percent(week.distractedSecs, active)})`} tone="bad" />
        <Stat label="Idle" value={formatDuration(week.idleSecs)} />
      </dl>
      {active < ENOUGH_SECS && (
        <p className="ins-hint">This week has less than 15 hours of activity, so the patterns below may not say much yet.</p>
      )}

      <div className="ins-panels">
        <Panel title="Weekly distribution">
          <Donut items={week.distribution} />
        </Panel>
        <Panel title="Time per day">
          <DayBars week={week} openTimeline={openTimeline} />
        </Panel>
      </div>

      <Panel title="Your workflow this week" wide>
        <div className="ins-grid">
          {week.workflow.rows.map((r) => (
            <CellRow
              key={r.day}
              label={r.label}
              cells={r.cells.map((c, i) => {
                const from = week.workflow.startMinute + i * week.workflow.slotMinutes;
                const when = `${r.label} ${minuteLabel(from)}`;
                return {
                  color: c?.color ?? null,
                  occupancy: c?.occupancy ?? 0,
                  title: c ? `${when}: mostly ${c.name}, ${formatDuration(c.secs)}` : when,
                  onClick: c ? () => openTimeline(r.day) : undefined,
                };
              })}
            />
          ))}
          <Axis start={week.workflow.startMinute} end={week.workflow.endMinute} />
        </div>
        <ul className="ins-legend">
          {week.workflow.totals.map((t) => (
            <li key={t.key}>
              <Swatch color={t.color} />
              {t.name} {formatDuration(t.secs)}
            </li>
          ))}
        </ul>
      </Panel>

      <Panel title="Focus and distraction heat map" wide>
        <div className="ins-grid ins-heat">
          {week.heatmap.rows.map((r) => (
            <CellRow
              key={r.day}
              label={r.label}
              cells={r.values.map((v, i) => {
                const when = `${r.label} ${minuteLabel(week.heatmap.startMinute + i * week.heatmap.bucketMinutes)}`;
                return {
                  color: v < 0 ? "var(--accent)" : v > 0 ? "var(--danger)" : null,
                  occupancy: Math.abs(v),
                  title: v < 0 ? `${when}: focused work` : v > 0 ? `${when}: distracted or switching` : when,
                };
              })}
            />
          ))}
          <Axis start={week.heatmap.startMinute} end={week.heatmap.endMinute} />
        </div>
        <ul className="ins-legend">
          <li>
            <Swatch color="var(--accent)" />
            Focused work
          </li>
          <li>
            <Swatch color="var(--danger)" />
            Distracted
          </li>
        </ul>
      </Panel>

      <div className="ins-panels">
        <Panel title="Distractions by hour">
          <HourTable week={week} />
        </Panel>
        <Panel title="Context shifts and distractions">
          <table className="ins-table">
            <thead>
              <tr>
                <th scope="col">Day</th>
                <th scope="col">Context shifts</th>
                <th scope="col">Distractions</th>
              </tr>
            </thead>
            <tbody>
              {week.days.map((d) => (
                <tr key={d.day}>
                  <th scope="row">{d.label}</th>
                  <td>{d.contextShifts}</td>
                  <td>{d.distractionCount}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="ins-hint">{week.contextInsight}</p>
        </Panel>
        <Panel title="Longest focus blocks">
          {week.focusBlocks.length ? (
            <ol className="ins-list">
              {week.focusBlocks.map((b, i) => (
                <li key={i}>
                  <button className="link" onClick={() => openTimeline(b.day)}>{shortDate(b.day)}</button>{" "}
                  <span className="ins-time">
                    {clock(b.startTs)} – {clock(b.endTs)}
                  </span>{" "}
                  {formatDuration(b.secs)}
                </li>
              ))}
            </ol>
          ) : (
            <p className="ins-empty">No focus time this week.</p>
          )}
        </Panel>
      </div>

      <div className="ins-panels">
        <Panel title="Most used apps and sites">
          <Bars
            items={week.topApps.map((a) => ({ key: a.name, name: a.name, secs: a.secs, note: change(a.secs, a.previousSecs) }))}
            empty="No cards with an app or site this week."
          />
        </Panel>
        <Panel title="Most used per category">
          {week.appsByCategory.length ? (
            week.appsByCategory.map((c) => (
              <div key={c.key} className="ins-group">
                <h4>
                  <Swatch color={c.color} />
                  {c.name}
                </h4>
                <Bars swatches={false} items={c.apps.map((a) => ({ key: a.name, name: a.name, secs: a.secs, color: c.color, note: change(a.secs, a.previousSecs) }))} empty="" />
              </div>
            ))
          ) : (
            <p className="ins-empty">No cards with an app or site this week.</p>
          )}
        </Panel>
        <Panel title="Programs in front">
          <Bars items={week.programs.map((p) => ({ key: p.name, name: p.name, secs: p.secs }))} empty="No program data for this week." />
          {week.programs.length > 0 && <p className="ins-hint">From the window in front at each screenshot.</p>}
        </Panel>
      </div>
    </>
  );
}

/** "+20 min on last week" when the app was used the week before (Dayflow's treemap change). */
function change(secs: number, previous: number): string | undefined {
  if (previous <= 0) return undefined;
  const delta = Math.round((secs - previous) / 60) * 60;
  if (delta === 0) return "same as last week";
  return `${delta > 0 ? "+" : "−"}${formatDuration(Math.abs(delta))} on last week`;
}

function Donut({ items }: { items: WeekStats["distribution"] }) {
  const total = items.reduce((a, i) => a + i.secs, 0);
  if (!total) return <p className="ins-empty">No focus or distraction time this week.</p>;
  const r = 40;
  const c = 2 * Math.PI * r;
  let offset = 0;
  return (
    <div className="ins-donut">
      <svg viewBox="0 0 100 100" role="img" aria-label="Share of time per category">
        <circle cx="50" cy="50" r={r} fill="none" stroke="var(--soft)" strokeWidth="14" />
        {items.map((i) => {
          const len = (i.secs / total) * c;
          const el = (
            <circle
              key={i.key}
              cx="50"
              cy="50"
              r={r}
              fill="none"
              stroke={i.color}
              strokeWidth="14"
              strokeDasharray={`${len} ${c - len}`}
              strokeDashoffset={-offset}
              transform="rotate(-90 50 50)"
            >
              <title>{`${i.name}: ${formatDuration(i.secs)}`}</title>
            </circle>
          );
          offset += len;
          return el;
        })}
        <text x="50" y="48" textAnchor="middle" className="ins-donut-total">{formatDuration(total)}</text>
        <text x="50" y="60" textAnchor="middle" className="ins-donut-sub">active</text>
      </svg>
      <ul className="ins-legend ins-legend-col">
        {items.map((i) => (
          <li key={i.key}>
            <Swatch color={i.color} />
            {i.name} <span className="ins-note">{formatDuration(i.secs)} · {percent(i.secs, total)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function DayBars({ week, openTimeline }: { week: WeekStats; openTimeline: ViewProps["openTimeline"] }) {
  const max = Math.max(...week.days.map((d) => d.capturedSecs), 1);
  return (
    <ul className="ins-daybars">
      {week.days.map((d) => (
        <li key={d.day}>
          <button className="link ins-bar-label" onClick={() => openTimeline(d.day)}>{d.label}</button>
          <span className="ins-stack" aria-hidden="true">
            {d.totals.map((t) => (
              <span key={t.key} style={{ width: `${(t.secs / max) * 100}%`, background: t.color }} title={`${t.name}: ${formatDuration(t.secs)}`} />
            ))}
          </span>
          <span className="ins-bar-value">
            {d.capturedSecs ? (
              <>
                {formatDuration(d.focusSecs)} focused
                {d.distractedSecs > 0 && <span className="ins-bad">, {formatDuration(d.distractedSecs)} distracted</span>}
              </>
            ) : (
              <span className="ins-note">Nothing recorded</span>
            )}
          </span>
        </li>
      ))}
    </ul>
  );
}

function HourTable({ week }: { week: WeekStats }) {
  const max = Math.max(...week.distractionHours.flatMap((r) => r.secs), 0);
  if (!max) return <p className="ins-empty">No distractions noted this week.</p>;
  // Only the hours that saw any activity in the week, from the workflow window.
  const first = Math.floor(week.workflow.startMinute / 60);
  const last = Math.min(23, Math.ceil(week.workflow.endMinute / 60) - 1);
  const hours = Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i);
  return (
    <div className="ins-grid">
      {week.distractionHours.map((r) => (
        <CellRow
          key={r.day}
          label={r.label}
          cells={hours.map((h) => ({
            color: r.secs[h] > 0 ? "var(--danger)" : null,
            occupancy: r.secs[h] / max,
            title: `${r.label} ${minuteLabel(h * 60)}: ${r.secs[h] ? formatDuration(r.secs[h]) + " distracted" : "no distractions"}`,
          }))}
        />
      ))}
      <Axis start={first * 60} end={(last + 1) * 60} step={hours.length > 8 ? 3 : 1} />
    </div>
  );
}
