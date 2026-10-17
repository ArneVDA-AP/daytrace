import * as React from "react";
import { useAppStatusSelect } from "@/app/state";
import { CARDS_CHANGED_EVENT, shellApi } from "@/app/shell-api";
import { href, type Section } from "@/lib/router";
import { teApi } from "@/timeline-extras-api";

/*
 * Counts the rail shows next to its items (Dayflow NotificationBadgeManager and the timeline's
 * "cards to review"): the new-recap dot on Daily, and today's "Not analyzed" cards in the status
 * block. Both are cheap reads, checked once a minute and when something they depend on changes.
 */

const CHECK_MS = 60_000;

/**
 * True while a recap is new (written after Daily was last opened). Opening Daily clears it. The
 * taskbar button shows the same dot (Windows overlay icon).
 */
export function useRecapBadge(section: Section): boolean {
  const [day, setDay] = React.useState<string | null>(null);
  const onDaily = section === "daily";

  React.useEffect(() => {
    let live = true;
    const load = () =>
      shellApi
        .badges()
        .then((b) => live && setDay(b.recapDay))
        .catch(() => undefined);
    void load();
    const t = setInterval(load, CHECK_MS);
    const onVisible = () => {
      if (document.visibilityState === "visible") void load();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      live = false;
      clearInterval(t);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

  // Opening Daily (or a recap arriving while it is open) marks the recap as seen.
  React.useEffect(() => {
    if (!onDaily) return;
    setDay(null);
    shellApi.recapOpened().catch(() => undefined);
  }, [onDaily, day]);

  const on = day !== null && !onDaily;
  React.useEffect(() => {
    shellApi.setOverlay(on).catch(() => undefined);
  }, [on]);
  return on;
}

const selectAnalysisKey = (s: { today: string; status: { pendingBatches: number; analysis: { lastSuccessAt: number | null; lastError: string | null; currentStep: string | null } } }) =>
  `${s.today}|${s.status.analysis.lastSuccessAt ?? ""}|${s.status.analysis.lastError ?? ""}|${s.status.pendingBatches}|${s.status.analysis.currentStep ?? ""}`;
const selectToday = (s: { today: string }) => s.today;

/** Today's cards that are "Not analyzed": how many, and a link to the first. */
export function useNotAnalyzedToday(): { count: number; href: string | null } {
  const today = useAppStatusSelect(selectToday);
  const key = useAppStatusSelect(selectAnalysisKey);
  const [state, setState] = React.useState<{ count: number; first: number | null }>({ count: 0, first: null });
  const [tick, setTick] = React.useState(0);

  React.useEffect(() => {
    const bump = () => setTick((t) => t + 1);
    const t = setInterval(() => document.visibilityState === "visible" && bump(), CHECK_MS);
    addEventListener(CARDS_CHANGED_EVENT, bump);
    return () => {
      clearInterval(t);
      removeEventListener(CARDS_CHANGED_EVENT, bump);
    };
  }, []);

  React.useEffect(() => {
    let live = true;
    teApi
      .dayExtras(today)
      .then((x) => {
        if (!live) return;
        const ids = x.failedGroups.flatMap((g) => g.cardIds);
        const first = x.failedGroups.length ? x.failedGroups.reduce((a, g) => (g.startTs < a.startTs ? g : a)).cardIds[0] : null;
        setState((cur) => (cur.count === ids.length && cur.first === first ? cur : { count: ids.length, first }));
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [today, key, tick]);

  return { count: state.count, href: state.first !== null ? href("timeline", { day: today, card: state.first }) : null };
}
