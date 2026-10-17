import * as React from "react";
import { ChartColumn } from "lucide-react";
import type { Category } from "@/api";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { kindOf } from "@/views/insights/format";
import { CategoryMark, Legend, SvgPatterns, useWidth } from "@/views/insights/parts";
import { endRounded } from "@/views/weekly/charts";
import { useChartImage } from "@/views/weekly/save-image";
import {
  CHART_LIMITS,
  FALLBACK_PALETTE,
  chartName,
  chartSummary,
  chartTable,
  formatTick,
  formatValue,
  markColor,
  niceScale,
  parseChart,
  share,
  type BasicChart,
  type ChartBlock,
  type ChartSpec,
  type DonutChart,
  type GanttChart,
  type HeatmapChart,
  type StackedChart,
} from "@/views/chat/chart-spec";

/*
 * Charts in chat answers (Dayflow's ChatChartBlockView, Views/UI/ChatCharts.swift): bar, line,
 * stacked bar, donut, heat map and Gantt, hand-drawn in SVG like the Weekly charts and with their
 * parts (legend, category marks and patterns, rounded bar ends, "Save as PNG…"). Each chart is a
 * figure named by its title, with a Chart/Table switch: the picture has a summary for screen
 * readers, the table every value. Series named like a category get the category's color
 * (colorFor; idle hatched, "System" striped); other colors come from the model, else from
 * Dayflow's palette where series must be told apart (stacked bars, donut slices) and the neutral
 * chart color where one color is enough (bars, a line, Gantt rows, which have their labels).
 * Model and palette colors are moved to 3:1 on the content layer in both themes (chart-spec.ts
 * markColor). A block that can't be drawn becomes a short note with the reason and the block
 * under "Chart data".
 */

const AXIS_TEXT = "font-mono text-axis";
const axisStyle: React.CSSProperties = { fill: "var(--chart-axis)" };
const gridStyle: React.CSSProperties = { stroke: "var(--chart-grid)" };
const labelStyle: React.CSSProperties = { fill: "var(--muted-foreground)" };
/** Average width of a text-caption character (12 px) and a text-axis mono one (11 px). */
const CAPTION_CHAR = 6.4;
const AXIS_CHAR = 6.7;
/** Height of the bar, line and stacked bar charts (Dayflow: 180 plus axis labels). */
const PLOT_H = 200;

export interface ChartContext {
  colorFor: (name: string) => string;
  categories: Category[];
  /** Second line of a saved image ("Daytrace chat, 16 Oct, 5:42 PM"). */
  subtitle: string;
}

interface Paint {
  fill: string;
  mark: React.ReactNode;
}

/** `fallback`: the place in Dayflow's palette for a series without a color, or null for the neutral. */
type Painter = (name: string, hex: string | null, fallback: number | null) => Paint;

interface Ids {
  idle: string;
  failed: string;
}

function swatch(fill: string) {
  return <span aria-hidden="true" data-mark="" className="size-2.5 shrink-0 rounded-[2px]" style={{ background: fill }} />;
}

/** Fill and legend key for a bar, series, slice or item called `name`. */
function painter(ctx: ChartContext, ids: Ids): Painter {
  const known = new Set(ctx.categories.map((c) => c.name.trim().toLowerCase()));
  return (name, hex, fallback) => {
    const key = name.trim().toLowerCase();
    if (known.has(key) || key === "system" || key === "idle") {
      const kind = kindOf(name, ctx.categories);
      const mark = <CategoryMark name={name} kind={kind} colorFor={ctx.colorFor} />;
      if (kind === "idle") return { fill: `url(#${ids.idle})`, mark };
      if (kind === "system") return { fill: `url(#${ids.failed})`, mark };
      return { fill: ctx.colorFor(name), mark };
    }
    const fill = hex ? markColor(hex) : fallback === null ? "var(--chart-5)" : markColor(FALLBACK_PALETTE[fallback % FALLBACK_PALETTE.length]);
    return { fill, mark: swatch(fill) };
  };
}

/** `s` cut to `chars` characters with an ellipsis. */
function fit(s: string, chars: number): string {
  const n = Math.max(2, Math.floor(chars));
  return s.length <= n ? s : `${s.slice(0, n - 1).trimEnd()}…`;
}

function slug(s: string): string {
  return (
    s
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 40)
      .replace(/-+$/, "") || "chart"
  );
}

// ---------------------------------------------------------------- the block

