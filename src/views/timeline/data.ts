import * as React from "react";
import { errorText } from "@/lib/utils";
import { timelineApi, type BatchProgress, type Card, type DayPlan, type RetryItem } from "./api";

/*
 * Data hooks of the Timeline. Polling refreshes compare before they set, so nothing re-renders or
 * moves when nothing changed (README: "Polling refreshes must not show loaders or move anything").
 */

const CARD_POLL_MS = 15_000;
const FAST_POLL_MS = 3_000;

function same<T>(a: T, b: T): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export interface DayCards {
  /** null while the first load of this day runs (or failed). */
  cards: Card[] | null;
  error: string | null;
  reload: () => void;
}

/** The day's cards (get_day), polled every 15 s while the window is visible, every 3 s with `fast`. */
export function useDayCards(day: string, fast: boolean): DayCards {
  const [state, setState] = React.useState<{ day: string; cards: Card[] | null; error: string | null }>({ day, cards: null, error: null });
  const dayRef = React.useRef(day);
  dayRef.current = day;

  const load = React.useCallback(() => {
    const d = day;
    timelineApi
      .getDay(d)
      .then((cards) => {
        if (dayRef.current !== d) return;
        setState((cur) => (cur.day === d && cur.cards && !cur.error && same(cur.cards, cards) ? cur : { day: d, cards, error: null }));
      })
      .catch((e) => {
        if (dayRef.current !== d) return;
        // A failed poll keeps what is on screen; only a failed first load shows the error.
        setState((cur) => (cur.day === d && cur.cards ? cur : { day: d, cards: null, error: errorText(e) }));
      });
  }, [day]);

  React.useEffect(() => {
    setState((cur) => (cur.day === day ? cur : { day, cards: null, error: null }));
    load();
  }, [day, load]);

  React.useEffect(() => {
    const t = setInterval(() => {
      if (document.visibilityState === "visible") load();
    }, fast ? FAST_POLL_MS : CARD_POLL_MS);
    return () => clearInterval(t);
  }, [load, fast]);

  const current = state.day === day ? state : { cards: null, error: null };
  return { cards: current.cards, error: current.error, reload: load };
}

const WEEK_POLL_MS = 60_000;

export interface WeekCards {
  /** Cards per day of `days` (future days are empty); null while the first load runs (or failed). */
  cards: Card[][] | null;
  error: string | null;
  reload: () => void;
}

/**
 * The cards of seven days (get_day for each day up to today), polled every minute while the
 * window is visible (Dayflow's week grid refreshes every 60 s).
 */
