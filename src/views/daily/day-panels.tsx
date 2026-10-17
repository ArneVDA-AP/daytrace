import { ArrowUpRight } from "lucide-react";
import { href } from "@/lib/router";
import { clock, formatDuration } from "@/lib/time";
import type { DayStats } from "@/views/insights/api";
import { percent } from "@/views/insights/format";
import { BarList, SectionCard, StatStrip, StatTile } from "@/views/insights/parts";

/*
 * The day's numbers without a model (Dayflow's workflow chips and day summary): the stat strip,
 * distractions, focus blocks and the programs in front.
 */

function times(n: number): string {
  return n === 1 ? "1 time" : `${n} times`;
}

// Three tiles then two below 960 px, five in a row from there.
const third = "col-span-2 split:col-span-1";
const half = "col-span-3 split:col-span-1";

export function DayStatStrip({ stats }: { stats: DayStats }) {
  const active = stats.focusSecs + stats.distractedSecs;
  return (
    <StatStrip label="The day in numbers" className="grid-cols-6 split:grid-cols-5">
      <StatTile className={third} label="Focused for" value={formatDuration(stats.focusSecs)} note={`${percent(stats.focusSecs, active)} of active time`} />
      <StatTile className={third} label="Distracted for" value={formatDuration(stats.distractedSecs)} note={`${percent(stats.distractedSecs, active)} of active time`} />
      <StatTile className={third} label="Context switched" value={times(stats.contextSwitches)} note="Changes of category" />
      <StatTile className={half} label="Interrupted" value={times(stats.interruptions)} note="Cards with a distraction noted" />
      <StatTile className={half} label="Transitioning" value={formatDuration(stats.transitionSecs)} note="Gaps between cards" />
    </StatStrip>
  );
}

export function DistractionsCard({ stats, day, isToday }: { stats: DayStats; day: string; isToday: boolean }) {
  const list = stats.distractions;
  return (
    <SectionCard
      title={isToday ? "Distractions so far" : "Distractions"}
      description={`${formatDuration(stats.distractedSecs)} of ${formatDuration(stats.capturedSecs)} recorded (${percent(stats.distractedSecs, stats.capturedSecs)}), from Distraction cards and distractions noted inside other cards.`}
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

export function FocusCard({ stats }: { stats: DayStats }) {
  const longest = stats.longestFocus;
  return (
    <SectionCard title="Focus blocks" description="Every category except Distraction and Idle counts as focus. Breaks under 5 minutes do not end a block.">
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
        items={stats.programs.map((p) => ({ key: p.name, name: p.name, secs: p.secs }))}
        empty="No program data for this day."
      />
    </SectionCard>
  );
}
