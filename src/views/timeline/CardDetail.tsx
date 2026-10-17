import * as React from "react";
import { ChevronDown, ChevronUp, Copy, MoreHorizontal, Pencil, RotateCcw, ThumbsDown, ThumbsUp, Trash2, TriangleAlert, X } from "lucide-react";
import type { Category } from "@/api";
import { Banner } from "@/components/banner";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Kbd } from "@/components/ui/kbd";
import { Label } from "@/components/ui/label";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { toast } from "@/components/ui/toast";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { href } from "@/lib/router";
import { clock, formatDuration } from "@/lib/time";
import { cn, errorText } from "@/lib/utils";
import { setUnsavedWork } from "@/views/platform-hooks";
import { timelineApi, type BatchProgress, type Card, type DayExtras, type FailedGroup, type ReviewRating } from "./api";
import { autoRetryText, type RetryUi } from "./CardList";
import { retryBusy, retryLine } from "./data";
import { cardText, isFailed, isIdleCard, keyOf, parseLogLine } from "./model";
import { Timelapse } from "./Timelapse";

/*
 * Card detail. Order keeps the reading text above the fold at 1280×800: time and title, summary,
 * interruptions, step-by-step log, the ratings, then the screenshots and timelapse (beside the text
 * from 1800 px). Everything the old CardDetail did: edit title, category and subcategory inline
 * (edited cards are kept by later analysis), reprocess the card, delete with a confirmation,
 * Focused / Neutral / Distracted rating (Dayflow timeline review), thumbs up/down with a note,
 * and the unsaved-work flag that holds off the 4 AM rollover and the idle reset.
 */

const RATINGS: { id: ReviewRating; label: string }[] = [
  { id: "focused", label: "Focused" },
  { id: "neutral", label: "Neutral" },
  { id: "distracted", label: "Distracted" },
];

