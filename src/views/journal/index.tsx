import * as React from "react";
import { RotateCcw } from "lucide-react";
import { api } from "@/api";
import { useViewProps } from "@/app/view-props";
import { Banner } from "@/components/banner";
import { DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "@/components/ui/toast";
import { formatDayTitle, shiftDay } from "@/lib/time";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { teApi, type DayExtras } from "@/timeline-extras-api";
import { journalApi, type GoalItem, type GoalsView } from "@/views/journal-api";
import { Checklist } from "@/views/journal/checklist";
import { Panel } from "@/views/journal/parts";
import { Ratings } from "@/views/journal/ratings";
import { GoalSetup, SkippedView, TargetsActions, TargetsView, YesterdayReview } from "@/views/journal/targets";

/*
 * Goals (view id "journal"): Dayflow's day goals for the shared day. A focus target and a
 * distraction budget with progress, the morning review and setup ("Skip today"), a to-do list
 * whose items show the cards that mention them (the evidence check), and the day's ratings.
 * Backend: features/journal.rs (goals) and features/timeline_extras (ratings).
 */

const REFRESH_MS = 30_000;

export default function GoalsSection() {
  const { day, today, settings, colorFor } = useViewProps();
  const [view, setView] = React.useState<GoalsView | null>(null);
  const [extras, setExtras] = React.useState<DayExtras | null>(null);
  const [categories, setCategories] = React.useState<Map<number, string>>(new Map());
  const [hasCards, setHasCards] = React.useState<boolean | null>(null);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [actionError, setActionError] = React.useState<string | null>(null);
  const [editing, setEditing] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const dayRef = React.useRef(day);
  dayRef.current = day;

  const load = React.useCallback((d: string) => {
    const same = <T,>(prev: T | null, next: T) => (prev !== null && JSON.stringify(prev) === JSON.stringify(next) ? prev : next);
    journalApi
      .get(d)
      .then((v) => {
        if (dayRef.current !== d) return;
        setView((prev) => same(prev, v));
        setLoadError(null);
      })
      .catch((e) => dayRef.current === d && setLoadError(errorText(e)));
    teApi
      .dayExtras(d)
      .then((x) => dayRef.current === d && setExtras((prev) => same(prev, x)))
      .catch(() => dayRef.current === d && setExtras(null));
    api
      .getDay(d)
      .then((cards) => {
        if (dayRef.current !== d) return;
        setHasCards(cards.length > 0);
        setCategories((prev) => {
          const next = new Map(cards.map((c) => [c.id, c.category]));
          return prev.size === next.size && [...next].every(([k, v]) => prev.get(k) === v) ? prev : next;
        });
      })
      .catch(() => {});
  }, []);

  React.useEffect(() => {
    setView(null);
    setExtras(null);
    setHasCards(null);
    setLoadError(null);
    setActionError(null);
    setEditing(false);
    load(day);
    const t = setInterval(() => load(day), REFRESH_MS);
    return () => clearInterval(t);
  }, [day, load]);

  /** Runs a change that answers with the new view; errors show above the panels. */
  const run = async (f: () => Promise<GoalsView>, quiet = false): Promise<boolean> => {
    const d = day;
    try {
      const v = await f();
      if (dayRef.current === d && v.day === d) setView(v);
      setActionError(null);
      return true;
    } catch (e) {
      if (!quiet) setActionError(errorText(e));
      return false;
    }
  };

  const toggle = (item: GoalItem, done: boolean) => {
    // Optimistic: the box ticks at once; a failure reloads the real state.
    setView((v) => (v ? { ...v, items: v.items.map((x) => (x.id === item.id ? { ...x, done } : x)) } : v));
    void run(() => journalApi.updateItem(item.id, { done })).then((ok) => !ok && load(day));
  };

  const remove = async (item: GoalItem) => {
    const d = day;
    if (!(await run(() => journalApi.deleteItem(item.id, d)))) return;
    toast(`Deleted “${item.text}”`, {
      action: {
        label: "Undo",
        onClick: () =>
          void (async () => {
            try {
              let v = await journalApi.addItem(d, item.text);
              const back = v.items[v.items.length - 1];
              if (item.done && back) v = await journalApi.updateItem(back.id, { done: true });
              if (dayRef.current === d) setView(v);
            } catch (e) {
              toast("Couldn’t restore the item", { description: errorText(e), tone: "error" });
            }
          })(),
      },
    });
  };

  const showSkeleton = useDelayedLoading(view === null && !loadError);
  const past = day < today;

  let body: React.ReactNode;
  if (!view) {
    body = loadError ? (
      <Banner
        tone="error"
        title={`Couldn’t load the goals for ${formatDayTitle(day)}`}
        actions={
          <Button size="sm" variant="outline" onClick={() => load(day)}>
            <RotateCcw aria-hidden="true" /> Try again
          </Button>
        }
      >
        {loadError}
      </Banner>
    ) : showSkeleton ? (
      <GoalsSkeleton />
    ) : null;
  } else {
    const showSetup = editing || (view.prompt && !view.plan.saved);
    const plan = view.plan;
    const noRecording = past && hasCards === false;
    const done = view.items.filter((i) => i.done).length;
    let targetsNote: React.ReactNode = null;
    if (showSetup)
      targetsNote = view.isToday ? "Where do you want to spend your time today?" : `Where did you want to spend your time on ${formatDayTitle(day)}?`;
    else if (plan.isSkipped) targetsNote = null;
    else if (noRecording) targetsNote = "Nothing was recorded on this day, so there is no progress to compare.";
    else if (!plan.saved)
      targetsNote = plan.carriedFrom ? `Same targets as ${formatDayTitle(plan.carriedFrom)}.` : "Default targets. Edit them to set your own.";
    else if (view.isToday) targetsNote = "Progress comes from your focus and distraction categories.";

    body = (
      <>
        {actionError && (
          <Banner
            tone="error"
            title="Couldn’t save that change"
            actions={
              <Button size="sm" variant="outline" onClick={() => setActionError(null)}>
                Dismiss
              </Button>
            }
          >
            {actionError}
          </Banner>
        )}
        {view.prompt && view.review && !editing && <YesterdayReview review={view.review} day={shiftDay(day, -1)} colorFor={colorFor} />}
        <div className="grid items-start gap-4 wide:grid-cols-[minmax(0,1fr)_minmax(0,1.25fr)] wide:grid-rows-[auto_1fr]">
          <Panel
            className="wide:col-start-1 wide:row-start-1"
            title={view.isToday ? "Today’s targets" : "Targets"}
            description={targetsNote}
            actions={!showSetup && !plan.isSkipped && <TargetsActions onEdit={() => setEditing(true)} />}
          >
            {showSetup ? (
              <GoalSetup
                key={`${day}-${plan.focusTargetMinutes}-${plan.distractionLimitMinutes}`}
                plan={plan}
                progress={view.progress}
                reference={view.reference}
                isToday={view.isToday}
                busy={saving}
                onSave={async (f, dl) => {
                  setSaving(true);
                  if (await run(() => journalApi.savePlan(day, f, dl))) {
                    setEditing(false);
                    toast("Targets saved");
                  }
                  setSaving(false);
                }}
                onSkip={async () => {
                  setSaving(true);
                  if (await run(() => journalApi.skip(day))) setEditing(false);
                  setSaving(false);
                }}
                onCancel={editing ? () => setEditing(false) : undefined}
              />
            ) : plan.isSkipped ? (
              <SkippedView isToday={view.isToday} onSet={() => setEditing(true)} />
            ) : (
              <TargetsView
                plan={plan}
                progress={view.progress}
                isToday={view.isToday}
                past={past}
                recording={settings.settings.recordingEnabled}
                noRecording={noRecording}
                colorFor={colorFor}
              />
            )}
          </Panel>

          <Panel
            className="wide:col-start-2 wide:row-span-2 wide:row-start-1"
            title="To do"
            description={
              view.items.length
                ? `${done} of ${view.items.length} done`
                : `Things you want to get done${view.isToday ? " today" : " on this day"}. Daytrace lists the cards that mention each one.`
            }
          >
            <Checklist
              day={day}
              today={today}
              items={view.items}
              categoryOf={(id) => categories.get(id) ?? null}
              colorFor={colorFor}
              onToggle={toggle}
              onAdd={(text) => run(() => journalApi.addItem(day, text))}
              onRename={(item, text) => run(() => journalApi.updateItem(item.id, { text }))}
              onDelete={(item) => void remove(item)}
            />
          </Panel>

          {extras && <Ratings className="wide:col-start-1 wide:row-start-2" day={day} summary={extras.reviewSummary} />}
        </div>
      </>
    );
  }

  return (
    <>
      <PageHeader label="Goals" title={formatDayTitle(day)} badge={day === today ? <TitleBadge>Today</TitleBadge> : undefined} nav={<DayNav />} />
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-(--page-x) py-5">
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-4">{body}</div>
      </div>
    </>
  );
}

function GoalsSkeleton() {
  return (
    <div className="grid items-start gap-4 wide:grid-cols-[minmax(0,1fr)_minmax(0,1.25fr)]" aria-hidden="true">
      <div className="flex flex-col gap-4 rounded-lg border p-4">
        <Skeleton className="h-4 w-32" />
        <Skeleton className="h-7 w-40" />
        <Skeleton className="h-2.5 w-full rounded-full" />
        <Skeleton className="h-7 w-36" />
        <Skeleton className="h-2.5 w-full rounded-full" />
      </div>
      <div className="flex flex-col gap-3 rounded-lg border p-4">
        <Skeleton className="h-4 w-20" />
        {[0, 1, 2].map((i) => (
          <div key={i} className="flex flex-col gap-2 py-1">
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="ml-7 h-6 w-1/2" />
          </div>
        ))}
      </div>
    </div>
  );
}
