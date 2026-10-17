import * as React from "react";
import { ArrowLeft, CalendarDays, ClipboardCopy, Clock3, Download, ListRestart, MoreHorizontal, Moon, PanelRight, Settings2, TriangleAlert } from "lucide-react";
import { useDay } from "@/app/day";
import { useAppState } from "@/app/state";
import { Banner } from "@/components/banner";
import { DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
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
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { navigate, numParam, setParams, useRoute } from "@/lib/router";
import { formatDayTitle, formatWeekRange, shiftDay, weekStart } from "@/lib/time";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText, isTypingTarget } from "@/lib/utils";
import { timelineApi, type Card, type FailedGroup } from "./api";
import { CardDetail } from "./CardDetail";
import { CardList, type LiveInfo, type RetryUi } from "./CardList";
import { CategoryChips } from "./CategoryChips";
import { useDayCards, useLoad, useMedia, useNow, useReprocessDay, useRetryStatus, readPref, writePref } from "./data";
import { DaySoFar } from "./DaySoFar";
import { DayTrace } from "./DayTrace";
import { ExportDialog } from "./ExportDialog";
import { buildRows, lanesFor, navIds, navIndexOf } from "./model";
import { Targets } from "./Targets";

/*
 * Timeline: the app's main screen. Page header (day navigation, Export…), the day trace, then the
 * card list and the detail side by side from 960 px (below that the detail opens over the list,
 * with "All cards" to go back). With no card selected the detail pane shows "Your day so far".
 *
 * URL: #/timeline?day=YYYY-MM-DD&card=<id>&detail=1&export=1. Keys: J next card, K previous card,
 * Esc closes the card (plus [ and ] for days from DayNav, arrows inside the trace and the list).
 */

const FIT_KEY = "daytrace.timelineFit";

// Dimmed categories survive switching sections (the view unmounts), like the old timeline.
let mutedKeys = new Set<string>();

