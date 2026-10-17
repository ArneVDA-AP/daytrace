import * as React from "react";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import { useDay } from "@/app/day";
import { DayPicker } from "@/components/day-picker";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { shiftDay, weekStart } from "@/lib/time";

/** True when a key press belongs to a text field, a slider or an open dialog/menu. */
export function isTypingTarget(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || !el.closest) return false;
  return !!el.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"], [role="slider"], [role="dialog"], [role="menu"], [role="listbox"], [role="grid"]');
}

/**
 * Previous day, next day, "Go to a day" (month calendar) and Today, for the page header of the
 * day sections (unit="week" steps by 7 days for Weekly). Keys while it is on screen: [ previous,
 * ] next.
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

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented || isTypingTarget(e.target)) return;
      if (e.key === "[") {
        e.preventDefault();
        setDay(shiftDay(day, -step));
      } else if (e.key === "]" && !atEnd) {
        e.preventDefault();
        setDay(shiftDay(day, step));
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [day, step, atEnd, setDay]);

  return (
    <div className="flex items-center gap-0.5">
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={prevLabel} aria-keyshortcuts="[" onClick={() => setDay(shiftDay(day, -step))}>
            <ChevronLeft aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {prevLabel} <Kbd className="ml-1">[</Kbd>
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={nextLabel}
            aria-keyshortcuts="]"
            disabled={atEnd}
            onClick={() => setDay(shiftDay(day, step) > today ? today : shiftDay(day, step))}
          >
            <ChevronRight aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {nextLabel} <Kbd className="ml-1">]</Kbd>
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
      {!atCurrent && (
        <Button variant="outline" size="sm" className="ml-1" onClick={() => setDay(today)}>
          {unit === "week" ? "This week" : "Today"}
        </Button>
      )}
    </div>
  );
}
