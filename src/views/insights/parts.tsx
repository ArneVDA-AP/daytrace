import * as React from "react";
import type { Category } from "@/api";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { CategoryTotal, Kind } from "./api";
import { byCategory, percent } from "./format";

/*
 * Building blocks shared by Daily and Weekly: titled cards, stat tiles, category marks and
 * legends, the time-by-category breakdown, bar lists and the inline error notice.
 */

/** A titled card (h2) inside a section page. */
export function SectionCard({
  title,
  description,
  actions,
  children,
  className,
  id,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  id?: string;
}) {
  const hid = React.useId();
  return (
    <section aria-labelledby={hid} id={id} className={cn("flex min-w-0 flex-col rounded-lg border bg-surface-1", className)}>
      <div className="flex flex-wrap items-start gap-x-3 gap-y-2 px-4 pt-3.5 pb-3">
        <div className="min-w-0 flex-1 basis-56">
          <h2 id={hid} className="text-body font-semibold">
            {title}
          </h2>
          {description && <p className="mt-0.5 text-meta text-muted-foreground">{description}</p>}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
      <div className="min-w-0 flex-1 px-4 pb-4">{children}</div>
    </section>
  );
}

/** A row of stat tiles in one bordered strip (a description list). */
export function StatStrip({ children, className, label }: { children: React.ReactNode; className?: string; label: string }) {
  return (
    <dl aria-label={label} className={cn("grid gap-px overflow-hidden rounded-lg border bg-border", className)}>
      {children}
    </dl>
  );
}

export function StatTile({
  label,
  value,
  note,
  mark,
  className,
}: {
  label: string;
  value: React.ReactNode;
  note?: React.ReactNode;
  mark?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-0.5 bg-surface-1 px-4 py-3", className)}>
      <dt className="flex items-center gap-1.5 text-meta text-muted-foreground">
        {mark}
        {label}
      </dt>
      <dd className="font-display text-title font-semibold tracking-[-0.01em] tnum">{value}</dd>
      {note && <dd className="text-caption text-muted-foreground">{note}</dd>}
    </div>
  );
}

/** A small square in a category's color: idle is a hatch, failed cards are red stripes. */
export function CategoryMark({ name, kind, colorFor, className }: { name: string; kind: Kind; colorFor: (n: string) => string; className?: string }) {
  if (kind === "idle") return <span aria-hidden="true" data-mark="" className={cn("hatch-idle size-2.5 shrink-0 rounded-[2px] bg-surface-1 ring-1 ring-cat-idle ring-inset", className)} />;
  if (kind === "system") return <span aria-hidden="true" data-mark="" className={cn("stripes-failed size-2.5 shrink-0 rounded-[2px] bg-surface-1", className)} />;
  return <span aria-hidden="true" className={cn("size-2.5 shrink-0 rounded-[2px]", className)} style={{ background: colorFor(name) }} />;
}

/** "Not recorded" key: the empty well with a hairline. */
export function EmptyMark({ className }: { className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn("size-2.5 shrink-0 rounded-[2px] bg-surface-2 ring-1 ring-border-strong ring-inset forced-colors:border forced-colors:border-[GrayText]", className)}
    />
  );
}

export interface LegendItem {
  key: string;
  label: React.ReactNode;
  mark: React.ReactNode;
}

/** Legend in stack order, one wrapping row. */
export function Legend({ items, className }: { items: LegendItem[]; className?: string }) {
  return (
    <ul className={cn("flex flex-wrap gap-x-4 gap-y-1 text-caption text-muted-foreground", className)}>
      {items.map((i) => (
        <li key={i.key} className="inline-flex items-center gap-1.5">
          {i.mark}
          {i.label}
        </li>
      ))}
    </ul>
  );
}

/**
 * Time by category: a 100 % bar (context) and a table (the numbers). Rows in settings order,
 * idle hatched. `footer` adds rows under the total (e.g. time not analyzed).
 */
