import * as React from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { OctagonAlert, Pencil, RefreshCw, Sparkles } from "lucide-react";
import type { SettingsDto } from "@/api";
import { providerLabel } from "@/app/status";
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
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";
import { toast } from "@/components/ui/toast";
import { at, clock, formatDayShort, formatDayTitle } from "@/lib/time";
import { cn, errorText } from "@/lib/utils";
import { insightsApi, type AutoPlan, type RecapEntry, type RecapView } from "@/views/insights/api";
import { relativeDayLabel } from "@/views/insights/format";
import { RetryButton, SectionCard } from "@/views/insights/parts";

/*
 * The standup (Dayflow's daily recap) for the selected day: highlights from the last active day
 * before it, tasks for the day, blockers. Generate or Regenerate asks the model; Edit changes the
 * lists by hand (and writes one when there is none); Copy as text and Export Markdown live in the
 * page header (useRecapActions). Results that arrive for another day than the one on screen are
 * dropped.
 */

type Busy = { kind: "generate" | "save" | "export"; day: string } | null;
type ActionError = { kind: "generate" | "save" | "export" | "copy"; message: string } | null;
interface Draft {
  highlights: string;
  tasks: string;
  blockers: string;
}

export interface RecapTitles {
  highlights: string;
  tasks: string;
  blockers: string;
}

export function recapTitles(day: string, today: string, sourceDay: string | null): RecapTitles {
  return {
    highlights: sourceDay ? `Highlights from ${relativeDayLabel(sourceDay, today)}` : "Recent highlights",
    tasks: `Tasks for ${relativeDayLabel(day, today)}`,
    blockers: "Blockers",
  };
}

/** Plain text for a standup message (Dayflow's standupClipboardText). */
export function standupText(entry: RecapEntry, titles: RecapTitles): string {
  const list = (items: string[]) => (items.length ? items.map((i) => `- ${i}`) : ["- None right now"]);
  return [titles.highlights, ...list(entry.highlights), "", titles.tasks, ...list(entry.tasks), "", titles.blockers, ...list(entry.blockers)].join("\n");
}

/** "08:00" -> "8:00 AM" (settings store the recap time as 24-hour HH:MM). */
function timeOfDay(hhmm: string, day: string): string {
  const [h, m] = hhmm.split(":").map(Number);
  if (!Number.isFinite(h) || !Number.isFinite(m)) return hhmm;
  return clock(at(day, h, m));
}

const lines = (t: string) =>
  t
    .split("\n")
    .map((l) => l.replace(/^\s*[-*•]\s+/, "").trim())
    .filter(Boolean);

export interface RecapActions {
  busy: Busy;
  error: ActionError;
  editing: Draft | null;
  generating: boolean;
  generate: () => Promise<void>;
  startEdit: () => void;
  setDraft: (d: Draft) => void;
  cancelEdit: () => void;
  saveEdit: () => Promise<void>;
  exportRecap: () => Promise<void>;
  copy: () => Promise<void>;
  clearError: () => void;
}

