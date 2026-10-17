import type * as React from "react";
import { ArrowUpRight } from "lucide-react";
import type { Card, Category } from "@/api";
import { href } from "@/lib/router";
import { clock, formatDuration, hourLabel } from "@/lib/time";
import type { AppCategory, WeekStats } from "@/views/insights/api";
import { changeOnLastWeek, kindOf, weekdayLong } from "@/views/insights/format";
import { BarList, CategoryMark, Legend, SectionCard } from "@/views/insights/parts";

/*
 * Weekly lists and tables: longest focus blocks, context shifts, apps and sites (overall and per
 * category) and the programs in front.
 */

const isDistractionKey = (k: string) => k === "distraction" || k === "distractions";

/** The category an app was mostly used in this week, from the per-category lists. */
function mainCategory(app: string, groups: AppCategory[]): AppCategory | null {
  let best: { g: AppCategory; secs: number } | null = null;
  for (const g of groups) {
    const a = g.apps.find((x) => x.name === app);
    if (a && (!best || a.secs > best.secs)) best = { g, secs: a.secs };
  }
  return best?.g ?? null;
}

export function FocusBlocksCard({ week, cardsOf }: { week: WeekStats; cardsOf: (day: string) => Card[] }) {
  return (
    <SectionCard title="Longest focus blocks" description="Focus cards in a row, with less than 5 minutes between them. Select one to open its first card.">
      {week.focusBlocks.length ? (
        <ol className="flex flex-col">
          {week.focusBlocks.map((b, i) => {
            const cards = cardsOf(b.day).filter((c) => c.endTs > b.startTs && c.startTs < b.endTs && kindOf(c.category, []) !== "system");
            const first = cards.find((c) => c.startTs <= b.startTs && c.endTs > b.startTs) ?? cards[0];
            return (
              <li key={`${b.day}-${b.startTs}`} className="border-b last:border-0">
                <a
                  href={href("timeline", { day: b.day, card: first?.id })}
                  className="group grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-baseline gap-x-2 rounded-sm px-1 py-2 hover:bg-surface-2 focus-visible:-outline-offset-2"
                >
                  <span className="text-right text-caption text-subtle-foreground tnum">{i + 1}</span>
                  <span className="min-w-0">
                    <span className="block truncate text-body font-medium">{first?.title || "Focus block"}</span>
                    <span className="block text-caption text-muted-foreground">
                      {weekdayLong(b.day)},{" "}
                      <span className="font-mono tnum">
                        {clock(b.startTs)} – {clock(b.endTs)}
                      </span>
                      {cards.length > 1 && ` · ${cards.length} cards`}
                    </span>
                  </span>
                  <span className="flex items-center gap-1 text-meta font-medium whitespace-nowrap tnum">
                    {formatDuration(b.secs)}
                    <ArrowUpRight className="size-3.5 text-subtle-foreground group-hover:text-foreground" aria-hidden="true" />
                  </span>
                </a>
              </li>
            );
          })}
        </ol>
      ) : (
        <p className="text-meta text-muted-foreground">No focus time this week.</p>
      )}
    </SectionCard>
  );
}

export function ContextCard({ week, today }: { week: WeekStats; today: string }) {
  const shifts = week.days.reduce((a, d) => a + d.contextShifts, 0);
  const count = week.days.reduce((a, d) => a + d.distractionCount, 0);
  return (
    <SectionCard title="Context shifts and distractions" description={week.contextInsight}>
      <table className="w-full text-meta">
        <caption className="sr-only">Context shifts and distractions per day</caption>
        <thead>
          <tr className="border-b text-left text-muted-foreground">
            <th scope="col" className="py-2 font-normal">
              Day
            </th>
            <th scope="col" className="py-2 text-right font-normal">
              Context shifts
            </th>
            <th scope="col" className="py-2 text-right font-normal">
              Distractions
            </th>
          </tr>
        </thead>
        <tbody>
          {week.days.map((d) => {
            const future = d.day > today;
            return (
              <tr key={d.day} className="border-b last:border-0">
                <th scope="row" className="py-1.5 text-left font-normal">
                  {weekdayLong(d.day)}
                </th>
                <td className={future ? "py-1.5 text-right text-subtle-foreground" : "py-1.5 text-right tnum"}>{future ? "Not yet" : d.contextShifts}</td>
                <td className={future ? "py-1.5 text-right" : "py-1.5 text-right tnum"}>{future ? "" : d.distractionCount}</td>
              </tr>
            );
          })}
        </tbody>
        <tfoot>
          <tr className="border-t">
            <th scope="row" className="pt-2 text-left font-normal text-muted-foreground">
              Week
            </th>
            <td className="pt-2 text-right font-medium tnum">{shifts}</td>
            <td className="pt-2 text-right font-medium tnum">{count}</td>
          </tr>
        </tfoot>
      </table>
    </SectionCard>
  );
}

