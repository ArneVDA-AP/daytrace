import { ArrowUpRight } from "lucide-react";
import type { Category } from "@/api";
import { href } from "@/lib/router";
import { clock, formatDuration } from "@/lib/time";
import type { DayStats } from "@/views/insights/api";
import { categoriesCounting, nameList, percent } from "@/views/insights/format";
import { BarList, SectionCard, StatStrip, StatTile } from "@/views/insights/parts";

/*
 * The day's numbers without a model (Dayflow's workflow chips and day summary): the stat strip,
 * distractions, focus blocks and the programs in front.
 */

function cards(n: number): string {
  return n === 1 ? "1 card" : `${n} cards`;
}

/** Recorded minus idle: the denominator of every percentage of the day. */
export function activeSecs(stats: Pick<DayStats, "activeSecs" | "capturedSecs" | "idleSecs">): number {
  return stats.activeSecs ?? Math.max(0, stats.capturedSecs - stats.idleSecs);
}

// Three tiles then two below 960 px, five in a row from there.
const third = "col-span-2 split:col-span-1";
const half = "col-span-3 split:col-span-1";

export function DayStatStrip({ stats }: { stats: DayStats }) {
  const active = activeSecs(stats);
  return (
    <StatStrip label="The day in numbers" className="grid-cols-6 split:grid-cols-5">
      <StatTile className={third} label="Focused for" value={formatDuration(stats.focusSecs)} note={`${percent(stats.focusSecs, active)} of active time`} />
      <StatTile className={third} label="Distracted for" value={formatDuration(stats.distractedSecs)} note={`${percent(stats.distractedSecs, active)} of active time`} />
      <StatTile className={third} label="Context switches" value={stats.contextSwitches} note="Changes of category" />
      <StatTile className={half} label="Interrupted" value={cards(stats.interruptions)} note="With a distraction noted inside" />
      <StatTile className={half} label="Transitioning" value={formatDuration(stats.transitionSecs)} note="Gaps between cards" />
    </StatStrip>
  );
}

export function DistractionsCard({ stats, day, isToday }: { stats: DayStats; day: string; isToday: boolean }) {
  const list = stats.distractions;
  const active = activeSecs(stats);
  return (
    <SectionCard
      title={isToday ? "Distractions so far" : "Distractions"}
      description={`${formatDuration(stats.distractedSecs)}, ${percent(stats.distractedSecs, active)} of active time, in cards that count as distraction. Distractions noted inside other cards are listed too; they stay in those cards’ time.`}
    >
      {list.length ? (
        <ol className="flex flex-col">
          {list.map((d, i) => (
            <li key={`${d.startTs}-${i}`} className="border-b last:border-0">
              <a
                href={href("timeline", { day, card: d.cardId })}
                className="group grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-0.5 rounded-sm px-1 py-2 hover:bg-surface-2 focus-visible:-outline-offset-2"
              >
                <span className="min-w-0 text-body font-medium text-pretty">{d.title || "Untitled distraction"}</span>
                <span className="flex items-center gap-1 text-meta whitespace-nowrap tnum">
                  {formatDuration(d.secs)}
                  <ArrowUpRight className="size-3.5 text-subtle-foreground group-hover:text-foreground" aria-hidden="true" />
                </span>
                <span className="col-span-2 text-caption text-muted-foreground">
                  <span className="font-mono tnum">
                    {clock(d.startTs)} – {clock(d.endTs)}
                  </span>
                  {d.withinCard ? `, during “${d.withinCard}”` : ""}
                </span>
                {d.summary && <span className="col-span-2 text-meta text-muted-foreground text-pretty">{d.summary}</span>}
              </a>
            </li>
          ))}
        </ol>
      ) : (
        <p className="text-meta text-muted-foreground">No distractions noted{isToday ? " so far" : ""}.</p>
      )}
    </SectionCard>
  );
}

/** "Work and Personal count as focus." from Settings > Categories. */
export function focusSentence(categories: Category[]): string {
  const names = categoriesCounting("focus", categories);
  if (!names.length) return "No category counts as focus.";
  return `${nameList(names)} ${names.length === 1 ? "counts" : "count"} as focus.`;
}

export function FocusCard({ stats, categories }: { stats: DayStats; categories: Category[] }) {
  const longest = stats.longestFocus;
  return (
    <SectionCard title="Focus blocks" description={`${focusSentence(categories)} Breaks under 5 minutes do not end a block.`}>
      {longest ? (
        <div className="flex flex-col gap-3">
          <div>
            <p className="text-meta text-muted-foreground">Longest focus block</p>
            <p className="font-display text-title font-semibold tnum">{formatDuration(longest.secs)}</p>
            <p className="text-caption text-muted-foreground">
              <span className="font-mono tnum">
                {clock(longest.startTs)} – {clock(longest.endTs)}
              </span>
            </p>
          </div>
          <table className="w-full text-meta">
            <caption className="sr-only">All focus blocks</caption>
            <thead>
              <tr className="border-b text-left text-muted-foreground">
                <th scope="col" className="py-1.5 font-normal">
                  Time
                </th>
                <th scope="col" className="py-1.5 text-right font-normal">
                  Length
                </th>
              </tr>
            </thead>
            <tbody>
              {stats.focusBlocks.map((b) => (
                <tr key={b.startTs} className="border-b last:border-0">
                  <td className="py-1.5 font-mono tnum">
                    {clock(b.startTs)} – {clock(b.endTs)}
                  </td>
                  <td className={b === longest || (b.startTs === longest.startTs && b.endTs === longest.endTs) ? "py-1.5 text-right font-semibold tnum" : "py-1.5 text-right tnum"}>
                    {formatDuration(b.secs)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <p className="text-meta text-muted-foreground">No focus time on this day.</p>
      )}
    </SectionCard>
  );
}

export function ProgramsCard({ stats }: { stats: DayStats }) {
  return (
    <SectionCard title="Programs in front" description="From the window in front at each screenshot.">
      <BarList
        label="Programs in front"
        marks
        items={stats.programs.map((p) => ({ key: p.name, name: p.name, secs: p.secs }))}
        empty="No program data for this day."
      />
    </SectionCard>
  );
}
