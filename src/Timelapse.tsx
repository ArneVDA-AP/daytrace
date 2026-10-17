import type React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, Shot, clock } from "./api";

/**
 * Plays a card's screenshots as a timelapse (Dayflow's screenshot slideshow,
 * Views/UI/MainView/ScreenshotSlideshow*.swift): frames are shown at their real spacing sped up
 * 20×, 40× or 60× (the choice is remembered), with a scrubber, a full-size view and keyboard
 * control (Space plays and pauses, ← and → step one frame). Thumbnails load lazily, a few frames
 * ahead of the playhead; nothing is encoded to video.
 */

const SPEEDS = [20, 40, 60];
const SPEED_KEY = "daytrace.timelapseSpeed";
/** Frames played: an even sample over the whole card, picked by the backend. */
const MAX_FRAMES = 600;
const PREFETCH = 8;
const PARALLEL = 3;
const STRIP = 12;
const CACHE_LIMIT = 400;

// Thumbnails shared by every player (data URLs; null = file gone).
const cache = new Map<number, string | null>();
const inflight = new Map<number, Promise<string | null>>();

function thumb(id: number): Promise<string | null> {
  if (cache.has(id)) return Promise.resolve(cache.get(id) ?? null);
  const running = inflight.get(id);
  if (running) return running;
  const p = api
    .getScreenshot(id, true)
    .catch(() => null)
    .then((src) => {
      inflight.delete(id);
      cache.set(id, src);
      if (cache.size > CACHE_LIMIT) {
        const oldest = cache.keys().next().value;
        if (oldest !== undefined) cache.delete(oldest);
      }
      return src;
    });
  inflight.set(id, p);
  return p;
}

function loadSpeed(): number {
  try {
    const v = Number(localStorage.getItem(SPEED_KEY));
    return SPEEDS.includes(v) ? v : SPEEDS[0];
  } catch {
    return SPEEDS[0];
  }
}

const isFrame = (s: Shot) => !s.hiddenApp;

