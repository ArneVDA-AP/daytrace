import { href } from "@/lib/router";
import { daysBetween, formatDayTitle, formatDuration, weekday } from "@/lib/time";
import { cn } from "@/lib/utils";

/*
 * A card reference: category edge, start time (with the day when it is not today), title and
 * length. It is a link to the card on the Timeline, so Back returns here. Used for the citations
 * in chat answers and for the evidence under a goal.
 */

const shortDate = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short" });

/** Minutes after midnight of a backend clock string ("9:20 AM"), or null. */
export function clockMinutes(s: string): number | null {
  const m = /^(\d{1,2}):(\d{2})\s*([AaPp])[Mm]$/.exec(s.trim());
  if (!m) return null;
  const h = Number(m[1]) % 12;
  return (m[3].toUpperCase() === "P" ? h + 12 : h) * 60 + Number(m[2]);
}

/** Seconds between two clock strings of one card (a card may run past midnight). */
export function clockSpanSecs(start: string, end: string): number | null {
  const a = clockMinutes(start);
  const b = clockMinutes(end);
  if (a === null || b === null) return null;
  return ((b - a + 1440) % 1440) * 60;
}

/** "9:20 AM" today, "Thu 9:20 AM" this week, "2 Oct 9:20 AM" before that. */
export function chipWhen(day: string, today: string, start: string): string {
  if (day === today) return start;
  const ago = daysBetween(day, today);
  if (ago > 0 && ago < 7) return `${weekday(day)} ${start}`;
  const [y, m, d] = day.split("-").map(Number);
  return `${shortDate.format(new Date(y, m - 1, d))} ${start}`;
}

export interface CardRef {
  /** The card to open; null when it no longer exists (the chip opens the day). */
  cardId: number | null;
  day: string;
  title: string;
  /** Category name, or null when unknown (neutral edge). */
  category: string | null;
  start: string;
  end: string;
  secs?: number | null;
}

export function CardChip({
  card,
  today,
  colorFor,
  showDay = true,
  className,
}: {
  card: CardRef;
  today: string;
  colorFor: (category: string) => string;
  /** False where the day is clear from the page (the Goals page of that day). */
  showDay?: boolean;
  className?: string;
}) {
  const secs = card.secs ?? clockSpanSecs(card.start, card.end);
  const when = showDay ? chipWhen(card.day, today, card.start) : card.start;
  const gone = card.cardId === null;
  const tip = [
    card.category,
    `${formatDayTitle(card.day)}, ${card.start} to ${card.end}`,
    card.title,
    gone ? "This card was replaced; opens the day on the Timeline" : "Opens the card on the Timeline",
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <a
      href={href("timeline", { day: card.day, card: card.cardId })}
      title={tip}
      data-slot="card-chip"
      className={cn(
        "inline-flex h-6 max-w-full min-w-0 items-center gap-1.5 rounded-sm border bg-surface-1 pr-2 pl-1 align-baseline text-caption leading-4 text-foreground no-underline shadow-xs",
        "transition-[background-color,border-color] duration-(--dur-fast) hover:border-input hover:bg-surface-2",
        className,
      )}
    >
      <span
        aria-hidden="true"
        data-mark=""
        className={cn("h-3.5 w-1 shrink-0 rounded-full", !card.category && "bg-chart-5")}
        style={card.category ? { background: colorFor(card.category) } : undefined}
      />
      <span className="shrink-0 font-mono text-muted-foreground tnum">{when}</span>
      <span className="min-w-0 truncate">{card.title}</span>
      {secs !== null && secs > 0 && <span className="shrink-0 text-subtle-foreground tnum">{formatDuration(secs)}</span>}
      <span className="sr-only">
        {card.category ? `, ${card.category}` : ""}, until {card.end}
      </span>
    </a>
  );
}
