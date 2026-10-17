import * as React from "react";
import { AlertTriangle, RotateCcw } from "lucide-react";
import { useAppStatus } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { href } from "@/lib/router";
import { clock, formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { Card, DayExtras, FailedGroup, RetryItem } from "./api";
import { retryBusy, retryLine } from "./data";
import { isFailed, isIdleCard, keyOf, navIds, navIndexOf, type Row } from "./model";

/*
 * The card list on a vertical trace (designer-shadcn's TimelineList with designer-bespoke's summary
 * lines). Row height follows duration (--px-per-min, at least --card-min-h); cards of 30 minutes
 * and more show the first lines of their summary. Gaps of 5 minutes or more are "Not recorded"
 * rows, runs of failed cards collapse into one row with Try again, and today ends with a "now" row.
 *
 * One tab stop: ↑/↓ move between rows and select, Home/End jump; → reaches a row's Try again.
 * Rows are links (#/timeline?day=…&card=…), so middle-click and Back work.
 *
 * Memoized: only the "now" row reads the status, so a status change re-renders that row only.
 */

const RATING_LABEL: Record<string, string> = { focused: "Focused", neutral: "Neutral", distracted: "Distracted" };
const DAY_SECS = 24 * 3600;

/** What Daytrace does by itself about a failed card (analysis.rs catch_up: after a later success,
 * failed batches from the last 24 hours are tried again, at most 3 times each). */
export function autoRetryText(endTs: number, now: number, retryable: boolean): string {
  if (!retryable) return "The screenshots were deleted, so this can’t be analyzed again.";
  if (now - endTs < DAY_SECS) return "Daytrace tries again by itself after the next card is written, up to 3 times.";
  return "Daytrace no longer tries this by itself.";
}

export interface LiveInfo {
  /** Start of the next card (end of the last one), null before the first card. */
  since: number | null;
}

export interface RetryUi {
  items: RetryItem[];
  /** Error of the last Try again, by row id. */
  error: { id: number; message: string } | null;
  start: (rowId: number, group: FailedGroup | null, card: Card) => void;
}

export const CardList = React.memo(function CardList({
  day,
  rows,
  extras,
  idleKey,
  colorFor,
  selectedId,
  onSelect,
  muted,
  retry,
  now,
  live,
}: {
  day: string;
  rows: Row[];
  extras: DayExtras | null;
  idleKey: string | null;
  colorFor: (category: string) => string;
  selectedId: number | null;
  /** `via`: keyboard moves keep the list on screen in the narrow layout (FE10). */
  onSelect: (id: number, via: "keyboard" | "pointer") => void;
  muted: Set<string>;
  retry: RetryUi;
  /** Unix seconds; only compared with card ends (24-hour retry window). */
  now: number;
  live: LiveInfo | null;
}) {
  const ids = navIds(rows);
  const pick = (id: number) => onSelect(id, "pointer");
  const current = navIndexOf(rows, selectedId);
  const tabbable = current >= 0 ? ids[current] : ids[ids.length - 1];
  const listRef = React.useRef<HTMLOListElement>(null);
  const edited = new Set(extras?.edited ?? []);
  const reviewOf = new Map((extras?.reviews ?? []).map((r) => [r.cardId, r]));
  const groupOf = new Map<number, FailedGroup>();
  (extras?.failedGroups ?? []).forEach((g) => g.cardIds.forEach((id) => groupOf.set(id, g)));

  const focusRow = (id: number) => listRef.current?.querySelector<HTMLAnchorElement>(`a[data-row-id="${id}"]`)?.focus();

  const onKeyDown = (e: React.KeyboardEvent) => {
    const t = e.target as HTMLElement;
    const rowId = Number(t.dataset.rowId ?? t.closest<HTMLElement>("[data-row]")?.dataset.row);
    if (!Number.isFinite(rowId)) return;
    const i = ids.indexOf(rowId);
    let next: number | undefined;
    if (e.key === "ArrowDown") next = ids[i + 1];
    else if (e.key === "ArrowUp") next = ids[i - 1];
    else if (e.key === "Home") next = ids[0];
    else if (e.key === "End") next = ids[ids.length - 1];
    else if (e.key === "ArrowRight" && t.tagName === "A") {
      const btn = t.closest("li")?.querySelector<HTMLButtonElement>("button[data-row-action]");
      if (btn) {
        e.preventDefault();
        btn.focus();
      }
      return;
    } else if (e.key === "ArrowLeft" && t.tagName === "BUTTON") {
      e.preventDefault();
      focusRow(rowId);
      return;
    }
    if (next === undefined) return;
    e.preventDefault();
    onSelect(next, "keyboard");
    focusRow(next);
  };

  return (
    <ol ref={listRef} aria-label="Cards" className="flex flex-col pt-1 pr-3 pb-2 pl-2" onKeyDown={onKeyDown}>
      {rows.map((r) => {
        if (r.kind === "gap") return <GapRow key={r.id} seconds={r.end - r.start} />;
        if (r.kind === "run") {
          const busy = retryBusy(retry.items, r.group.batchIds);
          return (
            <RunRow
              key={`run-${r.id}`}
              day={day}
              row={r}
              selected={selectedId !== null && r.group.cardIds.includes(selectedId)}
              tabbable={tabbable === r.id}
              onSelect={pick}
              status={retry.error?.id === r.id ? null : retryLine(retry.items, r.group.batchIds)}
              error={retry.error?.id === r.id ? retry.error.message : null}
              busy={busy}
              onRetry={() => retry.start(r.id, r.group, r.cards[0])}
              hint={autoRetryText(r.group.endTs, now, r.group.retryable)}
            />
          );
        }
        const c = r.card;
        const g = groupOf.get(c.id) ?? null;
        const failed = isFailed(c);
        const review = reviewOf.get(c.id);
        return (
          <CardRow
            key={c.id}
            day={day}
            card={c}
            color={colorFor(c.category)}
            idle={!failed && isIdleCard(c, idleKey)}
            selected={selectedId === c.id}
            tabbable={tabbable === c.id}
            dimmed={!failed && muted.has(c.isIdleShortcut && idleKey ? idleKey : keyOf(c.category))}
            edited={edited.has(c.id)}
            rating={review ? RATING_LABEL[review.rating] : null}
            onSelect={pick}
            failure={
              failed
                ? {
                    retryable: g ? g.retryable : true,
                    busy: g ? retryBusy(retry.items, g.batchIds) : false,
                    status: retry.error?.id === c.id ? null : g ? retryLine(retry.items, g.batchIds) : null,
                    error: retry.error?.id === c.id ? retry.error.message : null,
                    hint: autoRetryText(c.endTs, now, g ? g.retryable : true),
                    onRetry: () => retry.start(c.id, g, c),
                  }
                : null
            }
          />
        );
      })}
      {live && <LiveRows lastEnd={live.since} />}
    </ol>
  );
});

function Rail({ children }: { children?: React.ReactNode }) {
  return <div className="relative flex w-2.5 justify-center self-stretch">{children}</div>;
}

const ROW_GRID = "grid grid-cols-[4.25rem_0.625rem_minmax(0,1fr)] gap-x-2.5";

function rowLink(day: string, id: number) {
  return href("timeline", { day, card: id });
}

function onLinkClick(e: React.MouseEvent, select: () => void) {
  if (e.ctrlKey || e.metaKey || e.shiftKey || e.button !== 0) return;
  e.preventDefault();
  select();
}

interface Failure {
  retryable: boolean;
  busy: boolean;
  status: string | null;
  error: string | null;
  hint: string;
  onRetry: () => void;
}

function CardRow({
  day,
  card: c,
  color,
  idle,
  selected,
  tabbable,
  dimmed,
  edited,
  rating,
  onSelect,
  failure,
}: {
  day: string;
  card: Card;
  color: string;
  idle: boolean;
  selected: boolean;
  tabbable: boolean;
  dimmed: boolean;
  edited: boolean;
  rating: string | null;
  onSelect: (id: number) => void;
  failure: Failure | null;
}) {
  const mins = (c.endTs - c.startTs) / 60;
  const apps = [c.appSites?.primary, c.appSites?.secondary].filter(Boolean).join(", ");
  const ref = React.useRef<HTMLAnchorElement>(null);
  const clamp = mins >= 90 ? "line-clamp-3" : mins >= 60 ? "line-clamp-2" : "line-clamp-1";

  React.useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <li
      data-row={c.id}
      className={cn(ROW_GRID, "group/row relative transition-opacity duration-(--dur-fast)", dimmed && !selected && "opacity-35 hover:opacity-70")}
      style={{ minHeight: `max(var(--card-min-h), calc(${mins} * var(--px-per-min)))` }}
    >
      <div className="pt-2 text-right text-caption leading-4 tnum">
        <div className={cn("font-mono", selected ? "text-foreground" : "text-muted-foreground")}>{c.start}</div>
        <div className="text-subtle-foreground">{formatDuration(c.endTs - c.startTs)}</div>
      </div>
      <Rail>
        <span
          aria-hidden="true"
          data-mark=""
          className={cn(
            "absolute inset-y-0.5 w-1 rounded-full",
            idle && "hatch-idle w-1.5 border border-cat-idle bg-surface-1",
            failure && "stripes-failed w-1.5 bg-destructive-surface",
            selected && "w-1.5",
          )}
          style={idle || failure ? undefined : { background: color }}
        />
      </Rail>
      {failure ? (
        <FailedBox
          selected={selected}
          link={{ ref, href: rowLink(day, c.id), rowId: c.id, tabbable, onSelect: () => onSelect(c.id) }}
          title={c.title}
          meta="Not analyzed"
          retryable={failure.retryable}
          busy={failure.busy}
          onRetry={failure.onRetry}
          retryLabel={`Try again: card at ${c.start}`}
          status={failure.error ?? failure.status ?? failure.hint}
          isError={!!failure.error}
        />
      ) : (
        <div className="flex min-w-0 flex-col pb-1">
          <a
            ref={ref}
            href={rowLink(day, c.id)}
            data-row-id={c.id}
            tabIndex={tabbable ? 0 : -1}
            aria-current={selected ? "true" : undefined}
            onClick={(e) => onLinkClick(e, () => onSelect(c.id))}
            className={cn(
              "flex min-w-0 flex-1 flex-col justify-start rounded-md border border-transparent px-3 py-2 transition-[background-color,border-color,box-shadow] duration-(--dur-fast) focus-visible:-outline-offset-2",
              "hover:bg-surface-2",
              selected ? SELECTED : "forced-colors:border-[Canvas]",
            )}
          >
            <span className={cn("line-clamp-2 text-body font-medium text-pretty", idle && "font-normal text-muted-foreground")}>{c.title}</span>
            <span className="mt-0.5 flex min-w-0 items-center gap-x-1.5 overflow-hidden text-meta whitespace-nowrap text-muted-foreground">
              {idle ? (
                <span>{c.isIdleShortcut ? "Away, no screen changes" : c.category}</span>
              ) : (
                <>
                  <span className="shrink-0">{c.category}</span>
                  {apps && (
                    <>
                      <span aria-hidden="true" className="text-subtle-foreground">
                        ·
                      </span>
                      <span className="min-w-0 truncate" translate="no">
                        {apps}
                      </span>
                    </>
                  )}
                </>
              )}
              {edited && <Tag>Edited</Tag>}
              {rating && <Tag>{rating}</Tag>}
            </span>
            {!idle && mins >= 30 && c.summary && <span className={cn("mt-1 text-meta text-muted-foreground", clamp)}>{c.summary}</span>}
          </a>
        </div>
      )}
    </li>
  );
}

