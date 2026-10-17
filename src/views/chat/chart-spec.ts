/*
 * Fenced `chart` blocks in chat answers: the info string, the JSON schema, its validation and the
 * words and numbers every chart also has as text (its table, its summary for screen readers, its
 * copy as Markdown). Ports Dayflow's chart blocks (Views/UI/ChatCharts.swift ChatChartSpec.parse
 * and ChatContentParser, MIT, © Jerry Liu): the same six types, the same keys, `pie` as another
 * name for `donut`, hex colors with or without `#`. Daytrace is stricter than Dayflow: data that
 * does not add up (lengths that differ, a value that is not a number, a gantt item that ends
 * before it starts) rejects the whole block instead of dropping part of it, and every list, string
 * and number has a limit, so a hostile or oversized block cannot slow the answer down. A color
 * that is not a hex string is ignored (colors are optional).
 *
 * Pure functions without imports, so `node --experimental-strip-types --test` can run the tests
 * (chart-spec.test.mjs).
 */

export const CHART_TYPES = ["bar", "line", "stacked_bar", "donut", "heatmap", "gantt"] as const;
export type ChartType = (typeof CHART_TYPES)[number];

/** Limits of one block. Larger blocks get "This chart couldn't be drawn" with the reason. */
export const CHART_LIMITS = {
  /** Characters of JSON. */
  body: 16_000,
  /** Arrays inside objects inside arrays inside the top object (stacked_bar needs 4). */
  depth: 4,
  title: 120,
  label: 60,
  /** Bars, line points and stacked bar columns. */
  points: 60,
  series: 12,
  slices: 16,
  heatColumns: 48,
  heatRows: 48,
  heatCells: 1000,
  ganttItems: 60,
  /** Largest absolute value. */
  value: 1e9,
  /** Charts drawn per answer; later ones get the note. */
  perAnswer: 12,
} as const;

export interface BasicChart {
  type: "bar" | "line";
  title: string;
  labels: string[];
  values: number[];
  color: string | null;
}

export interface StackedChart {
  type: "stacked_bar";
  title: string;
  categories: string[];
  series: { name: string; values: number[]; color: string | null }[];
}

export interface DonutChart {
  type: "donut";
  title: string;
  labels: string[];
  values: number[];
  /** One per label (null: no color given), or all null when the list did not match the labels. */
  colors: (string | null)[];
}

export interface HeatmapChart {
  type: "heatmap";
  title: string;
  xLabels: string[];
  yLabels: string[];
  /** One row per y label, each with one value per x label. */
  values: number[][];
  color: string | null;
}

export interface GanttChart {
  type: "gantt";
  title: string;
  items: { label: string; start: number; end: number; color: string | null }[];
}

export type ChartSpec = BasicChart | StackedChart | DonutChart | HeatmapChart | GanttChart;

export type ChartResult = { ok: true; spec: ChartSpec } | { ok: false; reason: string };

// ---------------------------------------------------------------- the fence

/** A chart block of an answer, as the Markdown renderer hands it to the chart view. */
export interface ChartBlock {
  type: string | null;
  /** The JSON text between the fences. */
  body: string;
  /** The block as written, fences included (shown when it cannot be drawn). */
  raw: string;
  /** 0 for the answer's first chart block. */
  index: number;
}

export interface ChartFence {
  /** The type after `type=`, lowercased; null when the info string has none. */
  type: string | null;
  /** Text after the type on the opening line (Dayflow also reads JSON that starts there). */
  inline: string;
}

/**
 * The info string of a fence (the text after the backticks) when it opens a chart block:
 * `chart type=bar`. Dayflow's pattern is chart, spaces, `type`, `=`, then letters and underscores.
 * Written with string operations and anchored patterns, so it stays linear on long lines.
 */
export function chartFence(info: string): ChartFence | null {
  const t = info.trim();
  if (t.slice(0, 5).toLowerCase() !== "chart") return null;
  if (t.length > 5 && !/\s/.test(t[5])) return null; // ```chartjs, ```charts
  const rest = t.slice(5).trimStart();
  const m = /^type\s*=\s*([A-Za-z_]{1,32})/i.exec(rest);
  if (!m) return { type: null, inline: rest };
  return { type: m[1].toLowerCase(), inline: rest.slice(m[0].length).trim() };
}

