import { Monitor, Rows3, Send } from "lucide-react";
import type { Settings } from "@/api";
import { cn } from "@/lib/utils";

/**
 * The data-flow row (designer-bespoke): what is captured, where it goes, what comes back. The
 * middle step is the only one that leaves the PC; its line is dashed in the warning colour, and
 * the words say it, so the colour is never the only cue.
 */
export function DataFlow({ s }: { s: Settings }) {
  const kept = s.retentionDays === 1 ? "1 day" : `${s.retentionDays} days`;
  const steps = [
    {
      icon: Monitor,
      where: "On this PC",
      what: `A screenshot every ${s.captureIntervalSecs} seconds while you use the PC. Kept for ${kept}, then deleted.`,
      out: false,
    },
    {
      icon: Send,
      where: "Sent to the model you choose",
      what: "Every 15 minutes, a sample of that period’s screenshots, to write the cards. With Ollama or LM Studio they stay on this PC.",
      out: true,
    },
    {
      icon: Rows3,
      where: "Back on this PC",
      what: "Timeline cards: what you did, when, and in which app. Recaps, goals and chats are stored here too.",
      out: false,
    },
  ];
  return (
    <section aria-labelledby="flow-h" className="flex flex-col gap-3">
      <h2 id="flow-h" className="text-body font-semibold">
        Where your screenshots go
      </h2>
      <ol className="grid gap-x-0 gap-y-4 split:grid-cols-3">
        {steps.map((step, i) => (
          <li
            key={step.where}
            className={cn(
              "flex flex-col gap-1.5 border-t-2 pt-3 split:pr-5",
              step.out ? "border-dashed border-warn" : "border-foreground/70",
            )}
          >
            <step.icon className={cn("size-5", step.out ? "text-warn-foreground" : "text-muted-foreground")} aria-hidden />
            <p className="text-body font-semibold">
              <span className="sr-only">Step {i + 1}: </span>
              {step.where}
              {step.out && <span className="sr-only"> (leaves this PC)</span>}
            </p>
            <p className="text-meta text-muted-foreground">{step.what}</p>
          </li>
        ))}
      </ol>
    </section>
  );
}

/** A static example of a recorded day (invented): the overview strip and two cards. */
export function SampleDay() {
  const segs: [number, number, string][] = [
    [20, 3, "var(--cat-work)"],
    [23.6, 0.9, "var(--cat-distraction)"],
    [25, 3.4, "var(--cat-work)"],
    [35, 2.4, "var(--cat-personal)"],
    [40.5, 4.6, "var(--cat-work)"],
    [45.6, 1.1, "var(--cat-distraction)"],
    [66, 4.4, "var(--cat-work)"],
  ];
  const cards = [
    { time: "9:05 AM", title: "Fixing the export bug in the invoice app", meta: "Work · Visual Studio Code", color: "var(--cat-work)" },
    { time: "9:50 AM", title: "Watching keyboard reviews", meta: "Distraction · youtube.com", color: "var(--cat-distraction)" },
  ];
  return (
    <figure className="flex flex-col gap-2 rounded-lg border bg-background p-4">
      <div aria-hidden className="relative h-6 rounded-sm bg-surface-3">
        {segs.map(([left, width, c], i) => (
          <span key={i} className="absolute inset-y-0 rounded-[3px]" style={{ left: `${left}%`, width: `${width}%`, background: c }} />
        ))}
        <span data-mark className="hatch-idle absolute inset-y-0 rounded-[3px] bg-surface-1" style={{ left: "29%", width: "5%" }} />
      </div>
      <div aria-hidden className="grid gap-1.5 split:grid-cols-2">
        {cards.map((c) => (
          <div key={c.time} className="grid grid-cols-[3.75rem_0.25rem_minmax(0,1fr)] gap-2 rounded-md bg-surface-1 p-2 shadow-xs">
            <span className="pt-0.5 text-right font-mono text-caption text-muted-foreground">{c.time}</span>
            <span className="rounded-full" style={{ background: c.color }} />
            <span className="min-w-0">
              <span className="block truncate text-meta font-medium">{c.title}</span>
              <span className="block truncate text-caption text-muted-foreground">{c.meta}</span>
            </span>
          </div>
        ))}
      </div>
      <figcaption className="text-caption text-muted-foreground">
        An example day: the strip runs from 4 AM to 4 AM, with one card per stretch of work, distraction or idle time.
      </figcaption>
    </figure>
  );
}