function RunRow({
  day,
  row,
  selected,
  tabbable,
  onSelect,
  status,
  error,
  busy,
  onRetry,
  hint,
}: {
  day: string;
  row: Extract<Row, { kind: "run" }>;
  selected: boolean;
  tabbable: boolean;
  onSelect: (id: number) => void;
  status: string | null;
  error: string | null;
  busy: boolean;
  onRetry: () => void;
  hint: string;
}) {
  const g = row.group;
  const mins = (g.endTs - g.startTs) / 60;
  const ref = React.useRef<HTMLAnchorElement>(null);
  React.useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  return (
    <li data-row={row.id} className={cn(ROW_GRID, "relative")} style={{ minHeight: `max(var(--card-min-h), calc(${mins} * var(--px-per-min)))` }}>
      <div className="pt-2 text-right text-caption leading-4 tnum">
        <div className={cn("font-mono", selected ? "text-foreground" : "text-muted-foreground")}>{clock(g.startTs)}</div>
        <div className="text-subtle-foreground">{formatDuration(g.endTs - g.startTs)}</div>
      </div>
      <Rail>
        <span aria-hidden="true" data-mark="" className="stripes-failed absolute inset-y-0.5 w-1.5 rounded-full bg-destructive-surface" />
      </Rail>
      <FailedBox
        selected={selected}
        link={{ ref, href: rowLink(day, g.cardIds[0]), rowId: row.id, tabbable, onSelect: () => onSelect(g.cardIds[0]) }}
        title={`Processing failed · ${g.cardIds.length} intervals`}
        meta={`Not analyzed, ${clock(g.startTs)} to ${clock(g.endTs)}`}
        retryable={g.retryable}
        busy={busy}
        onRetry={onRetry}
        retryLabel={`Try again: ${g.cardIds.length} failed intervals from ${clock(g.startTs)}`}
        status={error ?? status ?? hint}
        isError={!!error}
      />
    </li>
  );
}

