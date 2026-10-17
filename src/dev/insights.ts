/*
 * DEV ONLY. Simplified client-side versions of the backend's insights (features/insights/*.rs),
 * so Daily, Weekly and Goals have consistent numbers in the browser preview, and the recap
 * commands with their states (makeRecapMock). Not a reference implementation: the Rust code is
 * the source of truth.
 */
import type { Card, Category } from "@/api";
import { dayBounds, shiftDay, weekStart, weekday } from "@/lib/time";
import type { AppUsage, Block, CategoryTotal, DayStats, DistractionItem, GridRow, Kind, Marker, NamedSecs, RecapEntry, RecapView, WeekStats } from "@/views/insights/api";

const SLOT = 15;
const FOCUS_GAP = 5 * 60;

interface Seg {
  card: Card;
  start: number;
  end: number;
  key: string;
  name: string;
  kind: Kind;
  order: number;
}

function catInfo(name: string, cats: Category[]) {
  const key = name.trim().toLowerCase();
  if (key === "system") return { key, name, kind: "system" as Kind, order: 999 };
  const i = cats.findIndex((c) => c.name.trim().toLowerCase() === key);
  const c = cats[i];
  const kind: Kind = c?.isIdle ? "idle" : key === "distraction" || key === "distractions" ? "distraction" : "focus";
  // The backend also sends a stored hex per category; views colour by name with colorFor().
  return { key, name: c?.name ?? name, kind, order: i < 0 ? 500 : i };
}

function segments(cards: Card[], cats: Category[], start: number, end: number): Seg[] {
  return cards
    .filter((c) => c.endTs > start && c.startTs < end)
    .map((c) => ({ card: c, start: Math.max(c.startTs, start), end: Math.min(c.endTs, end), ...catInfo(c.category, cats) }))
    .sort((a, b) => a.start - b.start);
}

function parseClock(day: string, s: string, near: number): number {
  const m = s.trim().match(/^(\d{1,2}):(\d{2})\s*(AM|PM)$/i);
  if (!m) return near;
  let h = Number(m[1]) % 12;
  if (m[3].toUpperCase() === "PM") h += 12;
  const [ds] = dayBounds(day);
  const base = new Date(ds * 1000);
  base.setHours(h, Number(m[2]), 0, 0);
  let ts = base.getTime() / 1000;
  if (ts < ds) ts += 86400;
  return ts;
}

function distractionsOf(segs: Seg[], day: string): DistractionItem[] {
  const out: DistractionItem[] = [];
  for (const s of segs) {
    if (s.kind === "distraction") {
      out.push({ startTs: s.start, endTs: s.end, secs: s.end - s.start, title: s.card.title, summary: s.card.summary, withinCard: null, cardId: s.card.id });
      continue;
    }
    for (const d of s.card.distractions) {
      const a = Math.max(parseClock(day, d.startTime, s.start), s.start);
      const b = Math.min(parseClock(day, d.endTime, s.end), s.end);
      if (b > a) out.push({ startTs: a, endTs: b, secs: b - a, title: d.title, summary: d.summary, withinCard: s.card.title, cardId: s.card.id });
    }
  }
  return out.sort((x, y) => x.startTs - y.startTs);
}

function totals(segs: Seg[]): CategoryTotal[] {
  const m = new Map<string, CategoryTotal & { order: number }>();
  for (const s of segs) {
    if (s.kind === "system") continue;
    const t = m.get(s.key) ?? { key: s.key, name: s.name, kind: s.kind, secs: 0, order: s.order };
    t.secs += s.end - s.start;
    m.set(s.key, t);
  }
  return [...m.values()]
    .sort((a, b) => Math.floor(b.secs / 60) - Math.floor(a.secs / 60) || a.order - b.order)
    .map(({ order: _o, ...t }) => t);
}

function focusBlocks(segs: Seg[]): Block[] {
  const spans = segs.filter((s) => s.kind === "focus").map((s) => [s.start, s.end] as [number, number]);
  const merged: [number, number][] = [];
  for (const [a, b] of spans) {
    const last = merged[merged.length - 1];
    if (last && a - last[1] < FOCUS_GAP) last[1] = Math.max(last[1], b);
    else merged.push([a, b]);
  }
  return merged.map(([a, b]) => ({ startTs: a, endTs: b, secs: b - a }));
}

