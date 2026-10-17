/*
 * DEV ONLY. Simplified client-side versions of the backend's insights (features/insights/*.rs),
 * so Daily, Weekly and Goals have consistent numbers in the browser preview. Not a reference
 * implementation: the Rust code is the source of truth.
 */
import type { Card, Category } from "@/api";
import { dayBounds, shiftDay, weekStart, weekday } from "@/lib/time";
import type { AppUsage, Block, CategoryTotal, DayStats, DistractionItem, GridRow, Kind, Marker, NamedSecs, WeekStats } from "@/views/insights-api";

const SLOT = 15;
const FOCUS_GAP = 5 * 60;

interface Seg {
  card: Card;
  start: number;
  end: number;
  key: string;
  name: string;
  color: string;
  kind: Kind;
  order: number;
}

function catInfo(name: string, cats: Category[]) {
  const key = name.trim().toLowerCase();
  if (key === "system") return { key, name, color: "#B4443A", kind: "system" as Kind, order: 999 };
  const i = cats.findIndex((c) => c.name.trim().toLowerCase() === key);
  const c = cats[i];
  const kind: Kind = c?.isIdle ? "idle" : key === "distraction" || key === "distractions" ? "distraction" : "focus";
  // Like the backend: the stored hex. Views colour by name with colorFor(), not with this field.
  return { key, name: c?.name ?? name, color: c ? c.color : "#BFB6AE", kind, order: i < 0 ? 500 : i };
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
    const t = m.get(s.key) ?? { key: s.key, name: s.name, color: s.color, kind: s.kind, secs: 0, order: s.order };
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
    color: k.color,
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
  const prev = Array.from({ length: 7 }, (_, i) => computeDay(shiftDay(start, i - 7), cardsOf(shiftDay(start, i - 7)), cats));
  const add = (xs: CategoryTotal[][]) => {
    const m = new Map<string, CategoryTotal>();
    xs.flat().forEach((t) => {
      const c = m.get(t.key) ?? { ...t, secs: 0 };
      c.secs += t.secs;
      m.set(t.key, c);
    });
    return [...m.values()].sort((a, b) => b.secs - a.secs);
  };
  const progs = (ss: DayStats[]) => {
    const m = new Map<string, number>();
    ss.forEach((s) => s.programs.forEach((p) => m.set(p.name, (m.get(p.name) ?? 0) + p.secs)));
    return m;
  };
  const now = progs(stats);
  const before = progs(prev);
  const topApps: AppUsage[] = [...now.entries()]
    .map(([name, secs]) => ({ name, secs, previousSecs: before.get(name) ?? 0 }))
    .sort((a, b) => b.secs - a.secs)
    .slice(0, 8);
  const wStart = 6 * 60;
  const wEnd = 24 * 60;
  const distractionHours = stats.map((s) => {
    const secs = new Array<number>(24).fill(0);
    s.distractions.forEach((d) => {
      const h = Math.floor((d.startTs - s.dayStartTs) / 3600);
      if (h >= 0 && h < 24) secs[h] += d.secs;
    });
    return { day: s.day, label: weekday(s.day), secs };
  });
  const sumOf = (k: "capturedSecs" | "focusSecs" | "distractedSecs" | "idleSecs") => stats.reduce((a, s) => a + s[k], 0);
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
    distribution: add(stats.map((s) => s.totals)),
    capturedSecs: sumOf("capturedSecs"),
    focusSecs: sumOf("focusSecs"),
    distractedSecs: sumOf("distractedSecs"),
    idleSecs: sumOf("idleSecs"),
    workflow: {
      startMinute: wStart - 240,
      endMinute: wEnd - 240,
      slotMinutes: SLOT,
      rows: stats.map((s) => {
        const n = (wEnd - wStart) / SLOT;
        return {
          day: s.day,
          label: weekday(s.day),
          cells: Array.from({ length: n }, (_, i) => {
            const slot = Math.floor((wStart - 240 - s.grid.startMinute) / SLOT) + i;
            const ri = slot >= 0 && slot < s.grid.dominant.length ? s.grid.dominant[slot] : null;
            if (ri === null || ri === undefined) return null;
            const row = s.grid.rows[ri];
            const occ = row.cells[slot].occupancy;
            return { key: row.key, name: row.name, color: row.color, occupancy: occ, secs: Math.round(occ * SLOT * 60) };
          }),
        };
      }),
      totals: add(stats.map((s) => s.totals)),
    },
    heatmap: {
      startMinute: wStart - 240,
      endMinute: wEnd - 240,
      bucketMinutes: 60,
      rows: distractionHours.map((r) => ({ day: r.day, label: r.label, values: r.secs.slice(2, 20).map((x) => Math.round(x / 60)) })),
    },
    distractionHours,
    contextInsight: "You switched context most on days with the most meetings. Afternoons after 3 PM hold most of the distraction time.",
    focusBlocks: stats
      .flatMap((s) => s.focusBlocks.map((b) => ({ day: s.day, ...b })))
      .sort((a, b) => b.secs - a.secs)
      .slice(0, 5),
    appsByCategory: add(stats.map((s) => s.totals))
      .filter((t) => t.kind !== "idle")
      .map((t) => ({ key: t.key, name: t.name, color: t.color, secs: t.secs, apps: topApps.slice(0, 3) })),
    topApps,
    programs: [...now.entries()].map(([name, secs]) => ({ name, secs })).sort((a, b) => b.secs - a.secs),
  };
}
