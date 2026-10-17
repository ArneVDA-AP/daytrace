import * as React from "react";
import { ClipboardCopy, Clock3, FileDown, Moon, SquareArrowOutUpRight } from "lucide-react";
import { api, type Card } from "@/api";
import { useDay } from "@/app/day";
import { useAppState } from "@/app/state";
import { DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useDelayedLoading } from "@/lib/use-delayed";
import { formatDayShort, formatDayTitle, formatDuration } from "@/lib/time";
import { errorText } from "@/lib/utils";
import { insightsApi, type DayStats, type RecapView } from "@/views/insights/api";
import { CardSkeleton, CategoryBreakdown, RetryButton, SectionCard } from "@/views/insights/parts";
import { failedSecs, quarterSlots } from "./activity";
import { ActivityGrid } from "./activity-grid";
import { DayStatStrip, DistractionsCard, FocusCard, ProgramsCard } from "./day-panels";
import { StandupCard, useRecapActions } from "./recap";

/*
 * Daily: the standup for the selected day (Dayflow's daily recap: highlights from the last active
 * day, tasks for this day, blockers) and that day's numbers without a model: the stat strip, the
 * quarter-hour activity grid, time by category, distractions, focus blocks and programs.
 * Data refreshes every minute while the window is visible (compare, then set: no loaders, no
 * movement); while a recap is being written elsewhere it looks again every 5 seconds.
 */

const REFRESH_MS = 60_000;
const GENERATING_MS = 5_000;

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

interface DayData {
  day: string;
  stats: DayStats;
  cards: Card[];
}

