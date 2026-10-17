import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { href } from "@/lib/router";
import { formatDuration } from "@/lib/time";
import type { ReviewSummary } from "@/timeline-extras-api";
import { Panel, Swatch } from "@/views/journal/parts";

/*
 * Dayflow's timeline review summary: how much of the day was rated Focused, Neutral or Distracted
 * in the card details on the Timeline. Ratings are statuses, not categories: focused takes the
 * first chart color, neutral the neutral one, distracted the second, always with the words.
 */
const KINDS = [
  { key: "focusedSecs", label: "Focused", color: "var(--chart-1)" },
  { key: "neutralSecs", label: "Neutral", color: "var(--chart-5)" },
  { key: "distractedSecs", label: "Distracted", color: "var(--chart-2)" },
] as const;

export function Ratings({ day, summary, className }: { day: string; summary: ReviewSummary; className?: string }) {
  const total = summary.focusedSecs + summary.neutralSecs + summary.distractedSecs;
  const next = summary.unreviewedCards > 0 && summary.firstUnreviewed !== null ? summary.firstUnreviewed : null;
  return (
    <Panel
      className={className}
      title="Your ratings"
      description="How you rated this day’s cards on the Timeline. Ratings stay on this PC."
      actions={
        next !== null && (
          <Button variant="ghost" size="sm" asChild>
            <a href={href("timeline", { day, card: next })}>
              Rate the next card <ArrowRight aria-hidden="true" />
            </a>
          </Button>
        )
      }
    >
      {total === 0 ? (
        <p className="text-meta text-muted-foreground">No time rated yet. Rate a card Focused, Neutral or Distracted in its details on the Timeline.</p>
      ) : (
        <div className="flex flex-col gap-3">
          <div
            role="img"
            aria-label={KINDS.map((k) => `${k.label} ${formatDuration(summary[k.key])}`).join(", ")}
            className="flex h-2.5 w-full gap-px overflow-hidden rounded-full"
          >
            {KINDS.filter((k) => summary[k.key] > 0).map((k) => (
              <span key={k.key} data-mark="" className="block h-full min-w-0.5" style={{ flex: `${summary[k.key]} 1 0%`, background: k.color }} />
            ))}
          </div>
          <dl className="grid grid-cols-3 gap-3">
            {KINDS.map((k) => (
              <div key={k.key} className="flex flex-col gap-0.5">
                <dt className="flex items-center gap-1.5 text-caption text-muted-foreground">
                  <Swatch color={k.color} />
                  {k.label}
                </dt>
                <dd className="text-body font-medium tnum">{summary[k.key] > 0 ? formatDuration(summary[k.key]) : "None"}</dd>
              </div>
            ))}
          </dl>
        </div>
      )}
      <p className="mt-3 text-caption text-muted-foreground">
        {summary.unreviewedCards === 0
          ? total > 0
            ? "Every card of this day is rated."
            : "This day has no cards to rate."
          : `${summary.unreviewedCards} ${summary.unreviewedCards === 1 ? "card is" : "cards are"} not rated yet.`}
      </p>
    </Panel>
  );
}