/** Recap commands for the day on screen; shared by the page header and the standup card. */
export function useRecapActions({
  day,
  today,
  recap,
  setRecap,
  exportDir,
  reload,
}: {
  day: string;
  today: string;
  recap: RecapView | null;
  setRecap: (r: RecapView) => void;
  exportDir: string;
  reload: () => void;
}): RecapActions {
  const [busy, setBusy] = React.useState<Busy>(null);
  const [error, setError] = React.useState<ActionError>(null);
  const [editing, setEditing] = React.useState<Draft | null>(null);
  const dayRef = React.useRef(day);
  dayRef.current = day;

  // A new day starts clean.
  React.useEffect(() => {
    setEditing(null);
    setError(null);
  }, [day]);

  const entry = recap?.entry ?? null;
  const sourceDay = entry?.sourceDay ?? recap?.sourceDay ?? null;
  const mine = busy && busy.day === day ? busy : null;

  const run = async (kind: NonNullable<Busy>["kind"], fn: (forDay: string) => Promise<void>) => {
    const forDay = day;
    setBusy({ kind, day: forDay });
    setError(null);
    try {
      await fn(forDay);
    } catch (e) {
      if (dayRef.current === forDay) setError({ kind, message: errorText(e) });
    } finally {
      setBusy((b) => (b && b.day === forDay && b.kind === kind ? null : b));
    }
  };

  return {
    busy: mine,
    error,
    editing,
    generating: mine?.kind === "generate" || !!recap?.generating,
    generate: () =>
      run("generate", async (forDay) => {
        const r = await insightsApi.generateRecap(forDay);
        if (dayRef.current !== forDay) return;
        setRecap(r);
        setEditing(null);
      }),
    startEdit: () => {
      setError(null);
      setEditing({ highlights: (entry?.highlights ?? []).join("\n"), tasks: (entry?.tasks ?? []).join("\n"), blockers: (entry?.blockers ?? []).join("\n") });
    },
    setDraft: setEditing,
    cancelEdit: () => setEditing(null),
    saveEdit: async () => {
      if (!editing) return;
      const draft = editing;
      await run("save", async (forDay) => {
        const r = await insightsApi.saveRecap(forDay, lines(draft.highlights), lines(draft.tasks), lines(draft.blockers));
        if (dayRef.current !== forDay) return;
        setRecap(r);
        setEditing(null);
        toast("Standup saved", { tone: "success" });
      });
    },
    exportRecap: async () => {
      if (!sourceDay) return;
      const from = sourceDay;
      await run("export", async (forDay) => {
        let path: string | undefined;
        if (!exportDir.trim()) {
          const p = await save({ defaultPath: `${from}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
          if (!p) return;
          path = p;
        }
        const written = await insightsApi.exportRecap(from, path);
        toast("Exported", { description: written, tone: "success" });
        if (dayRef.current === forDay) reload();
      });
    },
    copy: async () => {
      if (!entry) return;
      try {
        await navigator.clipboard.writeText(standupText(entry, recapTitles(day, today, sourceDay)));
        toast("Copied", { description: "The standup as plain text, ready to paste.", tone: "success" });
      } catch (e) {
        setError({ kind: "copy", message: errorText(e) });
      }
    },
    clearError: () => setError(null),
  };
}

export function StandupCard({
  day,
  today,
  recap,
  loadError,
  onRetryLoad,
  actions,
  settings,
}: {
  day: string;
  today: string;
  recap: RecapView | null;
  loadError: string | null;
  onRetryLoad: () => void;
  actions: RecapActions;
  settings: SettingsDto;
}) {
  const entry = recap?.entry ?? null;
  const sourceDay = entry?.sourceDay ?? recap?.sourceDay ?? null;
  const titles = recapTitles(day, today, sourceDay);
  const [confirm, setConfirm] = React.useState(false);
  const editRef = React.useRef<HTMLButtonElement>(null);
  const firstFieldRef = React.useRef<HTMLTextAreaElement>(null);
  const wasEditing = React.useRef(false);
  const { busy, error, editing, generating } = actions;

  // Focus the first list when editing starts, and the Edit button again when it ends.
  React.useEffect(() => {
    if (editing && !wasEditing.current) firstFieldRef.current?.focus();
    if (!editing && wasEditing.current) editRef.current?.focus();
    wasEditing.current = !!editing;
  }, [editing]);

  const regenerate = () => (entry?.edited ? setConfirm(true) : void actions.generate());

  const headerActions =
    entry && !editing && !generating ? (
      <>
        <Button ref={editRef} variant="ghost" size="sm" onClick={actions.startEdit}>
          <Pencil aria-hidden="true" /> Edit
        </Button>
        {sourceDay && (
          <Button variant="outline" size="sm" onClick={regenerate} disabled={!!busy}>
            <RefreshCw aria-hidden="true" /> Regenerate
          </Button>
        )}
      </>
    ) : undefined;

  const description = entry
    ? meta(entry)
    : "Highlights from the last day you worked, tasks for this day and blockers, ready to paste into a standup.";

  let body: React.ReactNode;
  if (!recap) {
    body = loadError ? (
      <Banner tone="error" icon={<OctagonAlert />} title="Couldn’t load the standup" details={loadError} actions={<RetryButton onClick={onRetryLoad} />}>
        The day’s numbers below may still show. Daytrace tries again every minute.
      </Banner>
    ) : (
      <ColumnsSkeleton />
    );
  } else if (editing) {
    body = (
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          void actions.saveEdit();
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            void actions.saveEdit();
          }
        }}
      >
        <div className="grid gap-4 split:grid-cols-3">
          {(["highlights", "tasks", "blockers"] as const).map((k, i) => (
            <div key={k} className="flex min-w-0 flex-col gap-1.5">
              <Label htmlFor={`recap-${k}`} className="text-meta font-semibold">
                {titles[k]}
              </Label>
              <Textarea
                id={`recap-${k}`}
                ref={i === 0 ? firstFieldRef : undefined}
                rows={5}
                className="min-h-32 text-body"
                value={editing[k]}
                aria-describedby="recap-edit-hint"
                onChange={(e) => actions.setDraft({ ...editing, [k]: e.target.value })}
              />
            </div>
          ))}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="submit" size="sm" disabled={busy?.kind === "save"}>
            {busy?.kind === "save" ? "Saving…" : "Save standup"}
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={actions.cancelEdit}>
            Cancel
          </Button>
          <p id="recap-edit-hint" className="text-caption text-muted-foreground">
            One item per line. Ctrl+Enter saves.
          </p>
        </div>
      </form>
    );
  } else if (generating) {
    body = (
      <div aria-busy="true" className="flex flex-col gap-4">
        <p role="status" className="inline-flex items-center gap-2 text-meta text-muted-foreground">
          <Spinner aria-hidden="true" />
          {sourceDay ? `Writing the standup from the cards of ${formatDayShort(sourceDay)}…` : "Writing the standup…"}
        </p>
        <ColumnsSkeleton />
      </div>
    );
  } else if (entry) {
    body = (
      <div className="grid gap-x-8 gap-y-5 split:grid-cols-3">
        <RecapColumn title={titles.highlights} items={entry.highlights} empty="No highlights." />
        <RecapColumn title={titles.tasks} items={entry.tasks} empty="No tasks." />
        <RecapColumn title={titles.blockers} items={entry.blockers} empty="No blockers." />
      </div>
    );
  } else {
    body = (
      <NotGenerated
        day={day}
        today={today}
        sourceDay={sourceDay}
        auto={day === today ? recap.auto : null}
        settings={settings}
        busy={!!busy}
        onGenerate={() => void actions.generate()}
        onWrite={actions.startEdit}
        writeRef={editRef}
      />
    );
  }

  const failed =
    error && error.kind !== "export" ? (
      <Banner
        tone="error"
        icon={<OctagonAlert />}
        title={
          error.kind === "generate" ? "The standup could not be written" : error.kind === "save" ? "Couldn’t save the standup" : "Couldn’t copy the standup"
        }
        details={error.message}
        actions={
          error.kind === "generate" ? (
            entry ? (
              <RetryButton onClick={() => void actions.generate()} busy={!!busy} />
            ) : undefined
          ) : error.kind === "save" ? (
            <RetryButton onClick={() => void actions.saveEdit()} busy={!!busy} />
          ) : undefined
        }
      >
        {error.kind === "generate" ? (entry ? "The standup below is unchanged." : "Nothing was lost. Generate again below, or write the standup yourself.") : error.kind === "save" ? "Your text is still in the fields." : undefined}
      </Banner>
    ) : null;

  const exportFailed =
    error?.kind === "export" ? (
      <Banner tone="error" icon={<OctagonAlert />} title="Couldn’t export the recap" details={error.message} actions={<RetryButton onClick={() => void actions.exportRecap()} busy={!!busy} />}>
        {settings.settings.recapExportDir.trim() ? (
          <>
            Check that the folder <span translate="no">{settings.settings.recapExportDir}</span> exists and can be written to, or change it in Settings.
          </>
        ) : (
          "Nothing was written."
        )}
      </Banner>
    ) : null;

  const autoFailed =
    recap && !entry && recap.autoError && !error && !generating && !editing ? (
      <Banner
        tone="error"
        icon={<OctagonAlert />}
        title="The automatic standup could not be written"
        details={recap.autoError}
      >
        Daytrace tries up to three times a day by itself. Generate standup below writes it now.
      </Banner>
    ) : null;

  return (
    <SectionCard title="Standup" description={description} actions={headerActions}>
      <div className="flex flex-col gap-4">
        {failed}
        {exportFailed}
        {autoFailed}
        {body}
        {entry && !editing && !generating && recap?.exportPath && (
          <p className="border-t pt-3 text-caption text-muted-foreground">
            Saved as{" "}
            <span className="font-mono break-all text-foreground" translate="no">
              {recap.exportPath}
            </span>
          </p>
        )}
      </div>
      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Replace your edited standup?</AlertDialogTitle>
            <AlertDialogDescription>
              Regenerate writes a new standup from the cards of {sourceDay ? formatDayTitle(sourceDay) : "the source day"}. The items you edited are replaced.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep my edits</AlertDialogCancel>
            <AlertDialogAction onClick={() => void actions.generate()}>Regenerate</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SectionCard>
  );
}

function meta(entry: RecapEntry): string {
  const from = formatDayShort(entry.sourceDay);
  const written = entry.model ? `Written by ${entry.model} from the cards of ${from}` : `From the cards of ${from}`;
  return `${written}${entry.edited ? ", edited by you." : "."}`;
}

function RecapColumn({ title, items, empty }: { title: string; items: string[]; empty: string }) {
  return (
    <section className="flex min-w-0 flex-col gap-2">
      <h3 className="flex items-center gap-2 text-meta font-semibold">
        {title}
        <span className="rounded-sm bg-surface-2 px-1.5 text-caption font-normal text-muted-foreground tnum" aria-hidden="true">
          {items.length}
        </span>
        <span className="sr-only">, {items.length === 1 ? "1 item" : `${items.length} items`}</span>
      </h3>
      {items.length === 0 ? (
        <p className="text-body text-muted-foreground">{empty}</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {items.map((it, i) => (
            <li key={i} className="grid grid-cols-[0.375rem_minmax(0,1fr)] gap-x-2.5 text-body">
              <span aria-hidden="true" className="mt-[0.5rem] size-1.5 rounded-full bg-foreground/50" />
              <span className="text-pretty break-words">{it}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function ColumnsSkeleton() {
  return (
    <div className="grid gap-6 split:grid-cols-3" aria-hidden="true">
      {[0, 1, 2].map((c) => (
        <div key={c} className="flex flex-col gap-2.5">
          <Skeleton className="h-4 w-36" />
          <Skeleton className="h-3.5 w-full" />
          <Skeleton className="h-3.5 w-11/12" />
          <Skeleton className="h-3.5 w-4/5" />
        </div>
      ))}
    </div>
  );
}

function NotGenerated({
  day,
  today,
  sourceDay,
  auto,
  settings,
  busy,
  onGenerate,
  onWrite,
  writeRef,
}: {
  day: string;
  today: string;
  sourceDay: string | null;
  auto: AutoPlan | null;
  settings: SettingsDto;
  busy: boolean;
  onGenerate: () => void;
  onWrite: () => void;
  writeRef: React.Ref<HTMLButtonElement>;
}) {
  const model = settings.settings.model.trim();
  return (
    <div className={cn("flex flex-col items-start gap-3 rounded-md border border-dashed border-border-strong p-4")}>
      {sourceDay ? (
        <>
          <p className="max-w-[68ch] text-body">
            No standup for {relativeDayLabel(day, today)} yet. Generate sends the card titles and summaries of {formatDayTitle(sourceDay)} to {providerLabel(settings)}
            {model && (
              <>
                {" "}
                (<span translate="no">{model}</span>)
              </>
            )}
            ; no screenshots are sent.
          </p>
          {auto && <p className="max-w-[68ch] text-meta text-muted-foreground">{autoSentence(auto, day)}</p>}
          <div className="flex flex-wrap gap-2">
            <Button onClick={onGenerate} disabled={busy}>
              <Sparkles aria-hidden="true" /> Generate standup
            </Button>
            <Button ref={writeRef} variant="outline" onClick={onWrite}>
              <Pencil aria-hidden="true" /> Write it yourself
            </Button>
          </div>
        </>
      ) : (
        <>
          <p className="max-w-[68ch] text-body">
            There is nothing to write a standup from: none of the 3 days before {formatDayTitle(day)} has 2 hours of activity.
          </p>
          <p className="max-w-[68ch] text-meta text-muted-foreground">You can still write the standup yourself.</p>
          <Button ref={writeRef} variant="outline" onClick={onWrite}>
            <Pencil aria-hidden="true" /> Write it yourself
          </Button>
        </>
      )}
    </div>
  );
}

/** What the automatic recap will do today (it needs 3 hours of activity on its source day). */
function autoSentence(auto: AutoPlan, day: string): string {
  if (auto.blocked) return auto.blocked;
  if (!auto.sourceDay) return "The automatic standup needs a day with at least 3 hours of activity in the last 3 days, so it will not run today.";
  return `Daytrace also writes it by itself after ${timeOfDay(auto.after, day)}, from the cards of ${formatDayShort(auto.sourceDay)}.`;
}
