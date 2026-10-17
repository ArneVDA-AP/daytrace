import * as React from "react";
import { DatabaseBackup, FolderOpen, RefreshCw } from "lucide-react";
import { api, formatDayTitle, shiftDay } from "@/api";
import { useAppState } from "@/app/state";
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
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { href } from "@/lib/router";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { Choice, ErrorBanner, FieldError, Group, OutcomeLine, Row, type Outcome } from "./fields";
import { useFieldSaves } from "./save";
import { formatBytes, formatWhen, GB, storageApi, type DbHealth, type PurgeResult, type StorageUsage } from "./storage-api";

/** Size limits offered, in GB (Dayflow offers 1, 2, 3, 5, 10, 20 GB or unlimited). */
const LIMITS = [0, 1, 2, 3, 5, 10, 20, 50];
const SPINNER = { "aria-hidden": true, role: undefined, "aria-label": undefined } as const;
const count = (n: number) => n.toLocaleString("en-US");

type Confirm =
  | { kind: "older"; days: number; bytes: number }
  | { kind: "day"; day: string; screenshots: number; bytes: number }
  | { kind: "limit"; gb: number; over: number };

/**
 * Settings > Storage: disk usage, database backups, the size limit, deleting screenshots and the
 * data folder. Ported from Dayflow's storage settings tab.
 */
