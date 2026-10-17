import * as React from "react";
import { ChevronDown, CircleAlert, CircleCheck, Hourglass, KeyRound, TriangleAlert } from "lucide-react";
import { api } from "@/api";
import { useNotAnalyzedToday } from "@/app/badges";
import { useAppSettings, useAppStatus } from "@/app/state";
import { withNotAnalyzed, type AnalysisView, type RecorderView } from "@/app/status";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { clock } from "@/lib/time";
import { cn, errorText } from "@/lib/utils";
import { pauseOptions } from "@/views/platform-api";

/*
 * Recording and analysis status, always in the rail (designer-fluent's status block). Pause and
 * Resume are text buttons. Pause opens the same timed choices as the tray's "Pause for" menu.
 * Collapsed rail: one button with the recorder mark that opens the same block in a popover.
 *
 * Screen readers hear changes from one live region that is always rendered (the block itself is
 * display:none below 1100 px), with the recorder and analysis lines only (UX5).
 */

function Dot({ tone, className }: { tone: RecorderView["tone"]; className?: string }) {
  return (
    <span
      aria-hidden="true"
      data-mark=""
      className={cn(
        "size-2 shrink-0 rounded-full",
        tone === "live" && "bg-live motion-safe:animate-pulse-live",
        tone === "error" && "bg-destructive",
        tone === "paused" && "bg-paused",
        className,
      )}
    />
  );
}

/**
 * Collapsed rail: the recorder's state as a mark, never as an action glyph (UX9). Recording: a
 * ring with a live dot. Paused or waiting: the same ring, empty and struck through. Failing: the
 * ring struck through in the error color.
 */
function RecorderGlyph({ tone }: { tone: RecorderView["tone"] }) {
  if (tone === "live") {
    return (
      <span aria-hidden="true" className="grid size-[1.125rem] place-items-center rounded-full border-2 border-current">
        <span data-mark="" className="size-2 rounded-full bg-live motion-safe:animate-pulse-live" />
      </span>
    );
  }
  return (
    <span aria-hidden="true" className={cn("relative grid size-[1.125rem] place-items-center rounded-full border-2 border-current", tone === "error" && "text-destructive")}>
      <span className="absolute h-0.5 w-[1.375rem] -rotate-45 rounded-full bg-current forced-colors:bg-[CanvasText] forced-colors:forced-color-adjust-none" />
    </span>
  );
}

function AnalysisIcon({ a }: { a: AnalysisView }) {
  if (a.icon === "key") return <KeyRound className="size-3.5 text-warn-foreground" aria-hidden="true" />;
  switch (a.tone) {
    case "busy":
      return <Spinner className="size-3.5" aria-hidden="true" role={undefined} aria-label={undefined} />;
    case "waiting":
      return <Hourglass className="size-3.5" aria-hidden="true" />;
    case "warn":
      return <TriangleAlert className="size-3.5 text-warn-foreground" aria-hidden="true" />;
    case "error":
      return <CircleAlert className="size-3.5 text-destructive" aria-hidden="true" />;
    default:
      return <CircleCheck className="size-3.5 text-live-foreground" aria-hidden="true" />;
  }
}

/** The analysis line of the rail: the app's wording plus today's "Not analyzed" cards (UX7). */
function useRailAnalysis(): AnalysisView {
  const { analysis } = useAppStatus();
  const failed = useNotAnalyzedToday();
  return withNotAnalyzed(analysis, failed.count, failed.href);
}

function useRecordingActions() {
  const app = useAppSettings();
  const [busy, setBusy] = React.useState(false);
  // Pause and Resume swap places; the button that appears takes the focus (UX4). The buttons are
  // disabled while the action runs, so the focus is lost meanwhile; it comes back afterwards, to
  // the same button when the action failed.
  const focusNext = React.useRef(false);
  const pause = async (choice: string, label: string) => {
    setBusy(true);
    focusNext.current = true;
    try {
      await app.pause(choice);
      const end = (await api.getSettings()).settings.pauseUntil;
      toast("Recording paused", {
        description: end ? `Recording resumes at ${clock(end)}. Resume earlier here or from the tray icon.` : `${label}. Resume here or from the tray icon.`,
      });
    } catch (e) {
      toast("Couldn’t pause recording", { description: errorText(e), tone: "error" });
    } finally {
      setBusy(false);
    }
  };
  const resume = async () => {
    setBusy(true);
    focusNext.current = true;
    try {
      await app.resume();
      toast("Recording resumed", { tone: "success" });
    } catch (e) {
      toast("Couldn’t resume recording", { description: errorText(e), tone: "error" });
    } finally {
      setBusy(false);
    }
  };
  return { busy, pause, resume, focusNext };
}