// Contrast themes drop the shadow and color every border: unselected rows get the canvas color
// (no border), the selected one Highlight (UX6).
const SELECTED =
  "border-border-strong bg-surface-1 shadow-md hover:bg-surface-1 dark:bg-surface-2 dark:hover:bg-surface-2 forced-colors:border-2 forced-colors:border-[Highlight]";

/** A failed card or run: the link (title, "Not analyzed") and, below it, Try again and what
 * Daytrace does by itself. Buttons cannot sit inside the link, so the box holds both. */
function FailedBox({
  selected,
  link,
  title,
  meta,
  retryable,
  busy,
  onRetry,
  retryLabel,
  status,
  isError,
}: {
  selected: boolean;
  link: { ref: React.RefObject<HTMLAnchorElement | null>; href: string; rowId: number; tabbable: boolean; onSelect: () => void };
  title: string;
  meta: string;
  retryable: boolean;
  busy: boolean;
  onRetry: () => void;
  retryLabel: string;
  status: string;
  isError: boolean;
}) {
  return (
    <div className="flex min-w-0 flex-col pb-1">
      <div
        className={cn(
          "flex flex-1 flex-col rounded-md border transition-[background-color,border-color,box-shadow] duration-(--dur-fast)",
          selected ? SELECTED : "border-transparent bg-destructive-surface/60 hover:border-destructive/30",
        )}
      >
        <a
          ref={link.ref}
          href={link.href}
          data-row-id={link.rowId}
          tabIndex={link.tabbable ? 0 : -1}
          aria-current={selected ? "true" : undefined}
          onClick={(e) => onLinkClick(e, link.onSelect)}
          className="flex min-w-0 flex-col rounded-md px-3 pt-2 pb-1 focus-visible:-outline-offset-2"
        >
          <span className="line-clamp-2 text-body font-medium text-pretty">
            <AlertTriangle className="mr-1.5 mb-0.5 inline size-3.5 text-destructive" aria-hidden="true" />
            {title}
          </span>
          <span className="mt-0.5 text-meta text-destructive">{meta}</span>
        </a>
        <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1 px-3 pt-0.5 pb-2">
          {retryable && (
            <Button size="sm" variant="outline" data-row-action="" tabIndex={link.tabbable ? 0 : -1} disabled={busy} onClick={onRetry} aria-label={retryLabel}>
              {busy ? <Spinner className="size-3.5" aria-hidden="true" role="presentation" /> : <RotateCcw aria-hidden="true" />}
              {busy ? "Trying again" : "Try again"}
            </Button>
          )}
          <span className={cn("min-w-0 flex-1 basis-40 text-caption", isError ? "text-destructive" : "text-muted-foreground")} aria-live="polite">
            {status}
          </span>
        </div>
      </div>
    </div>
  );
}