export default function DailySection() {
  const { settings, categories, colorFor } = useAppState();
  const { day, today, openTimeline, setDay } = useDay();
  const [dayData, setDayData] = React.useState<DayData | null>(null);
  const [recapData, setRecapData] = React.useState<{ day: string; recap: RecapView } | null>(null);
  const [dayError, setDayError] = React.useState<{ day: string; message: string } | null>(null);
  const [recapError, setRecapError] = React.useState<{ day: string; message: string } | null>(null);
  const dayRef = React.useRef(day);
  dayRef.current = day;
  // The activity grid marks the quarter hours still ahead today; this moves with each refresh.
  const [nowTs, setNowTs] = React.useState(() => Math.floor(Date.now() / 1000));

  const loadDay = React.useCallback(async (d: string) => {
    try {
      const [stats, cards] = await Promise.all([insightsApi.day(d), api.getDay(d)]);
      if (dayRef.current !== d) return;
      setDayData((cur) => (cur && cur.day === d && same(cur.stats, stats) && same(cur.cards, cards) ? cur : { day: d, stats, cards }));
      setDayError(null);
    } catch (e) {
      if (dayRef.current === d) setDayError({ day: d, message: errorText(e) });
    }
  }, []);

  const loadRecap = React.useCallback(async (d: string) => {
    try {
      const recap = await insightsApi.recap(d);
      if (dayRef.current !== d) return;
      setRecapData((cur) => (cur && cur.day === d && same(cur.recap, recap) ? cur : { day: d, recap }));
      setRecapError(null);
    } catch (e) {
      if (dayRef.current === d) setRecapError({ day: d, message: errorText(e) });
    }
  }, []);

  const reload = React.useCallback(() => {
    setNowTs(Math.floor(Date.now() / 1000));
    void loadDay(day);
    void loadRecap(day);
  }, [day, loadDay, loadRecap]);

  React.useEffect(() => {
    reload();
    const t = setInterval(() => document.visibilityState === "visible" && reload(), REFRESH_MS);
    return () => clearInterval(t);
  }, [reload]);

  const current = dayData?.day === day ? dayData : null;
  const recap = recapData?.day === day ? recapData.recap : null;
  const dayErr = dayError?.day === day ? dayError.message : null;
  const recapErr = recapError?.day === day ? recapError.message : null;

  // A recap being written by the scheduler (or another window): look again until it is done.
  const writingElsewhere = !!recap?.generating;
  React.useEffect(() => {
    if (!writingElsewhere) return;
    const t = setInterval(() => void loadRecap(day), GENERATING_MS);
    return () => clearInterval(t);
  }, [writingElsewhere, day, loadRecap]);

  const actions = useRecapActions({
    day,
    today,
    recap,
    setRecap: (r) => setRecapData({ day: r.targetDay, recap: r }),
    exportDir: settings.settings.recapExportDir,
    reload: () => void loadRecap(day),
  });

  const showDaySkeleton = useDelayedLoading(!current && !dayErr);
  const showRecapSkeleton = useDelayedLoading(!recap && !recapErr);

  const entry = recap?.entry ?? null;
  const sourceDay = entry?.sourceDay ?? recap?.sourceDay ?? null;
  const exportDir = settings.settings.recapExportDir.trim();
  const nothingAtAll = !!current && current.cards.length === 0 && !!recap && !entry && !sourceDay && !showDaySkeleton;
  const isToday = day === today;

  const slotNow = isToday ? nowTs : undefined;
  const slots = React.useMemo(
    () => (current ? quarterSlots(current.day, current.cards, categories, current.stats.distractions, slotNow) : []),
    [current, categories, slotNow],
  );
  const failed = React.useMemo(() => (current ? failedSecs(current.day, current.cards) : 0), [current]);

  const headerActions =
    recap && !actions.editing ? (
      <>
        {entry && (
          <Button variant="outline" size="sm" onClick={() => void actions.copy()} disabled={actions.generating}>
            <ClipboardCopy aria-hidden="true" /> Copy as text
          </Button>
        )}
        {sourceDay && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="outline" size="sm" onClick={() => void actions.exportRecap()} disabled={!!actions.busy || actions.generating}>
                <FileDown aria-hidden="true" />
                {actions.busy?.kind === "export" ? "Exporting…" : exportDir ? "Export as Markdown" : "Export as Markdown…"}
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              Saves the standup, numbers and cards of {formatDayShort(sourceDay)} {exportDir ? "to the recap folder" : "as a Markdown file"}
            </TooltipContent>
          </Tooltip>
        )}
      </>
    ) : undefined;

  return (
    <>
      <PageHeader
        label="Daily"
        title={formatDayTitle(day)}
        badge={isToday ? <TitleBadge>Today</TitleBadge> : undefined}
        nav={<DayNav />}
        actions={headerActions}
      />
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto">
        {nothingAtAll ? (
          <Empty className="min-h-full">
            <EmptyHeader>
              <EmptyMedia variant="icon">{isToday ? <Clock3 aria-hidden="true" /> : <Moon aria-hidden="true" />}</EmptyMedia>
              <EmptyTitle>{isToday ? "No cards yet today" : `Nothing recorded on ${formatDayTitle(day)}`}</EmptyTitle>
              <EmptyDescription>
                {isToday
                  ? "Daytrace writes a card about every 15 minutes while it records. The standup and the day’s numbers are made from those cards."
                  : "There are no cards for this day and no earlier day to write a standup from."}
              </EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              {isToday ? (
                <Button variant="outline" onClick={() => openTimeline(day)}>
                  Open the timeline
                </Button>
              ) : (
                <Button variant="outline" onClick={() => setDay(today)}>
                  Go to today
                </Button>
              )}
            </EmptyContent>
          </Empty>
        ) : (
          <div className="mx-auto flex w-full max-w-[90rem] flex-col gap-4 px-(--page-x) py-5">
            {showRecapSkeleton ? (
              <CardSkeleton lines={4} />
            ) : recap || recapErr ? (
              <StandupCard
                day={day}
                today={today}
                recap={recap}
                loadError={recapErr}
                onRetryLoad={() => void loadRecap(day)}
                actions={actions}
                settings={settings}
              />
            ) : (
              <div className="min-h-40" />
            )}

            {showDaySkeleton ? (
              <div className="flex flex-col gap-4" aria-hidden="true">
                <CardSkeleton lines={1} />
                <div className="grid gap-4 split:grid-cols-[minmax(0,3fr)_minmax(17rem,2fr)]">
                  <CardSkeleton tall lines={1} />
                  <CardSkeleton lines={4} />
                </div>
              </div>
            ) : dayErr && !current ? (
              <Banner tone="error" title="Couldn’t load this day’s numbers" details={dayErr} actions={<RetryButton onClick={() => void loadDay(day)} />}>
                Daytrace tries again every minute.
              </Banner>
            ) : current ? (
              current.cards.length === 0 ? (
                <Empty className="border py-10">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">{isToday ? <Clock3 aria-hidden="true" /> : <Moon aria-hidden="true" />}</EmptyMedia>
                    <EmptyTitle>{isToday ? "No cards yet today" : `Nothing recorded on ${formatDayTitle(day)}`}</EmptyTitle>
                    <EmptyDescription>
                      {isToday ? "The day’s numbers fill in as Daytrace writes cards, about every 15 minutes while it records." : "This day has no cards, so there are no numbers to show."}
                    </EmptyDescription>
                  </EmptyHeader>
                  <EmptyContent>
                    {isToday ? (
                      <Button variant="outline" size="sm" onClick={() => openTimeline(day)}>
                        Open the timeline
                      </Button>
                    ) : (
                      <Button variant="outline" size="sm" onClick={() => setDay(today)}>
                        Go to today
                      </Button>
                    )}
                  </EmptyContent>
                </Empty>
              ) : (
                <>
                  <DayStatStrip stats={current.stats} />
                  <div className="grid gap-4 split:grid-cols-[minmax(0,3fr)_minmax(17rem,2fr)]">
                    <SectionCard
                      title="Activity"
                      description="Each square is 15 minutes, filled from the bottom with the categories of the cards in it. Select a square to open its card."
                      actions={
                        <Button variant="ghost" size="sm" onClick={() => openTimeline(day)}>
                          <SquareArrowOutUpRight aria-hidden="true" /> Open timeline
                        </Button>
                      }
                    >
                      <ActivityGrid slots={slots} categories={categories} colorFor={colorFor} onOpen={(id) => openTimeline(day, id)} now={slotNow} />
                    </SectionCard>
                    <SectionCard title={isToday ? "Time by category so far" : "Time by category"}>
                      <CategoryBreakdown
                        totals={current.stats.totals}
                        categories={categories}
                        colorFor={colorFor}
                        caption={`Time per category on ${formatDayTitle(day)}`}
                        footer={
                          failed >= 60 ? (
                            <tr>
                              <th scope="row" className="pt-1 text-left font-normal text-muted-foreground">
                                Not analyzed
                              </th>
                              <td className="pt-1 text-right whitespace-nowrap text-muted-foreground tnum">{formatDuration(failed)}</td>
                              <td />
                            </tr>
                          ) : undefined
                        }
                      />
                      {failed >= 60 && (
                        <p className="mt-2 text-caption text-muted-foreground">Cards that could not be analyzed are not in the totals. Retry them on the timeline.</p>
                      )}
                    </SectionCard>
                  </div>
                  <div className="grid items-start gap-4 split:grid-cols-2">
                    <DistractionsCard stats={current.stats} day={day} isToday={isToday} />
                    <div className="flex min-w-0 flex-col gap-4">
                      <FocusCard stats={current.stats} categories={categories} />
                      <ProgramsCard stats={current.stats} />
                    </div>
                  </div>
                </>
              )
            ) : null}
          </div>
        )}
      </div>
    </>
  );
}
