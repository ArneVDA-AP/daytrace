import * as React from "react";
import { Pencil } from "lucide-react";
import { Button } from "@/components/ui/button";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { navigate } from "@/lib/router";
import { formatDayTitle, formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { Plan, Progress, Reference, Review } from "@/views/journal-api";
import { Panel, ResultBadge, Swatch, pct } from "@/views/journal/parts";

/*
 * Dayflow's day goals: a focus target and a distraction limit with progress bars, the setup
 * ("Where do you want to spend your time today?"), Skip, and yesterday's review in the morning.
 * What counts as focus or distraction is each category's "Counts as" in Settings > Categories.
 */

function names(list: string[]): string {
  if (list.length <= 1) return list.join("");
  return `${list.slice(0, -1).join(", ")} and ${list[list.length - 1]}`;
}

/** What the distraction numbers include, in the Daily view's words. */
export function distractionCounts(categories: string[]): string {
  if (!categories.length) return "No category counts as distraction.";
  return `Counts ${names(categories)} cards.`;
}

/** What the focus numbers include. */
export function focusCounts(categories: string[]): string {
  if (!categories.length) return "No category counts as focus.";
  return `Counts ${names(categories)}, interruptions inside those cards included.`;
}

function OpenCategories() {
  return (
    <Button variant="link" size="xs" className="h-auto px-0 text-caption" onClick={() => navigate("settings", { section: "categories" })}>
      Change in Settings
    </Button>
  );
}

/** A track with stacked fills; `target` draws a tick where the target sits when the fills pass it. */
function Bar({
  segments,
  scale,
  tick,
  label,
  valueText,
  max,
  now,
}: {
  segments: { key: string; secs: number; color?: string; className?: string; title?: string }[];
  scale: number;
  tick: number | null;
  label: string;
  valueText: string;
  max: number;
  now: number;
}) {
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={Math.min(now, max)}
      aria-valuetext={valueText}
      className="relative h-2.5 w-full rounded-full bg-surface-3"
    >
      <div className="flex h-full w-full gap-px overflow-hidden rounded-full">
        {segments
          .filter((s) => s.secs > 0)
          .map((s) => (
            <span
              key={s.key}
              data-mark=""
              title={s.title}
              className={cn("block h-full shrink-0", s.className)}
              style={{ width: `${pct(s.secs, scale)}%`, ...(s.color ? { background: s.color } : {}) }}
            />
          ))}
      </div>
      {tick !== null && (
        <span aria-hidden="true" className="absolute -top-1 -bottom-1 w-0.5 rounded-full bg-foreground" style={{ left: `calc(${pct(tick, scale)}% - 1px)` }} />
      )}
    </div>
  );
}

