import { useEffect, useState } from "react";
import { api, Card, Shot, clock, formatDuration } from "./api";

function Thumb({ shot, onOpen }: { shot: Shot; onOpen: (s: Shot) => void }) {
  const [src, setSrc] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    let live = true;
    if (shot.hiddenApp) return;
    api.getScreenshot(shot.id, true).then((s) => {
      if (!live) return;
      if (s) setSrc(s);
      else setMissing(true);
    });
    return () => {
      live = false;
    };
  }, [shot.id, shot.hiddenApp]);
  const label = clock(shot.capturedAt);
  if (shot.hiddenApp) {
    return (
      <div className="thumb hidden" title={`${shot.hiddenApp} was in front; not captured`}>
        <span>{shot.hiddenApp} hidden</span>
        <time>{label}</time>
      </div>
    );
  }
  return (
    <button className="thumb" onClick={() => src && onOpen(shot)} title={shot.windowTitle ?? undefined} disabled={!src}>
      {src ? <img src={src} alt={`Screen at ${label}`} /> : <span>{missing ? "Deleted" : ""}</span>}
      <time>{label}</time>
    </button>
  );
}

export function CardDetail({ card, color, onRetry }: { card: Card | null; color: string; onRetry: (id: number) => void }) {
  const [shots, setShots] = useState<Shot[]>([]);
  const [viewer, setViewer] = useState<{ shot: Shot; src: string } | null>(null);

  useEffect(() => {
    setShots([]);
    if (card) api.getCardScreenshots(card.id, 12).then(setShots);
  }, [card?.id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setViewer(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!card) {
    return (
      <aside className="detail empty">
        <p>Select a card to see what happened and the screenshots it was written from.</p>
      </aside>
    );
  }
  const failed = card.category === "System";
  const lines = card.detailedSummary.split("\n").map((l) => l.trim()).filter(Boolean);

  return (
    <aside className="detail" style={{ ["--cat" as string]: color }}>
      <div className="detail-head">
        <div className="detail-when">
          {card.start} – {card.end}, {formatDuration(card.endTs - card.startTs)}
        </div>
        <h2>{card.title}</h2>
        <div className="detail-cat">
          <span className="swatch" />
          {failed ? "Not analyzed" : card.category}
          {card.subcategory && !failed ? `, ${card.subcategory}` : ""}
        </div>
      </div>

      <p className="detail-summary">{card.summary}</p>
      {failed && (
        <div className="retry">
          <button className="primary" onClick={() => onRetry(card.id)}>Try again</button>
          {card.detailedSummary && <details><summary>Error details</summary><pre>{card.detailedSummary}</pre></details>}
        </div>
      )}

      {shots.length > 0 && (
        <div className="strip" aria-label="Screenshots">
          {shots.map((s) => (
            <Thumb
              key={s.id}
              shot={s}
              onOpen={async (sh) => {
                const src = await api.getScreenshot(sh.id, false);
                if (src) setViewer({ shot: sh, src });
              }}
            />
          ))}
        </div>
      )}

      {card.distractions.length > 0 && (
        <section className="sub">
          <h3>Interruptions</h3>
          <ul className="plain">
            {card.distractions.map((d, i) => (
              <li key={i}>
                <span className="t">{d.startTime}</span> {d.title}
                {d.summary && <span className="muted"> {d.summary}</span>}
              </li>
            ))}
          </ul>
        </section>
      )}

      {!failed && lines.length > 0 && (
        <section className="sub">
          <h3>Step by step</h3>
          <ul className="plain log">
            {lines.map((l, i) => (
              <li key={i}>{l.replace(/^[-•]\s*/, "")}</li>
            ))}
          </ul>
        </section>
      )}

      {viewer && (
        <div className="viewer" role="dialog" aria-label="Screenshot" onClick={() => setViewer(null)}>
          <img src={viewer.src} alt={`Screen at ${clock(viewer.shot.capturedAt)}`} />
          <div className="viewer-cap">
            {clock(viewer.shot.capturedAt)}
            {viewer.shot.windowTitle ? `, ${viewer.shot.windowTitle}` : ""}
          </div>
        </div>
      )}
    </aside>
  );
}