export default function TimelineSection() {
  const app = useAppState();
  const { day, today, setDay } = useDay();
  const route = useRoute();
  const isToday = day === today;
  const selected = numParam(route.params, "card");
  const exportOpen = route.params.get("export") === "1";
  const split = useMedia("(min-width: 60rem)");
  const now = useNow(30_000);

  const reloadRef = React.useRef<() => void>(() => {});
  const retry = useRetryStatus(() => reloadRef.current());
  const rpRef = React.useRef<() => void>(() => {});
  const reprocessDay = useReprocessDay(day, () => rpRef.current());
  const data = useDayCards(day, retry.active || reprocessDay.rp.stage === "running");
  reloadRef.current = data.reload;
  rpRef.current = data.reload;

  const cards = data.cards;
  const extras = useLoad(day, timelineApi.dayExtras, cards);
  const stats = useLoad(day, timelineApi.dayStats, cards);
  const goals = useLoad(day, timelineApi.goals, cards);
  const week = useLoad(weekStart(day), timelineApi.weekStats, cards);

  const [muted, setMuted] = React.useState<Set<string>>(mutedKeys);
  const [fit, setFit] = React.useState(() => readPref(FIT_KEY) !== "0");
  const [retryError, setRetryError] = React.useState<{ id: number; message: string } | null>(null);
  const [toolError, setToolError] = React.useState<string | null>(null);

  React.useEffect(() => {
    setRetryError(null);
    setToolError(null);
  }, [day]);

  const lanes = React.useMemo(() => lanesFor(app.categories, cards ?? []), [app.categories, cards]);
  const idleKey = lanes.find((l) => l.isIdle)?.key ?? null;
  const rows = React.useMemo(() => buildRows(cards ?? [], extras.data?.failedGroups ?? []), [cards, extras.data]);
  const ids = navIds(rows);
  const selectedCard = cards?.find((c) => c.id === selected) ?? null;
  const groupOf = (id: number): FailedGroup | null => extras.data?.failedGroups.find((g) => g.cardIds.includes(id)) ?? null;

  // Below 960 px the list and the detail take turns: detail=1 shows the detail, detail=0 the list.
  // A link with a card and no detail param (a chat citation, the command menu) opens the detail.
  const detailParam = route.params.get("detail");
  const select = React.useCallback(
    (id: number, open = true) => setParams({ card: id, detail: split ? null : open ? 1 : 0 }, { section: "timeline" }),
    [split],
  );
  const detailOpen = detailParam === "1" || (selected !== null && detailParam !== "0");
  const deselect = React.useCallback(() => setParams({ card: null, detail: null }, { section: "timeline" }), []);

  // A selected card that is gone (reprocessed, retried, deleted): select the card that now covers
  // the same start time, or show the day summary.
  const lastSelected = React.useRef<Card | null>(null);
  if (selectedCard) lastSelected.current = selectedCard;
  React.useEffect(() => {
    if (!cards || selected === null || cards.some((c) => c.id === selected)) return;
    const prev = lastSelected.current;
    const replacement = prev && prev.day === day ? cards.find((c) => c.startTs <= prev.startTs && c.endTs > prev.startTs) : undefined;
    setParams({ card: replacement?.id ?? null, detail: replacement ? route.params.get("detail") : null }, { section: "timeline" });
  }, [cards, selected, day]); // eslint-disable-line react-hooks/exhaustive-deps

  // J / K: next / previous card (one step per collapsed run). Esc closes the card.
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented || isTypingTarget(e.target)) return;
      if (e.key === "Escape" && selected !== null) {
        e.preventDefault();
        deselect();
        return;
      }
      const k = e.key.toLowerCase();
      if ((k !== "j" && k !== "k") || ids.length === 0) return;
      const i = navIndexOf(rows, selected);
      const next = i < 0 ? (k === "j" ? 0 : ids.length - 1) : k === "j" ? Math.min(ids.length - 1, i + 1) : Math.max(0, i - 1);
      e.preventDefault();
      select(ids[next], detailOpen);
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [ids, rows, selected, select, deselect, detailOpen]);

  const toggleMute = (key: string) => {
    const next = new Set(muted);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    mutedKeys = next;
    setMuted(next);
  };

  const changeFit = (v: boolean) => {
    setFit(v);
    writePref(FIT_KEY, v ? "1" : "0");
  };

  const startRetry = React.useCallback(
    async (rowId: number, group: FailedGroup | null, card: Card) => {
      setRetryError(null);
      try {
        if (group) await timelineApi.retryBatches(group.batchIds);
        else await timelineApi.retryCard(card.id);
        await retry.refresh();
        void app.reloadStatus();
      } catch (e) {
        setRetryError({ id: rowId, message: errorText(e) });
      }
    },
    [retry, app],
  );
  const retryUi: RetryUi = { items: retry.items, error: retryError, start: (id, g, c) => void startRetry(id, g, c) };

  const copyTimeline = async () => {
    setToolError(null);
    try {
      await navigator.clipboard.writeText(await timelineApi.timelineText(day));
      toast("Timeline copied", { tone: "success", description: `${formatDayTitle(day)}, as plain text` });
    } catch (e) {
      setToolError(`Couldn’t copy the timeline: ${errorText(e)}`);
    }
  };

  const lastEnd = cards?.length ? Math.max(...cards.map((c) => c.endTs)) : null;
  const live: LiveInfo | null =
    isToday && cards
      ? {
          since: lastEnd,
          recording: app.recorder.tone === "live",
          writing: app.analysis.tone === "busy" && !!app.status.analysis.currentStep,
          label: app.recorder.label,
          detail: app.recorder.detail,
        }
      : null;

  const showSkeleton = useDelayedLoading(cards === null && !data.error);
  const index = navIndexOf(rows, selected);
  const prevId = index > 0 ? ids[index - 1] : undefined;
  const nextId = index >= 0 && index < ids.length - 1 ? ids[index + 1] : undefined;
  const nowOnDay = isToday ? now : null;

  const empty = cards !== null && cards.length === 0;
  const showDetail = split || detailOpen;
  const showList = split || !detailOpen;

  return (
    <>
      <PageHeader
        label="Timeline"
        title={formatDayTitle(day)}
        badge={isToday ? <TitleBadge>Today</TitleBadge> : undefined}
        nav={<DayNav />}
        actions={
          <Button variant="outline" size="sm" aria-haspopup="dialog" aria-expanded={exportOpen} onClick={() => setParams({ export: 1 })}>
            <Download aria-hidden="true" />
            Export…
          </Button>
        }
      />
      <div className="flex min-h-0 flex-1 flex-col">
        <DayTrace
          day={day}
          cards={cards}
          lanes={lanes}
          colorFor={app.colorFor}
          selectedId={selected}
          onSelect={(id) => select(id, detailOpen)}
          now={nowOnDay}
          muted={muted}
          fit={fit}
          onFitChange={changeFit}
        />

        {data.error && cards === null ? (
          <div className="px-(--page-x) py-5">
            <Banner
              tone="error"
              icon={<TriangleAlert />}
              title={`Couldn’t load the timeline of ${formatDayTitle(day)}`}
              actions={
                <Button size="sm" variant="outline" onClick={data.reload}>
                  Try again
                </Button>
              }
            >
              Daytrace could not read this day from its database. Your recording is not affected.
            </Banner>
            <details className="mt-2 px-1 text-meta text-muted-foreground">
              <summary className="w-fit cursor-default rounded-sm select-none hover:text-foreground">Error details</summary>
              <pre className="mt-1.5 overflow-x-auto rounded-md border bg-surface-2 p-3 font-mono text-caption whitespace-pre-wrap">{data.error}</pre>
            </details>
          </div>
        ) : empty ? (
          <EmptyDay day={day} today={today} recording={app.recorder.tone === "live"} userPaused={app.recorder.userPaused} pending={app.status.pendingBatches} onDay={setDay} onResume={app.resume} />
        ) : (
          <div className="flex min-h-0 flex-1">
            {showList && (
              <div className="scroll-thin min-h-0 w-full overflow-y-auto overscroll-contain split:w-[clamp(21rem,38%,30rem)] split:shrink-0 split:border-r">
                <div className="flex flex-col gap-2.5 border-b px-3 py-2.5">
                  <div className="flex items-start gap-2">
                    <div className="min-w-0 flex-1">
                      <CategoryChips lanes={lanes} colorFor={app.colorFor} muted={muted} onToggle={toggleMute} />
                    </div>
                    <DropdownMenu>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <DropdownMenuTrigger asChild>
                            <Button variant="ghost" size="icon-sm" aria-label="Actions for this day">
                              <MoreHorizontal aria-hidden="true" />
                            </Button>
                          </DropdownMenuTrigger>
                        </TooltipTrigger>
                        <TooltipContent>Actions for this day</TooltipContent>
                      </Tooltip>
                      <DropdownMenuContent align="end" className="w-60">
                        <DropdownMenuItem onSelect={() => void copyTimeline()} disabled={!cards?.length}>
                          <ClipboardCopy aria-hidden="true" /> Copy timeline
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onSelect={() => void reprocessDay.ask()}
                          disabled={!cards?.length || reprocessDay.rp.stage === "running" || reprocessDay.rp.stage === "loading"}
                        >
                          <ListRestart aria-hidden="true" /> Reprocess day…
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem onSelect={() => navigate("settings", { section: "categories" })}>
                          <Settings2 aria-hidden="true" /> Edit categories…
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                  <Targets goals={goals.data} day={day} compact />
                  {!split && (
                    <Button variant="outline" size="sm" className="self-start" onClick={() => setParams({ card: null, detail: 1 })}>
                      <PanelRight aria-hidden="true" /> {isToday ? "Your day so far" : "Your day"}
                    </Button>
                  )}
                  {toolError && (
                    <p role="alert" className="text-caption text-destructive">
                      {toolError}
                    </p>
                  )}
                </div>

                <ReprocessStatus state={reprocessDay.rp} onClose={reprocessDay.close} />

                {showSkeleton ? (
                  <ListSkeleton />
                ) : cards === null ? null : (
                  <>
                    <CardList
                      day={day}
                      rows={rows}
                      extras={extras.data}
                      idleKey={idleKey}
                      colorFor={app.colorFor}
                      selectedId={selected}
                      onSelect={(id) => select(id)}
                      muted={muted}
                      retry={retryUi}
                      now={now}
                      live={live}
                    />
                    <WeekFooter day={day} today={today} secs={week.data?.capturedSecs ?? null} />
                  </>
                )}
              </div>
            )}
            {showDetail && (
              <div className="scroll-thin min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain">
                {!split && (
                  <div className="sticky top-0 z-10 border-b bg-surface-1 px-3 py-2">
                    <Button variant="ghost" size="sm" onClick={() => setParams({ detail: 0 })}>
                      <ArrowLeft aria-hidden="true" /> All cards
                    </Button>
                  </div>
                )}
                {showSkeleton ? (
                  <DetailSkeleton />
                ) : cards === null ? null : selectedCard ? (
                  <CardDetail
                    key={selectedCard.id}
                    card={selectedCard}
                    group={groupOf(selectedCard.id)}
                    extras={extras.data}
                    reloadExtras={extras.reload}
                    categories={app.categories}
                    colorFor={app.colorFor}
                    idleKey={idleKey}
                    onChanged={data.reload}
                    onPrev={prevId !== undefined ? () => select(prevId, detailOpen) : undefined}
                    onNext={nextId !== undefined ? () => select(nextId, detailOpen) : undefined}
                    onClose={() => (split ? deselect() : setParams({ card: null, detail: null }))}
                    retry={retryUi}
                    now={now}
                  />
                ) : (
                  <DaySoFar
                    day={day}
                    isToday={isToday}
                    cards={cards}
                    lanes={lanes}
                    colorFor={app.colorFor}
                    stats={stats.data}
                    statsError={stats.error}
                    reloadStats={stats.reload}
                    goals={goals.data}
                    extras={extras.data}
                    onSelect={(id) => select(id, true)}
                  />
                )}
              </div>
            )}
          </div>
        )}
      </div>

      <ReprocessDialog state={reprocessDay.rp} onStart={() => void reprocessDay.start()} onClose={reprocessDay.close} day={day} />
      <ExportDialog open={exportOpen} onOpenChange={(o) => setParams({ export: o ? 1 : null })} day={day} today={today} />
    </>
  );
}

