import * as React from "react";
import type { Category } from "@/api";
import { clock, formatDuration, hourLabel } from "@/lib/time";
import { cn } from "@/lib/utils";
import { byCategory } from "@/views/insights/format";
import { CategoryMark, EmptyMark, Legend, type LegendItem } from "@/views/insights/parts";
import { SLOTS, slotLabel, type Share, type Slot } from "./activity";

/*
 * GitHub-style grid of the day: 24 hour columns (4 AM to 4 AM) by 4 quarter-hour rows. Every
 * square is a small stacked column of the categories that filled its 15 minutes (per-category
 * share, bottom up in settings order); idle is hatched, failed cards striped, the rest of the
 * square is the empty well (not recorded). One tab stop: arrow keys move through the squares
 * (Left/Right an hour, Up/Down a quarter hour, Home/End), Enter opens the card that filled most
 * of the square in the timeline. Every square has a full text label.
 */

const QUARTERS = [":00", ":15", ":30", ":45"];

function dur(secs: number): string {
  return secs < 30 ? "under 1 min" : formatDuration(secs);
}

function shareLabel(s: Share): string {
  return s.kind === "system" ? "Not analyzed" : s.name;
}

function slotText(s: Slot, now: number | undefined): string {
  if (s.skipped) return `${slotLabel(s.index)} to ${slotLabel(s.index + 1)}: skipped, the clock moved forward an hour`;
  const when = `${clock(s.start)} to ${clock(s.end)}`;
  if (s.future) return `${when}: not yet`;
  if (!s.shares.length) return `${when}: not recorded`;
  const parts = s.shares.map((x) => `${shareLabel(x)} ${dur(x.secs)}`);
  // Only the part already behind us can be "not recorded".
  const gap = Math.min(s.end, now ?? s.end) - s.start - s.filled;
  if (gap >= 60) parts.push(`not recorded ${dur(gap)}`);
  let text = `${when}: ${parts.join(", ")}.`;
  if (s.top) text += ` Mostly “${s.top.title || "Untitled card"}”.`;
  if (s.noted.length) text += ` Distraction noted: ${s.noted.join(", ")}.`;
  return text;
}

/** "Not yet" key: a lighter, outlined well. */
function FutureMark() {
  return (
    <span
      aria-hidden="true"
      className="size-2.5 shrink-0 rounded-[2px] bg-surface-1 ring-1 ring-border-strong ring-inset forced-colors:border forced-colors:border-[GrayText]"
    />
  );
}