function programs(segs: Seg[]): NamedSecs[] {
  const m = new Map<string, number>();
  for (const s of segs) {
    const p = s.card.appSites?.primary;
    if (p) m.set(p, (m.get(p) ?? 0) + (s.end - s.start));
  }
  return [...m.entries()].map(([name, secs]) => ({ name, secs })).sort((a, b) => b.secs - a.secs);
}

export function computeDay(day: string, cards: Card[], cats: Category[]): DayStats {
  const [ds, de] = dayBounds(day);
  const segs = segments(cards, cats, ds, de);
  const real = segs.filter((s) => s.kind !== "system");
  const sum = (k: Kind) => real.filter((s) => s.kind === k).reduce((a, s) => a + s.end - s.start, 0);
  const distractions = distractionsOf(real, day);
  const embedded = distractions.filter((d) => d.withinCard).reduce((a, d) => a + d.secs, 0);
  const focusSecs = sum("focus") - embedded;
  const distractedSecs = sum("distraction") + embedded;
  let transition = 0;
  let switches = 0;
  for (let i = 1; i < segs.length; i++) {
    transition += Math.max(0, segs[i].start - segs[i - 1].end);
    if (segs[i].key !== segs[i - 1].key && segs[i].kind !== "idle" && segs[i - 1].kind !== "idle") switches++;
  }
  const blocks = focusBlocks(real);
  const longest = blocks.reduce<Block | null>((b, x) => (!b || x.secs > b.secs ? x : b), null);

  // Grid: 15-minute slots over the recorded hours (at least 12 hours), one row per category.
  const off = (ts: number) => (ts - ds) / 60;
  const first = real.length ? Math.min(...real.map((s) => off(s.start))) : 300;
  const last = real.length ? Math.max(...real.map((s) => off(s.end))) : 1020;
  const gStart = Math.floor(first / 60) * 60;
  const gEnd = Math.min(1440, Math.max(gStart + 720, Math.ceil(last / 60) * 60));
  const slots = Math.ceil((gEnd - gStart) / SLOT);
  const keys = cats.map((c) => catInfo(c.name, cats));
  const fill = keys.map(() => new Array<number>(slots).fill(0));
  const rows: GridRow[] = keys.map((k, ri) => ({
    key: k.key,
    name: k.name,
    cells: Array.from({ length: slots }, (_, i) => {
      const a = gStart + i * SLOT;
      const b = a + SLOT;
      let total = 0;
      let best: { o: number; c: Card } | null = null;
      for (const s of real) {
        if (s.key !== k.key) continue;
        const o = Math.max(0, Math.min(off(s.end), b) - Math.max(off(s.start), a));
        total += o;
        if (o > 0 && (!best || o > best.o)) best = { o, c: s.card };
      }
      fill[ri][i] = total;
      return best ? { occupancy: Math.min(1, total / SLOT), title: best.c.title, cardId: best.c.id } : { occupancy: Math.min(1, total / SLOT) };
    }),
  }));
  const dominant = Array.from({ length: slots }, (_, i) => {
    let bi: number | null = null;
    fill.forEach((r, ri) => {
      if (r[i] > 0 && (bi === null || r[i] > fill[bi][i])) bi = ri;
    });
    return bi;
  });
  const markers: Marker[] = distractions.map((d) => ({ startMinute: off(d.startTs), endMinute: off(d.endTs), title: d.title }));

  return {
    day,
    dayStartTs: ds,
    dayEndTs: de,
    cardCount: segs.length,
    capturedSecs: real.reduce((a, s) => a + s.end - s.start, 0),
    focusSecs,
    distractedSecs,
    idleSecs: sum("idle"),
    transitionSecs: transition,
    contextSwitches: switches,
    interruptions: real.filter((s) => s.card.distractions.length > 0).length,
    distractedRatio: focusSecs + distractedSecs > 0 ? distractedSecs / (focusSecs + distractedSecs) : 0,
    totals: totals(real),
    grid: { startMinute: gStart, endMinute: gEnd, slotMinutes: SLOT, rows, dominant, markers },
    distractions,
    focusBlocks: blocks,
    longestFocus: longest,
    programs: programs(real),
  };
}

