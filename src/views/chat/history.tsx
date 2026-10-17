import * as React from "react";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { Conversation } from "@/views/chat-api";
import { conversationWhen, groupConversations } from "@/views/chat/lib";

/**
 * Saved chats in Dayflow's groups (Today, Yesterday, This week, Earlier). One tab stop: Up and Down
 * (Home, End) move between chats, Enter opens one, Delete asks to delete it. While a question is
 * being answered, chats cannot be switched or deleted (the answer belongs to the open chat).
 */
export function ConversationList({
  conversations,
  loading,
  activeId,
  locked,
  busyConversationId,
  onOpen,
  onDelete,
  idPrefix,
}: {
  conversations: Conversation[] | null;
  loading: boolean;
  activeId: number | null;
  /** This window is answering: no switching or deleting. */
  locked: boolean;
  busyConversationId: number | null;
  onOpen: (id: number) => void;
  onDelete: (c: Conversation) => void;
  /** Keeps ids unique when the list is shown twice (column and popover). */
  idPrefix: string;
}) {
  const groups = React.useMemo(() => groupConversations(conversations ?? []), [conversations]);
  const flat = React.useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const [focusId, setFocusId] = React.useState<number | null>(null);
  const listRef = React.useRef<HTMLDivElement>(null);
  const rovingId = flat.some((c) => c.id === focusId) ? focusId : flat.some((c) => c.id === activeId) ? activeId : (flat[0]?.id ?? null);

  const focusRow = (id: number) => {
    setFocusId(id);
    listRef.current?.querySelector<HTMLButtonElement>(`[data-conv="${id}"]`)?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent, c: Conversation) => {
    const i = flat.findIndex((x) => x.id === c.id);
    const go = (j: number) => {
      e.preventDefault();
      const t = flat[Math.max(0, Math.min(flat.length - 1, j))];
      if (t) focusRow(t.id);
    };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(flat.length - 1);
    else if (e.key === "Delete" && !locked && c.id !== busyConversationId) {
      e.preventDefault();
      onDelete(c);
    }
  };

  if (conversations === null) {
    return loading ? (
      <div className="flex flex-col gap-2 px-1 pt-2" aria-hidden="true">
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className="flex flex-col gap-1.5 px-2 py-1.5">
            <Skeleton className="h-3.5 w-4/5" />
            <Skeleton className="h-3 w-2/5" />
          </div>
        ))}
      </div>
    ) : null;
  }

  if (!flat.length) {
    return <p className="px-3 py-2 text-meta text-muted-foreground">No saved chats yet. Questions you ask are kept here.</p>;
  }

  return (
    <div ref={listRef} className="flex flex-col gap-3">
      {groups.map((g) => {
        const hid = `${idPrefix}-${g.title.replace(/\s/g, "-").toLowerCase()}`;
        return (
          <div key={g.title} role="group" aria-labelledby={hid}>
            <h3 id={hid} className="px-3 pb-1 text-caption font-medium text-muted-foreground">
              {g.title}
            </h3>
            <ul className="flex flex-col gap-0.5">
              {g.items.map((c) => {
                const active = c.id === activeId;
                const answering = c.id === busyConversationId;
                const tab = c.id === rovingId ? 0 : -1;
                const deleteBlocked = locked || answering;
                return (
                  <li key={c.id} className={cn("group/conv relative flex items-stretch rounded-md", active ? "bg-surface-2" : "hover:bg-surface-2")}>
                    {active && <span aria-hidden="true" className="absolute top-2 bottom-2 left-0 w-[3px] rounded-full bg-foreground" />}
                    <button
                      type="button"
                      data-conv={c.id}
                      tabIndex={tab}
                      aria-disabled={locked && !active ? true : undefined}
                      aria-current={active ? "true" : undefined}
                      onFocus={() => setFocusId(c.id)}
                      onKeyDown={(e) => onKeyDown(e, c)}
                      onClick={() => !(locked && !active) && onOpen(c.id)}
                      className="flex min-w-0 flex-1 flex-col items-start gap-0.5 rounded-md py-1.5 pr-1 pl-3 text-left focus-visible:-outline-offset-2 aria-disabled:opacity-60"
                    >
                      <span className={cn("w-full truncate text-meta", active && "font-semibold")}>{c.title}</span>
                      <span className="w-full truncate text-caption text-muted-foreground tnum">{answering ? "Being answered…" : conversationWhen(c)}</span>
                    </button>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          tabIndex={tab}
                          aria-label={`Delete chat: ${c.title}`}
                          aria-disabled={deleteBlocked || undefined}
                          onClick={() => !deleteBlocked && onDelete(c)}
                          onKeyDown={(e) => onKeyDown(e, c)}
                          className={cn(
                            "my-auto mr-1 text-muted-foreground opacity-0 group-focus-within/conv:opacity-100 group-hover/conv:opacity-100 hover:text-foreground focus-visible:opacity-100 focus-visible:-outline-offset-2",
                            active && "opacity-100",
                            deleteBlocked && "cursor-not-allowed hover:bg-transparent hover:text-muted-foreground",
                          )}
                        >
                          <Trash2 aria-hidden="true" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="right">
                        {answering ? "This chat is still being answered" : locked ? "Wait for the answer to finish" : "Delete chat"}
                      </TooltipContent>
                    </Tooltip>
                  </li>
                );
              })}
            </ul>
          </div>
        );
      })}
    </div>
  );
}
