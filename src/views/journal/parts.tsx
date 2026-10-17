import * as React from "react";
import { Check, X } from "lucide-react";
import { cn } from "@/lib/utils";

/** A titled panel on the page (one h2), with an optional line under the title and actions. */
export function Panel({
  title,
  description,
  actions,
  children,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  const hid = React.useId();
  return (
    <section aria-labelledby={hid} className={cn("flex min-w-0 flex-col rounded-lg border bg-surface-1 shadow-xs", className)}>
      <div className="flex flex-wrap items-start gap-x-3 gap-y-1 px-4 pt-3.5 pb-3">
        <div className="min-w-0 flex-1 basis-48">
          <h2 id={hid} className="text-body font-semibold">
            {title}
          </h2>
          {description && <div className="mt-0.5 text-meta text-muted-foreground">{description}</div>}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
      <div className="min-w-0 flex-1 px-4 pb-4">{children}</div>
    </section>
  );
}

/** "Met" or "Missed", always with the word and an icon. */
export function ResultBadge({ met, children }: { met: boolean; children?: React.ReactNode }) {
  return (
    <span
      className={cn(
        "inline-flex h-5 shrink-0 items-center gap-1 rounded-sm px-1.5 text-caption font-medium",
        met ? "bg-surface-2 text-live-foreground" : "bg-destructive-surface text-destructive",
      )}
    >
      {met ? <Check className="size-3" aria-hidden="true" /> : <X className="size-3" aria-hidden="true" />}
      {children ?? (met ? "Met" : "Missed")}
    </span>
  );
}

/** A small color key for a legend; the name is always next to it. */
export function Swatch({ color, className }: { color: string; className?: string }) {
  return <span aria-hidden="true" data-mark="" className={cn("inline-block size-2 shrink-0 rounded-[2px]", className)} style={{ background: color }} />;
}

export function pct(part: number, whole: number): number {
  return whole > 0 ? Math.max(0, Math.min(100, (part / whole) * 100)) : 0;
}

// The goal words the backend looks for (features/journal.rs `keywords`), so the evidence line can
// name the words of the goal instead of their stems ("status", not "statu").
const STOPWORDS = new Set(
  (
    "the and for with from into onto that this these those then than about after before finish start work do doing done get make some more " +
    "less today tomorrow week day time hour hours minute minutes all any have has had will just also are was were been being our your their " +
    "them they you its new few lot lots least at on off out over under again very much many most own same such what which who whom how why " +
    "when where not but can could should would must need try via per each every"
  ).split(" "),
);

function stem(word: string): string {
  let w = word;
  for (let n = 0; n < 2; n++) {
    const before = w;
    for (const suf of ["ing", "ed", "es", "s", "e"]) {
      if (w.endsWith(suf) && w.length - suf.length >= 4) {
        w = w.slice(0, -suf.length);
        break;
      }
    }
    if (w === before) break;
  }
  return w;
}

/** The goal's own words whose stems are in `stems`, in goal order. */
export function goalWords(goal: string, stems: string[]): string[] {
  const want = new Set(stems);
  const out: string[] = [];
  for (const w of goal.toLowerCase().split(/[^\p{L}\p{N}]+/u)) {
    if ([...w].length < 3 || STOPWORDS.has(w)) continue;
    if (want.has(stem(w)) && !out.includes(w)) out.push(w);
  }
  return out.length ? out : stems;
}
