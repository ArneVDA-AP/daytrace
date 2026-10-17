import * as React from "react";
import { ArrowUp, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { navigate } from "@/lib/router";
import { cn } from "@/lib/utils";

/**
 * The question field. Enter sends, Shift+Enter starts a new line, Esc stops an answer that is
 * being written (here or in another window). Typing stays possible while an answer runs; only
 * sending waits. When chat cannot run (no model, no key, a CLI provider), the field is disabled
 * and the reason is shown in one line, with Open Settings unless the page already offers the fix
 * (a missing API key has its own banner under the page title).
 */
export function Composer({
  value,
  onChange,
  onSend,
  onStop,
  canStop,
  stopping,
  sending,
  problem,
  fixInSettings = true,
  inputRef,
}: {
  value: string;
  onChange: (v: string) => void;
  onSend: () => void;
  onStop: () => void;
  /** An answer is being written: the send button becomes Stop. */
  canStop: boolean;
  stopping: boolean;
  /** This window is answering (the send button would do nothing). */
  sending: boolean;
  problem: string | null;
  /** Offer Open Settings next to the problem (false when a page banner already has the fix). */
  fixInSettings?: boolean;
  inputRef: React.RefObject<HTMLTextAreaElement | null>;
}) {
  const disabled = problem !== null;
  const empty = !value.trim();
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (!canStop) onSend();
    } else if (e.key === "Escape" && canStop) {
      e.preventDefault();
      onStop();
    }
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (!canStop) onSend();
      }}
      className={cn(
        "flex flex-col gap-1 rounded-xl border border-border-strong bg-surface-1 p-2 shadow-xs transition-[border-color] duration-(--dur-fast) focus-within:border-input",
        "has-[textarea:focus-visible]:outline-2 has-[textarea:focus-visible]:outline-offset-2 has-[textarea:focus-visible]:outline-ring",
        disabled && "bg-surface-2",
      )}
    >
      <label htmlFor="chat-input" className="sr-only">
        Question about your timeline
      </label>
      <textarea
        id="chat-input"
        ref={inputRef}
        name="question"
        autoComplete="off"
        rows={2}
        value={value}
        disabled={disabled}
        aria-describedby="chat-input-hint"
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={disabled ? "Chat is not available" : "Ask about your timeline, for example what you did after lunch on Tuesday…"}
        className="field-sizing-content max-h-40 min-h-12 w-full resize-none bg-transparent px-2 py-1.5 text-body leading-6 text-foreground outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed"
      />
      <div className="flex min-h-7 flex-wrap items-center gap-x-3 gap-y-1 px-1">
        {problem ? (
          <p id="chat-input-hint" className="flex min-w-0 flex-1 basis-64 items-center gap-2 text-meta text-warn-foreground">
            {problem}
          </p>
        ) : (
          <p id="chat-input-hint" className="min-w-0 flex-1 text-caption text-subtle-foreground">
            {canStop ? (
              <>
                <Kbd>Esc</Kbd> stops the answer
              </>
            ) : (
              <>
                <Kbd>Enter</Kbd> sends, <Kbd>Shift</Kbd> <Kbd>Enter</Kbd> starts a new line
              </>
            )}
          </p>
        )}
        {problem ? (
          fixInSettings && (
            <Button type="button" size="sm" variant="outline" onClick={() => navigate("settings", { section: "provider" })}>
              Open Settings
            </Button>
          )
        ) : canStop ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button type="button" size="icon-sm" variant="outline" className="rounded-full" aria-label="Stop the answer" onClick={onStop} disabled={stopping}>
                {stopping ? (
                  <Spinner aria-hidden="true" role={undefined} aria-label={undefined} />
                ) : (
                  <Square aria-hidden="true" className="size-3 fill-current" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{sending ? "Stop the answer (Esc)" : "Stop the earlier answer (Esc)"}</TooltipContent>
          </Tooltip>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="submit"
                size="icon-sm"
                className="rounded-full aria-disabled:opacity-50"
                aria-label="Send question"
                aria-disabled={empty || undefined}
                onClick={(e) => empty && e.preventDefault()}
              >
                <ArrowUp aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Send (Enter)</TooltipContent>
          </Tooltip>
        )}
      </div>
    </form>
  );
}
