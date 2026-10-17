import * as React from "react";
import { CalendarX2, OctagonAlert } from "lucide-react";
import { api, type Card } from "@/api";
import { useDay } from "@/app/day";
import { useAppState } from "@/app/state";
import { DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { setParams, useRoute } from "@/lib/router";
import { clock, formatDuration, formatWeekRange, shiftDay, weekStart } from "@/lib/time";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { insightsApi, type CategoryTotal, type WeekStats } from "@/views/insights/api";
import { kindOf, percent, weekdayLong } from "@/views/insights/format";
import { CardSkeleton, CategoryBreakdown, RetryButton, SectionCard, StatStrip, StatTile } from "@/views/insights/parts";
import {
  DistractionHeatmap,
  FocusChart,
  FocusTable,
  HeatTable,
  LanesTable,
  TimePerDayChart,
  TimePerDayTable,
  WeekLanes,
  type DayRow,
  type FocusRow,
  type Lane,
} from "./charts";
import { AppsByCategoryCard, ContextCard, distractionFacts, FocusBlocksCard, ProgramsCard, TopAppsCard } from "./panels";

/*
 * Weekly review (Dayflow's Weekly view, no model) for the week that contains the shared day:
 * the week in numbers, time per day, every card by time of day, time by category, focus and
 * distraction per day, when distractions happen, longest focus blocks, context shifts, apps and
 * sites, and programs. Weeks run Monday 4 AM to Monday 4 AM. Charts or tables for every chart
 * (?view=table). Refreshes every 2 minutes while visible, without loaders.
 */

/** Dayflow's weekly data gate: patterns need about 15 hours of a week. */
const ENOUGH_SECS = 15 * 3600;
const REFRESH_MS = 120_000;

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

interface WeekData {
  start: string;
  week: WeekStats;
  cards: Card[][];
}

export default function WeeklySection() {
  const { categories, colorFor } = useAppState();
  const { day, today, setDay, openTimeline } = useDay();
  const route = useRoute();
  const asTable = route.params.get("view") === "table";
  const start = weekStart(day);
  const isCurrent = start === weekStart(today);
  const [data, setData] = React.useState<WeekData | null>(null);
  const [error, setError] = React.useState<{ start: string; message: string } | null>(null);
  const startRef = React.useRef(start);
  startRef.current = start;

  const load = React.useCallback(async (s: string) => {
    try {
      const days = Array.from({ length: 7 }, (_, i) => shiftDay(s, i));
      const [week, ...cards] = await Promise.all([insightsApi.week(s), ...days.map((d) => api.getDay(d))]);
      if (startRef.current !== s) return;
      setData((cur) => (cur && cur.start === s && same(cur.week, week) && same(cur.cards, cards) ? cur : { start: s, week, cards }));
      setError(null);
    } catch (e) {
      if (startRef.current === s) setError({ start: s, message: errorText(e) });
    }
  }, []);

  React.useEffect(() => {
    void load(start);
    const t = setInterval(() => document.visibilityState === "visible" && void load(start), REFRESH_MS);
    return () => clearInterval(t);
  }, [start, load]);

  const current = data?.start === start ? data : null;
  const err = error?.start === start ? error.message : null;
  const showSkeleton = useDelayedLoading(!current && !err);

  const view = (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      aria-label="Show the numbers as"
      value={asTable ? "table" : "chart"}
      onValueChange={(v) => v && setParams({ view: v === "table" ? "table" : null }, { section: "weekly" })}
    >
      <ToggleGroupItem value="chart">Charts</ToggleGroupItem>
      <ToggleGroupItem value="table">Tables</ToggleGroupItem>
    </ToggleGroup>
  );

  const empty = !!current && !showSkeleton && current.week.capturedSecs === 0 && current.cards.every((c) => c.length === 0);

  return (
    <>
      <PageHeader
        label="Weekly"
        title={formatWeekRange(start)}
        badge={isCurrent ? <TitleBadge>This week</TitleBadge> : undefined}
        nav={<DayNav unit="week" />}
        actions={current && !empty ? view : undefined}
      />
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto">
        {empty ? (
          <Empty className="min-h-full">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <CalendarX2 aria-hidden="true" />
              </EmptyMedia>
              <EmptyTitle>{isCurrent ? "No cards this week yet" : `No recording in the week of ${formatWeekRange(start)}`}</EmptyTitle>
              <EmptyDescription>
                {isCurrent
                  ? "The weekly review fills in as soon as one day of this week has cards. Daytrace writes a card about every 15 minutes while it records."
                  : "There are no cards for any day of this week."}
              </EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              {isCurrent ? (
                <Button variant="outline" onClick={() => openTimeline(today)}>
                  Open the timeline
                </Button>
              ) : (
                <Button variant="outline" onClick={() => setDay(today)}>
                  Go to this week
                </Button>
              )}
            </EmptyContent>
          </Empty>
        ) : (
          <div className="mx-auto flex w-full max-w-[90rem] flex-col gap-4 px-(--page-x) py-5">
            {showSkeleton ? (
              <div className="flex flex-col gap-4" aria-hidden="true">
                <CardSkeleton lines={1} />
                <CardSkeleton tall lines={0} />
                <div className="grid gap-4 split:grid-cols-2">
                  <CardSkeleton lines={4} />
                  <CardSkeleton lines={4} />
                </div>
              </div>
            ) : err && !current ? (
              <Banner tone="error" icon={<OctagonAlert />} title="Couldn’t load this week" details={err} actions={<RetryButton onClick={() => void load(start)} />}>
                Daytrace tries again every 2 minutes.
              </Banner>
            ) : current ? (
              <WeekBody data={current} today={today} asTable={asTable} categories={categories} colorFor={colorFor} />
            ) : null}
          </div>
        )}
      </div>
    </>
  );
}

function WeekBody({
  data,
  today,
  asTable,
  categories,
  colorFor,
}: {
  data: WeekData;
  today: string;
  asTable: boolean;
  categories: ReturnType<typeof useAppState>["categories"];
  colorFor: (n: string) => string;
}) {
  const { week, cards } = data;
  const days = week.days.map((d) => d.day);
  const cardsOf = (d: string) => cards[days.indexOf(d)] ?? [];
  const active = week.capturedSecs - week.idleSecs;
  const withCards = week.days.filter((d) => d.capturedSecs > 0).length;
  const longest = week.focusBlocks[0] ?? null;

  const rows: DayRow[] = week.days.map((d) => ({ day: d.day, totals: d.totals, secs: d.capturedSecs, future: d.day > today }));
  const focusRows: FocusRow[] = week.days.map((d) => ({ day: d.day, focus: d.focusSecs, distracted: d.distractedSecs, future: d.day > today }));
  const lanes: Lane[] = week.days.map((d, i) => ({ day: d.day, cards: cards[i] ?? [], future: d.day > today }));
  const range: [number, number] = week.workflow.endMinute > week.workflow.startMinute ? [week.workflow.startMinute, week.workflow.endMinute] : [300, 1080];

  const totals = new Map<string, CategoryTotal>();
  for (const d of week.days)
    for (const t of d.totals) {
      const c = totals.get(t.key) ?? { ...t, secs: 0 };
      c.secs += t.secs;
      totals.set(t.key, c);
    }

  const distraction = categories.find((c) => kindOf(c.name, categories) === "distraction");
  const distractionColor = distraction ? colorFor(distraction.name) : "var(--cat-distraction)";
  const facts = distractionFacts(week);

  return (
    <>
      <StatStrip label="The week in numbers" className="grid-cols-2 split:grid-cols-4">
        <StatTile
          label="Recorded"
          value={formatDuration(week.capturedSecs)}
          note={`${withCards} ${withCards === 1 ? "day" : "days"} with cards${week.idleSecs > 0 ? `, ${formatDuration(week.idleSecs)} idle` : ""}`}
        />
        <StatTile label="Focused" value={formatDuration(week.focusSecs)} note={`${percent(week.focusSecs, active)} of active time`} />
        <StatTile label="Distracted" value={formatDuration(week.distractedSecs)} note={`${percent(week.distractedSecs, active)} of active time`} />
        <StatTile
          label="Longest focus block"
          value={longest ? formatDuration(longest.secs) : "None"}
          note={
            longest ? (
              <>
                {weekdayLong(longest.day)},{" "}
                <span className="font-mono tnum">
                  {clock(longest.startTs)} – {clock(longest.endTs)}
                </span>
              </>
            ) : (
              "No focus time this week"
            )
          }
        />
      </StatStrip>
      {active < ENOUGH_SECS && (
        <p className="text-meta text-muted-foreground">This week has less than 15 hours of activity, so the patterns below may not say much yet.</p>
      )}

      <SectionCard title="Time per day" description="Recorded time by category. Days run from 4 AM to 4 AM.">
        {asTable ? <TimePerDayTable rows={rows} categories={categories} colorFor={colorFor} /> : <TimePerDayChart rows={rows} categories={categories} colorFor={colorFor} />}
      </SectionCard>

      <SectionCard title="Your week by hour" description="Every card at its time of day, one row per day. Select a day to open its timeline.">
        {asTable ? <LanesTable lanes={lanes} /> : <WeekLanes lanes={lanes} range={range} categories={categories} colorFor={colorFor} />}
      </SectionCard>

      <div className="grid items-start gap-4 split:grid-cols-2">
        <SectionCard title="Time by category">
          <CategoryBreakdown totals={[...totals.values()]} categories={categories} colorFor={colorFor} caption={`Time per category in the week of ${formatWeekRange(week.weekStart)}`} />
        </SectionCard>
        <SectionCard title="Focus and distraction" description="Focused is every category except Distraction and Idle. Distracted adds the distractions noted inside other cards.">
          {asTable ? <FocusTable rows={focusRows} /> : <FocusChart rows={focusRows} distractionColor={distractionColor} />}
        </SectionCard>
      </div>

      <SectionCard title="When distractions happen" description="Distracted time per hour of the day, from Distraction cards and distractions noted inside other cards.">
        {facts.length ? (
          <div className="flex flex-col gap-4">
            <ul className="flex flex-col gap-1 text-body">
              {facts.map((f) => (
                <li key={f.text}>{f.node}</li>
              ))}
            </ul>
            {asTable ? <HeatTable week={week} /> : <DistractionHeatmap week={week} summary={facts.map((f) => f.text).join(" ")} />}
          </div>
        ) : (
          <p className="text-meta text-muted-foreground">No distractions noted this week.</p>
        )}
      </SectionCard>

      <div className="grid items-start gap-4 split:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-4">
          <FocusBlocksCard week={week} cardsOf={cardsOf} />
          <TopAppsCard week={week} categories={categories} colorFor={colorFor} />
        </div>
        <div className="flex min-w-0 flex-col gap-4">
          <ContextCard week={week} today={today} />
          <AppsByCategoryCard week={week} categories={categories} colorFor={colorFor} />
          <ProgramsCard week={week} />
        </div>
      </div>
    </>
  );
}
