import * as React from "react";
import { CircleCheck, CircleDashed, CircleX } from "lucide-react";
import { clock } from "@/lib/time";
import { cn } from "@/lib/utils";
import type { SessionCard, SessionGrade } from "./api";
import { conditionText, folderName, sessionCardCount } from "./session-card-text";

/*
 * Finished Claude Code session cards (backend: src-tauri/src/features/session_cards.rs). A session
 * card says what a Claude Code session worked toward, the check it set for itself, and its grade:
 * pass, partial or fail. Each one is shown on the card that covers the time the session finished
 * (CardDetail) and in the day's list (DaySoFar), where one that finished in time no card covers is
 * listed too. The text was written by an AI session and is rendered as plain text.
 */

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
  className,
}: {
  /** Id of the heading (one list per pane). */
  id: string;
  title: string;
  note?: React.ReactNode;
  cards: SessionCard[];
  /** A line under a card that says where it belongs (the day's list links to the card). */
  where?: (card: SessionCard) => React.ReactNode;
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
            <SessionCardItem key={`${c.sessionId}-${c.finishedAt}`} card={c} where={where?.(c)} />
          ))}
        </ul>
      )}
    </section>
  );
}

function SessionCardItem({ card: c, where }: { card: SessionCard; where?: React.ReactNode }) {
  const g = GRADE[c.grade] ?? GRADE.partial;
  const Icon = g.icon;
  const folder = folderName(c.cwd);
  const hasMore = c.check !== "" || c.conditions.length > 0 || c.steps.length > 0;
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
      <p className="text-body text-pretty [overflow-wrap:anywhere]">{c.outcome}</p>
      {c.gradeNote && <p className="text-pretty text-muted-foreground [overflow-wrap:anywhere]">{c.gradeNote}</p>}
      {where && <p className="text-muted-foreground">{where}</p>}
      {hasMore && (
        <details className="group">
          <summary className="w-fit cursor-default rounded-sm text-muted-foreground select-none hover:text-foreground">Check and steps</summary>
          <div className="mt-2 flex flex-col gap-2">
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
