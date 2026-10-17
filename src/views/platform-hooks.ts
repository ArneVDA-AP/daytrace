import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { applyAppearance, platformApi, type Appearance, type NavTarget } from "./platform-api";
import { rolloverDay, shouldIdleReset } from "./rollover";

/**
 * Keeps the open day current: at the 4 AM rollover a view on "today" moves to the new day, and
 * after 15 minutes without interaction the view returns to today (not while Settings is open;
 * then it happens when Settings closes, like Dayflow).
 */
export function useDayRollover(opts: { day: string | null; today: string | null; inSettings: boolean; setDay: (d: string) => void; onReset: () => void }) {
  const { day, today, inSettings, setDay, onReset } = opts;
  const prevToday = useRef<string | null>(null);
  const lastInteraction = useRef(Date.now());
  const lastReset = useRef<number | null>(null);
  const latest = useRef(opts);
  latest.current = opts;

  // Day change at 4 AM.
  useEffect(() => {
    if (!today) return;
    if (day && prevToday.current && prevToday.current !== today) {
      const next = rolloverDay(day, prevToday.current, today);
      if (next !== day) {
        setDay(next);
        onReset();
      }
    }
    prevToday.current = today;
    // Only reacts to a new "today"; the other values are read as they are at that moment.
  }, [today]);

  // Idle reset.
  useEffect(() => {
    const touch = () => {
      lastInteraction.current = Date.now();
    };
    const check = () => {
      const o = latest.current;
      if (!o.today || o.inSettings) return;
      const now = Date.now();
      if (!shouldIdleReset(now, lastInteraction.current, lastReset.current)) return;
      lastReset.current = now;
      if (o.day !== o.today) o.setDay(o.today);
      o.onReset();
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") check();
    };
    window.addEventListener("pointerdown", touch, true);
    window.addEventListener("keydown", touch, true);
    window.addEventListener("wheel", touch, { capture: true, passive: true });
    window.addEventListener("focus", check);
    document.addEventListener("visibilitychange", onVisible);
    const t = setInterval(check, 30_000);
    return () => {
      window.removeEventListener("pointerdown", touch, true);
      window.removeEventListener("keydown", touch, true);
      window.removeEventListener("wheel", touch, true);
      window.removeEventListener("focus", check);
      document.removeEventListener("visibilitychange", onVisible);
      clearInterval(t);
    };
  }, []);

  // A reset that waited for Settings to close.
  useEffect(() => {
    if (inSettings) return;
    const now = Date.now();
    if (today && shouldIdleReset(now, lastInteraction.current, lastReset.current)) {
      lastReset.current = now;
      if (day !== today) setDay(today);
      onReset();
    }
  }, [inSettings]);
}

/** Opens the view a clicked notification points to. */
export function useNotificationNavigation(go: (target: NavTarget) => void) {
  const goRef = useRef(go);
  goRef.current = go;
  useEffect(() => {
    const take = () =>
      platformApi
        .takeNavigation()
        .then((t) => {
          if (t) goRef.current(t);
        })
        .catch(() => {});
    take();
    const un = listen("platform-navigate", take);
    return () => {
      un.then((f) => f());
    };
  }, []);
}

/** Applies the saved light/dark override whenever it changes. */
export function useAppearance(appearance: Appearance | undefined) {
  useEffect(() => {
    if (appearance) applyAppearance(appearance);
  }, [appearance]);
}