export function Timelapse({ cardId }: { cardId: number }) {
  const [frames, setFrames] = useState<Shot[] | null>(null);
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(loadSpeed);
  const [, setLoadedTick] = useState(0);
  const [viewer, setViewer] = useState<{ index: number; src: string | null } | null>(null);
  const queue = useRef<number[]>([]);
  const active = useRef(0);

  useEffect(() => {
    let live = true;
    setFrames(null);
    setIndex(0);
    setPlaying(false);
    setViewer(null);
    api
      .getCardScreenshots(cardId, MAX_FRAMES)
      .then((s) => live && setFrames(s))
      .catch(() => live && setFrames([]));
    return () => {
      live = false;
    };
  }, [cardId]);

  // Load a few frames at a time, nearest first.
  const pump = useCallback(() => {
    while (active.current < PARALLEL && queue.current.length) {
      const id = queue.current.shift()!;
      if (cache.has(id)) continue;
      active.current += 1;
      thumb(id).finally(() => {
        active.current -= 1;
        setLoadedTick((t) => t + 1);
        pump();
      });
    }
  }, []);

  const want = useCallback(
    (ids: number[]) => {
      queue.current = [...ids.filter((id) => !cache.has(id)), ...queue.current.filter((id) => !ids.includes(id))];
      pump();
    },
    [pump],
  );

  const strip = frames ? sample(frames, STRIP) : [];

  useEffect(() => {
    if (!frames || frames.length === 0) return;
    const ahead = frames.slice(index, index + PREFETCH + 1).filter(isFrame).map((s) => s.id);
    want([...ahead, ...strip.filter(isFrame).map((s) => s.id)]);
  }, [frames, index, want]);

  // Playback: wait for the real gap to the next frame, divided by the speed; hold on a frame
  // whose image has not loaded yet (the effect runs again when it arrives).
  const nextReady = !!frames && index < frames.length - 1 && (!isFrame(frames[index + 1]) || cache.has(frames[index + 1].id));
  useEffect(() => {
    if (!playing || !frames) return;
    if (index >= frames.length - 1) {
      setPlaying(false);
      return;
    }
    if (!nextReady) return;
    const gap = Math.min(120, Math.max(1, frames[index + 1].capturedAt - frames[index].capturedAt));
    const t = setTimeout(() => setIndex((i) => Math.min(i + 1, frames.length - 1)), (gap * 1000) / speed);
    return () => clearTimeout(t);
  }, [playing, frames, index, speed, nextReady]);

  const step = (d: number) => {
    if (!frames) return;
    setPlaying(false);
    setIndex((i) => Math.max(0, Math.min(frames.length - 1, i + d)));
  };

  const togglePlay = () => {
    if (!frames || frames.length < 2) return;
    if (!playing && index >= frames.length - 1) setIndex(0);
    setPlaying((p) => !p);
  };

  const cycleSpeed = () => {
    const next = SPEEDS[(SPEEDS.indexOf(speed) + 1) % SPEEDS.length];
    setSpeed(next);
    try {
      localStorage.setItem(SPEED_KEY, String(next));
    } catch {
      /* the choice is not remembered */
    }
  };

  const openFull = async (i: number) => {
    if (!frames) return;
    const s = frames[i];
    setPlaying(false);
    setViewer({ index: i, src: null });
    const src = isFrame(s) ? await api.getScreenshot(s.id, false).catch(() => null) : null;
    setViewer((v) => (v && v.index === i ? { index: i, src } : v));
  };

  // Space plays and pauses (a focused button keeps its own Space), arrows step one frame (the
  // scrubber keeps its own arrows).
  const onKey = (e: React.KeyboardEvent) => {
    const t = e.target as HTMLElement;
    const isRange = t instanceof HTMLInputElement && t.type === "range";
    if (e.key === " " && !(t instanceof HTMLButtonElement)) {
      e.preventDefault();
      togglePlay();
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && !isRange) {
      e.preventDefault();
      step(e.key === "ArrowRight" ? 1 : -1);
    }
  };

  useEffect(() => {
    if (!viewer || !frames) return;
    const onWin = (e: KeyboardEvent) => {
      if (e.key === "Escape") setViewer(null);
      else if (e.key === "ArrowRight" && viewer.index < frames.length - 1) {
        setIndex(viewer.index + 1);
        openFull(viewer.index + 1);
      } else if (e.key === "ArrowLeft" && viewer.index > 0) {
        setIndex(viewer.index - 1);
        openFull(viewer.index - 1);
      }
    };
    window.addEventListener("keydown", onWin);
    return () => window.removeEventListener("keydown", onWin);
  });

  if (frames === null) return <p className="muted tlp-note">Loading screenshots…</p>;
  if (frames.length === 0) return <p className="muted tlp-note">No screenshots were kept for this card.</p>;

  const cur = frames[index];
  const src = isFrame(cur) ? cache.get(cur.id) : undefined;
  return (
    <section className="tlp" aria-label="Timelapse">
      <div className="tlp-stage" tabIndex={0} onKeyDown={onKey} aria-label="Timelapse frame. Space plays or pauses, arrow keys step.">
        {!isFrame(cur) ? (
          <div className="tlp-blank">{cur.hiddenApp} was in front; not captured</div>
        ) : src ? (
          <img src={src} alt={`Screen at ${clock(cur.capturedAt)}`} onDoubleClick={() => openFull(index)} />
        ) : (
          <div className="tlp-blank">{src === null ? "Screenshot deleted" : "Loading…"}</div>
        )}
        <time className="tlp-time">{clock(cur.capturedAt)}</time>
      </div>
      <div className="tlp-controls" onKeyDown={onKey}>
        <button className="ghost" onClick={togglePlay} disabled={frames.length < 2} aria-label={playing ? "Pause" : "Play"}>
          {playing ? "Pause" : "Play"}
        </button>
        <button className="ghost" onClick={cycleSpeed} aria-label={`Speed ${speed} times, change`} title="Playback speed">
          {speed}×
        </button>
        <input
          type="range"
          min={0}
          max={frames.length - 1}
          value={index}
          onChange={(e) => setIndex(Number(e.target.value))}
          aria-label="Position"
          aria-valuetext={`${clock(cur.capturedAt)}, frame ${index + 1} of ${frames.length}`}
        />
        <span className="muted tlp-count">
          {index + 1} / {frames.length}
        </span>
        <button className="ghost" onClick={() => openFull(index)} disabled={!isFrame(cur)}>
          Full size
        </button>
      </div>
      <div className="strip" aria-label="Screenshots">
        {strip.map((s) => {
          const i = frames.indexOf(s);
          const t = cache.get(s.id);
          return (
            <button
              key={s.id}
              className={`thumb${isFrame(s) ? "" : " thumb-hidden"}${i === index ? " on" : ""}`}
              onClick={() => {
                setPlaying(false);
                setIndex(i);
              }}
              title={s.hiddenApp ? `${s.hiddenApp} was in front; not captured` : s.windowTitle ?? undefined}
              aria-label={`Go to ${clock(s.capturedAt)}`}
            >
              {!isFrame(s) ? <span>{s.hiddenApp} hidden</span> : t ? <img src={t} alt="" /> : <span>{t === null ? "Deleted" : ""}</span>}
              <time>{clock(s.capturedAt)}</time>
            </button>
          );
        })}
      </div>
      {viewer && (
        <div className="viewer" role="dialog" aria-label="Screenshot" onClick={() => setViewer(null)}>
          {viewer.src ? (
            <img src={viewer.src} alt={`Screen at ${clock(frames[viewer.index].capturedAt)}`} />
          ) : (
            <div className="viewer-cap">{isFrame(frames[viewer.index]) ? "Loading…" : "Not captured"}</div>
          )}
          <div className="viewer-cap">
            {clock(frames[viewer.index].capturedAt)}
            {frames[viewer.index].windowTitle ? `, ${frames[viewer.index].windowTitle}` : ""} · ← → to move, Esc to close
          </div>
        </div>
      )}
    </section>
  );
}

/** `n` frames spread evenly over the card. */
function sample<T>(items: T[], n: number): T[] {
  if (items.length <= n) return items;
  const out: T[] = [];
  for (let i = 0; i < n; i++) out.push(items[Math.round((i * (items.length - 1)) / (n - 1))]);
  return out;
}