export function ActivityGrid({
  slots,
  categories,
  colorFor,
  onOpen,
  now,
}: {
  slots: Slot[];
  categories: Category[];
  colorFor: (n: string) => string;
  onOpen: (cardId: number) => void;
  /** Today: the moment the slots were made for (later quarter hours are "not yet"). */
  now?: number;
}) {
  const firstFilled = Math.max(0, slots.findIndex((s) => s.shares.length > 0));
  const [active, setActive] = React.useState(firstFilled);
  const [tip, setTip] = React.useState<{ i: number; x: number; y: number; align: "start" | "center" | "end" } | null>(null);
  const gridRef = React.useRef<HTMLDivElement>(null);
  const wrapRef = React.useRef<HTMLDivElement>(null);
  const recorded = slots.filter((s) => s.shares.length > 0).length;

  // A new day starts at its first recorded quarter hour.
  React.useEffect(() => setActive(firstFilled), [firstFilled]);

  const showTip = (i: number, el: HTMLElement) => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const w = wrap.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2 - w.left;
    const align = x < 130 ? "start" : x > w.width - 130 ? "end" : "center";
    setTip({ i, x: align === "start" ? r.left - w.left : align === "end" ? r.right - w.left : x, y: r.top - w.top, align });
  };

  const focusSlot = (i: number) => {
    const n = Math.min(SLOTS - 1, Math.max(0, i));
    setActive(n);
    const el = gridRef.current?.querySelector<HTMLButtonElement>(`[data-slot-index="${n}"]`);
    el?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const moves: Record<string, number> = { ArrowRight: 4, ArrowLeft: -4, ArrowDown: 1, ArrowUp: -1 };
    if (e.key in moves) {
      e.preventDefault();
      focusSlot(active + moves[e.key]);
    } else if (e.key === "Home") {
      e.preventDefault();
      focusSlot(0);
    } else if (e.key === "End") {
      e.preventDefault();
      focusSlot(SLOTS - 1);
    } else if (e.key === "Escape") {
      setTip(null);
    }
  };

  // Legend: the categories that appear today, in stack order, then the textures.
  const present = new Map<string, Share>();
  for (const s of slots) for (const x of s.shares) if (!present.has(x.name.toLowerCase())) present.set(x.name.toLowerCase(), x);
  const shares = [...present.values()];
  const legend: LegendItem[] = [
    ...byCategory(
      shares.filter((x) => x.kind !== "system"),
      categories,
    ).map((x) => ({ key: x.name, label: x.name, mark: <CategoryMark name={x.name} kind={x.kind} colorFor={colorFor} /> })),
    ...(shares.some((x) => x.kind === "system")
      ? [{ key: "system", label: "Not analyzed", mark: <CategoryMark name="System" kind="system" colorFor={colorFor} /> }]
      : []),
    { key: "none", label: "Not recorded", mark: <EmptyMark /> },
    ...(slots.some((s) => s.future) ? [{ key: "future", label: "Not yet", mark: <FutureMark /> }] : []),
  ];

  const t = tip ? slots[tip.i] : null;

  return (
    <figure className="flex flex-col gap-3">
      <div ref={wrapRef} className="relative grid grid-cols-[2.25rem_minmax(0,1fr)] gap-x-2" onMouseLeave={() => setTip(null)}>
        <div aria-hidden="true" className="grid grid-rows-[1.25rem_repeat(4,minmax(0,1fr))] gap-y-[3px] text-right font-mono text-axis leading-none text-subtle-foreground">
          <span />
          {QUARTERS.map((q, i) => (
            <span key={q} className={cn("flex items-center justify-end", i % 2 === 1 && "invisible")}>
              {q}
            </span>
          ))}
        </div>
        <div className="min-w-0">
          <div aria-hidden="true" className="grid h-5 grid-cols-24 gap-x-[3px] font-mono text-axis leading-4 text-subtle-foreground">
            {Array.from({ length: 24 }, (_, h) => (
              <span key={h} className={cn("whitespace-nowrap", h % 4 !== 0 && "invisible")}>
                {hourLabel(4 + h)}
              </span>
            ))}
          </div>
          <div
            ref={gridRef}
            role="group"
            aria-label={`Activity by quarter hour from 4 AM to 4 AM. ${recorded} of 96 quarter hours recorded. Arrow keys move between quarter hours; Enter opens the card in the timeline.`}
            className="grid grid-flow-col grid-cols-24 grid-rows-4 gap-[3px]"
            onKeyDown={onKeyDown}
          >
            {slots.map((s) => {
              const len = Math.max(1, s.end - s.start);
              const empty = !s.top;
              const ahead = s.future || s.skipped;
              return (
                <button
                  key={s.index}
                  type="button"
                  data-slot-index={s.index}
                  tabIndex={s.index === active ? 0 : -1}
                  aria-label={slotText(s, now)}
                  aria-disabled={empty || undefined}
                  className={cn(
                    "relative flex aspect-square min-w-0 flex-col-reverse overflow-hidden rounded-[3px] p-0",
                    // Time still ahead (or skipped by the clock): a lighter, outlined well.
                    ahead ? "bg-surface-1 ring-1 ring-border ring-inset" : "bg-surface-2",
                    !empty && "hover:ring-2 hover:ring-foreground/70",
                    // Windows Contrast themes drop the well color: keep every square's outline.
                    "forced-colors:border forced-colors:border-[GrayText]",
                  )}
                  onClick={() => s.top && onOpen(s.top.id)}
                  onFocus={(e) => {
                    setActive(s.index);
                    if (e.currentTarget.matches(":focus-visible")) showTip(s.index, e.currentTarget);
                  }}
                  onBlur={() => setTip(null)}
                  onMouseEnter={(e) => showTip(s.index, e.currentTarget)}
                >
                  {s.shares.map((x) => (
                    <span
                      key={x.name}
                      data-mark=""
                      className={cn("block w-full shrink-0", x.kind === "idle" && "hatch-idle bg-surface-1", x.kind === "system" && "stripes-failed bg-surface-1")}
                      style={{ height: `${(x.secs / len) * 100}%`, ...(x.kind === "idle" || x.kind === "system" ? {} : { background: colorFor(x.name) }) }}
                    />
                  ))}
                </button>
              );
            })}
          </div>
        </div>
        {t && tip && (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute z-20 w-max max-w-64 rounded-md bg-foreground px-3 py-2 text-caption text-background shadow-md"
            style={{
              left: tip.x,
              top: tip.y - 6,
              transform: `translate(${tip.align === "start" ? "0" : tip.align === "end" ? "-100%" : "-50%"}, -100%)`,
            }}
          >
            <p className="font-mono tnum">
              {t.skipped ? `${slotLabel(t.index)} – ${slotLabel(t.index + 1)}` : `${clock(t.start)} – ${clock(t.end)}`}
            </p>
            {t.skipped ? (
              <p className="mt-0.5">Skipped: the clock moved forward an hour</p>
            ) : t.future ? (
              <p className="mt-0.5">Not yet</p>
            ) : t.shares.length ? (
              <ul className="mt-1 flex flex-col gap-0.5">
                {t.shares.map((x) => (
                  <li key={x.name} className="flex items-center gap-1.5">
                    <CategoryMark name={x.name} kind={x.kind} colorFor={colorFor} className="ring-1 ring-background/40" />
                    <span>{shareLabel(x)}</span>
                    <span className="ml-auto pl-3 tnum">{dur(x.secs)}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-0.5">Not recorded</p>
            )}
            {t.top && <p className="mt-1 line-clamp-2 opacity-85">{t.top.title}</p>}
            {t.noted.length > 0 && <p className="mt-1 opacity-85">Distraction noted: {t.noted.join(", ")}</p>}
          </div>
        )}
      </div>
      <figcaption className="pl-[2.75rem]">
        <Legend items={legend} />
      </figcaption>
    </figure>
  );
}