export function CardDetail({
  card,
  group,
  extras,
  reloadExtras,
  categories,
  colorFor,
  idleKey,
  onChanged,
  onPrev,
  onNext,
  onClose,
  retry,
  now,
}: {
  card: Card;
  /** The failed-card group this card belongs to (a run when it has more than one card). */
  group: FailedGroup | null;
  extras: DayExtras | null;
  reloadExtras: () => void;
  categories: Category[];
  colorFor: (category: string) => string;
  idleKey: string | null;
  /** Reload the day's cards after an edit, delete or reprocess. */
  onChanged: () => void;
  onPrev?: () => void;
  onNext?: () => void;
  onClose: () => void;
  retry: RetryUi;
  now: number;
}) {
  const failed = isFailed(card);
  const idle = !failed && isIdleCard(card, idleKey);
  const run = group && group.cardIds.length > 1 ? group : null;
  const edited = extras?.edited.includes(card.id) ?? false;
  const review = extras?.reviews.find((r) => r.cardId === card.id);
  const feedback = extras?.feedback.find((f) => f.cardId === card.id);
  const lines = card.detailedSummary
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

  const [editing, setEditing] = React.useState(false);
  const [draft, setDraft] = React.useState({ title: "", category: "", subcategory: "" });
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [confirm, setConfirm] = React.useState<"delete" | "reprocess" | null>(null);
  const [reprocess, setReprocess] = React.useState<{ batchId: number; progress: BatchProgress | null } | null>(null);
  const [note, setNote] = React.useState(feedback?.note ?? "");
  const [noteSaved, setNoteSaved] = React.useState(false);
  const titleRef = React.useRef<HTMLInputElement>(null);

  // Card-level state resets when another card is shown.
  React.useEffect(() => {
    setEditing(false);
    setError(null);
    setNoteSaved(false);
    setConfirm(null);
  }, [card.id]);
  React.useEffect(() => setNote(feedback?.note ?? ""), [card.id, feedback?.note]);

  // An open edit or an unsaved note holds off the idle reset and the 4 AM rollover.
  const unsaved = editing || (!!feedback && note.trim() !== feedback.note.trim());
  React.useEffect(() => {
    setUnsavedWork(unsaved);
    return () => setUnsavedWork(false);
  }, [unsaved]);

  React.useEffect(() => {
    if (editing) titleRef.current?.focus();
  }, [editing]);

  // Progress of "Reprocess card" (the card is replaced when it finishes).
  React.useEffect(() => {
    if (!reprocess) return;
    let live = true;
    const t = setInterval(async () => {
      try {
        const p = await timelineApi.batchProgress([reprocess.batchId]);
        if (!live) return;
        if (p.waiting + p.running === 0) {
          setReprocess(null);
          if (p.failed) setError("Analyzing again failed. The card shows the error; you can try again.");
          else toast("Card written again");
          onChanged();
        } else setReprocess({ batchId: reprocess.batchId, progress: p });
      } catch {
        // Keep polling.
      }
    }, 2000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [reprocess?.batchId]); // eslint-disable-line react-hooks/exhaustive-deps

  const catNames = categories.map((c) => c.name);
  if (!failed && !catNames.some((n) => keyOf(n) === keyOf(card.category))) catNames.push(card.category);

  async function act<T>(f: () => Promise<T>, after?: (v: T) => void) {
    setBusy(true);
    setError(null);
    try {
      const v = await f();
      after?.(v);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  const startEdit = () => {
    const category = catNames.find((n) => keyOf(n) === keyOf(card.category)) ?? card.category;
    setDraft({ title: card.title, category, subcategory: card.subcategory });
    setError(null);
    setEditing(true);
  };
  const saveEdit = () =>
    act(
      () => timelineApi.updateCard(card.id, { ...draft, title: draft.title.trim() || card.title }),
      () => {
        setEditing(false);
        onChanged();
        reloadExtras();
        toast("Card saved", { description: "Later analysis keeps your edits." });
      },
    );

  const setRating = (r: ReviewRating | null) => act(() => timelineApi.rateCard(card.id, r), reloadExtras);
  const setThumb = (d: "up" | "down" | null) =>
    act(
      () => timelineApi.setFeedback(card.id, d, d ? note : ""),
      () => {
        setNoteSaved(false);
        reloadExtras();
      },
    );
  const saveNote = () =>
    feedback &&
    act(
      () => timelineApi.setFeedback(card.id, feedback.direction, note),
      () => {
        setNoteSaved(true);
        reloadExtras();
      },
    );

  const deleteIds = run ? run.cardIds : [card.id];
  const doDelete = () =>
    act(
      async () => {
        for (const id of deleteIds) await timelineApi.deleteCard(id);
      },
      () => {
        setConfirm(null);
        toast(deleteIds.length > 1 ? `${deleteIds.length} cards deleted` : "Card deleted");
        onChanged();
      },
    );
  const doReprocess = () =>
    act(
      () => timelineApi.reprocessCard(card.id),
      (batchId) => {
        setConfirm(null);
        setReprocess({ batchId, progress: null });
      },
    );

  const copyCard = async () => {
    try {
      await navigator.clipboard.writeText(cardText(card));
      toast("Card copied as text");
    } catch (e) {
      setError(`Couldn’t copy the card: ${errorText(e)}`);
    }
  };

  const range = run ? `${clock(run.startTs)} – ${clock(run.endTs)}` : `${card.start} – ${card.end}`;
  const secs = run ? run.endTs - run.startTs : card.endTs - card.startTs;
  const groupBusy = group ? retryBusy(retry.items, group.batchIds) : false;
  const retryStatus = group ? retryLine(retry.items, group.batchIds) : null;
  const rowId = run ? run.cardIds[0] : card.id;

  return (
    <article
      aria-labelledby="card-title"
      className="mx-auto flex w-full max-w-[52rem] flex-col gap-5 px-6 pt-4 pb-10 max-split:px-4 xwide:grid xwide:max-w-[110rem] xwide:grid-cols-[minmax(0,40rem)_minmax(0,1fr)] xwide:content-start xwide:gap-x-10"
    >
      <header className="flex flex-col gap-1.5 xwide:col-span-2">
        <div className="flex items-center gap-2 text-meta text-muted-foreground">
          <span className="font-mono text-foreground tnum">{range}</span>
          <span aria-hidden="true">·</span>
          <span className="tnum">{formatDuration(secs)}</span>
          <div className="ml-auto flex items-center gap-0.5">
            <NavButton label="Previous card" keys="K" onClick={onPrev} icon={<ChevronUp aria-hidden="true" />} />
            <NavButton label="Next card" keys="J" onClick={onNext} icon={<ChevronDown aria-hidden="true" />} />
            <DropdownMenu>
              <Tooltip>
                <TooltipTrigger asChild>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon-sm" aria-label="More actions for this card">
                      <MoreHorizontal aria-hidden="true" />
                    </Button>
                  </DropdownMenuTrigger>
                </TooltipTrigger>
                <TooltipContent>More actions</TooltipContent>
              </Tooltip>
              <DropdownMenuContent align="end" className="w-60">
                {!failed && (
                  <DropdownMenuItem onSelect={startEdit} disabled={editing}>
                    <Pencil aria-hidden="true" /> Edit title and category
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem onSelect={() => void copyCard()}>
                  <Copy aria-hidden="true" /> Copy card as text
                </DropdownMenuItem>
                {!failed && card.batchId !== null && (
                  <DropdownMenuItem onSelect={() => setConfirm("reprocess")} disabled={!!reprocess}>
                    <RotateCcw aria-hidden="true" /> Reprocess card…
                  </DropdownMenuItem>
                )}
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onSelect={() => setConfirm("delete")}>
                  <Trash2 aria-hidden="true" /> {run ? `Delete ${run.cardIds.length} cards…` : "Delete card…"}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon-sm" aria-label="Close card and show your day" aria-keyshortcuts="Escape" onClick={onClose}>
                  <X aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                Close card <Kbd className="ml-1">Esc</Kbd>
              </TooltipContent>
            </Tooltip>
          </div>
        </div>

        {editing ? (
          <form
            className="flex flex-col gap-3 rounded-lg border bg-surface-1 p-3 shadow-xs"
            aria-label="Edit card"
            onSubmit={(e) => {
              e.preventDefault();
              void saveEdit();
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                setEditing(false);
              }
            }}
          >
            <h2 id="card-title" className="sr-only">
              Edit “{card.title}”
            </h2>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="card-edit-title">Title</Label>
              <Input
                id="card-edit-title"
                ref={titleRef}
                value={draft.title}
                autoComplete="off"
                onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                className="font-display text-lead font-semibold"
              />
            </div>
            <div className="grid grid-cols-2 gap-3 max-split:grid-cols-1">
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="card-edit-category">Category</Label>
                <NativeSelect id="card-edit-category" value={draft.category} onChange={(e) => setDraft({ ...draft, category: e.target.value })} wrapperClassName="w-full">
                  {catNames.map((n) => (
                    <NativeSelectOption key={n} value={n}>
                      {n}
                    </NativeSelectOption>
                  ))}
                </NativeSelect>
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="card-edit-sub">Subcategory</Label>
                <Input
                  id="card-edit-sub"
                  value={draft.subcategory}
                  placeholder="Optional…"
                  autoComplete="off"
                  onChange={(e) => setDraft({ ...draft, subcategory: e.target.value })}
                />
              </div>
            </div>
            <p className="text-caption text-muted-foreground">Edited cards are kept as you leave them: later analysis does not rewrite them.</p>
            <div className="flex items-center gap-2">
              <Button type="submit" size="sm" disabled={busy}>
                {busy && <Spinner className="size-3.5" aria-hidden="true" role="presentation" />}
                Save
              </Button>
              <Button type="button" size="sm" variant="ghost" onClick={() => setEditing(false)}>
                Cancel
              </Button>
              <span className="ml-auto text-caption text-muted-foreground">
                <Kbd>Enter</Kbd> saves, <Kbd>Esc</Kbd> cancels
              </span>
            </div>
          </form>
        ) : (
          <>
            <h2 id="card-title" className="font-display text-title font-semibold tracking-[-0.01em] text-balance">
              {run ? `Processing failed · ${run.cardIds.length} intervals` : card.title}
            </h2>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-meta">
              {failed ? (
                <span className="inline-flex items-center gap-1.5 text-destructive">
                  <TriangleAlert className="size-3.5" aria-hidden="true" /> Not analyzed
                </span>
              ) : (
                <span className="inline-flex items-center gap-1.5">
                  <span
                    aria-hidden="true"
                    data-mark=""
                    className={cn("size-2.5 rounded-[2px]", idle && "hatch-idle border border-cat-idle bg-surface-1")}
                    style={idle ? undefined : { background: colorFor(card.category) }}
                  />
                  <span className="font-medium">{card.category}</span>
                  {card.subcategory && <span className="text-muted-foreground">/ {card.subcategory}</span>}
                </span>
              )}
              {[card.appSites?.primary, card.appSites?.secondary].filter(Boolean).map((a) => (
                <span key={a} className="rounded-sm border bg-surface-2 px-1.5 py-px text-caption text-muted-foreground" translate="no">
                  {a}
                </span>
              ))}
              {edited && <span className="rounded-sm border px-1.5 py-px text-caption text-muted-foreground">Edited</span>}
              {card.isIdleShortcut && <span className="text-caption text-muted-foreground">No model call: nothing changed on screen</span>}
              {!failed && (
                <Button variant="ghost" size="sm" className="ml-auto h-7 text-muted-foreground" onClick={startEdit}>
                  <Pencil aria-hidden="true" /> Edit
                </Button>
              )}
            </div>
          </>
        )}
      </header>

      {(reprocess || error) && (
        <div className="flex flex-col gap-2 xwide:col-start-1">
          {reprocess && (
            <p role="status" className="inline-flex items-center gap-2 text-meta text-muted-foreground">
              <Spinner className="size-3.5" aria-hidden="true" role="presentation" />
              {reprocess.progress?.running ? "Analyzing again" : "Waiting to analyze again"}
              {reprocess.progress?.step ? `: ${reprocess.progress.step}…` : "…"}
            </p>
          )}
          {error && (
            <Banner tone="error" title="That didn’t work" icon={<TriangleAlert />}>
              {error}
            </Banner>
          )}
        </div>
      )}

      {failed ? (
        <section aria-label="Processing failed" className="flex flex-col gap-3 rounded-lg border border-destructive/40 bg-destructive-surface p-4 xwide:col-start-1">
          <div className="flex items-start gap-3">
            <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
            <div className="flex min-w-0 flex-col gap-1">
              <p className="text-body text-pretty">{card.summary || "This part of the recording could not be turned into a card."}</p>
              <p className="text-meta text-muted-foreground">
                {group && !group.retryable
                  ? "The screenshots of this batch were deleted, so it can’t be analyzed again."
                  : `The screenshots are kept, so the card can be written again. ${autoRetryText(run ? run.endTs : card.endTs, now, true)}`}
              </p>
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-3 pl-7">
            {(!group || group.retryable) && (
              <Button onClick={() => retry.start(rowId, group, card)} disabled={groupBusy}>
                {groupBusy ? <Spinner className="size-4" aria-hidden="true" role="presentation" /> : <RotateCcw aria-hidden="true" />}
                {groupBusy ? "Trying again…" : "Try again"}
              </Button>
            )}
            <Button variant="outline" asChild>
              <a href={href("settings", { section: "provider" })}>Provider settings</a>
            </Button>
            <span className={cn("text-meta", retry.error?.id === rowId ? "text-destructive" : "text-muted-foreground")} aria-live="polite">
              {retry.error?.id === rowId
                ? retry.error.message
                : (retryStatus ?? (run ? `Daytrace analyzes the ${run.batchIds.length} batches one at a time and stops if one fails again.` : ""))}
            </span>
          </div>
          {card.detailedSummary && (
            <details className="group pl-7 text-meta">
              <summary className="w-fit cursor-default rounded-sm text-muted-foreground select-none hover:text-foreground">Error details</summary>
              <pre className="mt-2 overflow-x-auto rounded-md border bg-surface-1 p-3 font-mono text-caption whitespace-pre-wrap">{card.detailedSummary}</pre>
            </details>
          )}
        </section>
      ) : (
        card.summary && <p className="max-w-[68ch] text-lead text-pretty xwide:col-start-1">{card.summary}</p>
      )}

      {card.distractions.length > 0 && (
        <section aria-labelledby="int-h" className="flex flex-col gap-2 xwide:col-start-1">
          <h3 id="int-h" className="text-body font-semibold">
            Interruptions
          </h3>
          <ul className="flex flex-col divide-y rounded-md border">
            {card.distractions.map((d, i) => (
              <li key={i} className="grid grid-cols-[auto_minmax(0,1fr)] gap-4 px-3 py-2 text-meta">
                <span className="font-mono whitespace-nowrap text-muted-foreground tnum">
                  {d.startTime} – {d.endTime}
                </span>
                <span>
                  <span className="font-medium">{d.title}</span>
                  {d.summary && <span className="block text-muted-foreground">{d.summary}</span>}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {!failed && lines.length > 0 && (
        <section aria-labelledby="log-h" className="flex flex-col gap-2 xwide:col-start-1">
          <h3 id="log-h" className="text-body font-semibold">
            Step by step
          </h3>
          <ol className="flex flex-col">
            {lines.map((l, i) => {
              const { time, text } = parseLogLine(l);
              return (
                <li key={i} className="grid grid-cols-[4.75rem_0.75rem_minmax(0,1fr)] gap-2 text-meta">
                  <span className="py-1 text-right font-mono whitespace-nowrap text-muted-foreground tnum">{time ?? ""}</span>
                  <span aria-hidden="true" className="relative flex justify-center">
                    <span className={cn("absolute w-px bg-border-strong", i === 0 ? "top-2.5" : "top-0", i === lines.length - 1 ? "h-2.5" : "bottom-0")} />
                    <span className="relative mt-2 size-1.5 rounded-full bg-muted-foreground" />
                  </span>
                  <span className="py-1">{text}</span>
                </li>
              );
            })}
          </ol>
        </section>
      )}

      {!failed && (
        <section aria-labelledby="rate-h" className="flex flex-col gap-3 border-t pt-4 xwide:col-start-1">
          <h3 id="rate-h" className="sr-only">
            Your ratings
          </h3>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <span id="rate-time" className="w-48 text-meta text-muted-foreground max-split:w-full">
              How was this time?
            </span>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={0}
              aria-labelledby="rate-time"
              value={review?.rating ?? ""}
              disabled={busy}
              onValueChange={(v) => {
                if (v) void setRating(v as ReviewRating);
                else if (review) void setRating(review.coverage >= 0.99 ? null : review.rating);
              }}
            >
              {RATINGS.map((r) => (
                <ToggleGroupItem key={r.id} value={r.id}>
                  {r.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            {review && review.coverage < 0.8 && <span className="text-caption text-muted-foreground">Partly rated</span>}
          </div>
          {!card.isIdleShortcut && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <span id="rate-summary" className="w-48 text-meta text-muted-foreground max-split:w-full">
                Is this summary accurate?
              </span>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                spacing={0}
                aria-labelledby="rate-summary"
                value={feedback?.direction ?? ""}
                disabled={busy}
                onValueChange={(v) => void setThumb(v === "up" || v === "down" ? v : null)}
              >
                <ToggleGroupItem value="up">
                  <ThumbsUp aria-hidden="true" /> Accurate
                </ToggleGroupItem>
                <ToggleGroupItem value="down">
                  <ThumbsDown aria-hidden="true" /> Not accurate
                </ToggleGroupItem>
              </ToggleGroup>
            </div>
          )}
          {feedback && (
            <form
              className="flex max-w-[62ch] flex-col gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void saveNote();
              }}
            >
              <Label htmlFor={`note-${card.id}`} className="text-meta font-normal text-muted-foreground">
                {feedback.direction === "down" ? "What is wrong? Optional." : "What did it get right? Optional."} The note stays on this PC.
              </Label>
              <Textarea
                id={`note-${card.id}`}
                name="card-note"
                rows={2}
                autoComplete="off"
                value={note}
                placeholder={feedback.direction === "down" ? "The video was a lecture, not entertainment…" : "Got the project and the steps right…"}
                onChange={(e) => {
                  setNote(e.target.value);
                  setNoteSaved(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && e.ctrlKey) {
                    e.preventDefault();
                    e.currentTarget.form?.requestSubmit();
                  }
                }}
              />
              <div className="flex items-center gap-2">
                <Button type="submit" size="sm" variant="secondary" disabled={busy || note.trim() === feedback.note.trim()}>
                  Save note
                </Button>
                <span className="text-caption text-muted-foreground" aria-live="polite">
                  {noteSaved ? (
                    "Saved"
                  ) : (
                    <>
                      <Kbd>Ctrl</Kbd> <Kbd>Enter</Kbd>
                    </>
                  )}
                </span>
              </div>
            </form>
          )}
        </section>
      )}

      <section aria-labelledby="media-h" className="flex flex-col gap-2 xwide:sticky xwide:top-4 xwide:col-start-2 xwide:row-span-6 xwide:row-start-2 xwide:self-start">
        <h3 id="media-h" className="text-body font-semibold">
          Screenshots
        </h3>
        <Timelapse card={card} />
      </section>

      <AlertDialog open={confirm === "delete"} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{run ? `Delete these ${run.cardIds.length} failed cards?` : failed ? "Delete this failed card?" : "Delete this card?"}</AlertDialogTitle>
            <AlertDialogDescription>
              {run
                ? "Daytrace will not try these batches again on its own. Their time shows as not recorded."
                : failed
                  ? "Daytrace will not try this batch again on its own. Its time shows as not recorded."
                  : `“${card.title}” and its screen descriptions are deleted, and ${card.start} to ${card.end} shows as not recorded on the timeline.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={busy}
              onClick={(e) => {
                e.preventDefault();
                void doDelete();
              }}
            >
              {run ? `Delete ${run.cardIds.length} cards` : "Delete card"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={confirm === "reprocess"} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Analyze this card again?</AlertDialogTitle>
            <AlertDialogDescription>
              Daytrace sends this card’s screenshots to your model again (one or more requests) and replaces the card with the new result.
              {edited && " Your edits to this card are replaced too."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={busy}
              onClick={(e) => {
                e.preventDefault();
                void doReprocess();
              }}
            >
              Reprocess card
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </article>
  );
}

function NavButton({ label, keys, onClick, icon }: { label: string; keys: string; onClick?: () => void; icon: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon-sm" onClick={onClick} disabled={!onClick} aria-label={label} aria-keyshortcuts={keys}>
          {icon}
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        {label} <Kbd className="ml-1">{keys}</Kbd>
      </TooltipContent>
    </Tooltip>
  );
}
