import * as React from "react";
import { CalendarDays } from "lucide-react";
import type { Category } from "@/api";
import { useAppStatusSelect } from "@/app/state";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { href } from "@/lib/router";
import { clock, formatDayTitle, formatDuration, formatWeekRange, hourLabel, weekday, weekStart } from "@/lib/time";
import { useDelayedLoading } from "@/lib/use-delayed";
import { cn } from "@/lib/utils";
import { CategoryChips } from "./CategoryChips";
import { useHeight, useWeekCards } from "./data";
import { BREAK_PX, NIGHT_FROM_HOUR, NIGHT_TO_HOUR, isIdleCard, laneKeyOf, lanesFor, scaleFor, type Lane } from "./model";
import {
  DAY_SECS,
  columnGaps,
  columnRecorded,
  firstStop,
  moveInWeek,
  nightOffsets,
  offsetTicks,
  tickHour,
  wallOffset,
  weekAxis,
  weekColumns,
  weekDays,
  type GridPos,
  type WeekItem,
} from "./week-model";

/*
 * Week mode of the Timeline (gap 18; Dayflow Views/UI/MainView/WeekTimelineGridView.swift and
 * WeekTimelineActivityCard.swift): the seven days of the shared day's week as columns on one
 * vertical time scale, every card at its time with its title, in its category color. The day
 * trace's rules hold: "Fit to recorded hours" (here over the whole week) cuts out hours without
 * a card on any day, idle is a hatch, a failed card red stripes with "Not analyzed", stretches
 * between cards without recording a light hatch ("Not recorded"), night hours are shaded, today
 * has a now line and the next-card box. Dimmed categories (the chips) dim here too.
 *
 * A card is a link that opens it in Day mode; a day's header opens that day. The cards are one
 * tab stop: ↑/↓ move within a day, ←/→ to the card nearest in time on the day before/after,
 * Home/End to the day's first/last card, Enter opens. The scale fits the window height between
 * 44 and 120 px per hour; below that the grid scrolls.
 */

const MIN_PX_PER_HOUR = 44;
const MAX_PX_PER_HOUR = 120;
const MIN_CARD_PX = 6;
/** Space above the first and below the last hour, so the hour labels are not cut. */
const PAD = 8;
/** Line height of a card's text (text-caption). */
const LINE_PX = 16;
/** Time column, then the seven days (each part a grid of seven equal columns). */
const GRID_COLS = "3.25rem minmax(0, 1fr)";