export function TargetsView({
  plan,
  progress,
  isToday,
  past,
  recording,
  noRecording,
  colorFor,
}: {
  plan: Plan;
  progress: Progress;
  isToday: boolean;
  past: boolean;
  recording: boolean;
  /** Nothing was recorded on this (past) day: no verdicts. */
  noRecording: boolean;
  colorFor: (category: string) => string;
}) {
  const target = plan.focusTargetMinutes * 60;
  const limit = plan.distractionLimitMinutes * 60;
  const focus = progress.focusSecs;
  const used = progress.distractedSecs;
  const over = used > limit;
  const focusMet = focus >= target;
  const verdicts = past && !noRecording;
  const focusCats = progress.focusCategories.filter((c) => c.secs > 0);
  const distractionColor = colorFor(progress.distractionCategories[0] ?? "Distraction");

  return (
    <div className="flex flex-col gap-5">
      {/* Focus */}
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
          <h3 className="text-meta text-muted-foreground">Focus</h3>
          {verdicts ? (
            <ResultBadge met={focusMet}>{focusMet ? "Met" : `Missed by ${formatDuration(target - focus)}`}</ResultBadge>
          ) : isToday ? (
            <span className="text-caption text-muted-foreground tnum">{focusMet ? "Target reached" : `${formatDuration(target - focus)} to go`}</span>
          ) : null}
        </div>
        <p className="flex flex-wrap items-baseline gap-x-1.5">
          <span className="font-display text-title font-semibold tnum">{formatDuration(focus)}</span>
          <span className="text-meta text-muted-foreground tnum">of {formatDuration(target)}</span>
        </p>
        <Bar
          label="Focus"
          valueText={`${formatDuration(focus)} of ${formatDuration(target)}`}
          max={target}
          now={focus}
          scale={Math.max(target, focus)}
          tick={focus > target && target > 0 ? target : null}
          segments={focusCats.map((c) => ({ key: c.name, secs: c.secs, color: colorFor(c.name), title: `${c.name}: ${formatDuration(c.secs)}` }))}
        />
        {focusCats.length > 0 ? (
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-caption">
            {focusCats.map((c) => (
              <li key={c.name} className="inline-flex items-center gap-1.5">
                <Swatch color={colorFor(c.name)} />
                <span>{c.name}</span>
                <span className="text-muted-foreground tnum">{formatDuration(c.secs)}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-caption text-muted-foreground">
            {progress.focusCategories.length ? (
              `No time in ${names(progress.focusCategories.map((c) => c.name))} yet.`
            ) : (
              <>
                No category counts as focus. <OpenCategories />
              </>
            )}
          </p>
        )}
      </div>

      {/* Distraction limit */}
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
          <h3 className="text-meta text-muted-foreground">Distraction limit</h3>
          {verdicts ? (
            <ResultBadge met={!over}>{over ? `Over by ${formatDuration(used - limit)}` : "Met"}</ResultBadge>
          ) : over ? (
            <span className="text-caption font-medium text-destructive tnum">{formatDuration(used - limit)} over</span>
          ) : null}
        </div>
        <p className="flex flex-wrap items-baseline gap-x-1.5">
          {over ? (
            <>
              <span className="font-display text-title font-semibold tnum">{formatDuration(used)}</span>
              <span className="text-meta text-muted-foreground tnum">used of {formatDuration(limit)}</span>
            </>
          ) : (
            <>
              <span className="font-display text-title font-semibold tnum">{formatDuration(limit - used)}</span>
              <span className="text-meta text-muted-foreground tnum">
                left of {formatDuration(limit)}
                {used > 0 && `, ${formatDuration(used)} used`}
              </span>
            </>
          )}
        </p>
        <Bar
          label="Distraction limit used"
          valueText={`${formatDuration(used)} of ${formatDuration(limit)} used${over ? `, ${formatDuration(used - limit)} over` : ""}`}
          max={limit}
          now={used}
          scale={Math.max(limit, used)}
          tick={over && limit > 0 ? limit : null}
          segments={[
            { key: "used", secs: Math.min(used, limit), color: distractionColor },
            { key: "over", secs: over ? used - limit : 0, className: "bg-destructive" },
          ]}
        />
        <p className="text-caption text-muted-foreground">
          {distractionCounts(progress.distractionCategories)} <OpenCategories />
        </p>
      </div>

      {isToday && !recording && (
        <p className="rounded-md bg-warn-surface px-3 py-2 text-meta text-warn-foreground">Recording is paused. Resume it to keep tracking progress.</p>
      )}
    </div>
  );
}

const HOURS = Array.from({ length: 13 }, (_, i) => i);
const MINUTES = Array.from({ length: 12 }, (_, i) => i * 5);

function DurationPicker({ id, label, minutes, onChange }: { id: string; label: string; minutes: number; onChange: (m: number) => void }) {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return (
    <fieldset className="flex items-center gap-2">
      <legend className="sr-only">{label}</legend>
      <NativeSelect
        id={`${id}-h`}
        value={h}
        onChange={(e) => onChange(Math.min(720, Number(e.target.value) * 60 + m))}
        aria-label={`${label}, hours`}
        className="w-18 tnum"
      >
        {HOURS.map((i) => (
          <NativeSelectOption key={i} value={i}>
            {i}
          </NativeSelectOption>
        ))}
      </NativeSelect>
      <label htmlFor={`${id}-h`} className="text-meta text-muted-foreground">
        h
      </label>
      <NativeSelect
        id={`${id}-m`}
        value={m - (m % 5)}
        onChange={(e) => onChange(Math.min(720, h * 60 + Number(e.target.value)))}
        aria-label={`${label}, minutes`}
        className="w-20 tnum"
      >
        {MINUTES.map((v) => (
          <NativeSelectOption key={v} value={v}>
            {String(v).padStart(2, "0")}
          </NativeSelectOption>
        ))}
      </NativeSelect>
      <label htmlFor={`${id}-m`} className="text-meta text-muted-foreground">
        min
      </label>
    </fieldset>
  );
}

export function GoalSetup({
  plan,
  progress,
  reference,
  isToday,
  busy,
  onSave,
  onSkip,
  onCancel,
  cancelLabel = "Cancel",
  idPrefix = "goal",
  onDirty,
  settingsLink = true,
}: {
  plan: Plan;
  progress: Progress;
  reference: Reference;
  isToday: boolean;
  busy: boolean;
  onSave: (focusMinutes: number, distractionMinutes: number) => void;
  onSkip: () => void;
  onCancel?: () => void;
  cancelLabel?: string;
  /** Ids of the fields start with it (two setups can be on screen: the page and the prompt). */
  idPrefix?: string;
  /** The targets differ from the plan (unsaved choices). */
  onDirty?: (dirty: boolean) => void;
  /** "Change in Settings" after the note (not in the prompt dialog, which would stay open). */
  settingsLink?: boolean;
}) {
  const [focus, setFocus] = React.useState(plan.focusTargetMinutes);
  const [distraction, setDistraction] = React.useState(plan.distractionLimitMinutes);
  const focusNames = progress.focusCategories.map((c) => c.name);
  const dirty = focus !== plan.focusTargetMinutes || distraction !== plan.distractionLimitMinutes;
  React.useEffect(() => onDirty?.(dirty), [dirty, onDirty]);
  React.useEffect(() => () => onDirty?.(false), [onDirty]);
  return (
    <form
      className="@container flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(focus, distraction);
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape" && onCancel) {
          e.preventDefault();
          onCancel();
        }
      }}
    >
      <div className="grid gap-3 @lg:grid-cols-2">
        <div className="flex flex-col gap-2 rounded-md border bg-surface-2/40 p-3">
          <h3 className="text-meta font-semibold">Focus target</h3>
          <DurationPicker id={`${idPrefix}-focus`} label="Focus target" minutes={focus} onChange={setFocus} />
          <p className="text-caption text-muted-foreground tnum">
            Yesterday {formatDuration(reference.yesterdayFocusMinutes * 60)} · last week’s average {formatDuration(reference.lastWeekFocusMinutes * 60)}
          </p>
          <p className="text-caption text-muted-foreground">{focusCounts(focusNames)}</p>
        </div>
        <div className="flex flex-col gap-2 rounded-md border bg-surface-2/40 p-3">
          <h3 className="text-meta font-semibold">Distraction limit</h3>
          <DurationPicker id={`${idPrefix}-distraction`} label="Distraction limit" minutes={distraction} onChange={setDistraction} />
          <p className="text-caption text-muted-foreground tnum">
            Yesterday {formatDuration(reference.yesterdayDistractionMinutes * 60)} · last week’s average{" "}
            {formatDuration(reference.lastWeekDistractionMinutes * 60)}
          </p>
          <p className="text-caption text-muted-foreground">{distractionCounts(progress.distractionCategories)}</p>
        </div>
      </div>
      <p className="text-caption text-muted-foreground">
        What counts as focus or distraction is set per category in Settings, the same for Daily and Weekly. {settingsLink && <OpenCategories />}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button type="submit" disabled={busy}>
          {busy ? "Saving…" : "Save targets"}
        </Button>
        <Button type="button" variant="outline" onClick={onSkip} disabled={busy}>
          {isToday ? "Skip today" : "Skip this day"}
        </Button>
        {onCancel && (
          <Button type="button" variant="ghost" onClick={onCancel} disabled={busy}>
            {cancelLabel}
          </Button>
        )}
      </div>
    </form>
  );
}

