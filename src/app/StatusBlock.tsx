import * as React from "react";
import { ChevronDown, CircleAlert, CircleCheck, Hourglass, KeyRound, Pause, ScreenShareOff } from "lucide-react";
import { api } from "@/api";
import { useAppState } from "@/app/state";
import type { AnalysisView, RecorderView } from "@/app/status";
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
 * Collapsed rail: one button with the status dot that opens the same block in a popover.
 */

function Dot({ tone, className }: { tone: RecorderView["tone"]; className?: string }) {
  return (
    <span
      aria-hidden="true"
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

/** Collapsed rail: a record mark while recording, pause bars when paused, an alert when failing. */
function RecorderGlyph({ tone }: { tone: RecorderView["tone"] }) {
  if (tone === "error") return <ScreenShareOff className="size-[1.125rem] text-destructive" aria-hidden="true" />;
  if (tone === "paused") return <Pause className="size-[1.125rem]" aria-hidden="true" />;
  return (
    <span aria-hidden="true" className="grid size-[1.125rem] place-items-center rounded-full border-2 border-current">
      <span className="size-2 rounded-full bg-live motion-safe:animate-pulse-live" />
    </span>
  );
}

function AnalysisIcon({ a }: { a: AnalysisView }) {
  switch (a.tone) {
    case "busy":
      return <Spinner className="size-3.5" aria-hidden="true" role={undefined} aria-label={undefined} />;
    case "waiting":
      return <Hourglass className="size-3.5" aria-hidden="true" />;
    case "warn":
      return <KeyRound className="size-3.5 text-warn-foreground" aria-hidden="true" />;
    case "error":
      return <CircleAlert className="size-3.5 text-destructive" aria-hidden="true" />;
    default:
      return <CircleCheck className="size-3.5 text-live-foreground" aria-hidden="true" />;
  }
}

function useRecordingActions() {
  const app = useAppState();
  const [busy, setBusy] = React.useState(false);
  const pause = async (choice: string, label: string) => {
    setBusy(true);
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
    try {
      await app.resume();
      toast("Recording resumed", { tone: "success" });
    } catch (e) {
      toast("Couldn’t resume recording", { description: errorText(e), tone: "error" });
    } finally {
      setBusy(false);
    }
  };
  return { busy, pause, resume };
}

/** The status block's content: recorder line, Pause/Resume, analysis line. */
function StatusPanel({ className }: { className?: string }) {
  const { recorder, analysis, settings } = useAppState();
  const { busy, pause, resume } = useRecordingActions();
  const options = pauseOptions(settings.settings.pausePresets);

  return (
    <section aria-label="Recording status" className={cn("rounded-lg border bg-surface-1 text-meta shadow-xs", className)}>
      <div className="px-3 pt-2">
        <div className="flex min-h-7 items-center gap-2">
          <Dot tone={recorder.tone} />
          <p role="status" className={cn("min-w-0 flex-1 font-semibold", recorder.tone === "error" && "text-destructive")}>
            {recorder.label}
          </p>
          {recorder.userPaused ? (
            <Button size="sm" className="-mr-1" onClick={resume} disabled={busy}>
              Resume
            </Button>
          ) : (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button size="sm" variant="outline" className="-mr-1 gap-1 pr-1.5" disabled={busy} aria-label="Pause recording for…">
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
          <p aria-live="polite" className={cn(analysis.tone === "error" && "text-destructive")}>
            {analysis.label}
          </p>
          <p className="text-caption break-words text-muted-foreground tnum">{analysis.detail}</p>
        </div>
      </div>
    </section>
  );
}

/** Rail status: full block from 1100 px, one button with a popover below. */
export function StatusBlock() {
  const { recorder, analysis } = useAppState();
  const attention = recorder.tone === "error" || analysis.tone === "error" || analysis.tone === "warn";
  const summary = `${recorder.label}. ${analysis.label}.`;
  return (
    <>
      <StatusPanel className="mb-2 hidden wide:block" />
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
                  <span aria-hidden="true" className="absolute top-1.5 right-2.5 size-2 rounded-full bg-destructive ring-2 ring-background" />
                )}
              </button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent side="right" className="max-w-64">
            <p className="font-semibold">{recorder.label}</p>
            <p>{analysis.label}</p>
          </TooltipContent>
        </Tooltip>
        <PopoverContent side="right" align="end" className="w-64 border-none bg-transparent p-0 shadow-none">
          <StatusPanel className="shadow-lg" />
        </PopoverContent>
      </Popover>
    </>
  );
}