// ---------------------------------------------------------------- validation

class Invalid extends Error {}

function fail(reason: string): never {
  throw new Invalid(reason);
}

const nf = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });
const count = (n: number) => nf.format(n);

/** Deepest nesting of arrays and objects in a JSON text (strings skipped), in one pass. */
export function jsonDepth(text: string): number {
  let depth = 0;
  let max = 0;
  let inString = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text.charCodeAt(i);
    if (inString) {
      if (ch === 92) i++; // backslash: skip the escaped character
      else if (ch === 34) inString = false;
      continue;
    }
    if (ch === 34) inString = true;
    else if (ch === 91 || ch === 123) {
      depth++;
      if (depth > max) max = depth;
    } else if (ch === 93 || ch === 125) depth--;
  }
  return max;
}

function record(v: unknown): Record<string, unknown> {
  if (typeof v !== "object" || v === null || Array.isArray(v)) fail("The chart data must be a JSON object, like the examples: { \"title\": …, … }.");
  return v as Record<string, unknown>;
}

/** Whitespace runs (newlines too) become one space. */
function clean(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

function title(v: unknown): string {
  if (v === undefined || v === null) return "";
  if (typeof v !== "string") fail("\"title\" must be text.");
  const t = clean(v);
  if (t.length > CHART_LIMITS.title) fail(`The title is longer than ${CHART_LIMITS.title} characters.`);
  return t;
}

/** A label; `subject` names it in the reason ("Every item of \"x\"", "The \"name\" of every series"). */
function label(v: unknown, subject: string): string {
  if (typeof v !== "string") fail(`${subject} must be text.`);
  const t = clean(v);
  if (!t) fail(`${subject} must not be empty.`);
  if (t.length > CHART_LIMITS.label) fail(`${subject} must be at most ${CHART_LIMITS.label} characters long.`);
  return t;
}

function labels(v: unknown, key: string, max: number, what: string): string[] {
  if (!Array.isArray(v)) fail(`"${key}" is missing or is not a list.`);
  if (v.length === 0) fail(`"${key}" is empty.`);
  if (v.length > max) fail(`A chart can show at most ${count(max)} ${what}; "${key}" has ${count(v.length)}.`);
  return v.map((x) => label(x, `Every item of "${key}"`));
}

function num(v: unknown, key: string, opts: { min0?: boolean } = {}): number {
  if (typeof v !== "number" || !Number.isFinite(v)) fail(`Every value of "${key}" must be a number.`);
  if (Math.abs(v) > CHART_LIMITS.value) fail(`The values of "${key}" must lie between −1 billion and 1 billion.`);
  if (opts.min0 && v < 0) fail(`The values of "${key}" can’t be negative in this chart.`);
  return v;
}

function nums(v: unknown, key: string, length: number, lengthKey: string, opts: { min0?: boolean } = {}): number[] {
  if (!Array.isArray(v)) fail(`"${key}" is missing or is not a list of numbers.`);
  if (v.length !== length) fail(`"${key}" has ${count(v.length)} ${v.length === 1 ? "value" : "values"} but "${lengthKey}" has ${count(length)}; they must match.`);
  return v.map((x) => num(x, key, opts));
}

/** Dayflow's sanitizeHex: 6 hex digits (RRGGBB) or 8 (AARRGGBB, alpha dropped), `#` optional. */
export function sanitizeHex(v: unknown): string | null {
  if (typeof v !== "string") return null;
  let raw = v.trim();
  if (raw.startsWith("#")) raw = raw.slice(1);
  if (!/^(?:[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$/.test(raw)) return null;
  return `#${raw.slice(-6).toUpperCase()}`;
}

function basic(type: "bar" | "line", o: Record<string, unknown>): BasicChart {
  const x = labels(o.x, "x", CHART_LIMITS.points, type === "bar" ? "bars" : "points");
  return { type, title: title(o.title), labels: x, values: nums(o.y, "y", x.length, "x"), color: sanitizeHex(o.color) };
}

function stacked(o: Record<string, unknown>): StackedChart {
  const x = labels(o.x, "x", CHART_LIMITS.points, "bars");
  const s = o.series;
  if (!Array.isArray(s)) fail("\"series\" is missing or is not a list.");
  if (s.length === 0) fail("\"series\" is empty.");
  if (s.length > CHART_LIMITS.series) fail(`A chart can show at most ${CHART_LIMITS.series} series; this one has ${count(s.length)}.`);
  const series = s.map((entry) => {
    const e = record(entry);
    return { name: label(e.name, 'The "name" of every series'), values: nums(e.values, "values", x.length, "x", { min0: true }), color: sanitizeHex(e.color) };
  });
  return { type: "stacked_bar", title: title(o.title), categories: x, series };
}

function donut(o: Record<string, unknown>): DonutChart {
  const l = labels(o.labels, "labels", CHART_LIMITS.slices, "slices");
  const values = nums(o.values, "values", l.length, "labels", { min0: true });
  if (!values.some((v) => v > 0)) fail("All values are 0, so there is nothing to divide.");
  // Dayflow: the colors apply only when there is one per label.
  const colors = Array.isArray(o.colors) && o.colors.length === l.length ? o.colors.map(sanitizeHex) : l.map(() => null);
  return { type: "donut", title: title(o.title), labels: l, values, colors };
}

function heatmap(o: Record<string, unknown>): HeatmapChart {
  const x = labels(o.x, "x", CHART_LIMITS.heatColumns, "columns");
  const y = labels(o.y, "y", CHART_LIMITS.heatRows, "rows");
  if (x.length * y.length > CHART_LIMITS.heatCells) fail(`A heat map can show at most ${count(CHART_LIMITS.heatCells)} cells; this one has ${count(x.length * y.length)}.`);
  const v = o.values;
  if (!Array.isArray(v)) fail("\"values\" is missing or is not a list of rows.");
  if (v.length !== y.length) fail(`"values" has ${count(v.length)} rows but "y" has ${count(y.length)}; there must be one row per y label.`);
  const values = v.map((row) => {
    if (!Array.isArray(row)) fail("Every row of \"values\" must be a list of numbers.");
    if (row.length !== x.length) fail(`A row of "values" has ${count(row.length)} values but "x" has ${count(x.length)}; they must match.`);
    return row.map((n) => num(n, "values"));
  });
  return { type: "heatmap", title: title(o.title), xLabels: x, yLabels: y, values, color: sanitizeHex(o.color) };
}

function gantt(o: Record<string, unknown>): GanttChart {
  const it = o.items;
  if (!Array.isArray(it)) fail("\"items\" is missing or is not a list.");
  if (it.length === 0) fail("\"items\" is empty.");
  if (it.length > CHART_LIMITS.ganttItems) fail(`A chart can show at most ${CHART_LIMITS.ganttItems} bars; "items" has ${count(it.length)}.`);
  const items = it.map((entry) => {
    const e = record(entry);
    const start = num(e.start, "start");
    const end = num(e.end, "end");
    if (!(end > start)) fail("Every item needs a start before its end.");
    return { label: label(e.label, 'The "label" of every item'), start, end, color: sanitizeHex(e.color) };
  });
  return { type: "gantt", title: title(o.title), items };
}

/** Validates one block. Never throws; the reason is a sentence for the person reading the chat. */
export function parseChart(type: string | null, body: string): ChartResult {
  try {
    if (type === null) fail("The block has no chart type, so Daytrace can’t tell which chart to draw. It draws bar, line, stacked bar, donut, heat map and Gantt charts.");
    const t = type === "pie" ? "donut" : type;
    if (!(CHART_TYPES as readonly string[]).includes(t)) fail(`“${type.slice(0, 32)}” is not a chart type Daytrace draws. It draws bar, line, stacked bar, donut, heat map and Gantt charts.`);
    const text = body.trim();
    if (!text) fail("The block has no chart data.");
    if (text.length > CHART_LIMITS.body) fail(`The chart data is longer than ${count(CHART_LIMITS.body)} characters.`);
    if (jsonDepth(text) > CHART_LIMITS.depth) fail("The chart data is nested deeper than any chart type needs.");
    let data: unknown;
    try {
      data = JSON.parse(text);
    } catch {
      fail("The chart data is not valid JSON.");
    }
    const o = record(data);
    switch (t as ChartType) {
      case "bar":
      case "line":
        return { ok: true, spec: basic(t as "bar" | "line", o) };
      case "stacked_bar":
        return { ok: true, spec: stacked(o) };
      case "donut":
        return { ok: true, spec: donut(o) };
      case "heatmap":
        return { ok: true, spec: heatmap(o) };
      case "gantt":
        return { ok: true, spec: gantt(o) };
    }
  } catch (e) {
    return { ok: false, reason: e instanceof Invalid ? e.message : "The chart data could not be read." };
  }
}

// ---------------------------------------------------------------- numbers and scales

/** "1,234.5", "2.25", "-3": up to two decimals. */
export function formatValue(n: number): string {
  return nf.format(n);
}

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

/** Axis label: compact from 10,000 ("12K", "1.5M"). */
export function formatTick(n: number): string {
  return Math.abs(n) >= 10_000 ? compact.format(n) : nf.format(n);
}

export interface Scale {
  lo: number;
  hi: number;
  ticks: number[];
}

/**
 * A scale from `lo` to `hi` in round steps (1, 2, 2.5 or 5 times a power of 10): of the steps
 * that give 3 to 6 intervals, the one that wastes the least room (fewer ticks on a tie).
 */
export function niceScale(lo: number, hi: number): Scale {
  if (!(hi > lo)) {
    if (lo === 0 && hi === 0) hi = 1;
    else {
      const pad = Math.abs(hi) * 0.1 || 1;
      lo -= pad;
      hi += pad;
    }
  }
  const mag = Math.pow(10, Math.floor(Math.log10((hi - lo) / 4)));
  type Option = { a: number; b: number; step: number; n: number; span: number };
  let best: Option | null = null;
  let fallback: Option | null = null;
  for (const m of [0.1, 1, 10])
    for (const k of [1, 2, 2.5, 5]) {
      const step = k * m * mag;
      const a = Math.floor(lo / step + 1e-9) * step;
      const b = Math.ceil(hi / step - 1e-9) * step;
      const n = Math.round((b - a) / step);
      // Steps of 2.5 read less easily: they win only when clearly tighter.
      const c = { a, b, step, n, span: (b - a) * (k === 2.5 ? 1.08 : 1) };
      if (n >= 3 && n <= 6) {
        if (!best || c.span < best.span - 1e-9 || (Math.abs(c.span - best.span) <= 1e-9 && n < best.n)) best = c;
      } else if (!fallback || Math.abs(n - 4) < Math.abs(fallback.n - 4)) fallback = c;
    }
  const { a, step, n } = (best ?? fallback)!;
  const steps = Math.min(20, n);
  const ticks = Array.from({ length: steps + 1 }, (_, i) => Number((a + i * step).toPrecision(12)));
  return { lo: ticks[0], hi: ticks[ticks.length - 1], ticks };
}

// ---------------------------------------------------------------- colors

/*
 * Colors the model gives (and the fallback palette) are data, like a category hex the user picked.
 * Each one is moved to a lightness with at least 3:1 against the content layer, separately for
 * the light and the dark theme (hue and as much chroma as fits kept), and used as
 * `light-dark(<light>, <dark>)`, so a pastel the model chose stays visible on white and a deep
 * blue stays visible on graphite.
 */

/** Dayflow's ChatChartBlockView.defaultPalette, for series without a color or a category. */
export const FALLBACK_PALETTE = ["#F96E00", "#1F6FEB", "#2E7D32", "#8E24AA", "#00897B"] as const;

type Vec = [number, number, number];

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

function hexToLinear(hex: string): Vec {
  const n = parseInt(hex.slice(1), 16);
  return [toLinear(((n >> 16) & 255) / 255), toLinear(((n >> 8) & 255) / 255), toLinear((n & 255) / 255)];
}

function linearToHex(c: Vec): string {
  return `#${c
    .map((v) =>
      Math.round(Math.min(1, Math.max(0, toGamma(Math.min(1, Math.max(0, v))))) * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")
    .toUpperCase()}`;
}

function linearToOklab([r, g, b]: Vec): Vec {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
}

function oklabToLinear([L, a, b]: Vec): Vec {
  const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3);
  const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3);
  const s = Math.pow(L - 0.0894841775 * a - 1.291485548 * b, 3);
  return [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
}

const luminance = ([r, g, b]: Vec) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

export function contrastRatio(y1: number, y2: number): number {
  return (Math.max(y1, y2) + 0.05) / (Math.min(y1, y2) + 0.05);
}

/** Relative luminance of a hex color. */
export function luminanceOf(hex: string): number {
  return luminance(hexToLinear(hex));
}

function oklchToLinear(L: number, C: number, h: number): Vec {
  return oklabToLinear([L, C * Math.cos(h), C * Math.sin(h)]);
}

const inGamut = (c: Vec) => c.every((v) => v >= -1e-4 && v <= 1 + 1e-4);

/** The color at lightness L with the most chroma up to C that sRGB can show. */
function fitted(L: number, C: number, h: number): Vec {
  let c = oklchToLinear(L, C, h);
  if (inGamut(c)) return c;
  let lo = 0;
  let hi = C;
  for (let i = 0; i < 18; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut(oklchToLinear(L, mid, h))) lo = mid;
    else hi = mid;
  }
  c = oklchToLinear(L, lo, h);
  return c;
}

/** Content layer (--surface-1 in tokens.css): white in light, oklch(0.21 0.007 260) in dark. */
const SURFACE_LIGHT_Y = 1;
const SURFACE_DARK_Y = luminance(oklchToLinear(0.21, 0.007, (260 * Math.PI) / 180));
/** 3:1 for marks (WCAG 1.4.11), with a little room for rounding. */
const MARK_RATIO = 3.1;

/** `hex` moved in lightness until it has 3:1 against a surface of luminance `bgY` (unchanged when it has). */
export function withContrast(hex: string, bgY: number, ratio = MARK_RATIO): string {
  const lin = hexToLinear(hex);
  if (contrastRatio(luminance(lin), bgY) >= ratio) return hex;
  const [L, a, b] = linearToOklab(lin);
  const C = Math.hypot(a, b);
  const h = Math.atan2(b, a);
  const darker = bgY > 0.5;
  // Search between the color's own lightness and black (light theme) or white (dark theme).
  let ok = darker ? 0 : 1;
  let bad = L;
  for (let i = 0; i < 24; i++) {
    const mid = (ok + bad) / 2;
    if (contrastRatio(luminance(fitted(mid, C, h)), bgY) >= ratio) ok = mid;
    else bad = mid;
  }
  return linearToHex(fitted(ok, C, h));
}

const markCache = new Map<string, string>();

/** A model or palette color as a CSS color with 3:1 on the content layer in both themes. */
export function markColor(hex: string): string {
  const hit = markCache.get(hex);
  if (hit) return hit;
  const light = withContrast(hex, SURFACE_LIGHT_Y);
  const dark = withContrast(hex, SURFACE_DARK_Y);
  const css = light === dark ? light : `light-dark(${light}, ${dark})`;
  if (markCache.size > 256) markCache.clear();
  markCache.set(hex, css);
  return css;
}

/** The two hex values of markColor, for tests. */
export function markColors(hex: string): { light: string; dark: string; darkSurfaceY: number } {
  return { light: withContrast(hex, SURFACE_LIGHT_Y), dark: withContrast(hex, SURFACE_DARK_Y), darkSurfaceY: SURFACE_DARK_Y };
}

// ---------------------------------------------------------------- the chart as words

const TYPE_NAME: Record<ChartType, string> = {
  bar: "Bar chart",
  line: "Line chart",
  stacked_bar: "Stacked bar chart",
  donut: "Donut chart",
  heatmap: "Heat map",
  gantt: "Gantt chart",
};

/** The chart's title, or its kind when the model gave none. */
export function chartName(spec: ChartSpec): string {
  return spec.title || TYPE_NAME[spec.type];
}

/** "35%" of a total. */
export function share(part: number, total: number): string {
  return total > 0 ? `${Math.round((part / total) * 100)}%` : "0%";
}

function extreme(labelsOf: string[], values: number[], pick: (a: number, b: number) => boolean): { label: string; value: number } {
  let best = 0;
  for (let i = 1; i < values.length; i++) if (pick(values[i], values[best])) best = i;
  return { label: labelsOf[best], value: values[best] };
}

function list(items: string[], max = 6): string {
  const shown = items.slice(0, max);
  const more = items.length - shown.length;
  return more > 0 ? `${shown.join(", ")} and ${more} more` : shown.join(", ");
}

/** What the picture shows, for the image's accessible description. */
export function chartSummary(spec: ChartSpec): string {
  const end = "Switch to Table for every value.";
  switch (spec.type) {
    case "bar":
    case "line": {
      const n = spec.values.length;
      const hi = extreme(spec.labels, spec.values, (a, b) => a > b);
      const lo = extreme(spec.labels, spec.values, (a, b) => a < b);
      const what = spec.type === "bar" ? `${TYPE_NAME.bar} with ${n} ${n === 1 ? "bar" : "bars"}` : `${TYPE_NAME.line} with ${n} ${n === 1 ? "point" : "points"}, from ${spec.labels[0]} to ${spec.labels[n - 1]}`;
      const range = n > 1 ? ` Highest: ${hi.label}, ${formatValue(hi.value)}. Lowest: ${lo.label}, ${formatValue(lo.value)}.` : ` ${hi.label}: ${formatValue(hi.value)}.`;
      return `${what}.${range} ${end}`;
    }
    case "stacked_bar": {
      const totals = spec.categories.map((_, i) => spec.series.reduce((a, s) => a + s.values[i], 0));
      const hi = extreme(spec.categories, totals, (a, b) => a > b);
      const n = spec.categories.length;
      return `${TYPE_NAME.stacked_bar} with ${n} ${n === 1 ? "bar" : "bars"}, each split into ${list(spec.series.map((s) => s.name))}. Highest total: ${hi.label}, ${formatValue(hi.value)}. ${end}`;
    }
    case "donut": {
      const total = spec.values.reduce((a, v) => a + v, 0);
      return `${TYPE_NAME.donut}: ${list(spec.labels.map((l, i) => `${l} ${share(spec.values[i], total)}`), 8)}. ${end}`;
    }
    case "heatmap": {
      const flat = spec.values.flat();
      let best = 0;
      for (let i = 1; i < flat.length; i++) if (flat[i] > flat[best]) best = i;
      const row = Math.floor(best / spec.xLabels.length);
      const col = best % spec.xLabels.length;
      const lo = Math.min(...flat);
      return `${TYPE_NAME.heatmap} of ${spec.yLabels.length} ${spec.yLabels.length === 1 ? "row" : "rows"} by ${spec.xLabels.length} ${spec.xLabels.length === 1 ? "column" : "columns"}, values from ${formatValue(lo)} to ${formatValue(flat[best])}. Highest: ${spec.yLabels[row]}, ${spec.xLabels[col]}. ${end}`;
    }
    case "gantt": {
      const starts = spec.items.map((i) => i.start);
      const ends = spec.items.map((i) => i.end);
      const n = spec.items.length;
      return `${TYPE_NAME.gantt} with ${n} ${n === 1 ? "bar" : "bars"} from ${formatValue(Math.min(...starts))} to ${formatValue(Math.max(...ends))}: ${list(spec.items.map((i) => i.label))}. ${end}`;
    }
  }
}

export interface ChartTable {
  /** Column headers; the first is the row header's. */
  head: string[];
  /** Each row: its header cell, then the values. */
  rows: string[][];
  /** A total row, when the chart adds up. */
  foot?: string[];
}

/** The chart's numbers as a table (the Table view and Copy answer use the same). */
export function chartTable(spec: ChartSpec): ChartTable {
  switch (spec.type) {
    case "bar":
    case "line":
      return { head: ["Label", "Value"], rows: spec.labels.map((l, i) => [l, formatValue(spec.values[i])]) };
    case "stacked_bar": {
      const totals = spec.categories.map((_, i) => spec.series.reduce((a, s) => a + s.values[i], 0));
      return {
        head: ["Label", ...spec.series.map((s) => s.name), "Total"],
        rows: spec.categories.map((c, i) => [c, ...spec.series.map((s) => formatValue(s.values[i])), formatValue(totals[i])]),
        foot: ["Total", ...spec.series.map((s) => formatValue(s.values.reduce((a, v) => a + v, 0))), formatValue(totals.reduce((a, v) => a + v, 0))],
      };
    }
    case "donut": {
      const total = spec.values.reduce((a, v) => a + v, 0);
      return {
        head: ["Label", "Value", "Share"],
        rows: spec.labels.map((l, i) => [l, formatValue(spec.values[i]), share(spec.values[i], total)]),
        foot: ["Total", formatValue(total), "100%"],
      };
    }
    case "heatmap":
      return { head: ["", ...spec.xLabels], rows: spec.yLabels.map((y, r) => [y, ...spec.values[r].map(formatValue)]) };
    case "gantt":
      return { head: ["Label", "Start", "End", "Length"], rows: spec.items.map((i) => [i.label, formatValue(i.start), formatValue(i.end), formatValue(i.end - i.start)]) };
  }
}

const pipeCell = (s: string) => s.replace(/\|/g, "\\|");

/** The chart as Markdown text: its name in bold and its table. */
export function chartText(spec: ChartSpec): string {
  const t = chartTable(spec);
  const row = (cells: string[]) => `| ${cells.map(pipeCell).join(" | ")} |`;
  const lines = [`**${chartName(spec)}**`, "", row(t.head), row(t.head.map(() => "---")), ...t.rows.map(row)];
  if (t.foot) lines.push(row(t.foot));
  return lines.join("\n");
}

/**
 * An answer with every chart block that can be drawn written as its table (for the clipboard);
 * blocks that cannot be drawn stay as they are. Line by line, like the renderer.
 */
export function chartsAsText(text: string): string {
  if (!text.includes("```")) return text;
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const out: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const t = line.trimStart();
    const fence = t.startsWith("```") ? chartFence(t.slice(3)) : null;
    if (!fence) {
      out.push(line);
      if (t.startsWith("```")) {
        // Another code block: copy it as it is, so a chart example inside it stays an example.
        for (i++; i < lines.length; i++) {
          out.push(lines[i]);
          if (lines[i].trimStart().startsWith("```")) break;
        }
      }
      continue;
    }
    const { body, next, closed } = chartBody(lines, i + 1, fence.inline);
    const parsed = parseChart(fence.type, body);
    if (parsed.ok) out.push(chartText(parsed.spec));
    else out.push(...lines.slice(i, closed ? next : lines.length));
    i = next - 1;
  }
  return out.join("\n");
}

/**
 * The body of a chart block that starts at `from` (the line after the opening fence): up to a line
 * that starts with backticks, or that ends with them (`}````, which Dayflow also reads). `next`
 * is the line after the block.
 */
export function chartBody(lines: string[], from: number, inline: string): { body: string; next: number; closed: boolean } {
  const body: string[] = inline ? [inline] : [];
  if (inline.endsWith("```")) return { body: inline.slice(0, -3), next: from, closed: true };
  let i = from;
  for (; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t.startsWith("```")) return { body: body.join("\n"), next: i + 1, closed: true };
    if (t.endsWith("```")) {
      body.push(t.slice(0, -3));
      return { body: body.join("\n"), next: i + 1, closed: true };
    }
    body.push(lines[i]);
  }
  return { body: body.join("\n"), next: i, closed: false };
}
