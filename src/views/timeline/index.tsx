import * as React from "react";
import { Download } from "lucide-react";
import { api, type Card } from "@/api";
import { useDay } from "@/app/day";
import { LegacyFrame } from "@/app/LegacyFrame";
import { useAppState } from "@/app/state";
import { isTypingTarget, DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { numParam, setParams, useRoute } from "@/lib/router";
import { formatDayTitle } from "@/lib/time";
import { CardDetail } from "@/CardDetail";
import { DayRibbon } from "@/DayRibbon";
import { ExportPanel } from "@/ExportPanel";
import { Timeline } from "@/Timeline";

/*
 * INTERIM (owned by the Timeline engineer): the old timeline components inside the new shell.
 * The shell parts (page header, day navigation, banners) are final; replace everything inside
 * <LegacyFrame> with the ported Timeline. URL state: ?day=YYYY-MM-DD&card=<id>&export=1.
 * Keys: J next card, K previous card (plus [ and ] for days from DayNav).
 */

const CARD_POLL_MS = 15_000;

function same(a: Card[] | null, b: Card[]): boolean {
  return !!a && JSON.stringify(a) === JSON.stringify(b);
}

export default function TimelineSection() {
  const app = useAppState();
  const { day, today } = useDay();
  const route = useRoute();
  const [cards, setCards] = React.useState<Card[] | null>(null);
  const selected = numParam(route.params, "card");
  const exportOpen = route.params.get("export") === "1";

  const load = React.useCallback(() => {
    api
      .getDay(day)
      .then((c) => setCards((cur) => (same(cur, c) ? cur : c)))
      .catch(() => setCards((cur) => cur ?? []));
  }, [day]);

  React.useEffect(() => {
    setCards(null);
    load();
    const t = setInterval(load, CARD_POLL_MS);
    return () => clearInterval(t);
  }, [load]);

  // Select the newest card when nothing (or a card of another day) is selected.
  React.useEffect(() => {
    if (!cards) return;
    if (cards.length === 0) {
      if (selected !== null) setParams({ card: null });
    } else if (selected === null || !cards.some((c) => c.id === selected)) {
      setParams({ card: cards[cards.length - 1].id });
    }
  }, [cards, selected]);

  // J / K: next / previous card.
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented || isTypingTarget(e.target) || !cards?.length) return;
      const k = e.key.toLowerCase();
      if (k !== "j" && k !== "k") return;
      const i = cards.findIndex((c) => c.id === selected);
      const next = k === "j" ? Math.min(cards.length - 1, i + 1) : Math.max(0, i - 1);
      e.preventDefault();
      setParams({ card: cards[next].id });
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [cards, selected]);

  const list = cards ?? [];
  const selectedCard = list.find((c) => c.id === selected) ?? null;
  const select = (id: number) => setParams({ card: id });

  return (
    <>
      <PageHeader
        label="Timeline"
        title={formatDayTitle(day)}
        badge={day === today ? <TitleBadge>Today</TitleBadge> : undefined}
        nav={<DayNav />}
        actions={
          <Button variant="outline" size="sm" aria-expanded={exportOpen} onClick={() => setParams({ export: exportOpen ? null : 1 })}>
            <Download aria-hidden="true" />
            Export…
          </Button>
        }
      />
      <LegacyFrame>
        <DayRibbon key={day} day={day} cards={list} colorFor={app.colorFor} selected={selected} onSelect={select} />
        {exportOpen && <ExportPanel day={day} onClose={() => setParams({ export: null })} />}
        <div className="split">
          <Timeline
            day={day}
            cards={list}
            categories={app.categories}
            onChanged={load}
            colorFor={app.colorFor}
            selected={selected}
            onSelect={select}
            isToday={day === today}
            status={app.status}
            recording={app.settings.settings.recordingEnabled}
          />
          <CardDetail
            card={selectedCard}
            color={selectedCard ? app.colorFor(selectedCard.category) : ""}
            categories={app.categories}
            onChanged={load}
            onRetry={async (id) => {
              await api.retryCard(id);
              void app.reloadStatus();
            }}
          />
        </div>
      </LegacyFrame>
    </>
  );
}