export function CategoryBreakdown({
  totals,
  categories,
  colorFor,
  caption,
  totalLabel = "Recorded",
  footer,
}: {
  totals: CategoryTotal[];
  categories: Category[];
  colorFor: (n: string) => string;
  caption: string;
  totalLabel?: string;
  footer?: React.ReactNode;
}) {
  const rows = byCategory(
    totals.filter((t) => t.secs > 0 && t.kind !== "system"),
    categories,
  );
  const sum = rows.reduce((a, r) => a + r.secs, 0);
  if (!sum) return <p className="text-meta text-muted-foreground">No time recorded.</p>;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex h-3 w-full gap-[2px]" aria-hidden="true">
        {rows.map((r) => (
          <span
            key={r.key}
            data-mark=""
            className={cn("h-full min-w-[3px] first:rounded-l-[3px] last:rounded-r-[3px]", r.kind === "idle" && "hatch-idle bg-surface-1 ring-1 ring-cat-idle ring-inset")}
            style={{ flexGrow: r.secs, flexBasis: 0, ...(r.kind === "idle" ? {} : { background: colorFor(r.name) }) }}
          />
        ))}
      </div>
      <table className="w-full text-meta">
        <caption className="sr-only">{caption}</caption>
        <thead className="sr-only">
          <tr>
            <th scope="col">Category</th>
            <th scope="col">Time</th>
            <th scope="col">Share</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.key} className="border-b">
              <th scope="row" className="py-1.5 pr-2 text-left font-normal">
                <span className="inline-flex min-w-0 items-center gap-2">
                  <CategoryMark name={r.name} kind={r.kind} colorFor={colorFor} />
                  <span className="truncate">{r.name}</span>
                </span>
              </th>
              <td className="py-1.5 text-right font-medium whitespace-nowrap tnum">{formatDuration(r.secs)}</td>
              <td className="w-14 py-1.5 text-right text-muted-foreground tnum">{percent(r.secs, sum)}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <th scope="row" className="pt-2 text-left font-normal text-muted-foreground">
              {totalLabel}
            </th>
            <td className="pt-2 text-right font-medium whitespace-nowrap tnum">{formatDuration(sum)}</td>
            <td />
          </tr>
          {footer}
        </tfoot>
      </table>
    </div>
  );
}

export interface BarItem {
  key: string;
  name: string;
  secs: number;
  /** CSS color of the bar; neutral when absent. */
  color?: string;
  /** Visible note after the duration ("+20 min on last week"). */
  note?: string;
  /** Read after the duration by screen readers only (", mostly Work"). */
  srNote?: string;
}

/** Ranked bars with the name, the bar and the duration; names are app or program names (translate="no"). */
export function BarList({ items, empty, label }: { items: BarItem[]; empty: string; label: string }) {
  if (!items.length) return <p className="text-meta text-muted-foreground">{empty}</p>;
  const max = Math.max(...items.map((i) => i.secs), 1);
  return (
    <ul aria-label={label} className="flex flex-col gap-2.5">
      {items.map((i) => (
        <li key={i.key} className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)_auto] items-center gap-x-3 text-meta">
          <span className="min-w-0">
            <span className="block truncate" translate="no" title={i.name}>
              {i.name}
            </span>
            {i.note && (
              <span aria-hidden="true" className="block truncate text-caption text-muted-foreground">
                {i.note}
              </span>
            )}
          </span>
          <span className="h-2 overflow-hidden rounded-full bg-surface-2" aria-hidden="true">
            <span className="block h-full min-w-[3px] rounded-full" style={{ width: `${(i.secs / max) * 100}%`, background: i.color ?? "var(--chart-5)" }} />
          </span>
          <span className="text-right whitespace-nowrap tnum">
            {formatDuration(i.secs)}
            {i.srNote && <span className="sr-only">{i.srNote}</span>}
            {i.note && <span className="sr-only">, {i.note}</span>}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** "Try again" for an error Banner. */
export function RetryButton({ onClick, busy, label = "Try again" }: { onClick: () => void; busy?: boolean; label?: string }) {
  return (
    <Button variant="outline" size="sm" onClick={onClick} disabled={busy}>
      {label}
    </Button>
  );
}

/** Skeleton in the shape of a SectionCard with a few lines. */
export function CardSkeleton({ className, lines = 4, tall }: { className?: string; lines?: number; tall?: boolean }) {
  return (
    <div className={cn("flex flex-col gap-3 rounded-lg border bg-surface-1 p-4", className)} aria-hidden="true">
      <Skeleton className="h-4 w-32" />
      {tall && <Skeleton className="h-40 w-full" />}
      {Array.from({ length: lines }, (_, i) => (
        <Skeleton key={i} className="h-3.5" style={{ width: `${92 - ((i * 17) % 35)}%` }} />
      ))}
    </div>
  );
}

/** Width of an element in CSS pixels, updated on resize (0 before the first layout). */
export function useWidth<T extends HTMLElement>(): [React.RefObject<T | null>, number] {
  const ref = React.useRef<T>(null);
  const [width, setWidth] = React.useState(0);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(Math.round(el.getBoundingClientRect().width));
    const ro = new ResizeObserver((entries) => {
      const w = Math.round(entries[0].contentRect.width);
      setWidth((cur) => (cur === w ? cur : w));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, width];
}

/**
 * SVG patterns for idle (hatch over the surface) and failed cards (red stripes). Ids come from
 * the caller (React.useId) so two charts on a page do not share them.
 */
export function SvgPatterns({ idleId, failedId }: { idleId: string; failedId?: string }) {
  return (
    <defs>
      <pattern id={idleId} width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
        <rect width="5" height="5" style={{ fill: "var(--surface-1)" }} />
        <rect width="1.5" height="5" style={{ fill: "var(--cat-idle)" }} />
      </pattern>
      {failedId && (
        <pattern id={failedId} width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <rect width="6" height="6" style={{ fill: "var(--surface-1)" }} />
          <rect width="2" height="6" style={{ fill: "var(--destructive)" }} />
        </pattern>
      )}
    </defs>
  );
}
