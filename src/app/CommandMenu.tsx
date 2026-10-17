import * as React from "react";
import { CalendarClock, CalendarSearch, Download, FolderOpen, Images, Laptop, Moon, Pause, Play, RotateCcw, Sun } from "lucide-react";
import { api } from "@/api";
import { isDaySection, useDay } from "@/app/day";
import { NAV, SECTION_TITLES, SETTINGS_NAV } from "@/app/Shell";
import { useAppSettings } from "@/app/state";
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
import { navigate, useRoute } from "@/lib/router";
import { clock, formatDayTitle, parseDayQuery, shiftDay } from "@/lib/time";
import type { Appearance } from "@/lib/theme";
import { errorText } from "@/lib/utils";
import { OPEN_ONBOARDING_EVENT, pauseOptions, platformApi } from "@/views/platform-api";
import { SETTINGS_SECTION_META } from "@/views/settings/sections";

/**
 * Ctrl+K: every section, days (typed as "12 oct", "monday", "yesterday", "2026-10-12") and the
 * frequent actions, reachable without the mouse. A day opens in the day section you are in, else
 * in the Timeline.
 */
export function CommandMenu({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const app = useAppSettings();
  const { day, today } = useDay();
  const { section } = useRoute();
  const s = app.settings.settings;
  const [search, setSearch] = React.useState("");
  React.useEffect(() => {
    if (!open) setSearch("");
  }, [open]);

  const run = (fn: () => void | Promise<void>) => {
    onOpenChange(false);
    Promise.resolve()
      .then(fn)
      .catch((e) => toast("That didn’t work", { description: errorText(e), tone: "error" }));
  };

  const theme = (a: Appearance) => run(() => app.saveAppearance(a));
  const daySection = isDaySection(section) ? section : "timeline";
  const openDay = (d: string) => run(() => navigate(daySection, { day: d }));
  const typed = parseDayQuery(search, today);
  const relative = (d: string) => (d === today ? "Today" : d === shiftDay(today, -1) ? "Yesterday" : null);

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Jump to"
      description="Go to a section or a day, or run an action"
      returnFocus={() => document.getElementById("content")}
    >
      <CommandInput value={search} onValueChange={setSearch} placeholder="Type a section, a day (12 Oct, Monday) or an action…" />
      <CommandList className="max-h-[min(24rem,60vh)]">
        <CommandEmpty>{search.trim() ? "Nothing matches. Days work as “12 Oct”, “Monday” or “2026-10-12”." : "Nothing matches."}</CommandEmpty>
        {typed && (
          <CommandGroup heading="Day" forceMount>
            <CommandItem forceMount value={`typed day ${typed}`} onSelect={() => openDay(typed)}>
              <CalendarSearch aria-hidden="true" />
              {formatDayTitle(typed)}
              {typed.slice(0, 4) !== today.slice(0, 4) && <span className="text-muted-foreground">{typed.slice(0, 4)}</span>}
              {relative(typed) && <span className="text-muted-foreground">{relative(typed)}</span>}
              <CommandShortcut>{SECTION_TITLES[daySection]}</CommandShortcut>
            </CommandItem>
          </CommandGroup>
        )}
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
              <CommandItem key={d} value={`day ${formatDayTitle(d)} ${i === 0 ? "today" : i === 1 ? "yesterday" : ""}`} onSelect={() => openDay(d)}>
                <CalendarClock aria-hidden="true" />
                {formatDayTitle(d)}
                {relative(d) && <span className="text-muted-foreground">{relative(d)}</span>}
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
                Pause recording: {lowerFirst(o.label)}
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
          <CommandItem value="export markdown copy day as text" onSelect={() => run(() => navigate("timeline", { day, export: 1 }))}>
            <Download aria-hidden="true" />
            Export…
            <span className="text-muted-foreground">{day === today ? "Today" : formatDayTitle(day)}</span>
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
        <CommandGroup heading="Settings">
          {SETTINGS_SECTION_META.map((m) => (
            <CommandItem key={m.id} value={`settings ${m.label} ${m.keywords}`} onSelect={() => run(() => navigate("settings", { section: m.id }))}>
              <m.icon aria-hidden="true" />
              Settings: {lowerFirst(m.label)}
            </CommandItem>
          ))}
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

/** "Model provider" -> "model provider"; keeps acronyms such as "AI tools". */
function lowerFirst(s: string): string {
  return /^[A-Z]{2}/.test(s) ? s : s.charAt(0).toLowerCase() + s.slice(1);
}
