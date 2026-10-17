import * as React from "react";
import { DayProvider, isDaySection, useDay } from "@/app/day";
import { LogoMark } from "@/app/Logo";
import { NAV, SECTION_TITLES, Shell } from "@/app/Shell";
import { AppStateProvider, useAppSettings } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/toast";
import { TooltipProvider } from "@/components/ui/tooltip";
import { digitOf, isKey, modsAre } from "@/lib/keys";
import { navigate, setParams, useRoute, type Section } from "@/lib/router";
import { OPEN_ONBOARDING_EVENT } from "@/views/platform-api";
import { useDayRollover, useNotificationNavigation } from "@/views/platform-hooks";
import TimelineSection from "@/views/timeline";

/*
 * One component per route section, each the default export of src/views/<area>/index.tsx.
 * Timeline is the landing section and ships in the start-up bundle; the others load on first
 * visit (from local disk, so the fallback is rarely seen).
 */
const SECTIONS: Record<Section, React.ComponentType> = {
  timeline: TimelineSection,
  daily: React.lazy(() => import("@/views/daily")),
  weekly: React.lazy(() => import("@/views/weekly")),
  journal: React.lazy(() => import("@/views/journal")),
  chat: React.lazy(() => import("@/views/chat")),
  settings: React.lazy(() => import("@/views/settings")),
};
const OnboardingScreen = React.lazy(() => import("@/views/onboarding"));
/** cmdk and the dialog load on the first Ctrl+K. */
const CommandMenu = React.lazy(() => import("@/app/CommandMenu").then((m) => ({ default: m.CommandMenu })));

export function App() {
  return (
    <TooltipProvider delayDuration={500} skipDelayDuration={300}>
      <AppStateProvider fallback={(s) => <Boot {...s} />}>
        <DayProvider>
          <Root />
        </DayProvider>
      </AppStateProvider>
      <Toaster />
    </TooltipProvider>
  );
}

/**
 * Reads the settings (the setup flag), the route and the day; not the status, so a status poll
 * does not re-render it or the open section (FE5). Sections read what they need themselves.
 */
function Root() {
  const app = useAppSettings();
  const route = useRoute();
  const { day, today, setDay } = useDay();
  const todayRef = React.useRef(today);
  todayRef.current = today;
  const [commandOpen, setCommandOpen] = React.useState(false);
  const [commandUsed, setCommandUsed] = React.useState(false);
  if (commandOpen && !commandUsed) setCommandUsed(true);
  const [setupOpen, setSetupOpen] = React.useState(false);

  React.useEffect(() => {
    const open = () => setSetupOpen(true);
    addEventListener(OPEN_ONBOARDING_EVENT, open);
    return () => removeEventListener(OPEN_ONBOARDING_EVENT, open);
  }, []);

  React.useEffect(() => {
    document.title = `${SECTION_TITLES[route.section]} · Daytrace`;
  }, [route.section]);

  // 4 AM rollover and the return to today after 15 idle minutes (Dayflow's behavior).
  useDayRollover({
    day,
    today,
    inSettings: route.section === "settings",
    setDay: (d) => setDay(d, { replace: true }),
    // On the same day the selected card stays; on another day the newest card is selected.
    onReset: (dayChanged) => {
      if (dayChanged) setParams({ card: null }, { section: "timeline" });
    },
  });

  // Clicks on Windows notifications.
  useNotificationNavigation((target) => {
    if (target === "daily" || target === "timeline") navigate(target, { day: todayRef.current });
    else if (target === "settings") navigate("settings");
  });

  // Global keys: Ctrl+K command menu, Ctrl+1…5 sections, Ctrl+, Settings. Matched by physical key
  // (lib/keys.ts): on Belgian AZERTY Ctrl+1 is e.key "&", e.code "Digit1".
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!modsAre(e, { ctrl: true })) return;
      if (isKey(e, "KeyK", { ctrl: true })) {
        e.preventDefault();
        setCommandOpen((o) => !o);
        return;
      }
      // Ctrl+, by the "," key of the layout or the key in its US place (";" on Belgian AZERTY).
      if (e.code === "Comma" || e.key === ",") {
        e.preventDefault();
        navigate("settings");
        return;
      }
      const digit = digitOf(e);
      const item = digit === null ? undefined : NAV.find((n) => n.key === String(digit));
      if (item) {
        e.preventDefault();
        navigate(item.id, isDaySection(item.id) ? { day } : {});
      }
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [day]);

  // The same element while the section stays, so Root re-rendering (a day change, a settings
  // change) does not re-render the section; it updates from its own hooks.
  const View = SECTIONS[route.section];
  const view = React.useMemo(() => <View key={route.section} />, [View, route.section]);
  const openCommand = React.useCallback(() => setCommandOpen(true), []);

  const firstRun = !app.settings.settings.onboardingCompleted;
  if (firstRun || setupOpen) {
    return (
      <React.Suspense fallback={<SectionFallback />}>
        <OnboardingScreen firstRun={firstRun} onDone={() => setSetupOpen(false)} />
      </React.Suspense>
    );
  }

  return (
    <>
      <Shell onOpenCommand={openCommand}>
        <React.Suspense fallback={<SectionFallback />}>{view}</React.Suspense>
      </Shell>
      {commandUsed && (
        <React.Suspense fallback={null}>
          <CommandMenu open={commandOpen} onOpenChange={setCommandOpen} />
        </React.Suspense>
      )}
    </>
  );
}

/** While a section's code loads: nothing for 200 ms, then a quiet line (no layout to shift). */
function SectionFallback() {
  const [show, setShow] = React.useState(false);
  React.useEffect(() => {
    const t = setTimeout(() => setShow(true), 200);
    return () => clearTimeout(t);
  }, []);
  return (
    <div className="grid flex-1 place-items-center">
      {show && (
        <p role="status" className="inline-flex items-center gap-2 text-meta text-muted-foreground">
          <Spinner aria-hidden="true" role={undefined} aria-label={undefined} /> Loading…
        </p>
      )}
    </div>
  );
}

/** Before settings and status arrive: the window color only, then a quiet line if it takes long. */
function Boot({ error, retry }: { error: string | null; retry: () => void }) {
  const [slow, setSlow] = React.useState(false);
  React.useEffect(() => {
    const t = setTimeout(() => setSlow(true), 600);
    return () => clearTimeout(t);
  }, []);
  return (
    <div className="grid h-full place-items-center bg-background p-6">
      {error ? (
        <div role="alert" className="flex max-w-md flex-col items-center gap-3 text-center">
          <LogoMark className="size-8" />
          <h1 className="font-display text-title font-semibold">Daytrace couldn’t start its interface</h1>
          <p className="text-meta text-muted-foreground">{error}</p>
          <Button onClick={retry}>Try again</Button>
        </div>
      ) : (
        slow && (
          <p role="status" className="flex items-center gap-2 text-meta text-muted-foreground">
            <LogoMark className="size-5" />
            Starting Daytrace…
          </p>
        )
      )}
    </div>
  );
}