// ---------------------------------------------------------------- parts

function WeekFooter({ day, today, secs }: { day: string; today: string; secs: number | null }) {
  if (secs === null) return <div className="h-10" />;
  const hours = Math.floor(secs / 3600);
  const thisWeek = weekStart(day) === weekStart(today);
  return (
    <p className="border-t px-4 py-3 text-caption text-muted-foreground">
      <span className="font-semibold text-foreground tnum">
        {hours} {hours === 1 ? "hour" : "hours"}
      </span>{" "}
      tracked {thisWeek ? "this week" : `in the week of ${formatWeekRange(weekStart(day))}`}
    </p>
  );
}

function EmptyDay({
  day,
  today,
  recording,
  userPaused,
  pending,
  onDay,
  onResume,
}: {
  day: string;
  today: string;
  recording: boolean;
  userPaused: boolean;
  pending: number;
  onDay: (d: string) => void;
  onResume: () => Promise<void>;
}) {
  const isToday = day === today;
  const [prev, setPrev] = React.useState<string | null | undefined>(undefined);
  const [resuming, setResuming] = React.useState(false);
  React.useEffect(() => {
    let live = true;
    setPrev(undefined);
    timelineApi
      .daysWithData(shiftDay(day, -90), shiftDay(day, -1))
      .then((rows) => live && setPrev(rows.length ? rows[rows.length - 1].day : null))
      .catch(() => live && setPrev(null));
    return () => {
      live = false;
    };
  }, [day]);
  const yesterday = shiftDay(today, -1);
  return (
    <Empty className="flex-1">
      <EmptyHeader>
        <EmptyMedia variant="icon">{isToday ? <Clock3 aria-hidden="true" /> : <Moon aria-hidden="true" />}</EmptyMedia>
        <EmptyTitle>{isToday ? "No cards yet today" : `No recording on ${formatDayTitle(day)}`}</EmptyTitle>
        <EmptyDescription>
          {isToday
            ? recording
              ? `Daytrace writes the first card about 15 minutes after recording starts${pending > 0 ? ", and is working on it now" : ""}.`
              : userPaused
                ? "Recording is paused, so today’s timeline stays empty. Resume recording to start it."
                : "Recording is waiting (see the status in the side bar), so today’s timeline is still empty."
            : "Daytrace was not running, or recording was paused, for the whole day."}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent className="flex flex-row flex-wrap justify-center gap-2">
        {isToday && userPaused && (
          <Button
            disabled={resuming}
            onClick={async () => {
              setResuming(true);
              try {
                await onResume();
              } finally {
                setResuming(false);
              }
            }}
          >
            {resuming && <Spinner className="size-4" aria-hidden="true" role="presentation" />}
            Resume recording
          </Button>
        )}
        {prev && (
          <Button variant="outline" onClick={() => onDay(prev)}>
            <CalendarDays aria-hidden="true" />
            {prev === yesterday ? "Open yesterday" : `Open ${formatDayTitle(prev)}`}
          </Button>
        )}
        {!isToday && (
          <Button variant="outline" onClick={() => onDay(today)}>
            Go to today
          </Button>
        )}
      </EmptyContent>
    </Empty>
  );
}

