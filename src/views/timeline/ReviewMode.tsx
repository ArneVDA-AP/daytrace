import * as React from "react";
import { ArrowLeft, ArrowRight, ListChecks, X } from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { DialogOverlay, DialogPortal } from "@/components/ui/dialog";
import { canFocus, useOpener, type ReturnFocus } from "@/components/ui/focus-return";
import { Kbd } from "@/components/ui/kbd";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { digitOf } from "@/lib/keys";
import { href } from "@/lib/router";
import { clock, formatDayTitle, formatDuration } from "@/lib/time";
import { useDelayedLoading } from "@/lib/use-delayed";
import { cn, errorText } from "@/lib/utils";
import { timelineApi, type Card, type DayExtras, type ReviewRating } from "./api";
import { isFailed, isIdleCard, reviewQueue } from "./model";
import { Timelapse } from "./Timelapse";

/*
 * Review mode (gap 20; Dayflow Views/UI/TimelineReviewOverlay.swift, TimelineReviewCard.swift,
 * TimelineReviewChrome.swift): a layer over the whole window that walks through the day's cards
 * that are not rated yet, one at a time, playing each card's screenshots (the Timelapse, started
 * by itself as on Dayflow's review card). 1, 2 and 3 rate the card Focused, Neutral or Distracted
 * and move on; ← goes back one card (Dayflow's Undo) and → skips one; Space plays and pauses; Esc
 * closes. At the end a summary shows the day's time per rating. Dayflow's swipe gestures become
 * the buttons and keys; the list of cards is taken when the mode opens (Dayflow loadActivities).
 *
 * URL: #/timeline?day=…&review=1. Opened from "N cards to rate" and the command menu.
 */

const RATINGS: { id: ReviewRating; label: string; color: string; key: string }[] = [
  { id: "focused", label: "Focused", color: "var(--chart-1)", key: "1" },
  { id: "neutral", label: "Neutral", color: "var(--chart-5)", key: "2" },
  { id: "distracted", label: "Distracted", color: "var(--chart-2)", key: "3" },
];

export function ReviewMode({
  open,
  day,
  cards,
  extras,
  error,
  onReload,
  onRated,
  colorFor,
  idleKey,
  onClose,
  returnFocus,
}: {
  open: boolean;
  day: string;
  /** The day's cards; null while they load. */
  cards: Card[] | null;
  /** The day's ratings and counts; null while they load. */
  extras: DayExtras | null;
  /** The cards or the ratings could not be loaded. */
  error: string | null;
  onReload: () => void;
  /** A card was rated: the day's ratings and counts are read again. */
  onRated: () => void;
  colorFor: (category: string) => string;
  idleKey: string | null;
  onClose: () => void;
  /** Where focus goes on close when the control that opened it is gone. */
  returnFocus?: ReturnFocus;
}) {
  const opener = useOpener(open);
  const contentRef = React.useRef<HTMLDivElement>(null);
  return (
    <DialogPrimitive.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogPortal>
        <DialogOverlay className="bg-background" />
        <DialogPrimitive.Content
          ref={contentRef}
          aria-describedby="review-keys"
          className="fixed inset-0 z-50 flex flex-col bg-background text-foreground outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0"
          onOpenAutoFocus={(e) => {
            // The layer itself takes the focus, so Space plays: a focused rating button would rate.
            e.preventDefault();
            contentRef.current?.focus();
          }}
          onCloseAutoFocus={(e) => {
            const el = opener.ref.current;
            const target = canFocus(el) ? el : returnFocus?.();
            if (canFocus(target)) {
              e.preventDefault();
              target.focus();
            }
          }}
        >
          <ReviewBody
            key={day}
            layer={contentRef}
            day={day}
            cards={cards}
            extras={extras}
            error={error}
            onReload={onReload}
            onRated={onRated}
            colorFor={colorFor}
            idleKey={idleKey}
          />
        </DialogPrimitive.Content>
      </DialogPortal>
    </DialogPrimitive.Root>
  );
}