export function useWeekCards(days: string[], today: string): WeekCards {
  const key = `${days.join(",")}|${today}`;
  const [state, setState] = React.useState<{ key: string; cards: Card[][] | null; error: string | null }>({ key, cards: null, error: null });
  const keyRef = React.useRef(key);
  keyRef.current = key;
  const daysRef = React.useRef(days);
  daysRef.current = days;

  const load = React.useCallback(() => {
    const k = key;
    Promise.all(daysRef.current.map((d) => (d > today ? Promise.resolve<Card[]>([]) : timelineApi.getDay(d))))
      .then((cards) => {
        if (keyRef.current !== k) return;
        setState((cur) => (cur.key === k && cur.cards && !cur.error && same(cur.cards, cards) ? cur : { key: k, cards, error: null }));
      })
      .catch((e) => {
        if (keyRef.current !== k) return;
        setState((cur) => (cur.key === k && cur.cards ? cur : { key: k, cards: null, error: errorText(e) }));
      });
  }, [key, today]);

  React.useEffect(() => {
    setState((cur) => (cur.key === key ? cur : { key, cards: null, error: null }));
    load();
  }, [key, load]);

  React.useEffect(() => {
    const t = setInterval(() => {
      if (document.visibilityState === "visible") load();
    }, WEEK_POLL_MS);
    const onVisible = () => document.visibilityState === "visible" && load();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearInterval(t);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [load]);

  const current = state.key === key ? state : { cards: null, error: null };
  return { cards: current.cards, error: current.error, reload: load };
}

/**
 * Loads `fn(key)` whenever `key` or `refresh` changes. Keeps the last result for the same key
 * while reloading (no flash), drops it when the key changes.
 */
export function useLoad<T>(key: string, fn: (key: string) => Promise<T>, refresh: unknown): { data: T | null; error: string | null; reload: () => void } {
  const [state, setState] = React.useState<{ key: string; data: T | null; error: string | null }>({ key, data: null, error: null });
  const [tick, setTick] = React.useState(0);
  const fnRef = React.useRef(fn);
  fnRef.current = fn;

  React.useEffect(() => {
    let live = true;
    fnRef
      .current(key)
      .then((data) => {
        if (!live) return;
        setState((cur) => (cur.key === key && !cur.error && same(cur.data, data) ? cur : { key, data, error: null }));
      })
      .catch((e) => {
        if (!live) return;
        setState((cur) => (cur.key === key && cur.data ? cur : { key, data: null, error: errorText(e) }));
      });
    return () => {
      live = false;
    };
  }, [key, refresh, tick]);

  const reload = React.useCallback(() => setTick((t) => t + 1), []);
  return state.key === key ? { data: state.data, error: state.error, reload } : { data: null, error: null, reload };
}

// ---------------------------------------------------------------- retry of failed cards

const isActive = (i: RetryItem) => i.state === "queued" || i.state === "running";

/**
 * Status of "Try again" runs (Dayflow RetryCoordinator, te_retry_status): read once, then every
 * 2 s while a run is queued or running. `onFinished` runs when the last one ends.
 */
export function useRetryStatus(onFinished: () => void) {
  const [items, setItems] = React.useState<RetryItem[]>([]);
  const active = items.some(isActive);
  const wasActive = React.useRef(false);
  const onFinishedRef = React.useRef(onFinished);
  onFinishedRef.current = onFinished;

  const refresh = React.useCallback(async () => {
    try {
      const next = await timelineApi.retryStatus();
      const nowActive = next.some(isActive);
      if (wasActive.current && !nowActive) onFinishedRef.current();
      wasActive.current = nowActive;
      setItems((cur) => (same(cur, next) ? cur : next));
    } catch {
      // Keep the last known status.
    }
  }, []);

  React.useEffect(() => {
    void refresh();
    if (!active) return;
    const t = setInterval(() => void refresh(), 2000);
    return () => clearInterval(t);
  }, [active, refresh]);

  return { items, active, refresh };
}

/** Dayflow RetryCoordinator.statusLine for a group of batches. */
export function retryLine(items: RetryItem[], batchIds: number[]): string | null {
  const mine = items.filter((i) => batchIds.includes(i.batchId));
  if (mine.length === 0) return null;
  if (mine.some((i) => i.state === "running")) return "Analyzing again…";
  if (mine.some((i) => i.state === "waiting")) return "Waiting for an API key. Add one in Settings.";
  if (mine.some((i) => i.state === "failed")) return "Failed again. Retry stopped.";
  const queued = mine.filter((i) => i.state === "queued").map((i) => i.position);
  if (queued.length) {
    const total = mine[0].total;
    const lo = Math.min(...queued);
    const hi = Math.max(...queued);
    return lo === hi ? `Queued (${lo} of ${total})` : `Queued (${lo}–${hi} of ${total})`;
  }
  if (mine.some((i) => i.state === "stopped")) return "Stopped because an earlier batch failed.";
  return null;
}

export function retryBusy(items: RetryItem[], batchIds: number[]): boolean {
  return items.some((i) => batchIds.includes(i.batchId) && isActive(i));
}

// ---------------------------------------------------------------- reprocess day

export type ReprocessState =
  | { stage: "idle" }
  | { stage: "loading" }
  | { stage: "confirm"; plan: DayPlan }
  /** The confirmed request is on its way; the dialog stays, with its button disabled (FE8). */
  | { stage: "starting"; plan: DayPlan }
  | { stage: "running"; batchIds: number[]; progress: BatchProgress | null }
  | { stage: "done"; failed: number; total: number }
  | { stage: "error"; message: string };

// Kept at module level so a running reprocess survives switching sections (the view unmounts).
let activeReprocess: { day: string; batchIds: number[] } | null = null;

function initial(day: string): ReprocessState {
  return activeReprocess && activeReprocess.day === day ? { stage: "running", batchIds: activeReprocess.batchIds, progress: null } : { stage: "idle" };
}

/** "Reprocess day" (Dayflow reprocessDay): plan, confirm, run with progress, done. */
export function useReprocessDay(day: string, onChanged: () => void) {
  const [rp, setRp] = React.useState<ReprocessState>(() => initial(day));
  const onChangedRef = React.useRef(onChanged);
  onChangedRef.current = onChanged;

  React.useEffect(() => setRp(initial(day)), [day]);

  const runningIds = rp.stage === "running" ? rp.batchIds.join(",") : null;
  React.useEffect(() => {
    if (runningIds === null) return;
    const ids = runningIds ? runningIds.split(",").map(Number) : [];
    let live = true;
    let polls = 0;
    const tick = async () => {
      try {
        const p = await timelineApi.batchProgress(ids);
        if (!live) return;
        polls += 1;
        if (p.waiting + p.running === 0) {
          activeReprocess = null;
          setRp({ stage: "done", failed: p.failed, total: ids.length });
          onChangedRef.current();
        } else {
          setRp((cur) => (cur.stage === "running" && same(cur.progress, p) ? cur : { stage: "running", batchIds: ids, progress: p }));
          if (polls % 5 === 0) onChangedRef.current(); // new cards appear as batches finish
        }
      } catch (e) {
        if (live) setRp({ stage: "error", message: errorText(e) });
      }
    };
    void tick();
    const t = setInterval(() => void tick(), 2000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [runningIds]);

  const ask = React.useCallback(async () => {
    setRp({ stage: "loading" });
    try {
      setRp({ stage: "confirm", plan: await timelineApi.reprocessDayPlan(day) });
    } catch (e) {
      setRp({ stage: "error", message: errorText(e) });
    }
  }, [day]);

  // One request per confirmation: a second click while it runs does nothing (FE8).
  const starting = React.useRef(false);
  const start = React.useCallback(async () => {
    if (starting.current) return;
    starting.current = true;
    setRp((cur) => (cur.stage === "confirm" ? { stage: "starting", plan: cur.plan } : cur));
    try {
      const plan = await timelineApi.reprocessDay(day);
      activeReprocess = { day, batchIds: plan.batchIds };
      setRp({ stage: "running", batchIds: plan.batchIds, progress: null });
      onChangedRef.current();
    } catch (e) {
      setRp({ stage: "error", message: errorText(e) });
    } finally {
      starting.current = false;
    }
  }, [day]);

  const close = React.useCallback(() => setRp((cur) => (cur.stage === "starting" ? cur : { stage: "idle" })), []);
  return { rp, ask, start, close };
}

// ---------------------------------------------------------------- layout helpers

/** Width of an element, measured before paint and on every resize. */
export function useWidth<T extends HTMLElement>(fallback: number): [React.RefObject<T | null>, number] {
  const ref = React.useRef<T | null>(null);
  const [w, setW] = React.useState(fallback);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setW(el.clientWidth || fallback);
    const ro = new ResizeObserver(([e]) => setW(Math.round(e.contentRect.width) || fallback));
    ro.observe(el);
    return () => ro.disconnect();
  }, [fallback]);
  return [ref, w];
}

/**
 * Height of an element's box (e.g. a scroll area), measured when it mounts and on every resize.
 * A callback ref, so an element that is replaced (loading → loaded) is measured again.
 */
export function useHeight<T extends HTMLElement>(fallback: number): [(el: T | null) => void, number] {
  const [h, setH] = React.useState(fallback);
  const ro = React.useRef<ResizeObserver | null>(null);
  const ref = React.useCallback(
    (el: T | null) => {
      ro.current?.disconnect();
      ro.current = null;
      if (!el) return;
      setH(el.clientHeight || fallback);
      ro.current = new ResizeObserver(([e]) => setH(Math.round(e.contentRect.height) || fallback));
      ro.current.observe(el);
    },
    [fallback],
  );
  return [ref, h];
}

/** Matches a media query (e.g. the split breakpoint). */
export function useMedia(query: string): boolean {
  const [m, setM] = React.useState(() => matchMedia(query).matches);
  React.useEffect(() => {
    const mq = matchMedia(query);
    const on = () => setM(mq.matches);
    on();
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [query]);
  return m;
}

/** Wall clock in Unix seconds, ticking every `ms`. */
export function useNow(ms = 30_000): number {
  const [now, setNow] = React.useState(() => Math.floor(Date.now() / 1000));
  React.useEffect(() => {
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}

/** localStorage read and write that never throw (private mode, blocked storage). */
export function readPref(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
export function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The choice is not remembered.
  }
}