/** One chart block of an answer: the chart, or the note when it can't be drawn. */
export function ChatChart({ block, ctx }: { block: ChartBlock; ctx: ChartContext }) {
  const result = React.useMemo(() => (block.index >= CHART_LIMITS.perAnswer ? null : parseChart(block.type, block.body)), [block.type, block.body, block.index]);
  if (!result) return <ChartNote reason={`This answer has more than ${CHART_LIMITS.perAnswer} charts; Daytrace draws the first ${CHART_LIMITS.perAnswer}.`} raw={block.raw} plain />;
  if (!result.ok) return <ChartNote reason={result.reason} raw={block.raw} />;
  return (
    <ChartBoundary raw={block.raw}>
      <ChartFigure spec={result.spec} ctx={ctx} />
    </ChartBoundary>
  );
}

/**
 * "This chart couldn't be drawn", the reason, and the block as written. `plain` leaves out the
 * icon (the blocks past the limit of an answer, which can be many).
 */
export function ChartNote({ reason, raw, plain }: { reason: string; raw: string; plain?: boolean }) {
  return (
    <div className="flex flex-col gap-1 rounded-md border border-border-strong bg-surface-2 px-3.5 py-2.5" data-chart-note="">
      <p className="flex items-center gap-2 text-meta font-medium">
        {!plain && <ChartColumn className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />}
        This chart couldn’t be drawn.
      </p>
      <p className="text-meta text-muted-foreground">{reason}</p>
      <details className="mt-0.5 text-caption">
        <summary className="w-fit cursor-default rounded-sm text-muted-foreground select-none hover:text-foreground">Chart data</summary>
        <pre className="scroll-thin mt-1.5 max-h-48 overflow-auto rounded-md border bg-surface-1 p-2.5 font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere]">{raw}</pre>
      </details>
    </div>
  );
}

/** A chart that fails while drawing becomes the note instead of taking the answer down with it. */
class ChartBoundary extends React.Component<{ raw: string; children: React.ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    return this.state.failed ? <ChartNote reason="Daytrace ran into a problem while drawing it." raw={this.props.raw} /> : this.props.children;
  }
}

function ChartFigure({ spec, ctx }: { spec: ChartSpec; ctx: ChartContext }) {
  const [asTable, setAsTable] = React.useState(false);
  const name = chartName(spec);
  const titleId = React.useId();
  const uid = React.useId().replace(/:/g, "");
  const ids = { idle: `${uid}i`, failed: `${uid}f` };
  const paint = painter(ctx, ids);
  const image = useChartImage({ title: name, subtitle: ctx.subtitle, file: `daytrace-chart-${slug(name)}.png` });
  const save = React.isValidElement<{ "aria-describedby"?: string }>(image.button) ? React.cloneElement(image.button, { "aria-describedby": titleId }) : image.button;
  return (
    <figure aria-labelledby={titleId} data-chart={spec.type} className="flex min-w-0 flex-col gap-3 rounded-md border px-3.5 pt-2.5 pb-3.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <p id={titleId} className="min-w-0 flex-1 basis-48 font-semibold">
          {name}
        </p>
        <div className="flex shrink-0 items-center gap-1.5">
          {!asTable && save}
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            aria-label={`Show “${name}” as`}
            value={asTable ? "table" : "chart"}
            onValueChange={(v) => v && setAsTable(v === "table")}
          >
            <ToggleGroupItem value="chart">Chart</ToggleGroupItem>
            <ToggleGroupItem value="table">Table</ToggleGroupItem>
          </ToggleGroup>
        </div>
      </div>
      {asTable ? (
        <ChartTableView spec={spec} paint={paint} />
      ) : (
        <div ref={image.ref} className="min-w-0">
          <Picture spec={spec} paint={paint} ids={ids} />
        </div>
      )}
      {image.error}
    </figure>
  );
}

function Picture({ spec, paint, ids }: { spec: ChartSpec; paint: Painter; ids: Ids }) {
  switch (spec.type) {
    case "bar":
    case "line":
      return <BasicPlot spec={spec} paint={paint} ids={ids} />;
    case "stacked_bar":
      return <StackedPlot spec={spec} paint={paint} ids={ids} />;
    case "donut":
      return <DonutPlot spec={spec} paint={paint} ids={ids} />;
    case "heatmap":
      return <HeatmapPlot spec={spec} />;
    case "gantt":
      return <GanttPlot spec={spec} paint={paint} ids={ids} />;
  }
}

// ---------------------------------------------------------------- axes