function ReviewBody({
  layer,
  day,
  cards,
  extras,
  error,
  onReload,
  onRated,
  colorFor,
  idleKey,
}: {
  layer: React.RefObject<HTMLDivElement | null>;
  day: string;
  cards: Card[] | null;
  extras: DayExtras | null;
  error: string | null;
  onReload: () => void;
  onRated: () => void;
  colorFor: (category: string) => string;
  idleKey: string | null;
}) {
  const ready = cards !== null && extras !== null && extras.day === day;
  const [queue, setQueue] = React.useState<Card[] | null>(null);
  const [index, setIndex] = React.useState(0);
  const [rated, setRated] = React.useState<Map<number, ReviewRating>>(() => new Map());
  const [busy, setBusy] = React.useState(false);
  const [rateError, setRateError] = React.useState<string | null>(null);
  const [toggle, setToggle] = React.useState(0);
  const [announce, setAnnounce] = React.useState("");

  const coverage = React.useMemo(() => new Map((extras?.reviews ?? []).map((r) => [r.cardId, r.coverage])), [extras]);
  // The list is taken once, when the cards and their ratings are known (Dayflow loadActivities).
  React.useEffect(() => {
    if (queue === null && ready) setQueue(reviewQueue(cards, coverage));
  }, [queue, ready, cards, coverage]);

  const latest = (c: Card) => cards?.find((x) => x.id === c.id) ?? c;
  const card = queue && index < queue.length ? latest(queue[index]) : null;
  const done = queue !== null && index >= queue.length;
  const ratable = (cards ?? []).filter((c) => !isFailed(c)).length;
  const current = card ? (rated.get(card.id) ?? extras?.reviews.find((r) => r.cardId === card.id)?.rating ?? null) : null;

  const describe = (c: Card, i: number, n: number) => `Card ${i + 1} of ${n}: ${c.start} to ${c.end}, ${c.title}.`;
  const go = (i: number, prefix = "") => {
    if (!queue) return;
    const next = Math.max(0, Math.min(queue.length, i));
    setIndex(next);
    setRateError(null);
    setAnnounce(prefix + (next < queue.length ? describe(latest(queue[next]), next, queue.length) : "That was the last card."));
  };

  const busyRef = React.useRef(false);
  const rate = async (r: ReviewRating) => {
    if (!card || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setRateError(null);
    try {
      await timelineApi.rateCard(card.id, r);
      setRated((m) => new Map(m).set(card.id, r));
      onRated();
      go(index + 1, `Rated ${RATINGS.find((x) => x.id === r)!.label}. `);
    } catch (e) {
      setRateError(errorText(e));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const rateAgain = () => {
    if (!cards || !extras) return;
    const next = reviewQueue(cards, coverage).filter((c) => !rated.has(c.id));
    setQueue(next);
    setIndex(0);
    setAnnounce(next.length ? describe(next[0], 0, next.length) : "");
  };

  // Keys of the layer, read on the document: they also arrive when the focus fell back to <body>
  // (a focused control that turns disabled, such as the Timelapse's "Back to the first frame").
  // Handlers inside the layer run first (the Timelapse's Space); the full-size viewer keeps its
  // keys to itself.
  const onKeyDown = (e: KeyboardEvent) => {
    const t = e.target as HTMLElement;
    if (t !== document.body && !layer.current?.contains(t)) return;
    // The window's shortcuts (J/K, [ and ], Ctrl+K) stay out while the review is open.
    e.stopPropagation();
    // (isTypingTarget counts the whole dialog as typing; this layer has no text fields.)
    if (e.defaultPrevented || e.ctrlKey || e.altKey || e.metaKey || t.closest('textarea, select, [contenteditable="true"], input:not([type="range"])')) return;
    const d = digitOf(e);
    if (d !== null && d >= 1 && d <= 3 && card) {
      e.preventDefault();
      void rate(RATINGS[d - 1].id);
      return;
    }
    // Widgets that own their arrows: the scrubber and the speed buttons.
    const ownsArrows = (t instanceof HTMLInputElement && t.type === "range") || !!t.closest('[data-slot="toggle-group"]');
    if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && !ownsArrows && queue) {
      e.preventDefault();
      go(index + (e.key === "ArrowRight" ? 1 : -1));
    } else if (e.key === " " && !(t instanceof HTMLButtonElement) && !(t instanceof HTMLAnchorElement) && card) {
      e.preventDefault();
      setToggle((n) => n + 1);
    }
  };

  const keyRef = React.useRef(onKeyDown);
  keyRef.current = onKeyDown;
  React.useEffect(() => {
    const on = (e: KeyboardEvent) => keyRef.current(e);
    document.addEventListener("keydown", on);
    return () => document.removeEventListener("keydown", on);
  }, []);

  const showSkeleton = useDelayedLoading(!ready && !error);
  const total = queue?.length ?? 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-b px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <DialogPrimitive.Title className="truncate text-body font-semibold">Rate the cards of {formatDayTitle(day)}</DialogPrimitive.Title>
          <p id="review-keys" className="truncate text-caption text-muted-foreground">
            1 Focused, 2 Neutral, 3 Distracted · ← and → move between cards · Space plays and pauses · Esc closes
          </p>
        </div>
        {queue && total > 0 && (
          <div className="flex items-center gap-2 text-meta text-muted-foreground">
            <span className="tnum">{done ? `${total} of ${total}` : `Card ${index + 1} of ${total}`}</span>
            <span aria-hidden="true" className="h-1.5 w-24 overflow-hidden rounded-full bg-surface-3 forced-colors:border forced-colors:border-[CanvasText]">
              <span data-mark="" className="block h-full rounded-full bg-foreground" style={{ width: `${(Math.min(index, total) / total) * 100}%` }} />
            </span>
          </div>
        )}
        <Tooltip>
          <TooltipTrigger asChild>
            <DialogPrimitive.Close asChild>
              <Button variant="ghost" size="icon-sm" aria-label="Close review" aria-keyshortcuts="Escape">
                <X aria-hidden="true" />
              </Button>
            </DialogPrimitive.Close>
          </TooltipTrigger>
          <TooltipContent>
            Close <Kbd className="ml-1">Esc</Kbd>
          </TooltipContent>
        </Tooltip>
      </header>
      <p role="status" className="sr-only">
        {announce}
      </p>

      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto">
        {error && !ready ? (
          <div className="mx-auto max-w-[44rem] px-6 py-8">
            <Banner
              tone="error"
              title={`Couldn’t load the cards of ${formatDayTitle(day)}`}
              details={error}
              actions={
                <Button size="sm" variant="outline" onClick={onReload}>
                  Try again
                </Button>
              }
            >
              Daytrace could not read this day from its database. Your ratings are not affected.
            </Banner>
          </div>
        ) : !ready || queue === null ? (
          showSkeleton ? <ReviewSkeleton /> : null
        ) : ratable === 0 ? (
          <Centered>
            <h2 className="font-display text-title font-semibold">Nothing to rate on {formatDayTitle(day)}</h2>
            <p className="max-w-[52ch] text-meta text-muted-foreground">
              {cards.length === 0 ? "Come back after a few timeline cards appear." : "This day has only cards that were not analyzed. Try them again on the Timeline first."}
            </p>
            <DialogPrimitive.Close asChild>
              <Button>Close</Button>
            </DialogPrimitive.Close>
          </Centered>
        ) : done || !card ? (
          <Summary day={day} extras={extras} rated={rated} queue={queue} cards={cards} coverage={coverage} onBack={total > 0 ? () => go(total - 1) : undefined} onRateAgain={rateAgain} />
        ) : (
          <div
            key={card.id}
            className="mx-auto grid w-full max-w-[96rem] gap-x-8 gap-y-5 px-6 py-5 max-split:px-4 wide:grid-cols-[minmax(18rem,26rem)_minmax(0,1fr)] motion-safe:animate-in motion-safe:fade-in-0"
          >
            <CardInfo card={card} colorFor={colorFor} idleKey={idleKey} current={current} partly={!rated.has(card.id) && (coverage.get(card.id) ?? 0) > 0} day={day} />
            <section aria-label="Screenshots" className="min-w-0">
              <Timelapse key={card.id} card={card} autoPlay arrowKeys={false} strip={false} toggleToken={toggle} className="max-w-[60rem]" />
            </section>
          </div>
        )}
      </div>

      {card && !done && (
        <footer className="flex shrink-0 flex-col items-center gap-2 border-t px-4 py-3">
          {rateError && (
            <p role="alert" className="text-meta text-destructive">
              Couldn’t save the rating: {rateError}
            </p>
          )}
          <div className="flex flex-wrap items-center justify-center gap-2">
            {/* aria-disabled, not disabled: a focused button that turns disabled drops the focus out of the layer. */}
            <Button
              variant="ghost"
              onClick={() => index > 0 && go(index - 1)}
              aria-disabled={index === 0 || undefined}
              aria-keyshortcuts="ArrowLeft"
              className="aria-disabled:cursor-default aria-disabled:opacity-50"
            >
              <ArrowLeft aria-hidden="true" /> Previous card
            </Button>
            <div role="group" aria-label="Rate this card and go to the next" className="flex flex-wrap justify-center gap-2">
              {RATINGS.map((r) => (
                <Button
                  key={r.id}
                  variant={current === r.id ? "default" : "outline"}
                  aria-pressed={current === r.id}
                  aria-keyshortcuts={r.key}
                  aria-disabled={busy || undefined}
                  onClick={() => void rate(r.id)}
                  className="min-w-[8.5rem] aria-disabled:cursor-default"
                >
                  {busy ? (
                    <Spinner className="size-3.5" aria-hidden="true" role="presentation" />
                  ) : (
                    <span aria-hidden="true" data-mark="" className="inline-block size-2.5 rounded-[2px]" style={{ background: r.color }} />
                  )}
                  {r.label}
                  <Kbd className={cn("ml-1", current === r.id && "bg-background/20 text-background")}>{r.key}</Kbd>
                </Button>
              ))}
            </div>
            <Button variant="ghost" onClick={() => go(index + 1)} aria-keyshortcuts="ArrowRight">
              Skip <ArrowRight aria-hidden="true" />
            </Button>
          </div>
        </footer>
      )}
    </div>
  );
}

function CardInfo({
  card,
  colorFor,
  idleKey,
  current,
  partly,
  day,
}: {
  card: Card;
  colorFor: (category: string) => string;
  idleKey: string | null;
  current: ReviewRating | null;
  partly: boolean;
  day: string;
}) {
  const idle = isIdleCard(card, idleKey);
  return (
    <section aria-labelledby="review-card-title" className="flex min-w-0 flex-col gap-3">
      <p className="flex items-center gap-2 text-meta text-muted-foreground">
        <span className="font-mono text-foreground tnum">
          {card.start} – {card.end}
        </span>
        <span aria-hidden="true">·</span>
        <span className="tnum">{formatDuration(card.endTs - card.startTs)}</span>
      </p>
      <h2 id="review-card-title" className="font-display text-title font-semibold tracking-[-0.01em] text-balance">
        {card.title}
      </h2>
      <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-meta">
        <span
          aria-hidden="true"
          data-mark=""
          className={cn("size-2.5 rounded-[2px]", idle && "hatch-idle border border-cat-idle bg-surface-1")}
          style={idle ? undefined : { background: colorFor(card.category) }}
        />
        <span className="font-medium">{card.category}</span>
        {card.subcategory && <span className="text-muted-foreground">/ {card.subcategory}</span>}
        {[card.appSites?.primary, card.appSites?.secondary].filter(Boolean).map((a) => (
          <span key={a} className="rounded-sm border bg-surface-2 px-1.5 py-px text-caption text-muted-foreground" translate="no">
            {a}
          </span>
        ))}
      </p>
      {card.summary && <p className="max-w-[68ch] text-lead text-pretty">{card.summary}</p>}
      {current && (
        <p className="text-meta text-muted-foreground">
          {partly ? "Partly rated" : "Rated"} {RATINGS.find((r) => r.id === current)?.label}.
        </p>
      )}
      <p className="text-meta">
        <a href={href("timeline", { day, card: card.id })} className="text-muted-foreground underline underline-offset-2 hover:text-foreground">
          Show this card in the timeline
        </a>
      </p>
    </section>
  );
}

function Summary({
  day,
  extras,
  rated,
  queue,
  cards,
  coverage,
  onBack,
  onRateAgain,
}: {
  day: string;
  extras: DayExtras;
  rated: Map<number, ReviewRating>;
  queue: Card[];
  cards: Card[];
  coverage: Map<number, number>;
  onBack?: () => void;
  onRateAgain: () => void;
}) {
  const s = extras.reviewSummary;
  const secs: Record<ReviewRating, number> = { focused: s.focusedSecs, neutral: s.neutralSecs, distracted: s.distractedSecs };
  const totalSecs = secs.focused + secs.neutral + secs.distracted;
  const left = reviewQueue(cards, coverage).filter((c) => !rated.has(c.id)).length;
  const now = queue.filter((c) => rated.has(c.id));
  const nowSecs = now.reduce((a, c) => a + c.endTs - c.startTs, 0);
  return (
    <Centered>
      <ListChecks className="size-6 text-muted-foreground" aria-hidden="true" />
      <h2 className="font-display text-title font-semibold">{left === 0 ? "All caught up" : `${left} ${left === 1 ? "card is" : "cards are"} not rated yet`}</h2>
      <p className="max-w-[56ch] text-meta text-muted-foreground">
        {left === 0 ? `Every card of ${formatDayTitle(day)} is rated. ` : "You skipped them; rate them now or later in each card’s details. "}
        {now.length > 0
          ? `You rated ${now.length} ${now.length === 1 ? "card" : "cards"} (${formatDuration(nowSecs)}) just now. `
          : queue.length === 0
            ? ""
            : "You did not rate a card this time. "}
        Your day on the Timeline shows these totals too.
      </p>
      <section aria-labelledby="review-totals" className="flex w-full max-w-[34rem] flex-col gap-3 rounded-lg border bg-surface-1 p-4 text-left">
        <h3 id="review-totals" className="text-body font-semibold">
          Time per rating on {formatDayTitle(day)}
        </h3>
        {totalSecs === 0 ? (
          <p className="text-meta text-muted-foreground">No time rated yet.</p>
        ) : (
          <div
            role="img"
            aria-label={RATINGS.map((r) => `${r.label} ${formatDuration(secs[r.id])}`).join(", ")}
            className="flex h-3 w-full gap-0.5 overflow-hidden rounded-[3px]"
          >
            {RATINGS.filter((r) => secs[r.id] > 0).map((r) => (
              <span key={r.id} data-mark="" className="block h-full min-w-0.5" style={{ flex: `${secs[r.id]} 1 0%`, background: r.color }} />
            ))}
          </div>
        )}
        <dl className="grid grid-cols-3 gap-3">
          {RATINGS.map((r) => (
            <div key={r.id} className="flex flex-col gap-0.5">
              <dt className="flex items-center gap-1.5 text-caption text-muted-foreground">
                <span aria-hidden="true" data-mark="" className="inline-block size-2 rounded-[2px]" style={{ background: r.color }} />
                {r.label}
              </dt>
              <dd className="text-body font-medium tnum">
                {formatDuration(secs[r.id])}
                {totalSecs > 0 && <span className="ml-1.5 text-caption font-normal text-muted-foreground">{Math.round((secs[r.id] / totalSecs) * 100)}%</span>}
              </dd>
            </div>
          ))}
        </dl>
        {s.lastReviewedAt && <p className="text-caption text-muted-foreground">Rated up to {clock(s.lastReviewedAt)}.</p>}
      </section>
      <div className="flex flex-wrap justify-center gap-2">
        {left > 0 && <Button onClick={onRateAgain}>Rate the skipped cards</Button>}
        <DialogPrimitive.Close asChild>
          <Button variant={left > 0 ? "outline" : "default"}>Close</Button>
        </DialogPrimitive.Close>
        {onBack && (
          <Button variant="ghost" onClick={onBack}>
            <ArrowLeft aria-hidden="true" /> Back to the last card
          </Button>
        )}
      </div>
    </Centered>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return <div className="mx-auto flex min-h-full max-w-[44rem] flex-col items-center justify-center gap-4 px-6 py-10 text-center">{children}</div>;
}

function ReviewSkeleton() {
  return (
    <div aria-busy="true" aria-label="Loading the cards" className="mx-auto grid w-full max-w-[96rem] gap-x-8 gap-y-5 px-6 py-5 wide:grid-cols-[minmax(18rem,26rem)_minmax(0,1fr)]">
      <div className="flex flex-col gap-3">
        <Skeleton className="h-3.5 w-40" />
        <Skeleton className="h-6 w-64" />
        <Skeleton className="h-3.5 w-32" />
        <Skeleton className="h-16 w-full" />
      </div>
      <Skeleton className="aspect-video w-full max-w-[60rem]" />
    </div>
  );
}
