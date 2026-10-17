import { useEffect, useState } from "react";
import { Card, Category, clock, formatDuration } from "./api";
import { Timelapse } from "./Timelapse";
import { BatchProgress, DayExtras, Feedback, RetryItem, ReviewRating, errorText, retryLine, teApi } from "./timeline-extras-api";
import "./timeline-extras.css";

const RATINGS: { id: ReviewRating; label: string }[] = [
  { id: "focused", label: "Focused" },
  { id: "neutral", label: "Neutral" },
  { id: "distracted", label: "Distracted" },
];

type Mode = "view" | "edit" | "delete" | "reprocess";

export function CardDetail({
  card,
  color,
  categories,
  onRetry,
  onChanged,
}: {
  card: Card | null;
  color: string;
  categories: Category[];
  onRetry: (id: number) => void;
  /** Reload the day's cards after an edit, delete or reprocess. */
  onChanged: () => void;
}) {
  const [extras, setExtras] = useState<DayExtras | null>(null);
  const [mode, setMode] = useState<Mode>("view");
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState({ title: "", category: "", subcategory: "" });
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [noteSaved, setNoteSaved] = useState(false);
  const [reprocess, setReprocess] = useState<{ batchId: number; progress: BatchProgress | null } | null>(null);
  const [retryItems, setRetryItems] = useState<RetryItem[] | null>(null);

  // Card-level panels reset when another card is selected.
  useEffect(() => {
    setMode("view");
    setError(null);
    setNoteSaved(false);
  }, [card?.id]);

  const reload = () => {
    if (!card) return;
    teApi.dayExtras(card.day).then(setExtras).catch(() => setExtras(null));
  };
  useEffect(reload, [card]);

  const feedback: Feedback | undefined = extras?.feedback.find((f) => f.cardId === card?.id);
  useEffect(() => setNote(feedback?.note ?? ""), [card?.id, feedback?.note]);

  // Progress of "Reprocess this card" (the card is replaced when it finishes).
  useEffect(() => {
    if (!reprocess) return;
    let live = true;
    const t = setInterval(async () => {
      try {
        const p = await teApi.batchProgress([reprocess.batchId]);
        if (!live) return;
        if (p.waiting + p.running === 0) {
          setReprocess(null);
          if (p.failed) setError("Analyzing again failed. The card shows the error; you can try again.");
          onChanged();
        } else setReprocess({ batchId: reprocess.batchId, progress: p });
      } catch {
        /* keep polling */
      }
    }, 2000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [reprocess?.batchId]);

  // Status of a one-by-one retry of a failed group, polled while it runs.
  const retryRunning = retryItems?.some((i) => i.state === "queued" || i.state === "running") ?? false;
  useEffect(() => {
    if (!retryRunning) return;
    const t = setInterval(() => {
      teApi
        .retryStatus()
        .then((items) => {
          setRetryItems(items);
          if (!items.some((i) => i.state === "queued" || i.state === "running")) onChanged();
        })
        .catch(() => {});
    }, 2000);
    return () => clearInterval(t);
  }, [retryRunning]);

  if (!card) {
    return (
      <aside className="detail empty">
        <p>Select a card to see what happened and the screenshots it was written from.</p>
      </aside>
    );
  }
  const failed = card.category === "System";
  const lines = card.detailedSummary.split("\n").map((l) => l.trim()).filter(Boolean);
  const edited = extras?.edited.includes(card.id) ?? false;
  const review = extras?.reviews.find((r) => r.cardId === card.id);
  const group = extras?.failedGroups.find((g) => g.cardIds.includes(card.id));
  const multi = group && group.cardIds.length > 1 ? group : null;
  const catNames = categories.map((c) => c.name);
  if (!failed && !catNames.some((n) => n.toLowerCase() === card.category.toLowerCase())) catNames.push(card.category);

  async function act<T>(f: () => Promise<T>, after?: (v: T) => void) {
    setBusy(true);
    setError(null);
    try {
      const v = await f();
      after?.(v);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  const startEdit = () => {
    const category = catNames.find((n) => n.toLowerCase() === card.category.toLowerCase()) ?? card.category;
    setDraft({ title: card.title, category, subcategory: card.subcategory });
    setError(null);
    setMode("edit");
  };

  const saveEdit = () =>
    act(
      () => teApi.updateCard(card.id, draft),
      () => {
        setMode("view");
        onChanged();
        reload();
      },
    );

  const setRating = (r: ReviewRating | null) => act(() => teApi.rateCard(card.id, r), reload);
  const setThumb = (d: "up" | "down" | null) =>
    act(
      () => teApi.setFeedback(card.id, d, d ? note : ""),
      () => {
        setNoteSaved(false);
        reload();
      },
    );
  const saveNote = () =>
    feedback &&
    act(
      () => teApi.setFeedback(card.id, feedback.direction, note),
      () => {
        setNoteSaved(true);
        reload();
      },
    );

  return (
    <aside className="detail" style={{ ["--cat" as string]: color }}>
      <div className="detail-head">
        <div className="detail-when">
          {multi
            ? `${clock(multi.startTs)} – ${clock(multi.endTs)}, ${formatDuration(multi.endTs - multi.startTs)}`
            : `${card.start} – ${card.end}, ${formatDuration(card.endTs - card.startTs)}`}
        </div>
        {mode === "edit" ? (
          <form
            className="cd-edit"
            onSubmit={(e) => {
              e.preventDefault();
              saveEdit();
            }}
          >
            <label>
              <span>Title</span>
              <input value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} autoFocus />
            </label>
            <label>
              <span>Category</span>
              <select value={draft.category} onChange={(e) => setDraft({ ...draft, category: e.target.value })}>
                {catNames.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>Subcategory</span>
              <input value={draft.subcategory} placeholder="Optional" onChange={(e) => setDraft({ ...draft, subcategory: e.target.value })} />
            </label>
            <p className="hint">Edited cards are kept as you leave them: later analysis does not rewrite them.</p>
            <div className="row">
              <button className="primary" type="submit" disabled={busy}>
                Save
              </button>
              <button className="ghost" type="button" onClick={() => setMode("view")}>
                Cancel
              </button>
            </div>
          </form>
        ) : (
          <>
            <h2>{multi ? `Processing failed · ${multi.cardIds.length} intervals` : card.title}</h2>
            <div className="detail-cat">
              <span className="swatch" />
              {failed ? "Not analyzed" : card.category}
              {card.subcategory && !failed ? `, ${card.subcategory}` : ""}
              {edited && <span className="tl-tag">Edited</span>}
            </div>
          </>
        )}
        {mode === "view" && (
          <div className="cd-actions">
            {!failed && (
              <button className="link" onClick={startEdit}>
                Edit
              </button>
            )}
            {!failed && card.batchId !== null && (
              <button className="link" onClick={() => setMode("reprocess")} disabled={!!reprocess}>
                Reprocess
              </button>
            )}
            <button className="link" onClick={() => setMode("delete")}>
              Delete
            </button>
          </div>
        )}
      </div>

      {mode === "delete" && (
        <div className="cd-confirm" role="alertdialog" aria-label="Delete card">
          <p>
            {multi
              ? `Delete these ${multi.cardIds.length} failed cards? Daytrace will not try these batches again on its own.`
              : failed
                ? "Delete this card? Daytrace will not try this batch again on its own."
                : "Delete this card? Its screen descriptions are deleted too, and this time shows as empty on the timeline."}
          </p>
          <div className="row">
            <button
              className="primary danger"
              disabled={busy}
              onClick={() =>
                act(async () => {
                  for (const id of multi ? multi.cardIds : [card.id]) await teApi.deleteCard(id);
                }, onChanged)
              }
            >
              {multi ? "Delete cards" : "Delete card"}
            </button>
            <button className="ghost" onClick={() => setMode("view")}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {mode === "reprocess" && (
        <div className="cd-confirm" role="alertdialog" aria-label="Reprocess card">
          <p>
            Analyze this card’s recording again? Daytrace sends its screenshots to your model again (one or more requests) and
            replaces this card with the new result.{edited && " Your edits to this card are replaced too."}
          </p>
          <div className="row">
            <button
              className="primary"
              disabled={busy}
              onClick={() =>
                act(
                  () => teApi.reprocessCard(card.id),
                  (batchId) => {
                    setMode("view");
                    setReprocess({ batchId, progress: null });
                  },
                )
              }
            >
              Reprocess card
            </button>
            <button className="ghost" onClick={() => setMode("view")}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {reprocess && (
        <p className="cd-progress" role="status">
          {reprocess.progress?.running ? "Analyzing again" : "Waiting to analyze again"}
          {reprocess.progress?.step ? `: ${reprocess.progress.step}…` : "…"}
        </p>
      )}
      {error && (
        <p className="bad" role="alert">
          {error}
        </p>
      )}

      <p className="detail-summary">{card.summary}</p>
      {failed && (
        <div className="retry">
          {group && !group.retryable ? (
            <p className="muted">The screenshots of this batch were deleted, so it can’t be analyzed again.</p>
          ) : multi ? (
            <>
              <button
                className="primary"
                disabled={busy || retryRunning}
                onClick={() => act(() => teApi.retryBatches(multi.batchIds), async () => setRetryItems(await teApi.retryStatus()))}
              >
                Try again
              </button>
              <p className="muted">
                {(retryItems && retryLine(retryItems, multi.batchIds)) ??
                  `Daytrace analyzes the ${multi.batchIds.length} batches one at a time and stops if one fails again.`}
              </p>
            </>
          ) : (
            <button className="primary" onClick={() => onRetry(card.id)}>
              Try again
            </button>
          )}
          {card.detailedSummary && (
            <details>
              <summary>Error details</summary>
              <pre>{card.detailedSummary}</pre>
            </details>
          )}
        </div>
      )}

      {!failed && (
        <section className="sub cd-rate" aria-label="Your rating">
          <div className="cd-rate-row">
            <span className="muted">How was this time?</span>
            <div className="cd-seg" role="group" aria-label="Rate this time">
              {RATINGS.map((r) => (
                <button
                  key={r.id}
                  className={`cd-seg-btn ${r.id}${review?.rating === r.id ? " on" : ""}`}
                  aria-pressed={review?.rating === r.id}
                  disabled={busy}
                  onClick={() => setRating(review?.rating === r.id && review.coverage >= 0.99 ? null : r.id)}
                >
                  {r.label}
                </button>
              ))}
            </div>
            {review && review.coverage < 0.8 && <span className="muted">Partly rated</span>}
          </div>
          {!card.isIdleShortcut && (
            <div className="cd-rate-row">
              <span className="muted">Is this summary accurate?</span>
              <div className="cd-seg" role="group" aria-label="Rate this summary">
                {(["up", "down"] as const).map((d) => (
                  <button
                    key={d}
                    className={`cd-seg-btn${feedback?.direction === d ? " on" : ""}`}
                    aria-pressed={feedback?.direction === d}
                    disabled={busy}
                    onClick={() => setThumb(feedback?.direction === d ? null : d)}
                  >
                    {d === "up" ? "Thumbs up" : "Thumbs down"}
                  </button>
                ))}
              </div>
            </div>
          )}
          {feedback && (
            <div className="cd-note">
              <label htmlFor={`note-${card.id}`} className="muted">
                Note (stays on this PC)
              </label>
              <textarea
                id={`note-${card.id}`}
                rows={2}
                value={note}
                placeholder={feedback.direction === "down" ? "What is wrong with this summary?" : "What did it get right?"}
                onChange={(e) => {
                  setNote(e.target.value);
                  setNoteSaved(false);
                }}
              />
              <div className="row">
                <button className="ghost" disabled={busy || note.trim() === feedback.note} onClick={saveNote}>
                  Save note
                </button>
                {noteSaved && <span className="ok">Saved</span>}
              </div>
            </div>
          )}
        </section>
      )}

      <Timelapse cardId={card.id} />

      {card.distractions.length > 0 && (
        <section className="sub">
          <h3>Interruptions</h3>
          <ul className="plain">
            {card.distractions.map((d, i) => (
              <li key={i}>
                <span className="t">{d.startTime}</span> {d.title}
                {d.summary && <span className="muted"> {d.summary}</span>}
              </li>
            ))}
          </ul>
        </section>
      )}

      {!failed && lines.length > 0 && (
        <section className="sub">
          <h3>Step by step</h3>
          <ul className="plain log">
            {lines.map((l, i) => (
              <li key={i}>{l.replace(/^[-•]\s*/, "")}</li>
            ))}
          </ul>
        </section>
      )}
    </aside>
  );
}