function ReprocessStatus({ state, onClose }: { state: ReturnType<typeof useReprocessDay>["rp"]; onClose: () => void }) {
  if (state.stage === "running") {
    const left = state.progress ? state.progress.waiting + state.progress.running : state.batchIds.length;
    return (
      <div className="border-b px-3 py-2.5">
        <Banner
          tone="info"
          title={
            <span className="inline-flex items-center gap-2">
              <Spinner className="size-3.5" aria-hidden="true" role="presentation" /> Reprocessing this day…
            </span>
          }
        >
          {left} of {state.batchIds.length} {state.batchIds.length === 1 ? "batch" : "batches"} left.
          {state.progress?.step ? ` ${state.progress.step}.` : ""} New cards appear as each batch finishes.
        </Banner>
      </div>
    );
  }
  if (state.stage === "done") {
    return (
      <div className="border-b px-3 py-2.5">
        <Banner
          tone={state.failed > 0 ? "warn" : "info"}
          title="Reprocessing finished"
          actions={
            <Button size="sm" variant="ghost" onClick={onClose}>
              Close
            </Button>
          }
        >
          {state.failed > 0
            ? `${state.failed} of ${state.total} ${state.total === 1 ? "batch" : "batches"} failed; use Try again on those cards.`
            : `All ${state.total} ${state.total === 1 ? "batch was" : "batches were"} analyzed again.`}
        </Banner>
      </div>
    );
  }
  if (state.stage === "error") {
    return (
      <div className="border-b px-3 py-2.5">
        <Banner
          tone="error"
          title="Couldn’t reprocess this day"
          actions={
            <Button size="sm" variant="ghost" onClick={onClose}>
              Close
            </Button>
          }
        >
          {state.message}
        </Banner>
      </div>
    );
  }
  return null;
}

