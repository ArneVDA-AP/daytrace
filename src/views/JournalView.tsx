import { useEffect, useState } from "react";
import { formatDayTitle, formatDuration, shiftDay } from "../api";
import { DayExtras, errorText, teApi } from "../timeline-extras-api";
import { GoalItem, GoalsView, Plan, Progress, Reference, Review, journalApi } from "./journal-api";
import type { ViewProps } from "./types";
import "./journal.css";

/**
 * Goals: Dayflow's day goals (a focus target and a distraction budget with progress bars, the
 * morning review and setup, "Skip today"), a to-do list with the cards that mention each item,
 * and the timeline review summary. See features/journal.rs for the Dayflow sources.
 */
export function JournalView({ day, today, settings, openTimeline }: ViewProps) {
  const [view, setView] = useState<GoalsView | null>(null);
  const [extras, setExtras] = useState<DayExtras | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);

  const load = () => {
    journalApi
      .get(day)
      .then((v) => {
        setView(v);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
    teApi.dayExtras(day).then(setExtras).catch(() => setExtras(null));
  };

  useEffect(() => {
    setEditing(false);
    setView(null);
    load();
    const t = setInterval(load, 30000);
    return () => clearInterval(t);
    // Reload when the day changes.
  }, [day]);

  const run = async (f: () => Promise<GoalsView>) => {
    try {
      setView(await f());
      setError(null);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    }
  };

  if (!view) {
    return <div className="ins-view goals">{error ? <p className="ins-error">Couldn’t load goals: {error}</p> : <p className="ins-empty">Loading…</p>}</div>;
  }

  const past = day < today;
  const showSetup = editing || (view.prompt && !view.plan.saved);

  return (
    <div className="ins-view goals">
      {error && <p className="ins-error">{error}</p>}

      {view.prompt && view.review && !editing && <YesterdayReview review={view.review} day={shiftDay(day, -1)} />}

      <section className="ins-section" aria-labelledby="goals-targets">
        <div className="ins-head">
          <h2 id="goals-targets">{view.isToday ? "Today’s targets" : `Targets for ${formatDayTitle(day)}`}</h2>
          {!showSetup && !view.plan.isSkipped && (
            <div className="ins-actions">
              <button className="ghost" onClick={() => setEditing(true)}>
                Edit goals
              </button>
            </div>
          )}
        </div>
        {showSetup ? (
          <GoalSetup
            plan={view.plan}
            progress={view.progress}
            reference={view.reference}
            prompt={view.isToday ? "Where do you want to spend your time today?" : `Where did you want to spend your time on ${formatDayTitle(day)}?`}
            onConfirm={async (f, d) => {
              if (await run(() => journalApi.savePlan(day, f, d))) setEditing(false);
            }}
            onSkip={async () => {
              if (await run(() => journalApi.skip(day))) setEditing(false);
            }}
            onCancel={editing ? () => setEditing(false) : undefined}
          />
        ) : view.plan.isSkipped ? (
          <div className="goals-off">
            <p>{view.isToday ? "Set your goals for today to activate the progress bars." : "Goals were skipped for this day."}</p>
            <button className="primary" onClick={() => setEditing(true)}>
              Set goals
            </button>
          </div>
        ) : (
          <Targets
            plan={view.plan}
            progress={view.progress}
            past={past}
            statusText={
              !view.isToday
                ? null
                : settings.settings.recordingEnabled
                  ? "Tracking progress from your focus and distraction categories."
                  : "Daytrace is paused. Resume recording to continue tracking your progress."
            }
          />
        )}
      </section>

      <TodoList view={view} onRun={run} openCard={(id) => openTimeline(day, id)} />

      {extras && <ReviewSummary extras={extras} openCard={(id) => openTimeline(day, id)} />}
    </div>
  );
}

function pct(part: number, whole: number) {
  return whole > 0 ? Math.max(0, Math.min(100, (part / whole) * 100)) : 0;
}

function Targets({ plan, progress, past, statusText }: { plan: Plan; progress: Progress; past: boolean; statusText: string | null }) {
  const target = plan.focusTargetMinutes * 60;
  const limit = plan.distractionLimitMinutes * 60;
  const used = progress.distractedSecs;
  const over = limit > 0 && used > limit;
  const focusMet = progress.focusSecs >= target;
  return (
    <div className="goals-targets">
      {statusText && <p className="ins-hint">{statusText}</p>}
      {!plan.saved && (
        <p className="ins-note">
          {plan.carriedFrom ? `Same targets as ${formatDayTitle(plan.carriedFrom)}.` : "Default targets. Use Edit goals to set your own."}
        </p>
      )}
      <div className="goals-metric">
        <div className="goals-metric-head">
          <span>Focus</span>
          <span>
            <b>{formatDuration(progress.focusSecs)}</b> of {formatDuration(target)}
            {past && <span className={`goals-badge ${focusMet ? "met" : "missed"}`}>{focusMet ? "Nailed it" : "Missed"}</span>}
          </span>
        </div>
        <div className="goals-bar" role="meter" aria-label="Focus" aria-valuemin={0} aria-valuemax={target} aria-valuenow={Math.min(progress.focusSecs, target)} aria-valuetext={`${formatDuration(progress.focusSecs)} of ${formatDuration(target)}`}>
          {progress.focusCategories
            .filter((c) => c.secs > 0)
            .map((c) => (
              <span key={c.name} style={{ width: `${pct(c.secs, Math.max(target, progress.focusSecs))}%`, background: c.color }} title={`${c.name}: ${formatDuration(c.secs)}`} />
            ))}
        </div>
        <ul className="goals-legend">
          {progress.focusCategories.map((c) => (
            <li key={c.name}>
              <span className="tl-dot" style={{ background: c.color }} /> {c.name} <span className="muted">{formatDuration(c.secs)}</span>
            </li>
          ))}
        </ul>
      </div>
      <div className="goals-metric">
        <div className="goals-metric-head">
          <span>Distraction budget</span>
          <span>
            {over ? (
              <>
                <b>{formatDuration(used)}</b> used of {formatDuration(limit)}
              </>
            ) : (
              <>
                <b>{formatDuration(limit - used)}</b> left of {formatDuration(limit)}
              </>
            )}
            {past && <span className={`goals-badge ${!over ? "met" : "missed"}`}>{!over ? "Nailed it" : "Missed"}</span>}
          </span>
        </div>
        <div className={`goals-bar distraction${over ? " over" : ""}`} role="meter" aria-label="Distraction budget used" aria-valuemin={0} aria-valuemax={limit} aria-valuenow={Math.min(used, limit)} aria-valuetext={`${formatDuration(used)} of ${formatDuration(limit)} used`}>
          <span style={{ width: `${pct(used, limit)}%` }} />
        </div>
        <p className="ins-note">
          {progress.distractionCategories.length
            ? `Counts ${progress.distractionCategories.join(", ")}.`
            : "No category counts as distraction. Name a category Distraction in Settings to track it."}
        </p>
      </div>
    </div>
  );
}

function DurationPicker({ label, minutes, onChange }: { label: string; minutes: number; onChange: (m: number) => void }) {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return (
    <fieldset className="goals-picker">
      <legend>{label}</legend>
      <label>
        <select value={h} onChange={(e) => onChange(Math.min(720, Number(e.target.value) * 60 + m))} aria-label={`${label}, hours`}>
          {Array.from({ length: 13 }, (_, i) => (
            <option key={i} value={i}>
              {i}
            </option>
          ))}
        </select>{" "}
        h
      </label>
      <label>
        <select value={m - (m % 5)} onChange={(e) => onChange(Math.min(720, h * 60 + Number(e.target.value)))} aria-label={`${label}, minutes`}>
          {Array.from({ length: 12 }, (_, i) => i * 5).map((v) => (
            <option key={v} value={v}>
              {String(v).padStart(2, "0")}
            </option>
          ))}
        </select>{" "}
        min
      </label>
    </fieldset>
  );
}

function GoalSetup({
  plan,
  progress,
  reference,
  prompt,
  onConfirm,
  onSkip,
  onCancel,
}: {
  plan: Plan;
  progress: Progress;
  reference: Reference;
  prompt: string;
  onConfirm: (focusMinutes: number, distractionMinutes: number) => void;
  onSkip: () => void;
  onCancel?: () => void;
}) {
  const [focus, setFocus] = useState(plan.focusTargetMinutes);
  const [distraction, setDistraction] = useState(plan.distractionLimitMinutes);
  const focusNames = progress.focusCategories.map((c) => c.name);
  return (
    <div className="goals-setup">
      <p className="goals-prompt">{prompt}</p>
      <div className="goals-setup-grid">
        <div className="goals-panel">
          <h3>Focus goal</h3>
          <DurationPicker label="Focus goal" minutes={focus} onChange={setFocus} />
          <p className="ins-note">
            Yesterday’s focus: {formatDuration(reference.yesterdayFocusMinutes * 60)} · Last week’s focus average:{" "}
            {formatDuration(reference.lastWeekFocusMinutes * 60)}
          </p>
          <p className="ins-note">{focusNames.length ? `Counts ${focusNames.join(", ")}.` : "No focus categories."}</p>
        </div>
        <div className="goals-panel">
          <h3>Distraction limit</h3>
          <DurationPicker label="Distraction limit" minutes={distraction} onChange={setDistraction} />
          <p className="ins-note">
            Yesterday’s distractions: {formatDuration(reference.yesterdayDistractionMinutes * 60)} · Last week’s distraction average:{" "}
            {formatDuration(reference.lastWeekDistractionMinutes * 60)}
          </p>
          <p className="ins-note">
            {progress.distractionCategories.length ? `Counts ${progress.distractionCategories.join(", ")}.` : "No category counts as distraction."}
          </p>
        </div>
      </div>
      <p className="ins-hint">Which categories count as focus or distraction follows your categories in Settings, the same as in the Daily view.</p>
      <div className="ins-actions">
        <button className="primary" onClick={() => onConfirm(focus, distraction)}>
          Confirm
        </button>
        <button className="ghost" onClick={onSkip}>
          Skip today
        </button>
        {onCancel && (
          <button className="link" onClick={onCancel}>
            Cancel
          </button>
        )}
      </div>
    </div>
  );
}

function YesterdayReview({ review, day }: { review: Review; day: string }) {
  const p = review.progress;
  return (
    <section className="ins-section" aria-labelledby="goals-review">
      <div className="ins-head">
        <h2 id="goals-review">Yesterday’s review</h2>
        <span className="muted">{formatDayTitle(day)}</span>
      </div>
      <div className="goals-review">
        <div className="goals-panel">
          <div className="goals-metric-head">
            <span>
              Focus target: {formatDuration(review.plan.focusTargetMinutes * 60)}
              <br />
              Time spent: {formatDuration(p.focusSecs)}
            </span>
            <span className={`goals-badge ${review.focusMet ? "met" : "missed"}`}>{review.focusMet ? "Nailed it" : "Missed"}</span>
          </div>
          {p.focusCategories.some((c) => c.secs > 0) ? (
            <ul className="goals-legend">
              {p.focusCategories
                .filter((c) => c.secs > 0)
                .map((c) => (
                  <li key={c.name}>
                    <span className="tl-dot" style={{ background: c.color }} /> {c.name} <span className="muted">{formatDuration(c.secs)}</span>
                  </li>
                ))}
            </ul>
          ) : (
            <p className="ins-note">No focus categories tracked</p>
          )}
        </div>
        <div className="goals-panel">
          <div className="goals-metric-head">
            <span>
              Distraction limit: {formatDuration(review.plan.distractionLimitMinutes * 60)}
              <br />
              Time spent distracted: {formatDuration(p.distractedSecs)}
            </span>
            <span className={`goals-badge ${review.distractionMet ? "met" : "missed"}`}>{review.distractionMet ? "Nailed it" : "Missed"}</span>
          </div>
        </div>
      </div>
    </section>
  );
}

function TodoList({ view, onRun, openCard }: { view: GoalsView; onRun: (f: () => Promise<GoalsView>) => Promise<boolean>; openCard: (id: number) => void }) {
  const [text, setText] = useState("");
  const [editId, setEditId] = useState<number | null>(null);
  const [editText, setEditText] = useState("");
  const done = view.items.filter((i) => i.done).length;

  const add = async () => {
    if (!text.trim()) return;
    if (await onRun(() => journalApi.addItem(view.day, text))) setText("");
  };

  return (
    <section className="ins-section" aria-labelledby="goals-todo">
      <div className="ins-head">
        <h2 id="goals-todo">To do</h2>
        {view.items.length > 0 && (
          <span className="muted">
            {done} of {view.items.length} done
          </span>
        )}
      </div>
      <p className="ins-hint">Things you want to get done{view.isToday ? " today" : " this day"}. Daytrace lists the cards that mention each one.</p>
      {view.items.length === 0 && <p className="ins-empty">Nothing added yet.</p>}
      <ul className="goals-items">
        {view.items.map((item) => (
          <li key={item.id} className={item.done ? "done" : ""}>
            <div className="goals-item-row">
              <input
                type="checkbox"
                id={`goal-${item.id}`}
                checked={item.done}
                onChange={(e) => onRun(() => journalApi.updateItem(item.id, { done: e.target.checked }))}
              />
              {editId === item.id ? (
                <form
                  className="goals-item-edit"
                  onSubmit={async (e) => {
                    e.preventDefault();
                    if (await onRun(() => journalApi.updateItem(item.id, { text: editText }))) setEditId(null);
                  }}
                >
                  <input value={editText} onChange={(e) => setEditText(e.target.value)} aria-label="Goal" autoFocus />
                  <button className="ghost" type="submit">
                    Save
                  </button>
                  <button className="link" type="button" onClick={() => setEditId(null)}>
                    Cancel
                  </button>
                </form>
              ) : (
                <>
                  <label htmlFor={`goal-${item.id}`}>{item.text}</label>
                  <span className="goals-item-tools">
                    <button
                      className="link"
                      onClick={() => {
                        setEditId(item.id);
                        setEditText(item.text);
                      }}
                    >
                      Edit
                    </button>
                    <button className="link" onClick={() => onRun(() => journalApi.deleteItem(item.id, view.day))}>
                      Delete
                    </button>
                  </span>
                </>
              )}
            </div>
            <Evidence item={item} openCard={openCard} />
          </li>
        ))}
      </ul>
      <form
        className="goals-add"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <input value={text} onChange={(e) => setText(e.target.value)} placeholder="Add something to get done" aria-label="New to-do" maxLength={200} />
        <button className="ghost" type="submit" disabled={!text.trim()}>
          Add
        </button>
      </form>
    </section>
  );
}

function Evidence({ item, openCard }: { item: GoalItem; openCard: (id: number) => void }) {
  const ev = item.evidence;
  if (ev.cardCount === 0) return <p className="goals-evidence muted">No matching cards yet.</p>;
  return (
    <details className="goals-evidence">
      <summary>
        Matching activity: {formatDuration(ev.totalSecs)} in {ev.cardCount} {ev.cardCount === 1 ? "card" : "cards"}
      </summary>
      <ul className="plain">
        {ev.cards.map((c) => (
          <li key={c.cardId}>
            <button className="link" onClick={() => openCard(c.cardId)}>
              {c.start} – {c.end} {c.title}
            </button>{" "}
            <span className="muted">
              {formatDuration(c.secs)} · mentions {c.matched.join(", ")}
            </span>
          </li>
        ))}
      </ul>
    </details>
  );
}

function ReviewSummary({ extras, openCard }: { extras: DayExtras; openCard: (id: number) => void }) {
  const s = extras.reviewSummary;
  const total = s.focusedSecs + s.neutralSecs + s.distractedSecs;
  return (
    <section className="ins-section" aria-labelledby="goals-review-summary">
      <div className="ins-head">
        <h2 id="goals-review-summary">Your ratings</h2>
        {s.unreviewedCards > 0 && s.firstUnreviewed !== null && (
          <div className="ins-actions">
            <button className="ghost" onClick={() => openCard(s.firstUnreviewed!)}>
              Rate the next card
            </button>
          </div>
        )}
      </div>
      {total === 0 ? (
        <p className="ins-empty">No time rated yet. Rate a card Focused, Neutral or Distracted in its details on the timeline.</p>
      ) : (
        <>
          <div className="goals-bar rating" role="img" aria-label={`Focused ${formatDuration(s.focusedSecs)}, neutral ${formatDuration(s.neutralSecs)}, distracted ${formatDuration(s.distractedSecs)}`}>
            <span className="focused" style={{ width: `${pct(s.focusedSecs, total)}%` }} />
            <span className="neutral" style={{ width: `${pct(s.neutralSecs, total)}%` }} />
            <span className="distracted" style={{ width: `${pct(s.distractedSecs, total)}%` }} />
          </div>
          <p className="goals-rating-legend">
            <span>
              <span className="goals-swatch focused" /> Focused {formatDuration(s.focusedSecs)}
            </span>
            <span>
              <span className="goals-swatch neutral" /> Neutral {formatDuration(s.neutralSecs)}
            </span>
            <span>
              <span className="goals-swatch distracted" /> Distracted {formatDuration(s.distractedSecs)}
            </span>
          </p>
        </>
      )}
      <p className="ins-note">
        {s.unreviewedCards === 0
          ? total > 0
            ? "Every card of this day is rated."
            : ""
          : `${s.unreviewedCards} ${s.unreviewedCards === 1 ? "card is" : "cards are"} not rated yet.`}{" "}
        Ratings stay on this PC.
      </p>
    </section>
  );
}
