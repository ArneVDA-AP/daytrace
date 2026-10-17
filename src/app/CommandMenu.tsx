import { CalendarClock, Download, FolderOpen, Images, Laptop, Moon, Pause, Play, RotateCcw, Sun } from "lucide-react";
import { api } from "@/api";
import { useDay } from "@/app/day";
import { NAV, SETTINGS_NAV } from "@/app/Shell";
import { useAppState } from "@/app/state";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command";
import { toast } from "@/components/ui/toast";
import { isDaySection } from "@/app/day";
import { navigate } from "@/lib/router";
import { clock, formatDayTitle, shiftDay } from "@/lib/time";
import type { Appearance } from "@/lib/theme";
import { errorText } from "@/lib/utils";
import { OPEN_ONBOARDING_EVENT, pauseOptions, platformApi } from "@/views/platform-api";

/** Ctrl+K: every section, recent days and the frequent actions, reachable without the mouse. */
export function CommandMenu({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const app = useAppState();
  const { day, today } = useDay();
  const s = app.settings.settings;

  const run = (fn: () => void | Promise<void>) => {
    onOpenChange(false);
    Promise.resolve()
      .then(fn)
      .catch((e) => toast("That didn’t work", { description: errorText(e), tone: "error" }));
  };

  const theme = (a: Appearance) => run(() => app.saveAppearance(a));

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange} title="Jump to" description="Go to a section or a day, or run an action">
      <CommandInput placeholder="Type a section, a day or an action…" />
      <CommandList className="max-h-[min(24rem,60vh)]">
        <CommandEmpty>Nothing matches.</CommandEmpty>
        <CommandGroup heading="Go to">
          {[...NAV, SETTINGS_NAV].map((n) => (
            <CommandItem key={n.id} value={`go ${n.label}`} onSelect={() => run(() => navigate(n.id, isDaySection(n.id) ? { day } : {}))}>
              <n.icon aria-hidden="true" />
              {n.label}
              <CommandShortcut>Ctrl+{n.key}</CommandShortcut>
            </CommandItem>
          ))}
        </CommandGroup>
        <CommandGroup heading="Days">
          {[0, 1, 2, 3].map((i) => {
            const d = shiftDay(today, -i);
            return (
              <CommandItem key={d} value={`day ${formatDayTitle(d)} ${i === 0 ? "today" : i === 1 ? "yesterday" : ""}`} onSelect={() => run(() => navigate("timeline", { day: d }))}>
                <CalendarClock aria-hidden="true" />
                {formatDayTitle(d)}
                {i < 2 && <span className="text-muted-foreground">{i === 0 ? "Today" : "Yesterday"}</span>}
              </CommandItem>
            );
          })}
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Recording">
          {s.recordingEnabled ? (
            pauseOptions(s.pausePresets).map((o) => (
              <CommandItem
                key={o.id}
                value={`pause recording ${o.label}`}
                onSelect={() =>
                  run(async () => {
                    await app.pause(o.id);
                    const until = (await api.getSettings()).settings.pauseUntil;
                    toast("Recording paused", { description: until ? `Recording resumes at ${clock(until)}.` : "Resume from the rail or the tray icon." });
                  })
                }
              >
                <Pause aria-hidden="true" />
                Pause recording: {o.label.charAt(0).toLowerCase() + o.label.slice(1)}
              </CommandItem>
            ))
          ) : (
            <CommandItem
              value="resume recording"
              onSelect={() =>
                run(async () => {
                  await app.resume();
                  toast("Recording resumed", { tone: "success" });
                })
              }
            >
              <Play aria-hidden="true" />
              Resume recording
            </CommandItem>
          )}
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Actions">
          <CommandItem value="export day markdown" onSelect={() => run(() => navigate("timeline", { day, export: 1 }))}>
            <Download aria-hidden="true" />
            Export {day === today ? "today" : formatDayTitle(day)} as Markdown…
          </CommandItem>
          <CommandItem value="open screenshots folder" onSelect={() => run(() => platformApi.openScreenshots())}>
            <Images aria-hidden="true" />
            Open the screenshots folder
          </CommandItem>
          <CommandItem value="open data folder" onSelect={() => run(() => api.openDataDir())}>
            <FolderOpen aria-hidden="true" />
            Open the data folder
          </CommandItem>
          <CommandItem value="run first-run setup again onboarding" onSelect={() => run(() => void dispatchEvent(new Event(OPEN_ONBOARDING_EVENT)))}>
            <RotateCcw aria-hidden="true" />
            Run the first-run setup again
          </CommandItem>
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Theme">
          <CommandItem value="theme follow windows system" onSelect={() => theme("system")}>
            <Laptop aria-hidden="true" />
            Theme: follow Windows
            {s.appearance === "system" && <CommandShortcut>Current</CommandShortcut>}
          </CommandItem>
          <CommandItem value="theme light" onSelect={() => theme("light")}>
            <Sun aria-hidden="true" />
            Theme: light
            {s.appearance === "light" && <CommandShortcut>Current</CommandShortcut>}
          </CommandItem>
          <CommandItem value="theme dark" onSelect={() => theme("dark")}>
            <Moon aria-hidden="true" />
            Theme: dark
            {s.appearance === "dark" && <CommandShortcut>Current</CommandShortcut>}
          </CommandItem>
        </CommandGroup>
      </CommandList>
    </CommandDialog>
  );
}