function ReprocessDialog({ state, onStart, onClose, day }: { state: ReturnType<typeof useReprocessDay>["rp"]; onStart: () => void; onClose: () => void; day: string }) {
  const open = state.stage === "loading" || state.stage === "confirm";
  const plan = state.stage === "confirm" ? state.plan : null;
  const n = plan?.batchIds.length ?? 0;
  return (
    <AlertDialog open={open} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Reprocess {formatDayTitle(day)}?</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="flex flex-col gap-2 text-meta">
              {!plan ? (
                <p className="inline-flex items-center gap-2">
                  <Spinner className="size-4" aria-hidden="true" role="presentation" /> Checking this day…
                </p>
              ) : plan.blocked ? (
                <p>{plan.blocked}</p>
              ) : (
                <>
                  <p>
                    Daytrace clears this day’s cards and screen descriptions, then analyzes the saved screenshots again: {n} {n === 1 ? "batch" : "batches"}, one
                    after another.
                    {plan.keptEditedCards > 0 &&
                      ` ${plan.keptEditedCards === 1 ? "The card you edited is" : `The ${plan.keptEditedCards} cards you edited are`} kept.`}
                  </p>
                  <p className="rounded-md border border-warn/50 bg-warn-surface px-3 py-2 font-medium text-foreground">
                    Each batch takes one or more model requests, so this can use a large number of API calls.
                  </p>
                </>
              )}
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{plan?.blocked ? "Close" : "Cancel"}</AlertDialogCancel>
          {plan && !plan.blocked && (
            <AlertDialogAction
              variant="destructive"
              onClick={(e) => {
                e.preventDefault();
                onStart();
              }}
            >
              Reprocess {n} {n === 1 ? "batch" : "batches"}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Mirrors the loaded rows (time, rail, title and meta) so nothing shifts when they arrive. */
function ListSkeleton() {
  return (
    <div aria-busy="true" aria-label="Loading the cards" className="flex flex-col gap-3 py-3 pr-3 pl-2">
      {[64, 56, 96, 56, 72].map((h, i) => (
        <div key={i} className="grid grid-cols-[4.25rem_0.625rem_1fr] gap-x-2.5" style={{ height: h }}>
          <Skeleton className="mt-2 ml-auto h-3 w-12" />
          <Skeleton className="h-full w-1 justify-self-center rounded-full" />
          <div className="flex flex-col gap-2 px-3 py-2">
            <Skeleton className="h-3.5 w-4/5" />
            <Skeleton className="h-3 w-2/5" />
          </div>
        </div>
      ))}
    </div>
  );
}

function DetailSkeleton() {
  return (
    <div aria-busy="true" aria-label="Loading" className="mx-auto flex w-full max-w-[52rem] flex-col gap-3 px-6 pt-5">
      <Skeleton className="h-6 w-56" />
      <Skeleton className="h-3.5 w-80" />
      <div className="mt-2 grid grid-cols-2 gap-2">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-[4.75rem] rounded-md" />
        ))}
      </div>
      <Skeleton className="mt-4 h-3 w-full" />
      <Skeleton className="h-4 w-2/3" />
    </div>
  );
}