/** The status block's content: recorder line, Pause/Resume, analysis line. */
function StatusPanel({ analysis, className }: { analysis: AnalysisView; className?: string }) {
  const { settings } = useAppSettings();
  const { recorder } = useAppStatus();
  const { busy, pause, resume, focusNext } = useRecordingActions();
  const options = pauseOptions(settings.settings.pausePresets);
  const actionRef = React.useRef<HTMLButtonElement>(null);
  const panelRef = React.useRef<HTMLElement>(null);

  // Once the action is done (the buttons are enabled again), the new button takes the focus, if
  // the focus was lost with the old one or is still in this block.
  React.useEffect(() => {
    if (!focusNext.current || busy) return;
    focusNext.current = false;
    const a = document.activeElement;
    if (a && a !== document.body && !panelRef.current?.contains(a)) return;
    actionRef.current?.focus();
  }, [recorder.userPaused, busy, focusNext]);

  return (
    <section ref={panelRef} aria-label="Recording status" className={cn("rounded-lg border bg-surface-1 text-meta shadow-xs", className)}>
      <div className="px-3 pt-2">
        <div className="flex items-start gap-2">
          <span className="grid h-7 shrink-0 place-items-center">
            <Dot tone={recorder.tone} />
          </span>
          <p className={cn("min-w-0 flex-1 py-[0.3125rem] font-semibold", recorder.tone === "error" && "text-destructive")}>{recorder.label}</p>
          {recorder.userPaused ? (
            <Button ref={actionRef} size="sm" className="-mr-1" onClick={resume} disabled={busy}>
              Resume
            </Button>
          ) : (
            <DropdownMenu modal={false}>
              <DropdownMenuTrigger asChild>
                <Button ref={actionRef} size="sm" variant="outline" className="-mr-1 gap-1 pr-1.5" disabled={busy} aria-label="Pause recording for…">
                  Pause
                  <ChevronDown aria-hidden="true" className="size-3.5 text-muted-foreground" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent side="right" align="start" className="w-56">
                <DropdownMenuLabel className="text-caption font-normal text-muted-foreground">Pause recording for</DropdownMenuLabel>
                {options.slice(0, 3).map((o) => (
                  <DropdownMenuItem key={o.id} onSelect={() => pause(o.id, `Paused for ${o.label}`)}>
                    {o.label}
                  </DropdownMenuItem>
                ))}
                <DropdownMenuSeparator />
                {options.slice(3).map((o) => (
                  <DropdownMenuItem key={o.id} onSelect={() => pause(o.id, o.id === "tomorrow" ? "Paused until 4 AM" : "Paused until you resume")}>
                    {o.label}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>
        <p className="pl-4 text-caption text-muted-foreground tnum">{recorder.detail}</p>
      </div>
      <div className="mt-2.5 flex items-start gap-2 border-t px-3 py-2.5">
        <span className="mt-0.5 grid size-3.5 shrink-0 place-items-center text-muted-foreground">
          <AnalysisIcon a={analysis} />
        </span>
        <div className="min-w-0 flex-1">
          <p className={cn(analysis.tone === "error" && "text-destructive")}>{analysis.label}</p>
          <p className="text-caption break-words text-muted-foreground tnum">{analysis.detail}</p>
          {analysis.link && (
            <a href={analysis.link.href} className="mt-0.5 inline-block rounded-sm text-caption font-medium text-foreground underline underline-offset-2">
              {analysis.link.label}
            </a>
          )}
        </div>
      </div>
    </section>
  );
}

/** Rail status: full block from 1100 px, one button with a popover below. */
export function StatusBlock() {
  const { recorder } = useAppStatus();
  const analysis = useRailAnalysis();
  const attention = recorder.tone === "error" || analysis.tone === "error" || analysis.tone === "warn";
  const summary = `${recorder.label}. ${analysis.label}.`;
  return (
    <>
      {/* The one live region for status changes, at every width (UX5). */}
      <p role="status" className="sr-only">
        {summary}
      </p>
      <StatusPanel analysis={analysis} className="mb-2 hidden wide:block" />
      <Popover>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <button
                type="button"
                aria-label={`Status: ${summary} Show details`}
                className="relative mb-1 grid h-9 w-full place-items-center rounded-md text-muted-foreground hover:bg-surface-2 hover:text-foreground wide:hidden"
              >
                <RecorderGlyph tone={recorder.tone} />
                {attention && (
                  <span aria-hidden="true" data-mark="" className="absolute top-1.5 right-2.5 size-2 rounded-full bg-destructive ring-2 ring-background" />
                )}
              </button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent side="right" className="max-w-64">
            <p className="font-semibold">{recorder.label}</p>
            <p>{analysis.label}</p>
          </TooltipContent>
        </Tooltip>
        <PopoverContent side="right" align="end" aria-label="Recording status" className="w-64 border-none bg-transparent p-0 shadow-none">
          <StatusPanel analysis={analysis} className="shadow-lg" />
        </PopoverContent>
      </Popover>
    </>
  );
}
