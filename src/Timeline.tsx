import type React from "react";
import { Card, Status, formatDuration } from "./api";

const PX_PER_MIN = 1.5;
const MIN_HEIGHT = 64;

export function Timeline({
  cards,
  colorFor,
  selected,
  onSelect,
  isToday,
  status,
  recording,
}: {
  cards: Card[];
  colorFor: (c: string) => string;
  selected: number | null;
  onSelect: (id: number) => void;
  isToday: boolean;
  status: Status;
  recording: boolean;
}) {
  if (cards.length === 0) {
    return (
      <section className="timeline empty">
        {isToday ? (
          recording ? (
            <p>
              Nothing on today’s timeline yet. Daytrace writes the first card about 15 minutes after recording starts
              {status.pendingBatches > 0 ? ", and is working on it now." : "."}
            </p>
          ) : (
            <p>Recording is paused, so today’s timeline is empty. Resume recording to start a new day.</p>
          )
        ) : (
          <p>No timeline for this day.</p>
        )}
      </section>
    );
  }

  const rows: React.ReactElement[] = [];
  cards.forEach((c, i) => {
    const prev = cards[i - 1];
    if (prev && c.startTs - prev.endTs > 5 * 60) {
      rows.push(
        <div key={`gap-${c.id}`} className="gap">
          Not recording, {formatDuration(c.startTs - prev.endTs)}
        </div>,
      );
    }
    const mins = (c.endTs - c.startTs) / 60;
    const failed = c.category === "System";
    const sites = [c.appSites?.primary, c.appSites?.secondary].filter(Boolean).join(", ");
    rows.push(
      <button
        key={c.id}
        className={`card${selected === c.id ? " sel" : ""}${failed ? " failed" : ""}${c.isIdleShortcut ? " idle" : ""}`}
        style={{ minHeight: Math.max(MIN_HEIGHT, mins * PX_PER_MIN), ["--cat" as string]: colorFor(c.category) }}
        onClick={() => onSelect(c.id)}
      >
        <div className="card-time">
          <span>{c.start}</span>
          <span className="card-dur">{formatDuration(c.endTs - c.startTs)}</span>
        </div>
        <div className="card-body">
          <div className="card-title">{c.title}</div>
          <div className="card-meta">
            {failed ? "Not analyzed" : c.category}
            {sites && <span className="sites">{sites}</span>}
          </div>
        </div>
      </button>,
    );
  });
  return <section className="timeline">{rows}</section>;
}
