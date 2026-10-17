import * as React from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { ClipboardCopy, FileDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { ReturnFocus } from "@/components/ui/focus-return";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "@/components/ui/toast";
import { formatDayShort, formatDayTitle, monthDay, shiftDay, weekStart } from "@/lib/time";
import { cn, errorText } from "@/lib/utils";
import { timelineApi } from "./api";

/*
 * Export from the page header (Dayflow TimelineClipboardFormatter, Daytrace export.rs): a date
 * range as one Markdown file, or the open day as plain text on the clipboard. Each card carries its
 * category and subcategory, summary, step-by-step log and interruptions.
 */

export function ExportDialog({
  open,
  onOpenChange,
  day,
  today,
  returnFocus,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  day: string;
  today: string;
  /** Focus on close when the opener is gone (opened from a link or the command menu). */
  returnFocus?: ReturnFocus;
}) {
  const [from, setFrom] = React.useState(shiftDay(day, -6));
  const [to, setTo] = React.useState(day);
  const [busy, setBusy] = React.useState<"export" | "copy" | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!open) return;
    setFrom(shiftDay(day, -6));
    setTo(day);
    setError(null);
    setBusy(null);
  }, [open, day]);

  const wk = weekStart(day);
  const presets: { label: string; from: string; to: string }[] = [
    { label: "This day", from: day, to: day },
    { label: "Last 7 days", from: shiftDay(day, -6), to: day },
    { label: wk === weekStart(today) ? "This week" : `Week of ${monthDay(wk)}`, from: wk, to: shiftDay(wk, 6) > today ? today : shiftDay(wk, 6) },
  ];
  const invalid = !from || !to || from > to;

  const run = async () => {
    if (invalid) return;
    setError(null);
    let path: string | null;
    try {
      path = await save({ defaultPath: from === to ? `daytrace-${from}.md` : `daytrace-${from}-to-${to}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
    } catch (e) {
      setError(`Couldn’t open the save dialog: ${errorText(e)}`);
      return;
    }
    if (!path) return;
    setBusy("export");
    try {
      const n = await timelineApi.exportMarkdown(from, to, path);
      toast(n === 0 ? "Exported, with no cards in that range" : `Exported ${n} ${n === 1 ? "card" : "cards"}`, { tone: "success", description: path });
      onOpenChange(false);
    } catch (e) {
      setError(`Couldn’t export: ${errorText(e)}`);
    } finally {
      setBusy(null);
    }
  };

  const copyDay = async () => {
    setError(null);
    setBusy("copy");
    try {
      await navigator.clipboard.writeText(await timelineApi.timelineText(day));
      toast(`Copied ${formatDayTitle(day)} as text`, { tone: "success" });
      onOpenChange(false);
    } catch (e) {
      setError(`Couldn’t copy the day: ${errorText(e)}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="gap-5 sm:max-w-[30rem]"
        returnFocus={returnFocus}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          document.getElementById("export-from")?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle className="font-display text-title font-semibold">Export</DialogTitle>
          <DialogDescription className="text-meta">
            Each card comes with its category and subcategory, summary, step-by-step log and interruptions. Copy day as text copies{" "}
            {day === today ? "today" : formatDayTitle(day)} to the clipboard.
          </DialogDescription>
        </DialogHeader>

        <form
          id="export-form"
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void run();
          }}
        >
          <fieldset className="flex flex-col gap-3">
            <legend className="mb-2 text-body font-semibold">Markdown file for a date range</legend>
            <div className="flex flex-wrap gap-1.5" role="group" aria-label="Quick ranges">
              {presets.map((p) => {
                const on = p.from === from && p.to === to;
                return (
                  <Button
                    key={p.label}
                    type="button"
                    size="sm"
                    variant="outline"
                    aria-pressed={on}
                    data-choice=""
                    className={cn("h-7", on && "border-foreground bg-surface-2 font-semibold")}
                    onClick={() => {
                      setFrom(p.from);
                      setTo(p.to);
                    }}
                  >
                    {p.label}
                  </Button>
                );
              })}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="export-from">From</Label>
                <Input id="export-from" type="date" value={from} max={to || today} onChange={(e) => setFrom(e.target.value)} aria-invalid={invalid || undefined} />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="export-to">To</Label>
                <Input id="export-to" type="date" value={to} min={from} max={today} onChange={(e) => setTo(e.target.value)} aria-invalid={invalid || undefined} />
              </div>
            </div>
            <p className={cn("text-caption", invalid ? "text-destructive" : "text-muted-foreground")} aria-live="polite">
              {invalid ? "Pick a start date on or before the end date." : from === to ? formatDayTitle(from) : `${formatDayShort(from)} to ${formatDayShort(to)}`}
            </p>
          </fieldset>
        </form>

        {error && (
          <p role="alert" className="rounded-md border border-destructive/40 bg-destructive-surface px-3 py-2 text-meta">
            {error}
          </p>
        )}

        <DialogFooter className="gap-2 sm:justify-between">
          <Button type="button" variant="outline" onClick={() => void copyDay()} disabled={busy !== null}>
            {busy === "copy" ? <Spinner className="size-4" aria-hidden="true" role="presentation" /> : <ClipboardCopy aria-hidden="true" />}
            Copy day as text
          </Button>
          <Button type="submit" form="export-form" disabled={invalid || busy !== null}>
            {busy === "export" ? <Spinner className="size-4" aria-hidden="true" role="presentation" /> : <FileDown aria-hidden="true" />}
            Export as Markdown
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
