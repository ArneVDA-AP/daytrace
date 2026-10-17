import type React from "react";
import { useEffect, useRef, useState } from "react";
import { formatDayTitle } from "./api";
import { teApi } from "./timeline-extras-api";
import "./timeline-extras.css";

/**
 * The day title in the header, which opens a month calendar to jump to any date (Dayflow's
 * Views/UI/MainView/TimelineCalendarPopover.swift: Monday-first grid, month arrows, future days
 * disabled). Days with timeline cards are marked.
 */

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

function parse(day: string): Date {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** Six Monday-first weeks covering the month of `month`. */
function grid(month: Date): Date[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const lead = (first.getDay() + 6) % 7;
  return Array.from({ length: 42 }, (_, i) => new Date(first.getFullYear(), first.getMonth(), 1 - lead + i));
}

export function DayCalendar({ day, today, onPick }: { day: string; today: string; onPick: (day: string) => void }) {
  const [open, setOpen] = useState(false);
  const [month, setMonth] = useState(() => parse(day));
  const [focus, setFocus] = useState(day);
  const [counts, setCounts] = useState<Map<string, number>>(new Map());
  const [loadError, setLoadError] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  /** Move keyboard focus to the focused day after opening or arrow-key moves only. */
  const focusGrid = useRef(false);

  const cells = grid(month);
  const from = iso(cells[0]);
  const to = iso(cells[cells.length - 1]);

  useEffect(() => {
    if (!open) return;
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
  }, [open, from, to]);

  // Close on a click outside or Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  // Keep keyboard focus on the focused day.
  useEffect(() => {
    if (!open || !focusGrid.current) return;
    focusGrid.current = false;
    gridRef.current?.querySelector<HTMLButtonElement>(`[data-day="${focus}"]`)?.focus();
  }, [open, focus, month]);

  const toggle = () => {
    if (!open) {
      setMonth(parse(day));
      setFocus(day);
      focusGrid.current = true;
    }
    setOpen(!open);
  };

  const pick = (d: string) => {
    if (d > today) return;
    onPick(d);
    setOpen(false);
  };

  const moveFocus = (delta: number) => {
    const d = parse(focus);
    d.setDate(d.getDate() + delta);
    const next = iso(d);
    if (next > today) return;
    focusGrid.current = true;
    setFocus(next);
    if (d.getMonth() !== month.getMonth() || d.getFullYear() !== month.getFullYear()) setMonth(new Date(d.getFullYear(), d.getMonth(), 1));
  };

  const shiftMonth = (delta: number, fromKeyboard = false) => {
    const m = new Date(month.getFullYear(), month.getMonth() + delta, 1);
    setMonth(m);
    const f = parse(focus);
    const clampDay = Math.min(f.getDate(), new Date(m.getFullYear(), m.getMonth() + 1, 0).getDate());
    const next = iso(new Date(m.getFullYear(), m.getMonth(), clampDay));
    focusGrid.current = fromKeyboard;
    setFocus(next > today ? today : next);
  };

  const onKey = (e: React.KeyboardEvent) => {
    const moves: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };
    if (e.key in moves && (e.target as HTMLElement).classList.contains("cal-day")) {
      e.preventDefault();
      moveFocus(moves[e.key]);
    } else if (e.key === "PageUp" || e.key === "PageDown") {
      e.preventDefault();
      shiftMonth(e.key === "PageUp" ? -1 : 1, true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
    }
  };

  const monthLabel = month.toLocaleDateString("en-GB", { month: "long", year: "numeric" });
  const nextDisabled = iso(new Date(month.getFullYear(), month.getMonth() + 1, 1)) > today;

  return (
    <div className="cal-wrap" ref={wrap}>
      <h1>
        <button className="cal-title" onClick={toggle} aria-haspopup="dialog" aria-expanded={open} title="Pick a day">
          {formatDayTitle(day)}
        </button>
      </h1>
      {open && (
        <div className="cal" role="dialog" aria-label="Pick a day" onKeyDown={onKey}>
          <div className="cal-head">
            <span>{monthLabel}</span>
            <div>
              <button className="icon" aria-label="Previous month" onClick={() => shiftMonth(-1)}>
                ‹
              </button>
              <button className="icon" aria-label="Next month" onClick={() => shiftMonth(1)} disabled={nextDisabled}>
                ›
              </button>
            </div>
          </div>
          <div className="cal-grid" role="grid" ref={gridRef}>
            <div role="row" className="cal-row">
              {WEEKDAYS.map((w) => (
                <span key={w} role="columnheader" className="cal-wd">
                  {w}
                </span>
              ))}
            </div>
            {Array.from({ length: 6 }, (_, w) => (
              <div role="row" className="cal-row" key={w}>
                {cells.slice(w * 7, w * 7 + 7).map((c) => {
                  const d = iso(c);
                  const n = counts.get(d) ?? 0;
                  const future = d > today;
                  const outside = c.getMonth() !== month.getMonth();
                  const label = `${c.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long", year: "numeric" })}${
                    n ? `, ${n} ${n === 1 ? "card" : "cards"}` : future ? "" : ", no timeline"
                  }`;
                  return (
                    <span role="gridcell" key={d} aria-selected={d === day}>
                      <button
                        data-day={d}
                        className={`cal-day${d === day ? " sel" : ""}${d === today ? " today" : ""}${outside ? " out" : ""}${n ? " has" : ""}`}
                        disabled={future}
                        tabIndex={d === focus ? 0 : -1}
                        aria-label={label}
                        aria-current={d === today ? "date" : undefined}
                        onClick={() => pick(d)}
                        onFocus={() => setFocus(d)}
                      >
                        {c.getDate()}
                      </button>
                    </span>
                  );
                })}
              </div>
            ))}
          </div>
          <div className="cal-foot">
            {loadError ? (
              <span className="bad">Couldn’t load which days have a timeline.</span>
            ) : (
              <span className="muted">
                <span className="cal-mark" aria-hidden="true" /> Day with a timeline
              </span>
            )}
            {day !== today && (
              <button className="link" onClick={() => pick(today)}>
                Today
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
