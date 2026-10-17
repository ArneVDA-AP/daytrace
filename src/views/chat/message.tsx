import * as React from "react";
import { Check, ChevronDown, Copy, RotateCcw, TriangleAlert, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Category } from "@/api";
import { cn } from "@/lib/utils";
import { providerName, type ChatMessage, type Citation } from "@/views/chat-api";
import { CardChip } from "@/views/chat/card-chip";
import { ChatChart } from "@/views/chat/chart";
import { dateTimeOf, formatTokens, whenOf } from "@/views/chat/lib";
import { renderMarkdown } from "@/views/chat/markdown";

export interface Step {
  label: string;
  summary?: string;
  ok?: boolean;
}

export function UserMessage({ text }: { text: string }) {
  return (
    <div className="flex justify-end" data-role="user">
      <p className="max-w-[80%] rounded-2xl rounded-br-md border border-transparent bg-surface-3 px-3.5 py-2 text-body whitespace-pre-wrap [overflow-wrap:anywhere]">
        <span className="sr-only">You asked: </span>
        {text}
      </p>
    </div>
  );
}

/** Tool steps: a check per finished step, a cross for a failed one, a spinner while one runs. */
export function StepList({ steps, running = false }: { steps: Step[]; running?: boolean }) {
  const [open, setOpen] = React.useState(false);
  const many = !running && steps.length > 3;
  const shown = many && !open ? steps.slice(0, 2) : steps;
  return (
    <ul className="flex flex-col gap-1 text-caption text-muted-foreground">
      {shown.map((s, i) => {
        const done = s.summary !== undefined;
        const failed = s.ok === false;
        return (
          <li key={i} className={cn("flex items-start gap-2", failed && "text-destructive")}>
            <span className="mt-px flex size-3.5 shrink-0 items-center justify-center">
              {!done ? (
                <Spinner className="size-3.5" aria-hidden="true" role={undefined} aria-label={undefined} />
              ) : failed ? (
                <X className="size-3.5" aria-hidden="true" />
              ) : (
                <Check className="size-3.5" aria-hidden="true" />
              )}
            </span>
            <span className="min-w-0 [overflow-wrap:anywhere]">
              {failed && <span className="sr-only">Failed: </span>}
              {!done ? `${s.label}…` : failed ? `${s.label}: ${s.summary}` : s.summary}
            </span>
          </li>
        );
      })}
      {many && (
        <li>
          <Button variant="ghost" size="xs" className="-ml-1.5 text-muted-foreground" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
            <ChevronDown aria-hidden="true" className={cn("transition-transform duration-(--dur-fast)", open && "rotate-180")} />
            {open ? "Show fewer steps" : `Show all ${steps.length} steps`}
          </Button>
        </li>
      )}
    </ul>
  );
}

function toolSteps(m: ChatMessage): Step[] {
  return m.tools.map((t) => ({ label: t.label, summary: t.summary, ok: t.ok }));
}

function metaLine(m: ChatMessage): string {
  const parts = [whenOf(m.createdAt), m.provider ? providerName(m.provider) : "", m.model ?? ""];
  if (m.inputTokens || m.outputTokens) parts.push(`${formatTokens(m.inputTokens ?? 0)} tokens in, ${formatTokens(m.outputTokens ?? 0)} out`);
  return parts.filter(Boolean).join(" · ");
}

export function AssistantMessage({
  m,
  today,
  colorFor,
  categories,
  onLink,
  onCopy,
  copied,
}: {
  m: ChatMessage;
  today: string;
  colorFor: (category: string) => string;
  /** For the colors of chart series named like a category. */
  categories: Category[];
  onLink: (url: string) => void;
  onCopy: () => void;
  copied: boolean;
}) {
  const byId = React.useMemo(() => new Map(m.citations.map((c) => [c.citedId, c])), [m.citations]);
  const chip = React.useCallback(
    (c: Citation, key?: string) => (
      <CardChip
        key={key}
        card={{ cardId: c.cardId, day: c.day, title: c.title, category: c.category, start: c.start, end: c.end }}
        today={today}
        colorFor={colorFor}
        className="mx-0.5 max-w-[min(100%,22rem)]"
      />
    ),
    [today, colorFor],
  );
  const chartCtx = React.useMemo(() => ({ colorFor, categories, subtitle: `Daytrace chat, ${dateTimeOf(m.createdAt)}` }), [colorFor, categories, m.createdAt]);
  const rendered = React.useMemo(
    () =>
      renderMarkdown(m.content, {
        cardChip: (id, key) => {
          const c = byId.get(id);
          return c ? chip(c, key) : null;
        },
        onExternalLink: onLink,
        chart: (block, key) => <ChatChart key={key} block={block} ctx={chartCtx} />,
      }),
    [m.content, byId, chip, onLink, chartCtx],
  );
  // Cited cards whose link did not render as a chip (for example inside code) still get one.
  const rest = m.citations.filter((c) => !rendered.cardIds.has(c.citedId));
  const steps = toolSteps(m);
  return (
    <div className="flex flex-col gap-3">
      <span className="sr-only">Answer:</span>
      {steps.length > 0 && <StepList steps={steps} />}
      <div className="flex flex-col gap-3 text-body leading-6 [overflow-wrap:anywhere]">{rendered.nodes}</div>
      {rest.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <p className="text-caption text-muted-foreground">Also cited</p>
          <ul className="flex flex-wrap gap-1.5">
            {rest.map((c) => (
              <li key={c.citedId} className="max-w-full">
                {chip(c)}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-caption text-subtle-foreground">
        <span className="tnum">{metaLine(m)}</span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-xs" aria-label={copied ? "Copied" : "Copy answer"} onClick={onCopy} className="text-muted-foreground">
              {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{copied ? "Copied" : "Copy answer"}</TooltipContent>
        </Tooltip>
      </div>
    </div>
  );
}

/** A stored failure (the provider refused, a timeout, Stop): what happened, details, Try again. */
export function ErrorMessage({ m, onRetry, retryDisabled }: { m: ChatMessage; onRetry?: () => void; retryDisabled?: boolean }) {
  const steps = toolSteps(m);
  const stopped = m.content.startsWith("Stopped");
  return (
    <div className="flex flex-col gap-3">
      {steps.length > 0 && <StepList steps={steps} />}
      <div
        className={cn(
          "flex flex-wrap items-start gap-x-3 gap-y-2 rounded-lg border p-3.5",
          stopped ? "border-border-strong bg-surface-2" : "border-destructive/40 bg-destructive-surface",
        )}
      >
        <TriangleAlert className={cn("mt-0.5 size-4 shrink-0", stopped ? "text-muted-foreground" : "text-destructive")} aria-hidden="true" />
        <div className="flex min-w-0 flex-1 basis-64 flex-col gap-1.5">
          <p className="text-meta">
            <span className="sr-only">No answer: </span>
            {m.content}
          </p>
          {m.detail && (
            <details className="group/details text-caption text-muted-foreground">
              <summary className="w-fit cursor-default rounded-sm select-none hover:text-foreground">Error details</summary>
              <pre className="scroll-thin mt-1.5 max-h-40 overflow-auto rounded-md border bg-surface-1 p-2 font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere]">
                {m.detail}
              </pre>
            </details>
          )}
          {(m.provider || m.model) && <p className="text-caption text-muted-foreground">{metaLine(m)}</p>}
        </div>
        {onRetry && (
          <Button size="sm" variant="outline" onClick={onRetry} disabled={retryDisabled} className="self-center">
            <RotateCcw aria-hidden="true" /> Try again
          </Button>
        )}
      </div>
    </div>
  );
}
