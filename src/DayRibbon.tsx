import { Card, dayBounds } from "./api";

const HOURS = [4, 8, 12, 16, 20, 0];

/** The whole logical day (4 AM to 4 AM) as one strip, each card placed at its real time. */
export function DayRibbon({
  day,
  cards,
  colorFor,
  selected,
  onSelect,
}: {
  day: string;
  cards: Card[];
  colorFor: (c: string) => string;
  selected: number | null;
  onSelect: (id: number) => void;
}) {
  const [start, end] = dayBounds(day);
  const span = end - start;
  const now = Date.now() / 1000;
  const nowPct = now > start && now < end ? ((now - start) / span) * 100 : null;
  return (
    <div className="ribbon" aria-label="Day overview">
      <div className="ribbon-track">
        {cards.map((c) => {
          const left = ((Math.max(c.startTs, start) - start) / span) * 100;
          const width = Math.max(0.25, ((Math.min(c.endTs, end) - Math.max(c.startTs, start)) / span) * 100);
          return (
            <button
              key={c.id}
              className={`seg${selected === c.id ? " sel" : ""}`}
              style={{ left: `${left}%`, width: `${width}%`, background: colorFor(c.category) }}
              title={`${c.start} – ${c.end}  ${c.title}`}
              aria-label={`${c.start} to ${c.end}: ${c.title}`}
              onClick={() => onSelect(c.id)}
            />
          );
        })}
        {nowPct !== null && <div className="now" style={{ left: `${nowPct}%` }} />}
      </div>
      <div className="ribbon-hours">
        {HOURS.map((h, i) => (
          <span key={h} style={{ left: `${(i * 4 * 3600 * 100) / span}%` }}>
            {h === 0 ? "12 AM" : h === 12 ? "12 PM" : h < 12 ? `${h} AM` : `${h - 12} PM`}
          </span>
        ))}
      </div>
    </div>
  );
}
