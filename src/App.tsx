import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, Card, Category, SettingsDto, Status, shiftDay } from "./api";
import { Timeline } from "./Timeline";
import { CardDetail } from "./CardDetail";
import { DayRibbon } from "./DayRibbon";
import { DayCalendar } from "./DayCalendar";
import { SettingsView } from "./SettingsView";
import { ExportPanel } from "./ExportPanel";
import { StatusPill } from "./StatusPill";
import { DailyView } from "./views/DailyView";
import { WeeklyView } from "./views/WeeklyView";
import { ChatView } from "./views/ChatView";
import { JournalView } from "./views/JournalView";
import type { ViewProps } from "./views/types";
import { Onboarding } from "./views/Onboarding";
import { OPEN_ONBOARDING_EVENT } from "./views/platform-api";
import { useAppearance, useDayRollover, useNotificationNavigation } from "./views/platform-hooks";

type View = "timeline" | "daily" | "weekly" | "journal" | "chat" | "settings";

const TABS: { id: View; label: string }[] = [
  { id: "timeline", label: "Timeline" },
  { id: "daily", label: "Daily" },
  { id: "weekly", label: "Weekly" },
  { id: "journal", label: "Goals" },
  { id: "chat", label: "Chat" },
];

export default function App() {
  const [view, setView] = useState<View>("timeline");
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [day, setDay] = useState<string | null>(null);
  const [cards, setCards] = useState<Card[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);

  const loadSettings = useCallback(() => api.getSettings().then(setSettings), []);
  const loadStatus = useCallback(() => api.getStatus().then((s) => {
    setStatus(s);
    setDay((d) => d ?? s.today);
  }), []);
  const loadCards = useCallback(() => {
    if (day) api.getDay(day).then(setCards);
  }, [day]);

  useEffect(() => {
    loadSettings();
    loadStatus();
    const t = setInterval(loadStatus, 5000);
    const un = listen("settings-changed", () => {
      loadSettings();
      loadStatus();
    });
    return () => {
      clearInterval(t);
      un.then((f) => f());
    };
  }, [loadSettings, loadStatus]);

  useEffect(() => {
    loadCards();
    const t = setInterval(loadCards, 15000);
    return () => clearInterval(t);
  }, [loadCards]);

  // platform_extras: first-run setup reopened from Settings, light/dark override, 4 AM rollover
  // and idle reset, and clicks on notifications.
  useEffect(() => {
    const open = () => setSetupOpen(true);
    window.addEventListener(OPEN_ONBOARDING_EVENT, open);
    return () => window.removeEventListener(OPEN_ONBOARDING_EVENT, open);
  }, []);
  useAppearance(settings?.settings.appearance);
  // On the same day the selected card stays; on another day the newest card is selected.
  useDayRollover({ day, today: status?.today ?? null, inSettings: view === "settings", setDay, onReset: (dayChanged) => dayChanged && setSelected(null) });
  useNotificationNavigation((target) => {
    if (target === "daily" && status) setDay(status.today);
    if (target === "daily" || target === "settings" || target === "timeline") setView(target);
  });

  // Select the newest card when nothing is selected, and keep the selection valid when cards are
  // rewritten by a new batch.
  useEffect(() => {
    if (selected === null && cards.length) {
      setSelected(cards[cards.length - 1].id);
    } else if (selected !== null && !cards.some((c) => c.id === selected)) {
      const prev = cards.length ? cards[cards.length - 1].id : null;
      setSelected(prev);
    }
  }, [cards, selected]);

  const categories: Category[] = settings?.settings.categories ?? [];
  const colorFor = useMemo(() => {
    const map = new Map(categories.map((c) => [c.name.toLowerCase(), c.color]));
    return (name: string) => (name === "System" ? "var(--danger)" : map.get(name.toLowerCase()) ?? "var(--muted)");
  }, [categories]);

  if (!settings || !status || !day) {
    return <div className="boot">Starting Daytrace</div>;
  }

  if (!settings.settings.onboardingCompleted || setupOpen) {
    return (
      <div className="app">
        <Onboarding
          dto={settings}
          firstRun={!settings.settings.onboardingCompleted}
          onDone={async () => {
            setSetupOpen(false);
            await loadSettings();
            await loadStatus();
          }}
        />
      </div>
    );
  }

  const selectedCard = cards.find((c) => c.id === selected) ?? null;
  const isToday = day === status.today;
  const needsKey = settings.settings.provider === "anthropic" && !settings.hasApiKey;

  return (
    <div className="app">
      <nav className="tabs" aria-label="Sections">
        {TABS.map((t) => (
          <button key={t.id} className={view === t.id ? "tab on" : "tab"} aria-current={view === t.id ? "page" : undefined} onClick={() => setView(t.id)}>
            {t.label}
          </button>
        ))}
      </nav>
      <header className="topbar">
        <div className="daynav">
          <button className="icon" aria-label="Previous day" onClick={() => setDay(shiftDay(day, -1))}>‹</button>
          <DayCalendar day={day} today={status.today} onPick={setDay} />
          <button className="icon" aria-label="Next day" disabled={isToday} onClick={() => setDay(shiftDay(day, 1))}>›</button>
          {!isToday && <button className="link" onClick={() => setDay(status.today)}>Today</button>}
        </div>
        <div className="actions">
          <StatusPill
            status={status}
            settings={settings.settings}
            onChanged={async () => {
              await loadSettings();
              await loadStatus();
            }}
          />
          <button className={view === "timeline" && exportOpen ? "ghost on" : "ghost"} onClick={() => { setView("timeline"); setExportOpen((o) => !o); }}>
            Export
          </button>
          <button className={view === "settings" ? "ghost on" : "ghost"} onClick={() => setView(view === "settings" ? "timeline" : "settings")}>
            Settings
          </button>
        </div>
      </header>

      {view !== "timeline" && view !== "settings" ? (
        (() => {
          const props: ViewProps = {
            day,
            setDay,
            today: status.today,
            settings,
            status,
            categories,
            colorFor,
            openTimeline: (d, cardId) => {
              setDay(d);
              setSelected(cardId ?? null);
              setView("timeline");
            },
          };
          switch (view) {
            case "daily":
              return <DailyView {...props} />;
            case "weekly":
              return <WeeklyView {...props} />;
            case "journal":
              return <JournalView {...props} />;
            case "chat":
              return <ChatView {...props} />;
          }
        })()
      ) : view === "settings" ? (
        <SettingsView
          dto={settings}
          onSaved={(dto) => {
            setSettings(dto);
            loadStatus();
          }}
          onClose={() => setView("timeline")}
        />
      ) : (
        <>
          <DayRibbon key={day} day={day} cards={cards} colorFor={colorFor} selected={selected} onSelect={setSelected} />
          {needsKey && (
            <div className="banner">
              Daytrace is recording, but it can’t describe your screen until you add an Anthropic API key.{" "}
              <button className="link" onClick={() => setView("settings")}>Add API key</button>
            </div>
          )}
          {exportOpen && <ExportPanel day={day} onClose={() => setExportOpen(false)} />}
          <main className="split">
            <Timeline
              day={day}
              cards={cards}
              categories={categories}
              onChanged={loadCards}
              colorFor={colorFor}
              selected={selected}
              onSelect={setSelected}
              isToday={isToday}
              status={status}
              recording={settings.settings.recordingEnabled}
            />
            <CardDetail
              card={selectedCard}
              color={selectedCard ? colorFor(selectedCard.category) : ""}
              categories={categories}
              onChanged={loadCards}
              onRetry={async (id) => {
                await api.retryCard(id);
                loadStatus();
              }}
            />
          </main>
        </>
      )}
    </div>
  );
}
