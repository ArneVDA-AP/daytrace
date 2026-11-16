import * as React from "react";
import { CircleCheck, CircleDashed, CircleX } from "lucide-react";
import { clock } from "@/lib/time";
import { cn } from "@/lib/utils";
import { timelineApi, type SessionCard, type SessionGrade } from "./api";
import { conditionText, drawingAlt, folderName, sessionCardCount, svgDataUrl } from "./session-card-text";

/*
 * Finished Claude Code session cards (backend: src-tauri/src/features/session_cards.rs). A session
 * card says what a Claude Code session worked toward, the check it set for itself, and its grade:
 * pass, partial or fail. Each one is shown on the card that covers the time the session finished
 * (CardDetail) and in the day's list (DaySoFar), where one that finished in time no card covers is
 * listed too. The text was written by an AI session and is rendered as plain text.
 *
 * A record can come with a drawing: the card as the session showed it, an SVG file. It is shown
 * as an image (an <img> with a data: URL), never put into the page, so nothing in the file can
 * run or reach the page. An image follows the color-scheme of the page, so it follows the theme.
 * The card as text stays below it, and is the whole card when there is no drawing. The drawing
 * ends a text that does not fit with "…", so the text below it is the place to read it whole.
 */

/** Width of the drawing in px: the width the session drew it at. */
const DRAWING_WIDTH = 352;

/**
 * The drawing of a card, read when `wanted` turns true. `undefined` while it is not read yet,
 * `null` when the card has none or the file cannot be read.
 */
function useDrawing(name: string | null, wanted: boolean): string | null | undefined {
  const [svg, setSvg] = React.useState<string | null | undefined>(undefined);
  React.useEffect(() => {
    if (name === null || !wanted) return;
    let live = true;
    timelineApi.sessionCardVisual(name).then(
      (s) => live && setSvg(s),
      () => live && setSvg(null),
    );
    return () => {
      live = false;
    };
  }, [name, wanted]);
  return name === null ? null : svg;
}

const GRADE: Record<SessionGrade, { label: string; icon: typeof CircleCheck; tone: string }> = {
  pass: { label: "Pass", icon: CircleCheck, tone: "text-foreground" },
  partial: { label: "Partial", icon: CircleDashed, tone: "text-warn-foreground" },
  fail: { label: "Fail", icon: CircleX, tone: "text-destructive" },
};

export { sessionCardCount };

export function SessionCardList({
  id,
  title,
  note,
  cards,
  where,
  drawing = "folded",
  className,
}: {
  /** Id of the heading (one list per pane). */
  id: string;
  title: string;
  note?: React.ReactNode;
  cards: SessionCard[];
  /** A line under a card that says where it belongs (the day's list links to the card). */
  where?: (card: SessionCard) => React.ReactNode;
  /** "open": the drawing of each card is shown at once. "folded": it opens on request. */
  drawing?: "open" | "folded";
  className?: string;
}) {
  return (
    <section aria-labelledby={id} className={cn("flex flex-col gap-2", className)}>
      <h3 id={id} className="text-body font-semibold">
        {title}
      </h3>
      {note && <p className="text-meta text-muted-foreground">{note}</p>}
      {cards.length > 0 && (
        <ul className="flex flex-col divide-y rounded-md border">
          {cards.map((c) => (
            <SessionCardItem key={`${c.sessionId}-${c.finishedAt}`} card={c} where={where?.(c)} isOpen={drawing === "open"} />
          ))}
        </ul>
      )}
    </section>
  );
}

