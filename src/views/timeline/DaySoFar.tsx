import * as React from "react";
import { MousePointerClick, TriangleAlert } from "lucide-react";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Skeleton } from "@/components/ui/skeleton";
import { href } from "@/lib/router";
import { clock, dayBounds, formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { Card, DayExtras, DayStatsLite, GoalsLite } from "./api";
import { laneTotals, type Lane } from "./model";
import { Targets } from "./Targets";

/*
 * "Your day so far" (Dayflow Views/Components/DaySummaryView.swift and its sections), shown in the
 * detail pane while no card is selected: where the time went per category (a 100 % bar plus the
 * numbers), time captured, focus total and the longest focus block, distractions so far, today's
 * targets and the review summary. Category times come from the cards (overlaps counted once, the
 * same numbers as the trace's lane totals); focus and distraction from insights_day.
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
}) {
  const [from, to] = dayBounds(day);
  const totals = React.useMemo(() => laneTotals(cards, lanes, from, to), [cards, lanes, from, to]);
  const shares = lanes.map((l) => ({ lane: l, secs: totals.get(l.key) ?? 0 })).filter((s) => s.secs > 0);
  const captured = shares.reduce((a, s) => a + s.secs, 0);
  const review = extras?.reviewSummary;
  const reviewed = review ? review.focusedSecs + review.neutralSecs + review.distractedSecs : 0;
  const longest = stats?.longestFocus ?? null;
  const longestCard = longest ? cards.find((c) => c.startTs <= longest.startTs && c.endTs > longest.startTs) : undefined;

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
            icon={<TriangleAlert />}
            title="Couldn’t work out this day’s numbers"
            actions={
              <Button size="sm" variant="outline" onClick={reloadStats}>
                Try again
              </Button>
            }
          >
            The cards are fine; only the focus and distraction totals are missing.
          </Banner>
          <details className="px-1 text-meta text-muted-foreground">
            <summary className="w-fit cursor-default rounded-sm select-none hover:text-foreground">Error details</summary>
            <pre className="mt-1.5 overflow-x-auto rounded-md border bg-surface-2 p-3 font-mono text-caption whitespace-pre-wrap">{statsError}</pre>
          </details>
        </div>
      )}

      <dl className="grid grid-cols-2 gap-2 xwide:grid-cols-4">
        <Stat label="Time captured" value={formatDuration(captured)} sub={`${cards.length} ${cards.length === 1 ? "card" : "cards"}`} />
        {stats ? (
          <>
            <Stat label="Focus" value={formatDuration(stats.focusSecs)} sub={`${stats.focusBlocks.length} ${stats.focusBlocks.length === 1 ? "block" : "blocks"}`} />
            <Stat
              label="Longest focus"
              value={longest ? formatDuration(longest.secs) : "–"}
              sub={longest ? `${clock(longest.startTs)} – ${clock(longest.endTs)}` : "No focus block yet"}
            />
            <Stat
              label="Distracted"
              value={formatDuration(stats.distractedSecs)}
              sub={`${Math.round(stats.distractedRatio * 100)} % of captured · ${stats.interruptions} ${stats.interruptions === 1 ? "interruption" : "interruptions"}`}
            />
          </>
        ) : (
          ["Focus", "Longest focus", "Distracted"].map((label) => (
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
                  <th scope="col">Share</th>
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
                    <td className="w-16 py-1.5 text-right text-muted-foreground tnum">{Math.round((s.secs / captured) * 100)} %</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </section>

      {goals && !goals.plan.isSkipped && (
        <section aria-labelledby="dsf-targets" className="flex flex-col gap-2.5">
          <div className="flex items-baseline justify-between gap-3">
            <h3 id="dsf-targets" className="text-body font-semibold">
              {isToday ? "Today’s targets" : "Targets"}
            </h3>
            <a href={href("journal", { day })} className="text-meta text-muted-foreground underline underline-offset-2 hover:text-foreground">
              Change in Goals
            </a>
          </div>
          <Targets goals={goals} day={day} />
        </section>
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
              <Button variant="outline" size="sm" onClick={() => onSelect(review.firstUnreviewed!)}>
                Review the next card
              </Button>
            </div>
          )}
        </section>
      )}

      {longestCard && (
        <p className="text-meta text-muted-foreground">
          Longest focus started with{" "}
          <button type="button" className="font-medium text-foreground underline underline-offset-2" onClick={() => onSelect(longestCard.id)}>
            {longestCard.title}
          </button>
          .
        </p>
      )}
    </section>
  );
}