export function WeekView({
  day,
  today,
  categories,
  colorFor,
  muted,
  onToggleMute,
  fit,
  onFitChange,
  now,
  onShowDay,
}: {
  day: string;
  today: string;
  categories: Category[];
  colorFor: (category: string) => string;
  muted: Set<string>;
  onToggleMute: (key: string) => void;
  fit: boolean;
  onFitChange: (fit: boolean) => void;
  /** Wall clock in Unix seconds. */
  now: number;
  /** Goes to a day (Empty state: "Go to this week"). */
  onShowDay: (day: string) => void;
}) {
  const days = React.useMemo(() => weekDays(day), [day]);
  const data = useWeekCards(days, today);
  const cols = React.useMemo(() => (data.cards ? weekColumns(days, data.cards) : null), [days, data.cards]);
  const allCards = React.useMemo(() => data.cards?.flat() ?? [], [data.cards]);
  const lanes = React.useMemo(() => lanesFor(categories, allCards), [categories, allCards]);
  const idleKey = lanes.find((l) => l.isIdle)?.key ?? null;
  const todayCol = days.indexOf(today);
  const nowOff = todayCol >= 0 ? wallOffset(now) : null;

  const axis = React.useMemo(() => weekAxis(cols ?? [], nowOff, fit), [cols, nowOff, fit]);
  const breaks = axis.windows.length - 1;
  const cut = axis.windows.slice(1).reduce((a, w, i) => a + w.start - axis.windows[i].end, 0);

  // The scale: fill the height of the scroll area, within 44–120 px per hour.
  const [measure, viewH] = useHeight<HTMLDivElement>(600);
  const scrollRef = React.useRef<HTMLDivElement | null>(null);
  const setScrollEl = React.useCallback(
    (el: HTMLDivElement | null) => {
      scrollRef.current = el;
      measure(el);
    },
    [measure],
  );
  const hours = axis.shown / 3600;
  const fitPph = (viewH - HEADER_PX - 2 * PAD - breaks * BREAK_PX) / Math.max(1, hours);
  const pph = Math.min(MAX_PX_PER_HOUR, Math.max(MIN_PX_PER_HOUR, fitPph));
  const plotH = Math.round(hours * pph + breaks * BREAK_PX);
  const scale = scaleFor(axis, plotH);
  const y = (t: number) => {
    const v = scale.x(t);
    return v === null ? null : v + PAD;
  };
  const step = [1, 2, 3, 4, 6].find((s) => s * scale.pxPerHour >= 26) ?? 6;
  const ticks = offsetTicks(axis, step).filter((t) => !axis.windows.slice(0, -1).some((w) => w.end === t));
  const allTicks = offsetTicks(axis, 1);

  // Night bands, cut to the windows.
  const nights: { top: number; height: number; key: string }[] = [];
  for (const [s, e] of nightOffsets()) {
    for (const w of axis.windows) {
      const a = Math.max(s, w.start);
      const b = Math.min(e, w.end);
      const y0 = y(a);
      const y1 = y(b);
      if (b > a && y0 !== null && y1 !== null) nights.push({ top: y0, height: y1 - y0, key: `${a}` });
    }
  }

  const loading = cols === null && !data.error;
  const showSkeleton = useDelayedLoading(loading);
  const totals = cols?.map(columnRecorded) ?? null;
  const weekTotal = totals?.reduce((a, s) => a + s, 0) ?? 0;
  const anyCards = !!cols && cols.some((c) => c.length > 0);
  const anyGap = !!cols && cols.some((c) => columnGaps(c).length > 0);
  const anyFailed = !!cols && cols.some((c) => c.some((it) => it.failed));
  const thisWeek = weekStart(day) === weekStart(today);

  // First view of a week (and after "Fit to recorded hours" changes): the shared day's first card
  // (else now, else the week's first card) an hour from the top. Polls keep the scroll position.
  const scrolledFor = React.useRef<string | null>(null);
  const scrollKey = `${days[0]}|${fit}`;
  React.useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !cols || scrolledFor.current === scrollKey) return;
    scrolledFor.current = scrollKey;
    const own = cols[days.indexOf(day)] ?? [];
    const target = own[0]?.start ?? nowOff ?? cols.flat().sort((a, b) => a.start - b.start)[0]?.start ?? null;
    if (target === null) return;
    let t = Math.max(0, target - 3600);
    while (t < target && y(t) === null) t += 60;
    el.scrollTop = Math.max(0, (y(t) ?? 0) - PAD);
  });

  return (
    <section aria-labelledby="week-title" className="flex min-h-0 flex-1 flex-col">
      <h2 id="week-title" className="sr-only">
        Week overview
      </h2>
      <div className="flex shrink-0 flex-col gap-2 border-b px-(--page-x) py-2.5">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <CategoryChips lanes={lanes} colorFor={colorFor} muted={muted} onToggle={onToggleMute} />
          </div>
          {/* A view option that is on by default (as on the day trace); hidden when there is nothing to fit. */}
          {(cols === null || anyCards) && !data.error && (
            <label className="inline-flex h-7 shrink-0 items-center gap-2 text-meta text-foreground">
              <Switch checked={fit} onCheckedChange={onFitChange} />
              Fit to recorded hours
            </label>
          )}
        </div>
        <div className="flex min-h-4 flex-wrap items-center gap-x-4 gap-y-1 text-caption text-muted-foreground">
          {nights.length > 0 && anyCards && (
            <span className="inline-flex items-center gap-1.5">
              <span aria-hidden="true" className="inline-block h-2.5 w-3.5 rounded-[2px] border border-border bg-surface-2" />
              Night, {hourLabel(NIGHT_FROM_HOUR)} to {hourLabel(NIGHT_TO_HOUR)}
            </span>
          )}
          {anyGap && (
            <span className="inline-flex items-center gap-1.5">
              <span aria-hidden="true" className="hatch-gap inline-block h-2.5 w-3.5 rounded-[2px] border border-border" />
              Not recorded
            </span>
          )}
          {anyFailed && (
            <span className="inline-flex items-center gap-1.5">
              <span aria-hidden="true" data-mark="" className="stripes-failed inline-block h-2.5 w-3.5 rounded-[2px] border border-destructive bg-destructive-surface" />
              Not analyzed
            </span>
          )}
          {breaks > 0 && (
            <span className="inline-flex items-center gap-1.5">
              <BreakMark />
              {formatDuration(cut)} without recording on any day left out
            </span>
          )}
          {nowOff !== null && anyCards && (
            <span className="inline-flex items-center gap-1.5">
              <span aria-hidden="true" data-mark="" className="inline-block h-0.5 w-3.5 rounded-full bg-(--trace-now) forced-colors:outline-0" />
              Now {clock(now)}
            </span>
          )}
          <span>
            Recorded this week{" "}
            <span className="tnum text-foreground">{cols ? formatDuration(weekTotal) : data.error ? "–" : "…"}</span>
          </span>
        </div>
      </div>

      {data.error && cols === null ? (
        <div className="px-(--page-x) py-5">
          <Banner
            tone="error"
            title={`Couldn’t load the week of ${formatWeekRange(days[0])}`}
            details={data.error}
            actions={
              <Button size="sm" variant="outline" onClick={data.reload}>
                Try again
              </Button>
            }
          >
            Daytrace could not read these days from its database. Your recording is not affected.
          </Banner>
        </div>
      ) : cols && !anyCards ? (
        <Empty className="flex-1">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <CalendarDays aria-hidden="true" />
            </EmptyMedia>
            <EmptyTitle>{thisWeek ? "No cards yet this week" : `Nothing recorded in the week of ${formatWeekRange(days[0])}`}</EmptyTitle>
            <EmptyDescription>
              {thisWeek
                ? "Daytrace writes the first card about 15 minutes after recording starts. Cards appear here at their time of day."
                : "Daytrace was not running, or recording was paused, on every day of this week."}
            </EmptyDescription>
          </EmptyHeader>
          {!thisWeek && (
            <EmptyContent>
              <Button variant="outline" onClick={() => onShowDay(today)}>
                Go to this week
              </Button>
            </EmptyContent>
          )}
        </Empty>
      ) : (
        <div ref={setScrollEl} className="scroll-thin relative min-h-0 flex-1 overflow-y-auto overscroll-contain">
          <div className="sticky top-0 z-20 grid border-b bg-surface-1 px-(--page-x)" style={{ gridTemplateColumns: GRID_COLS, height: HEADER_PX }}>
            <span />
            <div className="grid min-w-0 grid-cols-7">
              {days.map((d, i) => (
                <DayHeader key={d} day={d} today={today} recorded={totals?.[i] ?? null} cards={cols?.[i].length ?? null} />
              ))}
            </div>
          </div>

          {showSkeleton || !cols ? (
            showSkeleton ? <GridSkeleton future={days.map((d) => d > today)} /> : null
          ) : (
            <Grid
              days={days}
              day={day}
              today={today}
              cols={cols}
              lanes={lanes}
              idleKey={idleKey}
              colorFor={colorFor}
              muted={muted}
              y={y}
              plotH={plotH}
              ticks={ticks}
              afterBreak={new Set(axis.windows.slice(1).map((w) => w.start))}
              gridTicks={allTicks}
              nights={nights}
              breaks={axis.windows.slice(0, -1).map((_, i) => scale.breakX(i) + PAD)}
              todayCol={todayCol}
              nowOff={nowOff}
              windowEnd={(t) => axis.windows.find((w) => t >= w.start && t <= w.end)?.end ?? t}
            />
          )}
        </div>
      )}
    </section>
  );
}