export function SkippedView({ isToday, onSet, setRef }: { isToday: boolean; onSet: () => void; setRef?: React.Ref<HTMLButtonElement> }) {
  return (
    <div className="flex flex-col items-start gap-3 rounded-md border border-dashed p-4">
      <p className="text-meta text-muted-foreground">
        {isToday
          ? "You skipped goals for today. Set targets to see focus and distraction progress here and on the Timeline."
          : "Goals were skipped for this day."}
      </p>
      <Button ref={setRef} variant="outline" size="sm" onClick={onSet}>
        Set targets
      </Button>
    </div>
  );
}

/** Morning flow: how yesterday went against yesterday's targets. */
export function YesterdayReview({ review, day, colorFor }: { review: Review; day: string; colorFor: (c: string) => string }) {
  return (
    <Panel title="Yesterday’s review" description={formatDayTitle(day)}>
      <ReviewBody review={review} colorFor={colorFor} />
    </Panel>
  );
}

/** Yesterday's focus and distraction against its targets (the page panel and the prompt). */
export function ReviewBody({ review, colorFor }: { review: Review; colorFor: (c: string) => string }) {
  const p = review.progress;
  const cats = p.focusCategories.filter((c) => c.secs > 0);
  return (
    <>
      <div className="@container">
        <div className="grid gap-3 @lg:grid-cols-2">
          <div className="flex flex-col gap-2 rounded-md border p-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-meta text-muted-foreground">Focus</h3>
              <ResultBadge met={review.focusMet} />
            </div>
            <p className="flex flex-wrap items-baseline gap-x-1.5">
              <span className="font-display text-title font-semibold tnum">{formatDuration(p.focusSecs)}</span>
              <span className="text-meta text-muted-foreground tnum">of {formatDuration(review.plan.focusTargetMinutes * 60)} target</span>
            </p>
            {cats.length ? (
              <ul className="flex flex-wrap gap-x-4 gap-y-1 text-caption">
                {cats.map((c) => (
                  <li key={c.name} className="inline-flex items-center gap-1.5">
                    <Swatch color={colorFor(c.name)} />
                    {c.name} <span className="text-muted-foreground tnum">{formatDuration(c.secs)}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-caption text-muted-foreground">No focus time recorded.</p>
            )}
          </div>
          <div className="flex flex-col gap-2 rounded-md border p-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-meta text-muted-foreground">Distraction</h3>
              <ResultBadge met={review.distractionMet} />
            </div>
            <p className="flex flex-wrap items-baseline gap-x-1.5">
              <span className="font-display text-title font-semibold tnum">{formatDuration(p.distractedSecs)}</span>
              <span className="text-meta text-muted-foreground tnum">of {formatDuration(review.plan.distractionLimitMinutes * 60)} limit</span>
            </p>
          </div>
        </div>
      </div>
    </>
  );
}

export function TargetsActions({ onEdit, editRef }: { onEdit: () => void; editRef?: React.Ref<HTMLButtonElement> }) {
  return (
    <Button ref={editRef} variant="ghost" size="sm" onClick={onEdit}>
      <Pencil aria-hidden="true" /> Edit targets
    </Button>
  );
}
