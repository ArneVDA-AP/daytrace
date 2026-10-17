import * as React from "react";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import { useDay } from "@/app/day";
import { DayPicker } from "@/components/day-picker";
import { JumpBackSlot } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ariaKey, isKey, useKeyLabel } from "@/lib/keys";
import { shiftDay, weekStart } from "@/lib/time";
import { isTypingTarget } from "@/lib/utils";

/**
 * Previous day, next day, "Go to a day" (month calendar) and Today, for the page header of the
 * day sections (unit="week" steps by 7 days for Weekly). Keys while it is on screen: [ previous,
 * ] next, matched by physical key (the two keys right of P; "^" and "$" on Belgian AZERTY, where
 * the labels follow the layout). Today / This week sits right of the title (PageHeader), so the
 * title does not move when you leave today.
 * Uses the shared day (app/day.tsx), so the day carries over between sections.
 */
export function DayNav({ unit = "day" }: { unit?: "day" | "week" }) {
  const { day, today, setDay } = useDay();
  const step = unit === "week" ? 7 : 1;
  const [open, setOpen] = React.useState(false);
  const atEnd = unit === "week" ? weekStart(day) >= weekStart(today) : day >= today;
  const atCurrent = unit === "week" ? weekStart(day) === weekStart(today) : day === today;
  const prevLabel = unit === "week" ? "Previous week" : "Previous day";
  const nextLabel = unit === "week" ? "Next week" : "Next day";

  const prevKey = useKeyLabel("BracketLeft");
  const nextKey = useKeyLabel("BracketRight");

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || isTypingTarget(e.target)) return;
      if (isKey(e, "BracketLeft")) {
        e.preventDefault();
        setDay(shiftDay(day, -step));
      } else if (isKey(e, "BracketRight") && !atEnd) {
        e.preventDefault();
        setDay(shiftDay(day, step));
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [day, step, atEnd, setDay]);

  // Today / This week goes to the page header's slot (right of the title); inline without one.
  const slot = React.useContext(JumpBackSlot);
  const goRef = React.useRef(() => setDay(today));
  goRef.current = () => setDay(today);
  const jumpLabel = unit === "week" ? "This week" : "Today";
  React.useLayoutEffect(() => {
    if (!slot) return;
    slot(atCurrent ? null : { label: jumpLabel, go: () => goRef.current() });
    return () => slot(null);
  }, [slot, atCurrent, jumpLabel]);

  return (
    <div className="flex items-center gap-0.5">
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={prevLabel} aria-keyshortcuts={ariaKey("BracketLeft")} onClick={() => setDay(shiftDay(day, -step))}>
            <ChevronLeft aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {prevLabel} <Kbd className="ml-1">{prevKey}</Kbd>
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={nextLabel}
            aria-keyshortcuts={ariaKey("BracketRight")}
            disabled={atEnd}
            onClick={() => setDay(shiftDay(day, step) > today ? today : shiftDay(day, step))}
          >
            <ChevronRight aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {nextLabel} <Kbd className="ml-1">{nextKey}</Kbd>
        </TooltipContent>
      </Tooltip>
      <Popover open={open} onOpenChange={setOpen}>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button variant="ghost" size="icon-sm" aria-label="Go to a day">
                <CalendarDays aria-hidden="true" />
              </Button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent>Go to a day</TooltipContent>
        </Tooltip>
        <PopoverContent align="start" className="w-auto p-2" onOpenAutoFocus={(e) => e.preventDefault()}>
          <DayPicker
            day={day}
            today={today}
            onPick={(d) => {
              setOpen(false);
              setDay(d);
            }}
          />
        </PopoverContent>
      </Popover>
      {!slot && !atCurrent && (
        <Button variant="outline" size="sm" className="ml-1" onClick={() => setDay(today)}>
          {jumpLabel}
        </Button>
      )}
    </div>
  );
}