export function computeWeek(day: string, cardsOf: (d: string) => Card[], cats: Category[]): WeekStats {
  const start = weekStart(day);
  const days = Array.from({ length: 7 }, (_, i) => shiftDay(start, i));
  const stats = days.map((d) => computeDay(d, cardsOf(d), cats));
  const prevDays = Array.from({ length: 7 }, (_, i) => shiftDay(start, i - 7));
  const add = (xs: CategoryTotal[][]) => {
    const m = new Map<string, CategoryTotal>();
    xs.flat().forEach((t) => {
      const c = m.get(t.key) ?? { ...t, secs: 0 };
      c.secs += t.secs;
      m.set(t.key, c);
    });
    return [...m.values()].sort((a, b) => b.secs - a.secs);
  };

  // App time per category (like the backend's app_seconds: card apps, idle left out).
  const appSeconds = (ds: string[]) => {
    const out = new Map<string, { cat: string; name: string; app: string; secs: number }>();
    for (const d of ds) {
      const [a, b] = dayBounds(d);
      for (const s of segments(cardsOf(d), cats, a, b)) {
        const app = s.card.appSites?.primary;
        if (!app || s.kind === "idle" || s.kind === "system") continue;
        const k = `${s.key}|${app}`;
        const e = out.get(k) ?? { cat: s.key, name: s.name, app, secs: 0 };
        e.secs += s.end - s.start;
        out.set(k, e);
      }
    }
    return [...out.values()];
  };
  const current = appSeconds(days);
  const before = appSeconds(prevDays);
  const byApp = new Map<string, AppUsage>();
  for (const e of current) {
    const a = byApp.get(e.app) ?? { name: e.app, secs: 0, previousSecs: before.filter((x) => x.app === e.app).reduce((n, x) => n + x.secs, 0) };
    a.secs += e.secs;
    byApp.set(e.app, a);
  }
  const topApps = [...byApp.values()].sort((a, b) => b.secs - a.secs).slice(0, 10);
  const byCat = new Map<string, WeekStats["appsByCategory"][number]>();
  for (const e of current) {
    const c = byCat.get(e.cat) ?? { key: e.cat, name: e.name, secs: 0, apps: [] };
    c.secs += e.secs;
    c.apps.push({ name: e.app, secs: e.secs, previousSecs: before.find((x) => x.cat === e.cat && x.app === e.app)?.secs ?? 0 });
    byCat.set(e.cat, c);
  }
  const appsByCategory = [...byCat.values()]
    .map((c) => ({ ...c, apps: c.apps.sort((a, b) => b.secs - a.secs).slice(0, 8) }))
    .sort((a, b) => b.secs - a.secs)
    .slice(0, 5);

  // Activity window of the week: earliest start and latest end ±30 min, in 15-minute steps.
  const offsets: [number, number][] = [];
  stats.forEach((s, i) => {
    const [a, b] = dayBounds(days[i]);
    for (const g of segments(cardsOf(days[i]), cats, a, b)) {
      if (g.kind === "idle" || g.kind === "system") continue;
      offsets.push([(g.start - s.dayStartTs) / 60, (g.end - s.dayStartTs) / 60]);
    }
  });
  const wStart = offsets.length ? Math.floor(Math.max(0, Math.min(...offsets.map((o) => o[0])) - 30) / 15) * 15 : 300;
  const wEnd = offsets.length ? Math.ceil(Math.min(1440, Math.max(...offsets.map((o) => o[1])) + 30) / 15) * 15 : 1080;

  const distractionHours = stats.map((s) => {
    const secs = new Array<number>(24).fill(0);
    s.distractions.forEach((d) => {
      for (let t = d.startTs; t < d.endTs; t += 60) {
        const h = Math.floor((t - s.dayStartTs) / 3600);
        if (h >= 0 && h < 24) secs[h] += Math.min(60, d.endTs - t);
      }
    });
    return { day: s.day, label: weekday(s.day), secs };
  });
  const sumOf = (k: "capturedSecs" | "focusSecs" | "distractedSecs" | "idleSecs") => stats.reduce((a, s) => a + s[k], 0);
  const busiest = [...stats].sort((a, b) => b.contextSwitches + b.distractions.length - (a.contextSwitches + a.distractions.length))[0];
  return {
    weekStart: start,
    weekEnd: days[6],
    days: stats.map((s) => ({
      day: s.day,
      label: weekday(s.day),
      capturedSecs: s.capturedSecs,
      focusSecs: s.focusSecs,
      distractedSecs: s.distractedSecs,
      idleSecs: s.idleSecs,
      totals: s.totals,
      contextShifts: s.contextSwitches,
      distractionCount: s.distractions.length,
      longestFocus: s.longestFocus,
    })),
    distribution: add(stats.map((s) => s.totals.filter((t) => t.kind !== "idle"))),
    capturedSecs: sumOf("capturedSecs"),
    focusSecs: sumOf("focusSecs"),
    distractedSecs: sumOf("distractedSecs"),
    idleSecs: sumOf("idleSecs"),
    workflow: {
      startMinute: wStart,
      endMinute: wEnd,
      slotMinutes: SLOT,
      rows: stats.map((s) => ({ day: s.day, label: weekday(s.day), cells: [] })),
      totals: add(stats.map((s) => s.totals)),
    },
    heatmap: { startMinute: wStart, endMinute: wEnd, bucketMinutes: 5, rows: stats.map((s) => ({ day: s.day, label: weekday(s.day), values: [] })) },
    distractionHours,
    contextInsight:
      busiest && busiest.contextSwitches + busiest.distractions.length > 0
        ? `${new Intl.DateTimeFormat("en-GB", { weekday: "long" }).format(new Date(`${busiest.day}T12:00:00`))} had the most interruptions, with ${busiest.contextSwitches} context shifts and ${busiest.distractions.length} distractions.`
        : "No context shift or distraction pattern was detected in this week.",
    focusBlocks: stats
      .flatMap((s) => s.focusBlocks.map((b) => ({ day: s.day, ...b })))
      .sort((a, b) => b.secs - a.secs)
      .slice(0, 5),
    appsByCategory,
    topApps,
    programs: [...byApp.values()].map((a) => ({ name: a.name, secs: a.secs })).sort((a, b) => b.secs - a.secs),
  };
}