export function TopAppsCard({ week, categories, colorFor }: { week: WeekStats; categories: Category[]; colorFor: (n: string) => string }) {
  const items = week.topApps.map((a) => {
    const g = mainCategory(a.name, week.appsByCategory);
    return {
      key: a.name,
      name: a.name,
      secs: a.secs,
      color: g ? colorFor(g.name) : undefined,
      note: changeOnLastWeek(a.secs, a.previousSecs),
      srNote: g ? `, mostly ${g.name}` : undefined,
      cat: g,
    };
  });
  const used = new Map<string, AppCategory>();
  for (const i of items) if (i.cat && !used.has(i.cat.key)) used.set(i.cat.key, i.cat);
  return (
    <SectionCard title="Top apps and sites" description="The main app or site of each card, colored by the category it was mostly used in.">
      <div className="flex flex-col gap-3">
        <BarList label="Top apps and sites" items={items} empty="No cards with an app or site this week." />
        {used.size > 0 && (
          <Legend
            items={[...used.values()].map((g) => ({
              key: g.key,
              label: g.name,
              mark: <CategoryMark name={g.name} kind={kindOf(g.name, categories)} colorFor={colorFor} />,
            }))}
          />
        )}
      </div>
    </SectionCard>
  );
}

export function AppsByCategoryCard({ week, categories, colorFor }: { week: WeekStats; categories: Category[]; colorFor: (n: string) => string }) {
  return (
    <SectionCard title="Most used per category" description="Change against the week before, for the same app in the same category.">
      {week.appsByCategory.length ? (
        <div className="flex flex-col gap-4">
          {week.appsByCategory.map((g) => (
            <section key={g.key} aria-label={g.name} className="flex flex-col gap-1.5">
              <h3 className="flex items-center gap-2 text-meta font-semibold">
                <CategoryMark name={g.name} kind={kindOf(g.name, categories)} colorFor={colorFor} />
                {g.name}
                <span className="ml-auto font-normal text-muted-foreground tnum">{formatDuration(g.secs)}</span>
              </h3>
              <ul className="flex flex-col">
                {g.apps.map((a) => {
                  const note = changeOnLastWeek(a.secs, a.previousSecs);
                  return (
                    <li key={a.name} className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-3 border-b py-1 pl-[1.125rem] text-meta last:border-0">
                      <span className="truncate" translate="no" title={a.name}>
                        {a.name}
                      </span>
                      <span className="text-right whitespace-nowrap tnum">
                        {formatDuration(a.secs)}
                        {note && <span className="ml-2 text-caption text-muted-foreground">{note}</span>}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
        </div>
      ) : (
        <p className="text-meta text-muted-foreground">No cards with an app or site this week.</p>
      )}
    </SectionCard>
  );
}

export function ProgramsCard({ week }: { week: WeekStats }) {
  return (
    <SectionCard title="Programs in front" description="From the window in front at each screenshot.">
      <BarList label="Programs in front" items={week.programs.map((p) => ({ key: p.name, name: p.name, secs: p.secs }))} empty="No program data for this week." />
    </SectionCard>
  );
}

/** Plain sentences about when distraction happens (the heat map's text equivalent). */
export function distractionFacts(week: WeekStats): { text: string; node: React.ReactNode }[] {
  const facts: { text: string; node: React.ReactNode }[] = [];
  const byHour = Array.from({ length: 24 }, (_, h) => week.distractionHours.reduce((a, r) => a + (r.secs[h] ?? 0), 0));
  const total = byHour.reduce((a, b) => a + b, 0);
  if (!total) return facts;
  const peak = byHour.indexOf(Math.max(...byHour));
  const peakText = `Most distraction falls between ${hourLabel(4 + peak)} and ${hourLabel(5 + peak)}: ${formatDuration(byHour[peak])} this week.`;
  facts.push({ text: peakText, node: peakText });
  const worst = [...week.days].sort((a, b) => b.distractedSecs - a.distractedSecs)[0];
  if (worst && worst.distractedSecs > 0) {
    const t = `${weekdayLong(worst.day)} had the most: ${formatDuration(worst.distractedSecs)}.`;
    facts.push({ text: t, node: t });
  }
  const sources = week.appsByCategory.find((g) => isDistractionKey(g.key))?.apps.slice(0, 3) ?? [];
  if (sources.length)
    facts.push({
      text: `Top sources: ${sources.map((a) => `${a.name} ${formatDuration(a.secs)}`).join(", ")}.`,
      node: (
        <>
          Top sources:{" "}
          {sources.map((a, i) => (
            <span key={a.name}>
              <span className="font-medium" translate="no">
                {a.name}
              </span>{" "}
              {formatDuration(a.secs)}
              {i < sources.length - 1 ? ", " : "."}
            </span>
          ))}
        </>
      ),
    });
  return facts;
}