const HEADER_PX = 44;

function BreakMark({ vertical = false }: { vertical?: boolean }) {
  return (
    <span aria-hidden="true" className={cn("inline-flex items-center justify-center gap-[3px]", vertical ? "h-3.5 w-3 flex-col" : "h-3 w-3.5")}>
      <span className={cn("bg-muted-foreground forced-colors:bg-[CanvasText]", vertical ? "h-px w-3 -rotate-[20deg]" : "h-3 w-px rotate-[20deg]")} />
      <span className={cn("bg-muted-foreground forced-colors:bg-[CanvasText]", vertical ? "h-px w-3 -rotate-[20deg]" : "h-3 w-px rotate-[20deg]")} />
    </span>
  );
}

function DayHeader({ day, today, recorded, cards }: { day: string; today: string; recorded: number | null; cards: number | null }) {
  const isToday = day === today;
  const future = day > today;
  const date = Number(day.slice(8));
  const sub = future ? "Not yet" : recorded === null ? "" : cards === 0 ? "Not recorded" : formatDuration(recorded);
  // Today: the date in an ink circle (Dayflow's day number badge), plus the word from 1100 px.
  const inner = (
    <>
      <span className="flex min-w-0 items-center gap-1">
        <span className={cn("truncate font-medium", future ? "text-subtle-foreground" : "text-foreground")}>{weekday(day)}</span>
        <span
          className={cn(
            "inline-flex h-[18px] min-w-[18px] shrink-0 items-center justify-center rounded-full px-1 font-medium tnum",
            isToday ? "bg-foreground text-background forced-colors:border forced-colors:border-[CanvasText]" : future ? "text-subtle-foreground" : "text-foreground",
          )}
        >
          {date}
        </span>
        {isToday && <span className="truncate text-caption text-muted-foreground max-wide:hidden">Today</span>}
      </span>
      <span className="truncate text-caption text-muted-foreground tnum">{sub || "\u00a0"}</span>
    </>
  );
  const cls = "flex min-w-0 flex-col justify-center border-l px-2 text-meta leading-4";
  if (future) return <div className={cls}>{inner}</div>;
  return (
    <a
      href={href("timeline", { day })}
      className={cn(cls, "rounded-sm hover:bg-surface-2 focus-visible:-outline-offset-2")}
      aria-label={`${formatDayTitle(day)}${isToday ? ", today" : ""}${sub ? `, ${sub === "Not recorded" ? "not recorded" : `${sub} recorded`}` : ""}. Open in Day view`}
    >
      {inner}
    </a>
  );
}

