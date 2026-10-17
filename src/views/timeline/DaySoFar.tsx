import * as React from "react";
import { MousePointerClick } from "lucide-react";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Skeleton } from "@/components/ui/skeleton";
import { href } from "@/lib/router";
import { clock, dayBounds, formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { Card, DayExtras, DayStatsLite, GoalsLite } from "./api";
import { laneTotals, type Lane } from "./model";

/*
 * "Your day so far" (Dayflow Views/Components/DaySummaryView.swift and its sections), shown in the
 * detail pane while no card is selected: where the time went per category (a 100 % bar plus the
 * numbers), recorded time, focus total and the longest focus block, distractions so far, and the
 * review summary. The day's targets are shown once, above the card list (Targets.tsx), with a
 * link to Goals here. Category times come from the cards (overlaps counted once, the same numbers
 * as the trace's lane totals); focus and distraction from insights_day. Every percentage of the
 * day is "of active time": recorded minus idle (the glossary in src/design/README.md).
 */

function Stat({ label, value, sub }: { label: string; value: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-md border bg-surface-1 px-3 py-2.5">
      <dt className="text-caption text-muted-foreground">{label}</dt>
      <dd className="font-display text-title font-semibold tnum">{value}</dd>
      {sub && <dd className="min-h-4 truncate text-caption text-muted-foreground">{sub}</dd>}
    </div>
  );
}

export function DaySoFar({
  day,
  isToday,
  cards,
  lanes,
  colorFor,
  stats,
  statsError,
  reloadStats,
  goals,
  extras,
  onSelect,
  onRate,
}: {
  day: string;
  isToday: boolean;
  cards: Card[];
  lanes: Lane[];
  colorFor: (name: string) => string;
  stats: DayStatsLite | null;
  statsError: string | null;
  reloadStats: () => void;
  goals: GoalsLite | null;
  extras: DayExtras | null;
  onSelect: (id: number) => void;
  /** Jump to the next card to rate (with its rating focused). */
  onRate?: () => void;
}) {
  const [from, to] = dayBounds(day);
  const totals = React.useMemo(() => laneTotals(cards, lanes, from, to), [cards, lanes, from, to]);
  const shares = lanes.map((l) => ({ lane: l, secs: totals.get(l.key) ?? 0 })).filter((s) => s.secs > 0);
  const captured = shares.reduce((a, s) => a + s.secs, 0);
  // Shares are of active time (the glossary's denominator); Idle keeps its duration, no share.
  const activeShare = shares.reduce((a, s) => a + (s.lane.isIdle ? 0 : s.secs), 0);
  const review = extras?.reviewSummary;
  const reviewed = review ? review.focusedSecs + review.neutralSecs + review.distractedSecs : 0;
  const longest = stats?.longestFocus ?? null;
  const longestCard = longest ? cards.find((c) => c.startTs <= longest.startTs && c.endTs > longest.startTs) : undefined;
  const active = stats ? Math.max(0, stats.capturedSecs - stats.idleSecs) : 0;
  const distractedPct = stats && active > 0 ? Math.round((stats.distractedSecs / active) * 100) : 0;

  return (
    <section aria-labelledby="dsf-title" className="mx-auto flex w-full max-w-[52rem] flex-col gap-6 px-6 pt-5 pb-10 max-split:px-4">
      <header className="flex flex-col gap-1">
        <h2 id="dsf-title" className="font-display text-title font-semibold tracking-[-0.01em]">
          {isToday ? "Your day so far" : "Your day"}
        </h2>
        <p className="text-meta text-muted-foreground">
          <MousePointerClick className="mr-1.5 inline size-3.5 align-[-2px]" aria-hidden="true" />
          Select a card to see what happened and its screenshots. <Kbd>J</Kbd> and <Kbd>K</Kbd> step through the cards.
        </p>
      </header>

      {statsError && (
        <div className="flex flex-col gap-2">
          <Banner
            tone="error"
            title="Couldn’t work out this day’s numbers"
            details={statsError}
            actions={
              <Button size="sm" variant="outline" onClick={reloadStats}>
                Try again
              </Button>
            }
          >
            The cards are fine; only the focus and distraction totals are missing.
          </Banner>
        </div>
      )}

      <dl className="grid grid-cols-2 gap-2 xwide:grid-cols-4">
        <Stat label="Recorded" value={formatDuration(captured)} sub={`${cards.length} ${cards.length === 1 ? "card" : "cards"}`} />
        {stats ? (
          <>
            <Stat label="Focus" value={formatDuration(stats.focusSecs)} sub={`${stats.focusBlocks.length} ${stats.focusBlocks.length === 1 ? "block" : "blocks"}`} />
            <Stat
              label="Longest focus block"
              value={formatDuration(longest ? longest.secs : 0)}
              sub={longest ? `${clock(longest.startTs)} – ${clock(longest.endTs)}` : "No focus block yet"}
            />
            <Stat
              label="Distracted"
              value={formatDuration(stats.distractedSecs)}
              sub={`${distractedPct}% of active time · ${stats.interruptions} ${stats.interruptions === 1 ? "interruption" : "interruptions"}`}
            />
          </>
        ) : (
          ["Focus", "Longest focus block", "Distracted"].map((label) => (
            <Stat key={label} label={label} value={<Skeleton className="mt-1 h-6 w-24" />} sub={<Skeleton className="mt-1 h-3 w-16" />} />
          ))
        )}
      </dl>

      <section aria-labelledby="dsf-share" className="flex flex-col gap-2.5">
        <h3 id="dsf-share" className="text-body font-semibold">
          Where the time went
        </h3>
        {captured === 0 ? (
          <p className="text-meta text-muted-foreground">No activity on this day yet.</p>
        ) : (
          <>
            <div aria-hidden="true" className="flex h-3 w-full gap-0.5 overflow-hidden rounded-[3px]">
              {shares.map((s) => (
                <span
                  key={s.lane.key}
                  data-mark=""
                  className={cn("h-full first:rounded-l-[3px] last:rounded-r-[3px]", s.lane.isIdle && "hatch-idle border border-cat-idle bg-surface-1")}
                  style={{ width: `${(s.secs / captured) * 100}%`, ...(s.lane.isIdle ? {} : { background: colorFor(s.lane.name) }) }}
                />
              ))}
            </div>
            <table className="w-full text-meta">
              <caption className="sr-only">Time per category</caption>
              <thead className="sr-only">
                <tr>
                  <th scope="col">Category</th>
                  <th scope="col">Time</th>
                  <th scope="col">Share of active time</th>
                </tr>
              </thead>
              <tbody>
                {shares.map((s) => (
                  <tr key={s.lane.key} className="border-b last:border-b-0">
                    <th scope="row" className="py-1.5 text-left font-normal">
                      <span className="inline-flex items-center gap-2">
                        <span
                          aria-hidden="true"
                          data-mark=""
                          className={cn("size-2.5 rounded-[2px]", s.lane.isIdle && "hatch-idle border border-cat-idle bg-surface-1")}
                          style={s.lane.isIdle ? undefined : { background: colorFor(s.lane.name) }}
                        />
                        {s.lane.name}
                      </span>
                    </th>
                    <td className="py-1.5 text-right tnum">{formatDuration(s.secs)}</td>
                    <td className="w-16 py-1.5 text-right text-muted-foreground tnum">
                      {s.lane.isIdle || activeShare === 0 ? "" : `${Math.round((s.secs / activeShare) * 100)}%`}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <th scope="row" className="pt-2 text-left font-normal text-muted-foreground">
                    Active time
                  </th>
                  <td className="pt-2 text-right tnum">{formatDuration(activeShare)}</td>
                  <td className="w-16 pt-2 text-right text-muted-foreground tnum">{activeShare > 0 ? "100%" : ""}</td>
                </tr>
              </tfoot>
            </table>
          </>
        )}
      </section>

      {goals && !goals.plan.isSkipped && !goals.prompt && (
        <p className="text-meta text-muted-foreground">
          {isToday ? "Today’s targets" : "The targets of this day"} are above the cards.{" "}
          <a href={href("journal", { day })} className="text-foreground underline underline-offset-2">
            Change them in Goals
          </a>
        </p>
      )}

      {review && (
        <section aria-labelledby="dsf-review" className="flex flex-col gap-2.5">
          <h3 id="dsf-review" className="text-body font-semibold">
            Your review
          </h3>
          <p className="text-meta text-muted-foreground">
            {review.lastReviewedAt ? `Last reviewed at ${clock(review.lastReviewedAt)}.` : "No reviews yet."}
            {review.unreviewedCards > 0 &&
              ` ${review.unreviewedCards} ${review.unreviewedCards === 1 ? "card is" : "cards are"} not rated Focused, Neutral or Distracted yet.`}
          </p>
          {reviewed > 0 && (
            <dl className="grid grid-cols-3 gap-2">
              {[
                ["Focused", review.focusedSecs],
                ["Neutral", review.neutralSecs],
                ["Distracted", review.distractedSecs],
              ].map(([label, secs]) => (
                <div key={label as string} className="flex flex-col rounded-md border px-3 py-2">
                  <dt className="text-caption text-muted-foreground">{label}</dt>
                  <dd className="text-body font-semibold tnum">{formatDuration(secs as number)}</dd>
                </div>
              ))}
            </dl>
          )}
          {review.firstUnreviewed !== null && review.unreviewedCards > 0 && (
            <div>
              <Button variant="outline" size="sm" onClick={() => (onRate ? onRate() : onSelect(review.firstUnreviewed!))}>
                Rate the next card
              </Button>
            </div>
          )}
        </section>
      )}

      {longestCard && (
        <p className="text-meta text-muted-foreground">
          The longest focus block started with{" "}
          <button type="button" className="font-medium text-foreground underline underline-offset-2" onClick={() => onSelect(longestCard.id)}>
            {longestCard.title}
          </button>
          .
        </p>
      )}
    </section>
  );
}