export function StorageSection() {
  const app = useAppState();
  const s = app.settings.settings;
  const fields = useFieldSaves();
  const id = React.useId();
  const [usage, setUsage] = React.useState<StorageUsage | null>(null);
  const [loadError, setLoadError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  const [confirm, setConfirm] = React.useState<Confirm | null>(null);
  const [olderDays, setOlderDays] = React.useState(String(s.retentionDays));
  const [outcome, setOutcome] = React.useState<Outcome | null>(null);
  const [backupOutcome, setBackupOutcome] = React.useState<Outcome | null>(null);
  const [folderError, setFolderError] = React.useState<string | null>(null);

  const load = React.useCallback(async () => {
    setLoadError(null);
    try {
      setUsage(await storageApi.usage());
    } catch (e) {
      setLoadError(errorText(e));
    }
  }, []);
  React.useEffect(() => void load(), [load]);
  const showSkeleton = useDelayedLoading(!usage && !loadError);

  const purged = (r: PurgeResult): Outcome =>
    r.filesRemoved === 0
      ? { ok: true, text: "Nothing to delete. Screenshots from the last hour and those still waiting for analysis are kept." }
      : { ok: true, text: `Deleted ${count(r.filesRemoved)} screenshots and freed ${formatBytes(r.bytesFreed)}.` };

  async function run(key: string, action: () => Promise<Outcome>, set = setOutcome) {
    setBusy(key);
    set(null);
    try {
      set(await action());
    } catch (e) {
      set({ ok: false, text: errorText(e) });
    } finally {
      setBusy(null);
      await load();
    }
  }

  const days = Math.max(1, Math.min(365, Math.round(Number(olderDays)) || 0));
  const olderValid = /^\d+$/.test(olderDays.trim()) && Number(olderDays) >= 1 && Number(olderDays) <= 365;
  const bytesBefore = (n: number) => (usage ? usage.days.filter((d) => d.day < shiftDay(app.today, -n)).reduce((t, d) => t + d.bytes, 0) : 0);

  const recordedBytes = usage ? usage.days.reduce((n, d) => n + d.bytes, 0) : 0;
  const limitOptions = (LIMITS.includes(s.storageLimitGb) ? LIMITS : [...LIMITS, s.storageLimitGb].sort((a, b) => a - b)).map((n) => ({
    value: n,
    label: n === 0 ? "No size limit" : `${n} GB`,
  }));
  const overLimit = s.storageLimitGb > 0 && recordedBytes > s.storageLimitGb * GB;

  function changeLimit(gb: number) {
    const over = recordedBytes - gb * GB;
    if (gb > 0 && over > 0 && (s.storageLimitGb === 0 || gb < s.storageLimitGb)) {
      setConfirm({ kind: "limit", gb, over });
      return;
    }
    void fields.apply("limit", gb, { storageLimitGb: gb });
  }

  function confirmed(c: Confirm) {
    setConfirm(null);
    if (c.kind === "older") void run("older", async () => purged(await storageApi.deleteOlderThan(c.days)));
    else if (c.kind === "day") void run(`day:${c.day}`, async () => purged(await storageApi.deleteDay(c.day)));
    else void fields.apply("limit", c.gb, { storageLimitGb: c.gb });
  }

  const total = usage ? usage.screenshotsBytes + usage.databaseBytes + usage.backupsBytes + usage.logsBytes : 0;
  const parts = usage
    ? [
        { label: "Screenshots", bytes: usage.screenshotsBytes, cls: "bg-foreground/70", extra: `${count(usage.screenshotCount)} from ${usage.days.length} ${usage.days.length === 1 ? "day" : "days"}` },
        { label: "Timeline database", bytes: usage.databaseBytes, cls: "bg-foreground/45", extra: "Cards, notes and goals" },
        {
          label: "Database backups",
          bytes: usage.backupsBytes,
          cls: "bg-foreground/25",
          extra: usage.backups[0] ? `${usage.backups.length} kept` : "No backup yet",
        },
        { label: "Log file", bytes: usage.logsBytes, cls: "bg-foreground/12", extra: "" },
      ]
    : [];
  const newest = usage?.backups[0];

  return (
    <>
      {usage && <HealthNotice h={usage.health} />}

      <Group
        title="Disk usage"
        description="Screenshots take most of the space. Deleting screenshots keeps the timeline cards and the notes they were written from, so the timeline, chat and recaps keep working."
        actions={
          <Button variant="ghost" size="sm" onClick={() => void load()} disabled={busy !== null}>
            <RefreshCw aria-hidden /> Refresh
          </Button>
        }
      >
        {loadError && (
          <div className="p-3">
            <ErrorBanner
              title="Couldn’t read disk usage"
              message={loadError}
              action={
                <Button size="sm" variant="outline" onClick={() => void load()}>
                  Try again
                </Button>
              }
            />
          </div>
        )}
        {!usage && !loadError && (
          <div className="flex flex-col gap-3 px-4 py-4" aria-busy="true" aria-label="Measuring disk usage">
            {showSkeleton && (
              <>
                <Skeleton className="h-8 w-32" />
                <Skeleton className="h-2.5 w-full rounded-full" />
                <Skeleton className="h-4 w-3/4" />
                <Skeleton className="h-4 w-2/3" />
              </>
            )}
          </div>
        )}
        {usage && (
          <div className="flex flex-col gap-3 px-4 py-4">
            <p className="flex flex-wrap items-baseline gap-x-3">
              <span className="font-display text-display font-semibold tnum">{formatBytes(total)}</span>
              <span className="text-meta text-muted-foreground">used by Daytrace</span>
            </p>
            <div className="flex h-2.5 gap-[2px] overflow-hidden rounded-full bg-surface-2" aria-hidden>
              {parts
                .filter((p) => p.bytes > 0)
                .map((p) => (
                  <span key={p.label} data-mark className={`h-full min-w-1 ${p.cls}`} style={{ width: `${(p.bytes / Math.max(total, 1)) * 100}%` }} />
                ))}
            </div>
            <dl className="grid gap-x-6 gap-y-1.5 text-meta split:grid-cols-2">
              {parts.map((p) => (
                <div key={p.label} className="flex items-center gap-2">
                  <span aria-hidden data-mark className={`size-2.5 shrink-0 rounded-[2px] ${p.cls}`} />
                  <dt>{p.label}</dt>
                  <dd className="ml-auto text-right text-muted-foreground tnum">
                    {formatBytes(p.bytes)}
                    {p.extra && <span className="text-subtle-foreground"> · {p.extra}</span>}
                  </dd>
                </div>
              ))}
            </dl>
            <p className="text-meta text-muted-foreground">
              {usage.bytesPerHour !== null && <>Recording takes about {formatBytes(usage.bytesPerHour)} per hour right now. </>}
              {usage.averageDayBytes !== null ? (
                <>
                  At your average of {formatBytes(usage.averageDayBytes)} per recorded day, keeping screenshots for {s.retentionDays}{" "}
                  {s.retentionDays === 1 ? "day" : "days"} takes about {formatBytes(usage.averageDayBytes * s.retentionDays)}.
                </>
              ) : (
                <>After the first full day of recording, Daytrace estimates how much space your retention period takes.</>
              )}{" "}
              <a href={href("settings", { section: "recording" })} className="underline underline-offset-2 hover:text-foreground">
                Change how long screenshots are kept
              </a>
              .
            </p>
          </div>
        )}
      </Group>

      <Group title="Size limit">
        <Row
          label="Limit screenshots to"
          htmlFor={`${id}-limit`}
          description={`When screenshots take more space than this, the oldest are deleted first, in addition to the ${s.retentionDays}-day limit. Screenshots from the last hour and those still waiting for analysis are kept.`}
        >
          <Choice id={`${id}-limit`} value={fields.shown("limit", s.storageLimitGb)} className="w-40" onChange={changeLimit} options={limitOptions} />
          <FieldError>{fields.error("limit")}</FieldError>
        </Row>
        {overLimit && (
          <p className="px-4 py-3 text-meta text-warn-foreground" role="status">
            Screenshots take {formatBytes(recordedBytes)} now. Daytrace deletes the oldest within 5 minutes until they fit in {s.storageLimitGb} GB.
          </p>
        )}
      </Group>

      <Group title="Free up space">
        <Row label="Delete screenshots older than" htmlFor={`${id}-older`} description="Deletes the screenshot files for good. Timeline cards stay.">
          <div className="flex items-center gap-2">
            <Input
              id={`${id}-older`}
              type="number"
              inputMode="numeric"
              min={1}
              max={365}
              value={olderDays}
              aria-invalid={olderValid ? undefined : true}
              aria-describedby={`${id}-older-unit`}
              onChange={(e) => setOlderDays(e.target.value)}
              className="w-20 tnum"
            />
            <span id={`${id}-older-unit`} className="text-meta text-muted-foreground">
              days
            </span>
            <Button
              variant="outline"
              disabled={busy !== null || !olderValid}
              onClick={() => setConfirm({ kind: "older", days, bytes: bytesBefore(days) })}
            >
              {busy === "older" && <Spinner {...SPINNER} />}
              {busy === "older" ? "Deleting…" : "Delete…"}
            </Button>
          </div>
          {!olderValid && <FieldError>Enter a number of days from 1 to 365.</FieldError>}
        </Row>
        <div aria-live="polite" className="empty:hidden">
          {outcome && <OutcomeLine outcome={outcome} className="mx-4 my-3" />}
        </div>
        {usage && usage.days.length === 0 && <p className="px-4 py-3 text-meta text-muted-foreground">No screenshots are stored right now.</p>}
        {usage && usage.days.length > 0 && (
          <div className="px-4 py-3">
            <table className="w-full text-meta">
              <caption className="pb-2 text-left text-body font-medium">Screenshots per day</caption>
              <thead>
                <tr className="border-b text-left text-caption text-muted-foreground">
                  <th scope="col" className="py-1.5 pr-3 font-normal">
                    Day
                  </th>
                  <th scope="col" className="py-1.5 pr-3 text-right font-normal">
                    Screenshots
                  </th>
                  <th scope="col" className="py-1.5 pr-3 text-right font-normal">
                    Size
                  </th>
                  <th scope="col" className="py-1.5 font-normal">
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {usage.days.map((d) => (
                  <tr key={d.day} className="border-b last:border-b-0">
                    <th scope="row" className="py-1 pr-3 text-left font-normal">
                      {formatDayTitle(d.day)}
                    </th>
                    <td className="py-1 pr-3 text-right tnum">{count(d.screenshots)}</td>
                    <td className="py-1 pr-3 text-right tnum">{formatBytes(d.bytes)}</td>
                    <td className="py-1 text-right">
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={busy !== null || d.screenshots === 0}
                        aria-label={`Delete the screenshots of ${formatDayTitle(d.day)}`}
                        onClick={() => setConfirm({ kind: "day", day: d.day, screenshots: d.screenshots, bytes: d.bytes })}
                      >
                        {busy === `day:${d.day}` ? "Deleting…" : "Delete…"}
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Group>

      <Group title="Database">
        <Row
          label="Backups"
          description={
            <>
              <p>
                {newest
                  ? `Newest backup ${formatWhen(newest.createdAt)}; ${usage!.backups.length} kept, ${formatBytes(usage!.backupsBytes)}.`
                  : usage
                    ? "No backup yet."
                    : loadError
                      ? "The backups couldn’t be read."
                      : "Reading backups…"}
              </p>
              <p>Daytrace backs up its database once a day and keeps the last 7 backups. It checks the database at every start and restores the newest good backup if the file is damaged.</p>
            </>
          }
        >
          <Button variant="outline" disabled={busy !== null} onClick={() => void run("backup", async () => {
            const b = await storageApi.backupNow();
            return { ok: true, text: `Backup saved as ${b.name} (${formatBytes(b.bytes)}).` };
          }, setBackupOutcome)}>
            {busy === "backup" ? <Spinner {...SPINNER} /> : <DatabaseBackup aria-hidden />}
            {busy === "backup" ? "Backing up…" : "Back up now"}
          </Button>
        </Row>
        <div aria-live="polite" className="empty:hidden">
          {backupOutcome && <OutcomeLine outcome={backupOutcome} className="mx-4 my-3" />}
        </div>
      </Group>

      <Group title="Data folder" description="Screenshots, the timeline database, backups and logs are stored here.">
        <Row label={<span className="font-mono text-meta font-normal [overflow-wrap:anywhere]" translate="no">{app.settings.dataDir}</span>}>
          <Button
            variant="outline"
            onClick={() => {
              setFolderError(null);
              api.openDataDir().catch((e) => setFolderError(`Couldn’t open the folder: ${errorText(e)}`));
            }}
          >
            <FolderOpen aria-hidden /> Open folder
          </Button>
          <FieldError>{folderError}</FieldError>
        </Row>
      </Group>

      <AlertDialog open={confirm !== null} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>{confirm && <ConfirmBody c={confirm} retention={s.retentionDays} onConfirm={() => confirmed(confirm)} />}</AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function ConfirmBody({ c, retention, onConfirm }: { c: Confirm; retention: number; onConfirm: () => void }) {
  let title: string;
  let text: string;
  let action: string;
  let cancel = "Keep screenshots";
  if (c.kind === "older") {
    title = `Delete screenshots older than ${c.days} ${c.days === 1 ? "day" : "days"}?`;
    text = `${c.bytes > 0 ? `This frees about ${formatBytes(c.bytes)}. ` : ""}Timeline cards and their notes stay. Screenshots from the last hour and those still waiting for analysis are kept.`;
    action = c.bytes > 0 ? `Delete about ${formatBytes(c.bytes)} of screenshots` : "Delete screenshots";
  } else if (c.kind === "day") {
    title = `Delete the screenshots of ${formatDayTitle(c.day)}?`;
    text = `${count(c.screenshots)} screenshots, ${formatBytes(c.bytes)}. The day’s timeline cards stay, but the screenshot viewer and timelapse will be empty and the day can’t be analyzed again.`;
    action = `Delete ${formatBytes(c.bytes)} of screenshots`;
  } else {
    title = `Limit screenshots to ${c.gb} GB?`;
    text = `Screenshots take ${formatBytes(c.over)} more than that now. Within 5 minutes Daytrace deletes the oldest until they fit, in addition to the ${retention}-day limit. Timeline cards stay.`;
    action = `Delete about ${formatBytes(c.over)} of the oldest screenshots`;
    cancel = "Keep the current limit";
  }
  return (
    <>
      <AlertDialogHeader>
        <AlertDialogTitle>{title}</AlertDialogTitle>
        <AlertDialogDescription>{text} Deleted screenshots can’t be recovered.</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel>{cancel}</AlertDialogCancel>
        <AlertDialogAction variant="destructive" onClick={onConfirm}>
          {action}
        </AlertDialogAction>
      </AlertDialogFooter>
    </>
  );
}

function HealthNotice({ h }: { h: DbHealth }) {
  if (h.status === "restored") {
    return (
      <Banner tone="warn" title="The database was restored from a backup">
        At startup Daytrace found damage in its database and restored the backup from{" "}
        {h.restoredBackupAt ? formatWhen(h.restoredBackupAt) : h.restoredFrom}. Activity recorded after that backup is missing. The damaged file was kept in
        the data folder as {h.movedTo}.
      </Banner>
    );
  }
  if (h.status === "reset") {
    return (
      <Banner tone="warn" title="Daytrace started a new database">
        At startup Daytrace found damage in its database and no usable backup, so it started a new, empty database. The damaged file was kept in the data
        folder as {h.movedTo}.
      </Banner>
    );
  }
  if (h.status === "damaged") {
    return (
      <ErrorBanner title="The database is damaged" message={h.detail ?? "No details."}>
        Daytrace could not move the file aside. Quit Daytrace, move daytrace.sqlite out of the data folder, and start Daytrace again to restore the newest
        backup.
      </ErrorBanner>
    );
  }
  return null;
}