// ---- recap ----

/** Captured minutes of a day without idle and failed cards (the recap's activity rule). */
function activeMinutes(cards: Card[]): number {
  return cards.filter((c) => c.category !== "System" && c.category !== "Idle" && !c.isIdleShortcut).reduce((a, c) => a + (c.endTs - c.startTs) / 60, 0);
}

/**
 * The recap commands, keyed by target day as in the backend: the standup for day D is written
 * from the most recent of the 3 days before D with 2 hours of activity. Earlier days have one;
 * today has none unless asked for. Variants (query before the hash):
 *   ?recap=today       today's standup is written
 *   ?recap=edited      ...and edited by hand
 *   ?recap=generating  the automatic recap is writing today's standup right now
 *   ?recap=failed      Generate fails (rate limit)
 *   ?recap=autoerror   the automatic recap failed today
 *   ?recap=auto        automatic recaps are on (today shows when it will run)
 *   ?recap=nohistory   no recent day has enough activity
 *   ?recap=exported    the source day's Markdown file exists
 *   ?recap=exportfail  Export Markdown fails
 *   ?insights=error    the day and week numbers fail to load
 */
export function makeRecapMock(opts: { today: string; cardsOf: (d: string) => Card[]; model: () => string; q: URLSearchParams; now: () => number; sleep: (ms: number) => Promise<unknown> }) {
  const { today, cardsOf, q } = opts;
  const variant = q.get("recap") ?? "";
  const entries = new Map<string, RecapEntry>();
  const generatingUntil = new Map<string, number>();
  if (variant === "generating") generatingUntil.set(today, opts.now() + 12);

  const sourceFor = (target: string, minMinutes = 120): string | null => {
    if (variant === "nohistory" && target === today) return null;
    for (let i = 1; i <= 3; i++) {
      const d = shiftDay(target, -i);
      if (activeMinutes(cardsOf(d)) >= minMinutes) return d;
    }
    return null;
  };

  const write = (target: string, source: string, edited = false): RecapEntry => {
    const work = cardsOf(source).filter((c) => c.category === "Work");
    return {
      targetDay: target,
      sourceDay: source,
      highlights: work.slice(0, 4).map((c) => c.title),
      tasks: ["Finish the retry scheduler persistence", "Write the test for empty days in the export"],
      blockers: target === today || target === shiftDay(today, -1) ? ["Waiting for a review of the activity table migration"] : [],
      provider: "Anthropic",
      model: opts.model(),
      edited,
      createdAt: dayBounds(target)[0] + 4 * 3600,
      updatedAt: dayBounds(target)[0] + 4 * 3600,
    };
  };

  const entryFor = (target: string): RecapEntry | null => {
    if (entries.has(target)) return entries.get(target) ?? null;
    if (target === today && variant !== "today" && variant !== "edited" && variant !== "exported") return null;
    const source = sourceFor(target);
    if (!source || !cardsOf(target).length) return null;
    const e = write(target, source, variant === "edited" && target === today);
    if (variant === "edited" && target === today) e.highlights = [...e.highlights.slice(0, 2), "Wrote the export test plan by hand"];
    entries.set(target, e);
    return e;
  };

  const view = (target: string): RecapView => {
    const entry = entryFor(target);
    const sourceDay = entry?.sourceDay ?? sourceFor(target);
    const generating = (generatingUntil.get(target) ?? 0) > opts.now();
    return {
      targetDay: target,
      entry,
      sourceDay,
      generating,
      autoError: variant === "autoerror" && target === today && !entry ? "The model provider answered 529 Overloaded three times. Daytrace stops after three attempts a day." : null,
      exportPath: variant === "exported" && sourceDay ? `C:\\Users\\Preview\\OneDrive\\Daytrace\\${sourceDay}.md` : null,
      auto: variant === "auto" && target === today && !entry ? { after: "08:00", sourceDay: sourceFor(target, 180), blocked: null } : null,
    };
  };

  // ?insights=error: the day and week numbers fail to load (the recap still answers).
  const failing =
    q.get("insights") === "error"
      ? {
          insights_day: () => {
            throw "database is locked";
          },
          insights_week: () => {
            throw "database is locked";
          },
        }
      : {};

  return {
    ...failing,
    insights_recap: (a: Record<string, unknown>) => {
      const day = a.day as string;
      // The automatic recap finishes after a few polls.
      if (variant === "generating" && day === today && (generatingUntil.get(day) ?? 0) <= opts.now() && !entries.has(day)) {
        const s = sourceFor(day);
        if (s) entries.set(day, write(day, s));
      }
      return view(day);
    },
    insights_generate_recap: async (a: Record<string, unknown>) => {
      const day = a.day as string;
      await opts.sleep(1200);
      if (variant === "failed") throw "The model provider is rate limiting requests (HTTP 429). Retry after 40 seconds.";
      const source = entries.get(day)?.sourceDay ?? sourceFor(day);
      if (!source) throw "Not enough captured activity in the previous 3 days to generate a standup.";
      entries.set(day, { ...write(day, source), createdAt: opts.now(), updatedAt: opts.now() });
      return view(day);
    },
    insights_save_recap: (a: Record<string, unknown>) => {
      const day = a.day as string;
      const cur = entryFor(day) ?? write(day, sourceFor(day) ?? shiftDay(day, -1));
      entries.set(day, {
        ...cur,
        highlights: a.highlights as string[],
        tasks: a.tasks as string[],
        blockers: a.blockers as string[],
        provider: entryFor(day) ? cur.provider : "",
        model: entryFor(day) ? cur.model : "",
        edited: true,
        updatedAt: opts.now(),
      });
      return view(day);
    },
    insights_export_recap: (a: Record<string, unknown>) => {
      if (q.get("recap") === "exportfail") throw "Access to the path 'C:\\Users\\Preview\\OneDrive\\Daytrace' is denied. (os error 5)";
      return (a.path as string | null) ?? `C:\\Users\\Preview\\OneDrive\\Daytrace\\${a.day}.md`;
    },
  };
}