/** Width of the value axis for these tick labels. */
function axisWidth(ticks: number[]): number {
  return Math.max(24, Math.max(...ticks.map((t) => formatTick(t).length)) * AXIS_CHAR + 10);
}

function ValueAxis({ ticks, y, x0, x1 }: { ticks: number[]; y: (v: number) => number; x0: number; x1: number }) {
  return (
    <>
      {ticks.map((t) => {
        const yy = Math.round(y(t)) + 0.5;
        return (
          <g key={t}>
            <line x1={x0} x2={x1} y1={yy} y2={yy} style={t === 0 ? { stroke: "var(--border-strong)" } : gridStyle} />
            <text x={x0 - 6} y={yy + 4} textAnchor="end" className={AXIS_TEXT} style={axisStyle}>
              {formatTick(t)}
            </text>
          </g>
        );
      })}
    </>
  );
}

/** Labels under columns of width `col`, every n-th one when they would overlap. */
function ColumnLabels({ labels, x0, col, y }: { labels: string[]; x0: number; col: number; y: number }) {
  const longest = Math.max(...labels.map((l) => l.length));
  const need = Math.min(longest, 12) * CAPTION_CHAR + 8;
  const every = Math.max(1, Math.ceil(need / Math.max(1, col)));
  const chars = (col * every - 8) / CAPTION_CHAR;
  return (
    <>
      {labels.map((l, i) =>
        i % every === 0 ? (
          <text key={i} x={x0 + col * i + col / 2} y={y} textAnchor="middle" className="text-caption" style={labelStyle}>
            {fit(l, chars)}
            {fit(l, chars) !== l && <title>{l}</title>}
          </text>
        ) : null,
      )}
    </>
  );
}

// ---------------------------------------------------------------- bar and line

