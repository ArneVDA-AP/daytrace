import * as React from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { dateOf, dayKey, monthYear } from "@/lib/time";
import { cn } from "@/lib/utils";
import { teApi } from "@/timeline-extras-api";

/*
 * Month calendar for "Go to a day" (Dayflow's TimelineCalendarPopover): Monday-first, future days
 * disabled, days with timeline cards marked. One tab stop; arrow keys move by day and week,
 * PageUp/PageDown by month, Home/End to the week's ends, Enter picks.
 */

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const LABEL = new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long", year: "numeric" });

function monthGrid(month: Date): Date[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const lead = (first.getDay() + 6) % 7;
  return Array.from({ length: 42 }, (_, i) => new Date(first.getFullYear(), first.getMonth(), 1 - lead + i));
}

export function DayPicker({ day, today, onPick }: { day: string; today: string; onPick: (day: string) => void }) {
  const [month, setMonth] = React.useState(() => {
    const d = dateOf(day);
    return new Date(d.getFullYear(), d.getMonth(), 1);
  });
  const [focus, setFocus] = React.useState(day);
  const [counts, setCounts] = React.useState<Map<string, number> | null>(null);
  const [loadError, setLoadError] = React.useState(false);
  const gridRef = React.useRef<HTMLDivElement>(null);
  const moveFocusToGrid = React.useRef(true);

  const cells = monthGrid(month);
  const from = dayKey(cells[0]);
  const to = dayKey(cells[cells.length - 1]);

  React.useEffect(() => {
    let live = true;
    teApi
      .daysWithData(from, to)
      .then((rows) => {
        if (!live) return;
        setCounts(new Map(rows.map((r) => [r.day, r.cards])));
        setLoadError(false);
      })
      .catch(() => live && setLoadError(true));
    return () => {
      live = false;
    };
  }, [from, to]);

  React.useEffect(() => {
    if (!moveFocusToGrid.current) return;
    moveFocusToGrid.current = false;
    gridRef.current?.querySelector<HTMLButtonElement>(`[data-day="${focus}"]`)?.focus();
  }, [focus, month]);

  const moveTo = (next: Date) => {
    const key = dayKey(next);
    if (key > today) return;
    moveFocusToGrid.current = true;
    setFocus(key);
    if (next.getMonth() !== month.getMonth() || next.getFullYear() !== month.getFullYear()) {
      setMonth(new Date(next.getFullYear(), next.getMonth(), 1));
    }
  };

  const shiftMonth = (delta: number, fromKeyboard: boolean) => {
    const m = new Date(month.getFullYear(), month.getMonth() + delta, 1);
    const f = dateOf(focus);
    const last = new Date(m.getFullYear(), m.getMonth() + 1, 0).getDate();
    const next = dayKey(new Date(m.getFullYear(), m.getMonth(), Math.min(f.getDate(), last)));
    moveFocusToGrid.current = fromKeyboard;
    setMonth(m);
    setFocus(next > today ? today : next);
  };

  const onKey = (e: React.KeyboardEvent) => {
    const f = dateOf(focus);
    const dow = (f.getDay() + 6) % 7;
    const moves: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7, Home: -dow, End: 6 - dow };
    if (e.key in moves) {
      e.preventDefault();
      moveTo(new Date(f.getFullYear(), f.getMonth(), f.getDate() + moves[e.key]));
    } else if (e.key === "PageUp" || e.key === "PageDown") {
      e.preventDefault();
      shiftMonth(e.key === "PageUp" ? -1 : 1, true);
    }
  };

  const nextDisabled = dayKey(new Date(month.getFullYear(), month.getMonth() + 1, 1)) > today;

  return (
    <div className="w-[17.5rem]">
      <div className="flex items-center justify-between gap-2 px-1 pb-2">
        <p className="font-semibold" aria-live="polite">
          {monthYear(month)}
        </p>
        <div className="flex gap-0.5">
          <Button variant="ghost" size="icon-sm" aria-label="Previous month" onClick={() => shiftMonth(-1, false)}>
            <ChevronLeft aria-hidden="true" />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="Next month" onClick={() => shiftMonth(1, false)} disabled={nextDisabled}>
            <ChevronRight aria-hidden="true" />
          </Button>
        </div>
      </div>
      <div role="grid" aria-label={monthYear(month)} ref={gridRef} onKeyDown={onKey} className="grid gap-0.5">
        <div role="row" className="grid grid-cols-7 gap-0.5">
          {WEEKDAYS.map((w) => (
            <span key={w} role="columnheader" className="grid h-7 place-items-center text-caption text-muted-foreground">
              {w}
            </span>
          ))}
        </div>
        {Array.from({ length: 6 }, (_, w) => (
          <div role="row" key={w} className="grid grid-cols-7 gap-0.5">
            {cells.slice(w * 7, w * 7 + 7).map((c) => {
              const d = dayKey(c);
              const n = counts?.get(d) ?? 0;
              const future = d > today;
              const outside = c.getMonth() !== month.getMonth();
              const selected = d === day;
              const label = `${LABEL.format(c)}${n ? `, ${n} ${n === 1 ? "card" : "cards"}` : future || !counts ? "" : ", no timeline"}`;
              return (
                <span role="gridcell" key={d} aria-selected={selected}>
                  <button
                    type="button"
                    data-day={d}
                    disabled={future}
                    tabIndex={d === focus ? 0 : -1}
                    aria-label={label}
                    aria-current={d === today ? "date" : undefined}
                    onClick={() => onPick(d)}
                    onFocus={() => setFocus(d)}
                    className={cn(
                      "relative grid size-9 place-items-center rounded-md text-meta tnum hover:bg-surface-2 disabled:pointer-events-none disabled:opacity-35",
                      outside && "text-subtle-foreground",
                      d === today && "font-semibold ring-1 ring-input ring-inset",
                      selected && "bg-primary text-primary-foreground hover:bg-primary",
                    )}
                  >
                    {c.getDate()}
                    {n > 0 && (
                      <span
                        aria-hidden="true"
                        className={cn("absolute bottom-1 size-1 rounded-full", selected ? "bg-primary-foreground" : "bg-foreground")}
                      />
                    )}
                  </button>
                </span>
              );
            })}
          </div>
        ))}
      </div>
      <p className="px-1 pt-2 text-caption text-muted-foreground">
        {loadError ? (
          <span className="text-destructive">Couldn’t load which days have a timeline.</span>
        ) : (
          <>
            <span aria-hidden="true" className="mr-1.5 inline-block size-1 rounded-full bg-foreground align-middle" />
            Day with a timeline
          </>
        )}
      </p>
    </div>
  );
}
