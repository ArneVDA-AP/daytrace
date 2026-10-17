import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Card, Category, Status, formatDuration } from "./api";
import { BatchProgress, DayExtras, DayPlan, FailedGroup, RetryItem, errorText, retryLine, teApi } from "./timeline-extras-api";
import { GoalStrip } from "./GoalStrip";
import "./timeline-extras.css";

const PX_PER_MIN = 1.5;
const MIN_HEIGHT = 64;
const RATING_LABEL: Record<string, string> = { focused: "Focused", neutral: "Neutral", distracted: "Distracted" };

// Kept at module level so they survive switching tabs (the timeline unmounts).
let mutedKeys = new Set<string>();
let activeReprocess: { day: string; batchIds: number[] } | null = null;

const keyOf = (name: string) => name.trim().toLowerCase();

type Reprocess =
  | { stage: "loading" }
  | { stage: "confirm"; plan: DayPlan }
  | { stage: "running"; batchIds: number[]; progress: BatchProgress | null }
  | { stage: "done"; failed: number }
  | { stage: "error"; message: string };

export function Timeline({
  day,
  cards,
  categories,
  colorFor,
  selected,
  onSelect,
  isToday,
  status,
  recording,
  onChanged,
}: {
  day: string;
  cards: Card[];
  categories: Category[];
  colorFor: (c: string) => string;
  selected: number | null;
  onSelect: (id: number) => void;
  isToday: boolean;
  status: Status;
  recording: boolean;
  /** Reload the day's cards after a change made here. */
  onChanged: () => void;
}) {
  const [extras, setExtras] = useState<DayExtras | null>(null);
  const [muted, setMuted] = useState<Set<string>>(mutedKeys);
  const [retryItems, setRetryItems] = useState<RetryItem[]>([]);
  const [retryError, setRetryError] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const [rp, setRp] = useState<Reprocess | null>(
    activeReprocess && activeReprocess.day === day ? { stage: "running", batchIds: activeReprocess.batchIds, progress: null } : null,
  );
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;

  useEffect(() => {
    let live = true;
    teApi
      .dayExtras(day)
      .then((x) => live && setExtras(x))
      .catch(() => live && setExtras(null));
    return () => {
      live = false;
    };
  }, [day, cards]);

  // A different day: drop panels that belong to the previous one.
  useEffect(() => {
    setRp(activeReprocess && activeReprocess.day === day ? { stage: "running", batchIds: activeReprocess.batchIds, progress: null } : null);
    setCopyState("idle");
    setRetryError(null);
  }, [day]);

  // Group retry status (Dayflow RetryCoordinator), polled while a run is going.
  const retryActive = retryItems.some((i) => i.state === "queued" || i.state === "running");
  const retryWasActive = useRef(false);
  useEffect(() => {
    let live = true;
    const tick = () =>
      teApi.retryStatus().then((items) => {
        if (!live) return;
        const nowActive = items.some((i) => i.state === "queued" || i.state === "running");
        if (retryWasActive.current && !nowActive) onChangedRef.current();
        retryWasActive.current = nowActive;
        setRetryItems(items);
      });
    tick();
    if (!retryActive) return () => void (live = false);
    const t = setInterval(tick, 2000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [retryActive]);

  // Day reprocess progress.
  const runningIds = rp?.stage === "running" ? rp.batchIds : null;
  useEffect(() => {
    if (!runningIds) return;
    let live = true;
    let polls = 0;
    const tick = async () => {
      try {
        const p = await teApi.batchProgress(runningIds);
        if (!live) return;
        polls += 1;
        if (p.waiting + p.running === 0) {
          activeReprocess = null;
          setRp({ stage: "done", failed: p.failed });
          onChangedRef.current();
        } else {
          setRp({ stage: "running", batchIds: runningIds, progress: p });
          if (polls % 5 === 0) onChangedRef.current(); // show new cards as they arrive
        }
      } catch (e) {
        if (live) setRp({ stage: "error", message: errorText(e) });
      }
    };
    tick();
    const t = setInterval(tick, 2000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [runningIds?.join(",")]);

  const toggleMute = (key: string) => {
    const next = new Set(muted);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    mutedKeys = next;
    setMuted(next);
  };

  // Settings order with the Idle category last (Dayflow TabFilterBar), then categories that only
  // appear on cards.
  const chips = useMemo(() => {
    const out: { key: string; name: string }[] = [];
    const seen = new Set<string>();
    const add = (name: string) => {
      const k = keyOf(name);
      if (!k || k === "system" || seen.has(k)) return;
      seen.add(k);
      out.push({ key: k, name: name.trim() });
    };
    categories.filter((c) => !c.isIdle).forEach((c) => add(c.name));
    categories.filter((c) => c.isIdle).forEach((c) => add(c.name));
    cards.forEach((c) => add(c.category));
    return out;
  }, [categories, cards]);

  async function copyTimeline() {
    try {
      const text = await teApi.timelineText(day);
      await navigator.clipboard.writeText(text);
      setCopyState("copied");
      setTimeout(() => setCopyState("idle"), 2000);
    } catch {
      setCopyState("error");
    }
  }

  async function askReprocess() {
    setRp({ stage: "loading" });
    try {
      setRp({ stage: "confirm", plan: await teApi.reprocessDayPlan(day) });
    } catch (e) {
      setRp({ stage: "error", message: errorText(e) });
    }
  }

  async function startReprocess() {
    try {
      const plan = await teApi.reprocessDay(day);
      activeReprocess = { day, batchIds: plan.batchIds };
      setRp({ stage: "running", batchIds: plan.batchIds, progress: null });
      onChanged();
    } catch (e) {
      setRp({ stage: "error", message: errorText(e) });
    }
  }

  async function retryGroup(g: FailedGroup) {
    setRetryError(null);
    try {
      await teApi.retryBatches(g.batchIds);
      setRetryItems(await teApi.retryStatus());
    } catch (e) {
      setRetryError(errorText(e));
    }
  }

  const reprocessPanel = rp && (
    <div className="tl-panel" role="status" aria-live="polite">
      {rp.stage === "loading" && <p>Checking this day…</p>}
      {rp.stage === "confirm" &&
        (rp.plan.blocked ? (
          <>
            <p>{rp.plan.blocked}</p>
            <button className="ghost" onClick={() => setRp(null)}>Close</button>
          </>
        ) : (
          <>
            <p>
              <b>Reprocess this day?</b> Daytrace clears this day’s cards and screen descriptions, then analyzes the saved
              screenshots again: {rp.plan.batchIds.length} {rp.plan.batchIds.length === 1 ? "batch" : "batches"}, one after another.
              {rp.plan.keptEditedCards > 0 &&
                ` ${rp.plan.keptEditedCards === 1 ? "The card you edited is" : `The ${rp.plan.keptEditedCards} cards you edited are`} kept.`}
            </p>
            <p className="tl-warn">Heads up: each batch takes one or more model requests, so this can use a large number of API calls.</p>
            <div className="row">
              <button className="primary" onClick={startReprocess}>Reprocess day</button>
              <button className="ghost" onClick={() => setRp(null)}>Cancel</button>
            </div>
          </>
        ))}
      {rp.stage === "running" && (
        <p>
          Reprocessing this day:{" "}
          {rp.progress
            ? `${rp.progress.waiting + rp.progress.running} of ${rp.batchIds.length} batches left.`
            : `${rp.batchIds.length} batches queued.`}
          {rp.progress?.step && <span className="muted"> {rp.progress.step}.</span>}
        </p>
      )}
      {rp.stage === "done" && (
        <>
          <p>
            Reprocessing finished.
            {rp.failed > 0 && ` ${rp.failed} ${rp.failed === 1 ? "batch" : "batches"} failed; use Try again on those cards.`}
          </p>
          <button className="ghost" onClick={() => setRp(null)}>Close</button>
        </>
      )}
      {rp.stage === "error" && (
        <>
          <p className="bad">{rp.message}</p>
          <button className="ghost" onClick={() => setRp(null)}>Close</button>
        </>
      )}
    </div>
  );

  if (cards.length === 0) {
    return (
      <section className="timeline">
        {reprocessPanel}
        <div className="tl-empty">
          {isToday ? (
            recording ? (
              <p>
                Nothing on today’s timeline yet. Daytrace writes the first card about 15 minutes after recording starts
                {status.pendingBatches > 0 ? ", and is working on it now." : "."}
              </p>
            ) : (
              <p>Recording is paused, so today’s timeline is empty. Resume recording to start a new day.</p>
            )
          ) : (
            <p>No timeline for this day.</p>
          )}
        </div>
      </section>
    );
  }

  const groupOf = new Map<number, FailedGroup>();
  extras?.failedGroups.forEach((g) => g.cardIds.forEach((id) => groupOf.set(id, g)));
  const edited = new Set(extras?.edited ?? []);
  const reviewOf = new Map((extras?.reviews ?? []).map((r) => [r.cardId, r]));
  const failedColor = colorFor("System");

  const rows: React.ReactElement[] = [];
  const shownGroups = new Set<FailedGroup>();
  let lastEnd: number | null = null;
  cards.forEach((c) => {
    const g = groupOf.get(c.id);
    const grouped = g && g.cardIds.length > 1;
    if (grouped && shownGroups.has(g)) return;
    const start = grouped ? g.startTs : c.startTs;
    const end = grouped ? g.endTs : c.endTs;
    if (lastEnd !== null && start - lastEnd > 5 * 60) {
      rows.push(
        <div key={`gap-${c.id}`} className="gap">
          Not recording, {formatDuration(start - lastEnd)}
        </div>,
      );
    }
    lastEnd = Math.max(lastEnd ?? end, end);
    const mins = (end - start) / 60;
    const minHeight = Math.max(MIN_HEIGHT, mins * PX_PER_MIN);

    if (grouped) {
      shownGroups.add(g);
      const line = retryLine(retryItems, g.batchIds);
      const busy = retryItems.some((i) => g.batchIds.includes(i.batchId) && (i.state === "queued" || i.state === "running"));
      const sel = selected !== null && g.cardIds.includes(selected);
      rows.push(
        <div key={`group-${c.id}`} className={`card failed tl-group${sel ? " sel" : ""}`} style={{ minHeight, ["--cat" as string]: failedColor }}>
          <button className="tl-group-open" onClick={() => onSelect(g.cardIds[0])}>
            <div className="card-time">
              <span>{c.start}</span>
              <span className="card-dur">{formatDuration(end - start)}</span>
            </div>
            <div className="card-body">
              <div className="card-title">Processing failed · {g.cardIds.length} intervals</div>
              <div className="card-meta">Not analyzed</div>
            </div>
          </button>
          <div className="tl-group-actions">
            {g.retryable ? (
              <button className="ghost" disabled={busy} onClick={() => retryGroup(g)}>
                Try again
              </button>
            ) : (
              <span className="muted">The screenshots were deleted, so this can’t be analyzed again.</span>
            )}
            {line && <span className="muted">{line}</span>}
          </div>
        </div>,
      );
      return;
    }

    const failed = c.category === "System";
    const sites = [c.appSites?.primary, c.appSites?.secondary].filter(Boolean).join(", ");
    const isMuted = !failed && muted.has(keyOf(c.category));
    const review = reviewOf.get(c.id);
    rows.push(
      <button
        key={c.id}
        className={`card${selected === c.id ? " sel" : ""}${failed ? " failed" : ""}${c.isIdleShortcut ? " idle" : ""}${isMuted ? " muted" : ""}`}
        style={{ minHeight, ["--cat" as string]: colorFor(c.category) }}
        onClick={() => onSelect(c.id)}
      >
        <div className="card-time">
          <span>{c.start}</span>
          <span className="card-dur">{formatDuration(c.endTs - c.startTs)}</span>
        </div>
        <div className="card-body">
          <div className="card-title">{c.title}</div>
          <div className="card-meta">
            {failed ? "Not analyzed" : c.category}
            {sites && <span className="sites">{sites}</span>}
            {edited.has(c.id) && <span className="tl-tag">Edited</span>}
            {review && <span className="tl-tag">{RATING_LABEL[review.rating]}</span>}
          </div>
        </div>
      </button>,
    );
  });

  return (
    <section className="timeline">
      <div className="tl-tools">
        <div className="tl-chips" role="group" aria-label="Categories shown on the timeline">
          {chips.map((ch) => (
            <button
              key={ch.key}
              className={`tl-chip${muted.has(ch.key) ? " off" : ""}`}
              aria-pressed={!muted.has(ch.key)}
              title={muted.has(ch.key) ? `Show ${ch.name}` : `Dim ${ch.name}`}
              onClick={() => toggleMute(ch.key)}
            >
              <span className="tl-dot" style={{ background: colorFor(ch.name) }} />
              {ch.name}
            </button>
          ))}
        </div>
        <div className="tl-actions">
          <button className="ghost" onClick={copyTimeline}>
            {copyState === "copied" ? "Copied" : "Copy timeline"}
          </button>
          <button className="ghost" onClick={askReprocess} disabled={rp?.stage === "running" || rp?.stage === "loading"}>
            Reprocess day
          </button>
        </div>
      </div>
      <GoalStrip day={day} refresh={cards} />
      {copyState === "error" && <p className="bad tl-note">Couldn’t copy the timeline to the clipboard.</p>}
      {retryError && <p className="bad tl-note">{retryError}</p>}
      {reprocessPanel}
      {rows}
    </section>
  );
}