function SessionCardItem({ card: c, where, isOpen }: { card: SessionCard; where?: React.ReactNode; isOpen: boolean }) {
  const g = GRADE[c.grade] ?? GRADE.partial;
  const Icon = g.icon;
  const folder = folderName(c.cwd);
  const hasMore = c.check !== "" || c.conditions.length > 0 || c.steps.length > 0;
  const [wanted, setWanted] = React.useState(isOpen);
  const svg = useDrawing(c.visual, wanted);
  const image = typeof svg === "string" && (
    <img
      src={svgDataUrl(svg)}
      alt={drawingAlt(g.label, c.outcome, c.gradeNote)}
      width={DRAWING_WIDTH}
      draggable={false}
      // A ring, not a border: a border would take 2 px of the width and scale the drawing.
      className="block h-auto max-w-full rounded-md ring-1 ring-border"
      data-session-drawing={c.visual}
    />
  );
  // Open: the drawing stands in for the outcome and the grade note, which it shows itself.
  const inPlace = isOpen && svg !== null;
  return (
    <li className="flex flex-col gap-1 px-3 py-2.5 text-meta" data-session-card={c.sessionId}>
      <div className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1">
        <span className={cn("inline-flex shrink-0 items-center gap-1 font-medium", g.tone)}>
          <Icon className="size-3.5" aria-hidden="true" />
          {g.label}
        </span>
        <span className="shrink-0 font-mono text-muted-foreground tnum">
          {c.approximate ? "about " : ""}
          {clock(c.finishedAt)}
        </span>
        {folder && (
          <span className="min-w-0 truncate rounded-sm border bg-surface-2 px-1.5 py-px text-caption text-muted-foreground" translate="no" title={c.cwd}>
            {folder}
          </span>
        )}
      </div>
      {inPlace ? (
        image
      ) : (
        <>
          <p className="text-body text-pretty [overflow-wrap:anywhere]">{c.outcome}</p>
          {c.gradeNote && <p className="text-pretty text-muted-foreground [overflow-wrap:anywhere]">{c.gradeNote}</p>}
        </>
      )}
      {where && <p className="text-muted-foreground">{where}</p>}
      {!isOpen && c.visual !== null && (
        <details className="group" onToggle={(e) => e.currentTarget.open && setWanted(true)}>
          <summary className="w-fit cursor-default rounded-sm text-muted-foreground select-none hover:text-foreground">Card as the session showed it</summary>
          <div className="mt-2">{svg === null ? <p className="text-muted-foreground">The drawing of this card cannot be read.</p> : image}</div>
        </details>
      )}
      {(hasMore || inPlace) && (
        <details className="group">
          <summary className="w-fit cursor-default rounded-sm text-muted-foreground select-none hover:text-foreground">
            {inPlace ? "Card as text" : "Check and steps"}
          </summary>
          <div className="mt-2 flex flex-col gap-2">
            {/* The drawing cuts a text that does not fit. Here the outcome and the grade note are whole. */}
            {inPlace && (
              <>
                <p className="text-body text-pretty [overflow-wrap:anywhere]">{c.outcome}</p>
                {c.gradeNote && <p className="text-pretty text-muted-foreground [overflow-wrap:anywhere]">{c.gradeNote}</p>}
              </>
            )}
            {c.check && (
              <p className="text-pretty [overflow-wrap:anywhere]">
                <span className="text-muted-foreground">Check: </span>
                {c.check}
              </p>
            )}
            {c.conditions.length > 0 && (
              <ul className="flex flex-col gap-0.5">
                {c.conditions.map((k, i) => (
                  <li key={i} className="grid grid-cols-[minmax(0,1fr)_auto] gap-3">
                    <span className="[overflow-wrap:anywhere]">{k.name}</span>
                    <span className={cn("whitespace-nowrap text-muted-foreground", k.state === "failed" && "text-destructive")}>
                      {conditionText(k.kind, k.state)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            {c.steps.length > 0 && (
              <ol className="list-decimal pl-5 text-muted-foreground">
                {c.steps.map((s, i) => (
                  <li key={i} className="[overflow-wrap:anywhere]">
                    {s}
                  </li>
                ))}
              </ol>
            )}
            <p className="text-caption text-muted-foreground">
              Graded by {c.gradedBy || "the session itself"} · started at {clock(c.startedAt)} · session{" "}
              <span className="font-mono" translate="no" title={c.sessionId}>
                {c.sessionId.slice(0, 8)}
              </span>
            </p>
          </div>
        </details>
      )}
    </li>
  );
}
