import type { ReactNode } from "react";
import { formatDuration } from "../api";
import { fill, hourLabel, hourTicks } from "./insights-api";

/** A titled block inside the Daily and Weekly views. */
export function Panel({ title, children, wide }: { title: string; children: ReactNode; wide?: boolean }) {
  return (
    <section className={wide ? "ins-panel ins-wide" : "ins-panel"}>
      <h3>{title}</h3>
      {children}
    </section>
  );
}

export function Swatch({ color }: { color: string }) {
  return <i className="ins-swatch" style={{ background: color }} aria-hidden="true" />;
}

/** Horizontal bars, longest first as given. */
export function Bars({
  items,
  empty,
  swatches = true,
}: {
  items: { key: string; name: string; secs: number; color?: string; note?: string }[];
  empty: string;
  swatches?: boolean;
}) {
  if (!items.length) return <p className="ins-empty">{empty}</p>;
  const max = Math.max(...items.map((i) => i.secs), 1);
  return (
    <ul className="ins-bars">
      {items.map((i) => (
        <li key={i.key}>
          <span className="ins-bar-label">
            {swatches && i.color && <Swatch color={i.color} />}
            {i.name}
          </span>
          <span className="ins-bar" aria-hidden="true">
            <span style={{ width: `${(i.secs / max) * 100}%`, background: i.color ?? "var(--accent)" }} />
          </span>
          <span className="ins-bar-value">
            {formatDuration(i.secs)}
            {i.note && <span className="ins-note"> {i.note}</span>}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** "9:15 AM" for minutes since 4 AM. */
export function minuteLabel(m: number): string {
  const total = Math.round(m);
  const h = (4 + Math.floor(total / 60)) % 24;
  const min = total % 60;
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${String(min).padStart(2, "0")} ${h < 12 ? "AM" : "PM"}`;
}

/** Hour labels under a track that runs from `start` to `end` (minutes since 4 AM). Labels start at
 * their tick, so ticks too close to the right edge are left out. */
export function Axis({ start, end, step }: { start: number; end: number; step?: number }) {
  const span = Math.max(1, end - start);
  const ticks = hourTicks(start, end, step ?? (span > 14 * 60 ? 2 : 1)).filter((m) => m < end - span * 0.05);
  return (
    <div className="ins-row ins-axis" aria-hidden="true">
      <span />
      <div className="ins-track">
        {ticks.map((m) => (
          <span key={m} style={{ left: `${((m - start) / span) * 100}%` }}>
            {hourLabel(m)}
          </span>
        ))}
      </div>
    </div>
  );
}

/** One row of equal cells. */
export function CellRow({
  label,
  cells,
}: {
  label: ReactNode;
  cells: { color: string | null; occupancy: number; title: string; onClick?: () => void }[];
}) {
  return (
    <div className="ins-row">
      <span className="ins-row-label">{label}</span>
      <div className="ins-cells" style={{ gridTemplateColumns: `repeat(${Math.max(1, cells.length)}, 1fr)` }}>
        {cells.map((c, i) => (
          <span
            key={i}
            className={c.onClick ? "ins-cell ins-link" : "ins-cell"}
            style={{ background: c.color ? fill(c.color, c.occupancy) : "var(--soft)" }}
            title={c.title}
            onClick={c.onClick}
          />
        ))}
      </div>
    </div>
  );
}

export function Stat({ label, value, tone }: { label: string; value: string; tone?: "bad" }) {
  return (
    <div className="ins-stat">
      <dt>{label}</dt>
      <dd className={tone === "bad" ? "ins-bad" : undefined}>{value}</dd>
    </div>
  );
}
