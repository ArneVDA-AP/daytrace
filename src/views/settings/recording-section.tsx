import * as React from "react";
import { shiftDay } from "@/api";
import { useAppState } from "@/app/state";
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
import { Switch } from "@/components/ui/switch";
import { formatDayTitle } from "@/api";
import { DEFAULT_PRESETS, minutesLabel } from "@/views/platform-api";
import { Choice, FieldError, Group, Row } from "./fields";
import { useFieldSaves, type Patch } from "./save";
import { formatBytes, storageApi } from "./storage-api";

/** Dayflow's PausePresets.minuteOptions. */
const MINUTE_OPTIONS = [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 480];
const RETENTION = [1, 3, 7, 14, 30, 90];
const days = (n: number) => (n === 1 ? "1 day" : `${n} days`);

interface RetentionConfirm {
  days: number;
  /** Screenshots that would go, measured from the per-day usage; null when unknown. */
  bytes: number | null;
  before: string;
}

/** Settings > Recording: capture, away pause, size, retention, start with Windows, pause lengths. */
export function RecordingSection() {
  const app = useAppState();
  const s = app.settings.settings;
  const fields = useFieldSaves();
  const id = React.useId();
  const [confirm, setConfirm] = React.useState<RetentionConfirm | null>(null);

  const apply = (field: string, value: unknown, patch: Patch) => void fields.apply(field, value, patch);
  const err = fields.error;
  const shown = fields.shown;

  /** Lowering retention deletes screenshots within minutes: say how much first. */
  async function changeRetention(n: number) {
    if (n >= s.retentionDays) return apply("retention", n, { retentionDays: n });
    const before = shiftDay(app.today, -n);
    let bytes: number | null = null;
    try {
      const usage = await storageApi.usage();
      bytes = usage.days.filter((d) => d.day < before).reduce((t, d) => t + d.bytes, 0);
    } catch {
      // Unknown: the dialog says it without a number.
    }
    if (bytes === 0) return apply("retention", n, { retentionDays: n });
    setConfirm({ days: n, bytes, before });
  }

  const presets = s.pausePresets.length === 3 ? s.pausePresets : DEFAULT_PRESETS;
  const minuteOptions = MINUTE_OPTIONS.map((m) => ({ value: m, label: minutesLabel(m) }));

  return (
    <>
      <Group title="Capture" description="Daytrace takes a screenshot of the screen with the active window. Screenshots stay on this PC; only the provider you chose sees them.">
        <Row label="Take a screenshot every" htmlFor={`${id}-interval`}>
          <Choice
            id={`${id}-interval`}
            value={shown("interval", s.captureIntervalSecs)}
            className="w-40"
            fallbackLabel={(v) => `${v} seconds`}
            onChange={(v) => apply("interval", v, { captureIntervalSecs: v })}
            options={[5, 10, 20, 30, 60].map((n) => ({ value: n, label: `${n} seconds` }))}
          />
          <FieldError>{err("interval")}</FieldError>
        </Row>
        <Row label="Screenshot size" htmlFor={`${id}-size`} description="Larger screenshots are easier for the model to read and take more disk space.">
          <Choice
            id={`${id}-size`}
            value={shown("size", s.captureHeight)}
            className="w-52"
            onChange={(v) => apply("size", v, { captureHeight: v })}
            options={[
              { value: 720, label: "720p (smaller files)" },
              { value: 1080, label: "1080p (easier to read)" },
            ]}
          />
          <FieldError>{err("size")}</FieldError>
        </Row>
        <Row
          label="Stop screenshots when I’m away for"
          htmlFor={`${id}-away`}
          description="“Away” means no keyboard or mouse input. While a video or call keeps the screen on, Daytrace keeps recording. Time away still shows on the timeline as Idle, without screenshots or model calls. Locking Windows always pauses."
        >
          <Choice
            id={`${id}-away`}
            value={shown("away", s.idlePauseSecs)}
            className="w-40"
            fallbackLabel={(v) => (v % 60 === 0 ? minutesLabel(v / 60) : `${v} seconds`)}
            onChange={(v) => apply("away", v, { idlePauseSecs: v })}
            options={[
              { value: 60, label: "1 minute" },
              { value: 120, label: "2 minutes" },
              { value: 300, label: "5 minutes" },
              { value: 600, label: "10 minutes" },
              { value: 0, label: "Never stop" },
            ]}
          />
          <FieldError>{err("away")}</FieldError>
        </Row>
        <Row label="Start Daytrace when I sign in to Windows" htmlFor={`${id}-login`} description="Daytrace starts in the tray.">
          <Switch id={`${id}-login`} checked={shown("login", s.launchAtLogin)} onCheckedChange={(v) => apply("login", v, { launchAtLogin: v })} />
          <FieldError>{err("login")}</FieldError>
        </Row>
      </Group>

      <Group title="Keep screenshots">
        <Row
          label="Keep screenshots for"
          htmlFor={`${id}-retention`}
          description="Timeline cards and the text descriptions of your screen are kept after their screenshots are deleted. The names of the apps and windows you used are kept for a year."
        >
          <Choice
            id={`${id}-retention`}
            value={shown("retention", s.retentionDays)}
            className="w-32"
            fallbackLabel={days}
            onChange={(v) => void changeRetention(v)}
            options={RETENTION.map((n) => ({ value: n, label: days(n) }))}
          />
          <FieldError>{err("retention")}</FieldError>
        </Row>
      </Group>

      <Group
        title="Pausing"
        description="The tray menu and the status panel offer three pause lengths, plus until tomorrow (4 AM) and until you resume. Recording resumes on its own when a timed pause ends, also after a restart."
      >
        {presets.map((m, i) => (
          <Row key={i} label={`Pause length ${i + 1}`} htmlFor={`${id}-preset-${i}`}>
            <Choice
              id={`${id}-preset-${i}`}
              value={shown(`preset-${i}`, m)}
              className="w-52"
              fallbackLabel={minutesLabel}
              onChange={(v) =>
                apply(`preset-${i}`, v, (cur) => ({ pausePresets: (cur.pausePresets.length === 3 ? cur.pausePresets : DEFAULT_PRESETS).map((x, j) => (j === i ? v : x)) }))
              }
              options={minuteOptions}
            />
            <FieldError>{err(`preset-${i}`)}</FieldError>
          </Row>
        ))}
      </Group>

      <AlertDialog open={confirm !== null} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>
          {confirm && (
            <>
              <AlertDialogHeader>
                <AlertDialogTitle>Keep screenshots for {days(confirm.days)}?</AlertDialogTitle>
                <AlertDialogDescription>
                  {confirm.bytes !== null
                    ? `Within a few minutes Daytrace deletes about ${formatBytes(confirm.bytes)} of screenshots from before ${formatDayTitle(confirm.before)}.`
                    : `Within a few minutes Daytrace deletes the screenshots older than ${days(confirm.days)}.`}{" "}
                  Timeline cards and their notes stay. Deleted screenshots can’t be recovered, and those days can’t be analyzed again.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Keep {days(s.retentionDays)}</AlertDialogCancel>
                <AlertDialogAction
                  variant="destructive"
                  onClick={() => {
                    apply("retention", confirm.days, { retentionDays: confirm.days });
                    setConfirm(null);
                  }}
                >
                  {confirm.bytes ? `Delete ${formatBytes(confirm.bytes)} of screenshots` : "Delete older screenshots"}
                </AlertDialogAction>
              </AlertDialogFooter>
            </>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