function Tag({ children }: { children: React.ReactNode }) {
  return <span className="shrink-0 rounded-sm border px-1 text-caption leading-4 text-muted-foreground">{children}</span>;
}

function GapRow({ seconds }: { seconds: number }) {
  return (
    <li className={ROW_GRID}>
      <span />
      <Rail>
        <span aria-hidden="true" className="absolute inset-y-1 w-px border-l border-dashed border-border-strong" />
      </Rail>
      <div className="hatch-gap my-1 flex h-8 items-center rounded-md px-3 text-meta text-muted-foreground">
        <span className="rounded-sm bg-surface-1 px-1.5 py-0.5">
          Not recorded, <span className="tnum">{formatDuration(seconds)}</span>
        </span>
      </div>
    </li>
  );
}

/** Today's last row: recording now and when the next card comes, from the status. */
function LiveRows({ lastEnd }: { lastEnd: number | null }) {
  const { recorder, analysis, status } = useAppStatus();
  const recording = recorder.tone === "live";
  const writing = analysis.tone === "busy" && !!status.analysis.currentStep;
  const nowTs = Math.floor(Date.now() / 1000);
  // Cards cover about 15 minutes and are written by the next analysis pass (every minute).
  const eta = lastEnd !== null ? lastEnd + 16 * 60 : null;
  const text = writing
    ? "Daytrace is writing the next card now."
    : recording
      ? lastEnd === null
        ? "The first card appears about 15 minutes after recording starts."
        : eta !== null && eta > nowTs
          ? `The next card covers ${clock(lastEnd)} onwards and appears around ${clock(eta)}.`
          : `The next card covers ${clock(lastEnd)} onwards and appears within a few minutes.`
      : recorder.detail;
  return (
    <li className={cn(ROW_GRID, "pt-0.5")} style={{ minHeight: "var(--card-min-h)" }}>
      <div className="pt-2 text-right font-mono text-caption leading-4 text-muted-foreground tnum">{lastEnd !== null ? clock(lastEnd) : "Now"}</div>
      <Rail>
        <span aria-hidden="true" className={cn("absolute inset-y-0.5 w-1 rounded-full", recording ? "bg-live/70 motion-safe:animate-pulse-live" : "bg-paused/50")} />
      </Rail>
      <div className="mb-1 rounded-md border border-dashed border-border-strong px-3 py-2">
        <p className="text-body font-medium">{recording ? "Recording now" : recorder.label}</p>
        <p className="mt-0.5 text-meta text-muted-foreground">{text}</p>
      </div>
    </li>
  );
}
