import * as React from "react";
import { CalendarClock, CalendarSearch, Download, FolderOpen, Images, Laptop, ListChecks, Moon, Pause, Play, RotateCcw, Sun } from "lucide-react";
import { defaultFilter } from "cmdk";
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
  const days = [0, 1, 2, 3].map((i) => ({ d: shiftDay(today, -i), i }));
  const dayValue = (d: string, i: number) => `day ${formatDayTitle(d)} ${i === 0 ? "today" : i === 1 ? "yesterday" : ""}`;
  const pauses = s.recordingEnabled ? pauseOptions(s.pausePresets) : [];

  // Each group with the values of its items. cmdk sorts the items inside a group by how well they
  // match, but not the groups (its group sort looks them up by an id they do not carry), and Enter
  // runs the first item in the list: typing "rate" ran a day, whose name fuzzy-matches it. So the
  // groups are put in order here, best match first (ties keep the usual order).
  const groups: { key: string; values: string[]; separated?: boolean; node: React.ReactNode }[] = [
    {
      key: "go",
      values: [...NAV, SETTINGS_NAV].map((n) => `go ${n.label}`),
      node: (
        <CommandGroup heading="Go to">
          {[...NAV, SETTINGS_NAV].map((n) => (
            <CommandItem key={n.id} value={`go ${n.label}`} onSelect={() => run(() => navigate(n.id, isDaySection(n.id) ? { day } : {}))}>
              <n.icon aria-hidden="true" />
              {n.label}
              <CommandShortcut>Ctrl+{n.key}</CommandShortcut>
            </CommandItem>
          ))}
        </CommandGroup>
      ),
    },
    {
      key: "days",
      values: days.map(({ d, i }) => dayValue(d, i)),
      node: (
        <CommandGroup heading="Days">
          {days.map(({ d, i }) => (
            <CommandItem key={d} value={dayValue(d, i)} onSelect={() => openDay(d)}>
              <CalendarClock aria-hidden="true" />
              {formatDayTitle(d)}
              {relative(d) && <span className="text-muted-foreground">{relative(d)}</span>}
            </CommandItem>
          ))}
        </CommandGroup>
      ),
    },
    {
      key: "recording",
      separated: true,
      values: s.recordingEnabled ? pauses.map((o) => `pause recording ${o.label}`) : ["resume recording"],
      node: (
        <CommandGroup heading="Recording">
          {s.recordingEnabled ? (
            pauses.map((o) => (
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
      ),
    },
    {
      key: "actions",
      separated: true,
      values: [ACTION_EXPORT, ACTION_RATE, "open screenshots folder", "open data folder", "run first-run setup again onboarding"],
      node: (
        <CommandGroup heading="Actions">
          <CommandItem value={ACTION_EXPORT} onSelect={() => run(() => navigate("timeline", { day, export: 1 }))}>
            <Download aria-hidden="true" />
            Export…
            <span className="text-muted-foreground">{day === today ? "Today" : formatDayTitle(day)}</span>
          </CommandItem>
          {/* Review mode of the Timeline (gap 20, w5-r1). */}
          <CommandItem value={ACTION_RATE} onSelect={() => run(() => navigate("timeline", { day, review: 1 }))}>
            <ListChecks aria-hidden="true" />
            Rate the day’s cards…
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
      ),
    },
    {
      key: "settings",
      separated: true,
      values: SETTINGS_SECTION_META.map((m) => `settings ${m.label} ${m.keywords}`),
      node: (
        <CommandGroup heading="Settings">
          {SETTINGS_SECTION_META.map((m) => (
            <CommandItem key={m.id} value={`settings ${m.label} ${m.keywords}`} onSelect={() => run(() => navigate("settings", { section: m.id }))}>
              <m.icon aria-hidden="true" />
              Settings: {lowerFirst(m.label)}
            </CommandItem>
          ))}
        </CommandGroup>
      ),
    },
    {
      key: "theme",
      separated: true,
      values: ["theme follow windows system", "theme light", "theme dark"],
      node: (
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
      ),
    },
  ];
  const ordered = orderGroups(groups, search);

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
        {ordered.map((g) => (
          <React.Fragment key={g.key}>
            {g.separated && <CommandSeparator />}
            {g.node}
          </React.Fragment>
        ))}
      </CommandList>
    </CommandDialog>
  );
}

const ACTION_EXPORT = "export markdown copy day as text";
const ACTION_RATE = "rate cards review focused neutral distracted";

/** Groups by their best match for the search (cmdk's own score), best first; ties keep their order. */
function orderGroups<T extends { values: string[] }>(groups: T[], search: string): T[] {
  if (!search.trim()) return groups;
  const best = (g: T) => g.values.reduce((m, v) => Math.max(m, defaultFilter(v, search)), 0);
  return groups
    .map((g, i) => ({ g, i, score: best(g) }))
    .sort((a, b) => b.score - a.score || a.i - b.i)
    .map((x) => x.g);
}

/** "Model provider" -> "model provider"; keeps acronyms such as "AI tools". */
function lowerFirst(s: string): string {
  return /^[A-Z]{2}/.test(s) ? s : s.charAt(0).toLowerCase() + s.slice(1);
}
