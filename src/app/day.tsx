import * as React from "react";
import { DAY_SECTIONS, dayParam, navigate, useRoute, type Params, type Section } from "@/lib/router";
import { useAppState } from "@/app/state";

/*
 * The day travels with you: Timeline, Daily, Weekly and Goals share the `day` route param. Moving
 * from Thursday's timeline to Daily shows Thursday's recap; Weekly takes the week of that day.
 * Chat and Settings do not show a day, so the rail remembers the last one for the way back.
 */

export interface DayState {
  /** Logical day shown by day sections (YYYY-MM-DD), never after today. */
  day: string;
  today: string;
  /** Goes to another day in the current day section (or remembers it elsewhere). */
  setDay: (day: string, opts?: { replace?: boolean }) => void;
  /** Opens the timeline at a day, optionally with a card selected. */
  openTimeline: (day: string, cardId?: number) => void;
}

const Ctx = React.createContext<DayState | null>(null);

export function useDay(): DayState {
  const v = React.useContext(Ctx);
  if (!v) throw new Error("useDay() outside <DayProvider>");
  return v;
}

export function isDaySection(s: Section): boolean {
  return DAY_SECTIONS.includes(s);
}

export function DayProvider({ children }: { children: React.ReactNode }) {
  const { today } = useAppState();
  const route = useRoute();
  const onDaySection = isDaySection(route.section);
  const routeDay = dayParam(route.params);
  const [remembered, setRemembered] = React.useState(today);

  const day = onDaySection ? clamp(routeDay ?? today, today) : clamp(remembered, today);

  // Remember the day of the last day section for the rail links from Chat and Settings.
  React.useEffect(() => {
    if (onDaySection) setRemembered(day);
  }, [onDaySection, day]);

  const value = React.useMemo<DayState>(
    () => ({
      day,
      today,
      setDay: (d, opts) => {
        const next = clamp(d, today);
        if (!onDaySection) {
          setRemembered(next);
          return;
        }
        const params: Params = {};
        route.params.forEach((v, k) => (params[k] = v));
        delete params.card; // a selection belongs to its day
        navigate(route.section, { ...params, day: next }, opts);
      },
      openTimeline: (d, cardId) => navigate("timeline", { day: d, card: cardId }),
    }),
    [day, today, onDaySection, route],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

function clamp(day: string, today: string): string {
  return day > today ? today : day;
}