function GridSkeleton({ future }: { future: boolean[] }) {
  return (
    <div aria-busy="true" aria-label="Loading the week" className="grid px-(--page-x) pt-3" style={{ gridTemplateColumns: GRID_COLS }}>
      <span />
      <div className="grid grid-cols-7">
        {future.map((f, i) => (
          <div key={i} className="flex flex-col gap-2 border-l px-1">
            {!f &&
              [56, 28, 72, 40].map((h, j) => (
                <Skeleton key={j} className="w-full rounded-[3px]" style={{ height: h, marginTop: j === 0 ? (i % 3) * 16 : 0 }} />
              ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function Grid({
  days,
  day,
  today,
  cols,
  lanes,
  idleKey,
  colorFor,
  muted,
  y,
  plotH,
  ticks,
  afterBreak,
  gridTicks,
  nights,
  breaks,
  todayCol,
  nowOff,
  windowEnd,
}: {
  days: string[];
  day: string;
  today: string;
  cols: WeekItem[][];
  lanes: Lane[];
  idleKey: string | null;
  colorFor: (category: string) => string;
  muted: Set<string>;
  y: (t: number) => number | null;
  plotH: number;
  ticks: number[];
  /** Ticks at the start of a window after an axis break. */
  afterBreak: Set<number>;
  gridTicks: number[];
  nights: { top: number; height: number; key: string }[];
  breaks: number[];
  todayCol: number;
  nowOff: number | null;
  windowEnd: (t: number) => number;
}) {
  const refs = React.useRef(new Map<string, HTMLAnchorElement>());
  const [stopId, setStopId] = React.useState<number | null>(null);
  const [tip, setTip] = React.useState<{ id: number; via: "pointer" | "focus" } | null>(null);
  const find = (id: number | null): GridPos | null => {
    if (id === null) return null;
    for (let c = 0; c < cols.length; c++) {
      const i = cols[c].findIndex((it) => it.card.id === id);
      if (i >= 0) return { col: c, i };
    }
    return null;
  };
  const stop = find(stopId) ?? firstStop(cols, days, day);
  const totalH = plotH + 2 * PAD;

  const onKeyDown = (e: React.KeyboardEvent) => {
    const t = e.target as HTMLElement;
    const col = Number(t.dataset.weekCol);
    const i = Number(t.dataset.weekI);
    if (!t.dataset.weekCol || !Number.isFinite(col) || !Number.isFinite(i)) return;
    if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
    e.preventDefault();
    const next = moveInWeek(cols, { col, i }, e.key);
    if (!next) return;
    const it = cols[next.col][next.i];
    setStopId(it.card.id);
    refs.current.get(`${next.col}:${next.i}`)?.focus();
  };

  const tipItem = tip ? cols.flat().find((it) => it.card.id === tip.id) : undefined;

  return (
    <div className="relative px-(--page-x)">
      <div className="relative grid" style={{ gridTemplateColumns: GRID_COLS, height: totalH }}>
        {/* Time column: hour labels; breaks get the // mark. */}
        <div aria-hidden="true" className="relative font-mono text-axis text-subtle-foreground">
          {ticks.map((t) => {
            const top = y(t);
            if (top === null) return null;
            // Right after an axis break the label hangs below its line, clear of the break mark.
            return (
              <span key={t} className={cn("absolute right-2 whitespace-nowrap", afterBreak.has(t) ? "translate-y-px" : "-translate-y-1/2")} style={{ top }}>
                {hourLabel(tickHour(t))}
              </span>
            );
          })}
          {breaks.map((top, i) => (
            <span key={`b${i}`} className="absolute right-3 flex items-center" style={{ top: top + BREAK_PX / 2 - 7 }}>
              <BreakMark vertical />
            </span>
          ))}
        </div>

        <div className="relative min-w-0">
          {/* Background of the seven columns: night bands, hour lines, axis breaks. */}
          <div aria-hidden="true" className="pointer-events-none absolute inset-0">
            {nights.map((n) => (
              <div key={`n${n.key}`} className="absolute inset-x-0 bg-surface-2" style={{ top: n.top, height: n.height }} />
            ))}
            {gridTicks.map((t) => {
              const top = y(t);
              return top === null ? null : <span key={`g${t}`} className="absolute inset-x-0 h-px bg-(--trace-gridline)" style={{ top: Math.round(top) }} />;
            })}
            {breaks.map((top, i) => (
              <div key={`k${i}`} className="hatch-gap absolute inset-x-0 border-y border-dashed border-border-strong bg-surface-1" style={{ top, height: BREAK_PX }} />
            ))}
          </div>

          <div
            role="group"
            aria-label="Cards of the week, one column per day. Arrow keys move between cards: up and down within a day, left and right to the day before or after. Enter opens a card in Day view."
            className="relative grid h-full grid-cols-7"
            onKeyDown={onKeyDown}
          >
            {cols.map((items, c) => (
              <Column
                key={days[c]}
                day={days[c]}
                future={days[c] > today}
                col={c}
                items={items}
                lanes={lanes}
                idleKey={idleKey}
                colorFor={colorFor}
                muted={muted}
                y={y}
                stop={stop}
                refs={refs.current}
                isToday={c === todayCol}
                nowOff={c === todayCol ? nowOff : null}
                windowEnd={windowEnd}
                onFocusItem={(id) => {
                  setStopId(id);
                  setTip({ id, via: "focus" });
                }}
                onBlurItem={(id) => setTip((cur) => (cur?.id === id && cur.via === "focus" ? null : cur))}
                onHover={(id) => setTip(id === null ? null : { id, via: "pointer" })}
                tipFor={tipItem && tipItem.col === c ? tipItem : null}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function Column({
  day,
  future,
  col,
  items,
  lanes,
  idleKey,
  colorFor,
  muted,
  y,
  stop,
  refs,
  isToday,
  nowOff,
  windowEnd,
  onFocusItem,
  onBlurItem,
  onHover,
  tipFor,
}: {
  day: string;
  future: boolean;
  col: number;
  items: WeekItem[];
  lanes: Lane[];
  idleKey: string | null;
  colorFor: (category: string) => string;
  muted: Set<string>;
  y: (t: number) => number | null;
  stop: GridPos | null;
  refs: Map<string, HTMLAnchorElement>;
  isToday: boolean;
  nowOff: number | null;
  windowEnd: (t: number) => number;
  onFocusItem: (id: number) => void;
  onBlurItem: (id: number) => void;
  onHover: (id: number | null) => void;
  tipFor: WeekItem | null;
}) {
  const recorded = columnRecorded(items);
  const gaps = columnGaps(items);
  const lastEnd = items.length ? Math.max(...items.map((it) => it.end)) : null;
  const nowY = nowOff !== null ? y(nowOff) : null;
  return (
    <div
      role="group"
      aria-label={`${formatDayTitle(day)}${isToday ? ", today" : ""}: ${items.length ? `${items.length} ${items.length === 1 ? "card" : "cards"}, ${formatDuration(recorded)} recorded` : "no cards"}`}
      className="relative min-w-0 border-l"
    >
      {gaps.map(([s, e]) => {
        const y0 = y(s);
        const y1 = y(e);
        if (y0 === null || y1 === null) return null;
        return <div key={`gap${s}`} aria-hidden="true" className="hatch-gap absolute inset-x-0" style={{ top: y0, height: y1 - y0 }} />;
      })}
      {items.length === 0 && !future && !isToday && (
        <span aria-hidden="true" className="absolute inset-x-0 top-3 text-center text-caption text-subtle-foreground">
          Not recorded
        </span>
      )}
      {items.map((it, i) => {
        const top = y(it.start);
        const bottom = y(it.end);
        if (top === null || bottom === null) return null;
        return (
          <WeekCard
            key={it.card.id}
            day={day}
            item={it}
            index={i}
            top={top}
            height={Math.max(MIN_CARD_PX, bottom - top - 1)}
            color={colorFor(it.card.category)}
            idle={!it.failed && isIdleCard(it.card, idleKey)}
            dimmed={!it.failed && muted.has(laneKeyOf(it.card, lanes))}
            tabbable={stop?.col === col && stop.i === i}
            refs={refs}
            onFocus={() => onFocusItem(it.card.id)}
            onBlur={() => onBlurItem(it.card.id)}
            onHover={onHover}
          />
        );
      })}
      {isToday && nowOff !== null && <NextCardBox lastEnd={lastEnd} nowOff={nowOff} y={y} windowEnd={windowEnd} />}
      {nowY !== null && nowY !== undefined && (
        <div aria-hidden="true" className="pointer-events-none absolute inset-x-0 z-10 h-0" style={{ top: nowY }}>
          <span data-mark="" className="absolute inset-x-0 -top-px h-0.5 rounded-full bg-(--trace-now) forced-colors:outline-0" />
          <span data-mark="" className="absolute -top-[3px] -left-[3px] size-1.5 rounded-full bg-(--trace-now) forced-colors:outline-0" />
        </div>
      )}
      {tipFor && <Tip item={tipFor} y={y} side={col >= 4 ? "left" : "right"} />}
    </div>
  );
}

function WeekCard({
  day,
  item: it,
  index,
  top,
  height,
  color,
  idle,
  dimmed,
  tabbable,
  refs,
  onFocus,
  onBlur,
  onHover,
}: {
  day: string;
  item: WeekItem;
  index: number;
  top: number;
  height: number;
  color: string;
  idle: boolean;
  dimmed: boolean;
  tabbable: boolean;
  refs: Map<string, HTMLAnchorElement>;
  onFocus: () => void;
  onBlur: () => void;
  onHover: (id: number | null) => void;
}) {
  const c = it.card;
  const key = `${it.col}:${index}`;
  const secs = it.endTs - it.startTs;
  const lines = Math.floor((height - 4) / LINE_PX);
  const title = it.failed ? (it.count > 1 ? `Not analyzed · ${it.count} intervals` : "Not analyzed") : c.title;
  const titleLines = lines >= 3 ? Math.min(3, lines - 1) : Math.max(0, Math.min(lines, 1));
  const showTime = lines >= 2;
  return (
    <a
      ref={(el) => {
        if (el) refs.set(key, el);
        else refs.delete(key);
      }}
      href={href("timeline", { day, card: c.id })}
      data-week-col={it.col}
      data-week-i={index}
      tabIndex={tabbable ? 0 : -1}
      aria-label={`${clock(it.startTs)} to ${clock(it.endTs)}, ${formatDuration(secs)}. ${it.failed ? (it.count > 1 ? `Not analyzed, ${it.count} intervals` : "Not analyzed") : `${c.category}: ${c.title}`}`}
      onFocus={onFocus}
      onBlur={onBlur}
      onPointerEnter={() => onHover(c.id)}
      onPointerLeave={() => onHover(null)}
      className={cn(
        "group absolute right-1 left-0.5 z-[1] overflow-hidden rounded-[3px] border hover:z-[2] hover:shadow-xs focus-visible:z-[3] focus-visible:outline-offset-1",
        it.failed ? "border-dashed border-destructive bg-destructive-surface" : idle ? "border-dashed border-cat-idle bg-surface-1" : "border-border bg-surface-1",
        dimmed && "opacity-30",
      )}
      style={{ top, height }}
    >
      {!it.failed && !idle && (
        <span
          aria-hidden="true"
          className="absolute inset-0 forced-colors:hidden"
          style={{ background: `color-mix(in oklab, ${color} 16%, var(--surface-1))` }}
        />
      )}
      <span
        aria-hidden="true"
        data-mark=""
        className={cn("absolute inset-y-0 left-0", it.failed ? "stripes-failed w-1.5 bg-destructive-surface" : idle ? "hatch-idle w-1.5 bg-surface-1" : "w-1")}
        style={it.failed || idle ? undefined : { background: color }}
      />
      {titleLines > 0 && (
        <span aria-hidden="true" className="relative flex flex-col py-px pr-1 pl-2.5 text-caption">
          <span className={cn("font-medium break-words", it.failed ? "text-destructive" : "text-foreground", CLAMP[titleLines])}>{title}</span>
          {showTime && <span className="truncate text-muted-foreground tnum">{clock(it.startTs)}</span>}
        </span>
      )}
    </a>
  );
}

const CLAMP: Record<number, string> = { 1: "line-clamp-1", 2: "line-clamp-2", 3: "line-clamp-3" };

function Tip({ item: it, y, side }: { item: WeekItem; y: (t: number) => number | null; side: "left" | "right" }) {
  const top = y(it.start) ?? 0;
  const c = it.card;
  return (
    <div
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute z-30 w-max max-w-64 rounded-md border bg-popover px-2.5 py-1.5 text-caption text-popover-foreground shadow-md",
        side === "right" ? "left-full ml-1" : "right-full mr-1",
      )}
      style={{ top }}
    >
      <span className="block font-mono text-muted-foreground tnum">
        {clock(it.startTs)} – {clock(it.endTs)} · {formatDuration(it.endTs - it.startTs)}
      </span>
      <span className="block font-medium">{it.failed ? (it.count > 1 ? `Not analyzed, ${it.count} intervals` : "Not analyzed") : c.title}</span>
      {!it.failed && <span className="block text-muted-foreground">{c.category}</span>}
    </div>
  );
}

const selectRecorder = (s: { recorder: { tone: string; label: string } }) => `${s.recorder.tone}|${s.recorder.label}`;

/**
 * Today's next card (Dayflow WeekTimelineStatusCard): a dashed box after the last card, about 15
 * minutes tall, saying what the recorder does. Reads the status itself, so a status poll does not
 * re-render the grid.
 */
function NextCardBox({ lastEnd, nowOff, y, windowEnd }: { lastEnd: number | null; nowOff: number; y: (t: number) => number | null; windowEnd: (t: number) => number }) {
  const [tone, label] = useAppStatusSelect(selectRecorder).split("|");
  const start = lastEnd !== null && lastEnd > nowOff - 2 * 3600 ? lastEnd : Math.max(0, nowOff - 450);
  const end = Math.min(DAY_SECS, windowEnd(start), start + 15 * 60);
  const y0 = y(start);
  const y1 = y(end);
  if (y0 === null || y1 === null || y1 - y0 < 4) return null;
  const live = tone === "live";
  return (
    <div
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute right-1 left-0.5 overflow-hidden rounded-[3px] border border-dashed px-1.5 text-caption",
        live ? "border-border-strong bg-surface-1 text-foreground" : "border-border bg-surface-2 text-muted-foreground",
      )}
      style={{ top: y0, height: Math.max(MIN_CARD_PX, y1 - y0 - 1) }}
    >
      {y1 - y0 >= 18 && <span className="block truncate leading-4 font-medium">{live ? "Next card…" : label}</span>}
    </div>
  );
}