function BasicPlot({ spec, paint, ids }: { spec: BasicChart; paint: Painter; ids: Ids }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const n = spec.values.length;
  const scale = niceScale(Math.min(0, ...spec.values), Math.max(0, ...spec.values));
  const axisW = axisWidth(scale.ticks);
  const top = 8;
  const bottom = 24;
  const plotH = PLOT_H - top - bottom;
  const plotW = Math.max(0, width - axisW);
  const col = plotW / n;
  const y = (v: number) => top + ((scale.hi - v) / (scale.hi - scale.lo)) * plotH;
  const zero = y(0);
  const line = paint("", spec.color, null).fill;

  return (
    <div ref={ref} className="w-full" style={{ height: PLOT_H }}>
      {width > 0 && (
        <svg width={width} height={PLOT_H} role="img" aria-label={chartSummary(spec)} className="block overflow-visible">
          <SvgPatterns idleId={ids.idle} failedId={ids.failed} />
          <ValueAxis ticks={scale.ticks} y={y} x0={axisW} x1={width} />
          {spec.type === "bar"
            ? spec.values.map((v, i) => {
                if (v === 0) return null;
                const barW = Math.max(2, Math.min(44, col * 0.62));
                const cx = axisW + col * i + col / 2;
                const yy = y(v);
                const h = Math.max(1, Math.abs(zero - yy));
                const p = paint(spec.labels[i], spec.color, null);
                return (
                  <path key={i} d={endRounded(cx - barW / 2, v > 0 ? zero - h : zero, barW, h, 3, v > 0)} style={{ fill: p.fill }}>
                    <title>{`${spec.labels[i]}: ${formatValue(v)}`}</title>
                  </path>
                );
              })
            : (() => {
                const pts = spec.values.map((v, i) => [axisW + col * i + col / 2, y(v)] as const);
                const r = n > 30 ? 2.5 : 3.5;
                return (
                  <>
                    {n > 1 && (
                      <path
                        d={`M${pts.map(([px, py]) => `${px.toFixed(1)},${py.toFixed(1)}`).join("L")}`}
                        style={{ fill: "none", stroke: line, strokeWidth: 2, strokeLinejoin: "round", strokeLinecap: "round" }}
                      />
                    )}
                    {pts.map(([px, py], i) => (
                      <circle key={i} cx={px} cy={py} r={r} style={{ fill: line, stroke: "var(--surface-1)", strokeWidth: 1.5 }}>
                        <title>{`${spec.labels[i]}: ${formatValue(spec.values[i])}`}</title>
                      </circle>
                    ))}
                  </>
                );
              })()}
          <ColumnLabels labels={spec.labels} x0={axisW} col={col} y={PLOT_H - 6} />
        </svg>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- stacked bar

function StackedPlot({ spec, paint, ids }: { spec: StackedChart; paint: Painter; ids: Ids }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const n = spec.categories.length;
  const totals = spec.categories.map((_, i) => spec.series.reduce((a, s) => a + s.values[i], 0));
  const scale = niceScale(0, Math.max(...totals));
  const axisW = axisWidth(scale.ticks);
  const top = 8;
  const bottom = 24;
  const plotH = PLOT_H - top - bottom;
  const plotW = Math.max(0, width - axisW);
  const col = plotW / n;
  const per = plotH / (scale.hi - scale.lo);
  const zero = top + plotH;
  const barW = Math.max(2, Math.min(44, col * 0.62));
  const paints = spec.series.map((s, i) => paint(s.name, s.color, i));

  return (
    <div className="flex flex-col gap-3">
      <Legend items={spec.series.map((s, i) => ({ key: `${i}`, label: s.name, mark: paints[i].mark }))} />
      <div ref={ref} className="w-full" style={{ height: PLOT_H }}>
        {width > 0 && (
          <svg width={width} height={PLOT_H} role="img" aria-label={chartSummary(spec)} className="block overflow-visible">
            <SvgPatterns idleId={ids.idle} failedId={ids.failed} />
            <ValueAxis ticks={scale.ticks} y={(v) => zero - v * per} x0={axisW} x1={width} />
            {spec.categories.map((c, i) => {
              const x = axisW + col * i + col / 2 - barW / 2;
              let topSeries = -1;
              spec.series.forEach((s, j) => s.values[i] > 0 && (topSeries = j));
              let base = zero;
              return (
                <g key={i}>
                  {spec.series.map((s, j) => {
                    const v = s.values[i];
                    if (v <= 0) return null;
                    const h = Math.max(1, v * per);
                    const y0 = base - h;
                    base = y0;
                    const tip = <title>{`${c}, ${s.name}: ${formatValue(v)}`}</title>;
                    // A 1 px gap of the content layer between segments.
                    return j === topSeries ? (
                      <path key={j} d={endRounded(x, y0, barW, h, 3, true)} style={{ fill: paints[j].fill }}>
                        {tip}
                      </path>
                    ) : (
                      <rect key={j} x={x} y={y0 + 1} width={barW} height={Math.max(0.5, h - 1)} style={{ fill: paints[j].fill }}>
                        {tip}
                      </rect>
                    );
                  })}
                </g>
              );
            })}
            <ColumnLabels labels={spec.categories} x0={axisW} col={col} y={PLOT_H - 6} />
          </svg>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- donut

const DONUT = 168;

/** An annular sector from angle a0 to a1 (radians, clockwise from the right). */
function sector(cx: number, cy: number, r0: number, r1: number, a0: number, a1: number): string {
  if (a1 - a0 >= Math.PI * 2 - 1e-6) {
    // A full ring: two halves (one arc cannot start and end at the same point).
    return `${sector(cx, cy, r0, r1, a0, a0 + Math.PI)}${sector(cx, cy, r0, r1, a0 + Math.PI, a0 + Math.PI * 2 - 1e-9)}`;
  }
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const p = (r: number, a: number) => `${(cx + r * Math.cos(a)).toFixed(2)},${(cy + r * Math.sin(a)).toFixed(2)}`;
  return `M${p(r1, a0)}A${r1},${r1} 0 ${large} 1 ${p(r1, a1)}L${p(r0, a1)}A${r0},${r0} 0 ${large} 0 ${p(r0, a0)}Z`;
}

function DonutPlot({ spec, paint, ids }: { spec: DonutChart; paint: Painter; ids: Ids }) {
  const total = spec.values.reduce((a, v) => a + v, 0);
  const paints = spec.labels.map((l, i) => paint(l, spec.colors[i], i));
  const r1 = DONUT / 2 - 2;
  const r0 = r1 * 0.6;
  let a = -Math.PI / 2;
  const slices = spec.values.map((v, i) => {
    const sweep = (v / total) * Math.PI * 2;
    const inset = sweep > 0.08 && sweep < Math.PI * 2 - 1e-6 ? 0.012 : 0;
    const s = { i, a0: a + inset, a1: a + sweep - inset, v };
    a += sweep;
    return s;
  });
  return (
    <div className="flex flex-wrap items-center gap-x-8 gap-y-3">
      <svg width={DONUT} height={DONUT} role="img" aria-label={chartSummary(spec)} className="block shrink-0">
        <SvgPatterns idleId={ids.idle} failedId={ids.failed} />
        {slices.map((s) =>
          s.v > 0 ? (
            <path key={s.i} d={sector(DONUT / 2, DONUT / 2, r0, r1, s.a0, s.a1)} style={{ fill: paints[s.i].fill }}>
              <title>{`${spec.labels[s.i]}: ${formatValue(s.v)}, ${share(s.v, total)}`}</title>
            </path>
          ) : null,
        )}
      </svg>
      <Legend
        className="flex-col flex-nowrap gap-y-1.5 text-meta"
        items={spec.labels.map((l, i) => ({
          key: `${i}`,
          mark: paints[i].mark,
          label: (
            <>
              <span className="text-foreground">{l}</span>
              <span className="tnum">{share(spec.values[i], total)}</span>
            </>
          ),
        }))}
      />
    </div>
  );
}

// ---------------------------------------------------------------- heat map

/** How much of the full color each step takes (Weekly's focus heat map). */
const MIX = [30, 55, 78, 100];

function HeatmapPlot({ spec }: { spec: HeatmapChart }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const cols = spec.xLabels.length;
  const rows = spec.yLabels.length;
  const gap = cols > 24 ? 2 : 3;
  const labelW = Math.min(140, Math.max(28, Math.min(18, Math.max(...spec.yLabels.map((l) => l.length))) * CAPTION_CHAR + 10));
  const cw = Math.max(2, (width - labelW - gap * (cols - 1)) / cols);
  const ch = 22;
  const top = 18;
  const height = top + rows * (ch + gap);
  const flat = spec.values.flat();
  const lo = Math.min(...flat);
  const hi = Math.max(...flat);
  const base = spec.color ? markColor(spec.color) : null;
  // Zero is "nothing" (the empty well) when nothing is negative; otherwise four steps from low to high.
  const step = (v: number) => {
    if (v === 0 && lo >= 0) return 0;
    if (hi === lo) return 4;
    return Math.min(4, 1 + Math.floor(((v - lo) / (hi - lo)) * 4));
  };
  const fill = (s: number) => (s === 0 ? "var(--heat-0)" : base ? (s === 4 ? base : `color-mix(in oklab, ${base} ${MIX[s - 1]}%, var(--surface-1))`) : `var(--heat-${s})`);
  const longestX = Math.max(...spec.xLabels.map((l) => l.length));
  const every = Math.max(1, Math.ceil((Math.min(longestX, 8) * CAPTION_CHAR + 4) / (cw + gap)));

  return (
    <div className="flex flex-col gap-2.5">
      <div ref={ref} className="w-full" style={{ height }}>
        {width > 0 && (
          <svg width={width} height={height} role="img" aria-label={chartSummary(spec)} className="block">
            {spec.xLabels.map((l, i) =>
              i % every === 0 ? (
                <text key={i} x={labelW + i * (cw + gap)} y={11} className="text-caption" style={labelStyle}>
                  {fit(l, ((cw + gap) * every - 4) / CAPTION_CHAR)}
                </text>
              ) : null,
            )}
            {spec.yLabels.map((yl, r) => {
              const yy = top + r * (ch + gap);
              return (
                <g key={r}>
                  <text x={0} y={yy + ch / 2 + 4} className="text-caption" style={labelStyle}>
                    {fit(yl, (labelW - 8) / CAPTION_CHAR)}
                  </text>
                  {spec.values[r].map((v, c) => (
                    <rect key={c} x={labelW + c * (cw + gap)} y={yy} width={cw} height={ch} rx={Math.min(3, cw / 3)} style={{ fill: fill(step(v)) }}>
                      <title>{`${yl}, ${spec.xLabels[c]}: ${formatValue(v)}`}</title>
                    </rect>
                  ))}
                </g>
              );
            })}
          </svg>
        )}
      </div>
      <p className="flex flex-wrap items-center gap-1.5 text-caption text-muted-foreground" style={{ paddingLeft: labelW }}>
        Less
        {[0, 1, 2, 3, 4].map((s) => (
          <span key={s} aria-hidden="true" data-mark="" className="h-2.5 w-4 rounded-[2px]" style={{ background: fill(s) }} />
        ))}
        More
        <span className="sr-only">
          . Darker cells hold higher values, from {formatValue(lo)} to {formatValue(hi)}.
        </span>
      </p>
    </div>
  );
}

// ---------------------------------------------------------------- Gantt

function GanttPlot({ spec, paint, ids }: { spec: GanttChart; paint: Painter; ids: Ids }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  // Items with the same label share a row (Dayflow's y axis is the label).
  const rowsOf = new Map<string, number>();
  for (const it of spec.items) if (!rowsOf.has(it.label)) rowsOf.set(it.label, rowsOf.size);
  const rowLabels = [...rowsOf.keys()];
  const scale = niceScale(Math.min(...spec.items.map((i) => i.start)), Math.max(...spec.items.map((i) => i.end)));
  const labelW = Math.min(160, Math.max(36, Math.min(22, Math.max(...rowLabels.map((l) => l.length))) * CAPTION_CHAR + 12));
  const rowH = 26;
  const barH = 14;
  const axisH = 20;
  const height = rowLabels.length * rowH + axisH;
  const plotW = Math.max(0, width - labelW - 8);
  const x = (v: number) => labelW + ((v - scale.lo) / (scale.hi - scale.lo)) * plotW;
  const plotBottom = rowLabels.length * rowH;

  return (
    <div ref={ref} className="w-full" style={{ height }}>
      {width > 0 && (
        <svg width={width} height={height} role="img" aria-label={chartSummary(spec)} className="block overflow-visible">
          <SvgPatterns idleId={ids.idle} failedId={ids.failed} />
          {scale.ticks.map((t, i) => {
            const xx = Math.round(x(t)) + 0.5;
            return (
              <g key={t}>
                <line x1={xx} x2={xx} y1={0} y2={plotBottom} style={gridStyle} />
                <text x={xx} y={plotBottom + 14} textAnchor={i === 0 ? "start" : i === scale.ticks.length - 1 ? "end" : "middle"} className={AXIS_TEXT} style={axisStyle}>
                  {formatTick(t)}
                </text>
              </g>
            );
          })}
          {rowLabels.map((l, r) => (
            <text key={r} x={0} y={r * rowH + rowH / 2 + 4} className="text-caption" style={labelStyle}>
              {fit(l, (labelW - 10) / CAPTION_CHAR)}
              {fit(l, (labelW - 10) / CAPTION_CHAR) !== l && <title>{l}</title>}
            </text>
          ))}
          {spec.items.map((it, i) => {
            const r = rowsOf.get(it.label)!;
            const x0 = x(it.start);
            const p = paint(it.label, it.color, null);
            return (
              <rect key={i} x={x0} y={r * rowH + (rowH - barH) / 2} width={Math.max(2, x(it.end) - x0)} height={barH} rx={3} style={{ fill: p.fill }}>
                <title>{`${it.label}: ${formatValue(it.start)} to ${formatValue(it.end)}`}</title>
              </rect>
            );
          })}
        </svg>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- table

function ChartTableView({ spec, paint }: { spec: ChartSpec; paint: Painter }) {
  const t = chartTable(spec);
  const rowMark = (i: number) => (spec.type === "donut" ? paint(spec.labels[i], spec.colors[i], i).mark : null);
  const colMark = (j: number) => (spec.type === "stacked_bar" && j >= 1 && j <= spec.series.length ? paint(spec.series[j - 1].name, spec.series[j - 1].color, j - 1).mark : null);
  return (
    <div className="scroll-thin max-w-full overflow-x-auto">
      <table className="w-full text-meta">
        <caption className="sr-only">{chartName(spec)}</caption>
        <thead>
          <tr className="border-b text-left text-muted-foreground">
            {t.head.map((h, j) => (
              <th key={j} scope="col" className={cn("py-2 font-normal whitespace-nowrap", j === 0 ? "pr-3" : "pl-3 text-right")}>
                <span className="inline-flex items-center gap-1.5">
                  {colMark(j)}
                  {h || <span className="sr-only">Label</span>}
                </span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {t.rows.map((r, i) => (
            <tr key={i} className="border-b last:border-0">
              <th scope="row" className="py-1.5 pr-3 text-left font-normal">
                <span className="inline-flex items-center gap-1.5">
                  {rowMark(i)}
                  {r[0]}
                </span>
              </th>
              {r.slice(1).map((c, j) => (
                <td key={j} className="py-1.5 pl-3 text-right whitespace-nowrap tnum">
                  {c}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
        {t.foot && (
          <tfoot>
            <tr className="border-t">
              <th scope="row" className="pt-2 pr-3 text-left font-normal text-muted-foreground">
                {t.foot[0]}
              </th>
              {t.foot.slice(1).map((c, j) => (
                <td key={j} className="pt-2 pl-3 text-right font-medium whitespace-nowrap tnum">
                  {c}
                </td>
              ))}
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}
