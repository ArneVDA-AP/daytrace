import { useDay } from "@/app/day";
import { useAppState } from "@/app/state";
import type { ViewProps } from "@/views/types";

/** The props every section receives (the ViewProps contract of src/views/types.ts). */
export function useViewProps(): ViewProps {
  const app = useAppState();
  const { day, today, setDay, openTimeline } = useDay();
  return {
    day,
    setDay: (d) => setDay(d),
    today,
    settings: app.settings,
    status: app.status,
    categories: app.categories,
    colorFor: app.colorFor,
    openTimeline,
  };
}
