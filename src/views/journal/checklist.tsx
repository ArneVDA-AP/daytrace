import * as React from "react";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import { CardChip } from "@/views/chat/card-chip";
import type { GoalItem } from "@/views/journal-api";
import { goalWords } from "@/views/journal/parts";

/** Backend limits (features/journal.rs MAX_ITEMS, MAX_ITEM_CHARS). */
export const MAX_ITEMS = 30;
const MAX_CHARS = 200;

/**
 * The day's to-do list. Each item shows the evidence check: the cards whose title, summary, steps
 * or sites mention the item's words, as chips that open the card on the Timeline.
 */
export function Checklist({
  day,
  today,
  items,
  categoryOf,
  colorFor,
  onToggle,
  onAdd,
  onRename,
  onDelete,
}: {
  day: string;
  today: string;
  items: GoalItem[];
  categoryOf: (cardId: number) => string | null;
  colorFor: (category: string) => string;
  onToggle: (item: GoalItem, done: boolean) => void;
  onAdd: (text: string) => Promise<boolean>;
  onRename: (item: GoalItem, text: string) => Promise<boolean>;
  onDelete: (item: GoalItem) => void;
}) {
  const [draft, setDraft] = React.useState("");
  const [adding, setAdding] = React.useState(false);
  const [editId, setEditId] = React.useState<number | null>(null);
  const addRef = React.useRef<HTMLInputElement>(null);
  const full = items.length >= MAX_ITEMS;

  const add = async (e: React.FormEvent) => {
    e.preventDefault();
    const t = draft.trim();
    if (!t || adding) return;
    setAdding(true);
    if (await onAdd(t)) setDraft("");
    setAdding(false);
    addRef.current?.focus();
  };

  return (
    <div className="flex flex-col gap-3">
      {items.length > 0 && (
        <ul className="flex flex-col">
          {items.map((item) => (
            <ItemRow
              key={item.id}
              item={item}
              day={day}
              today={today}
              editing={editId === item.id}
              onEdit={() => setEditId(item.id)}
              onEndEdit={() => setEditId(null)}
              categoryOf={categoryOf}
              colorFor={colorFor}
              onToggle={onToggle}
              onRename={onRename}
              onDelete={onDelete}
            />
          ))}
        </ul>
      )}
      <form className="flex gap-2" onSubmit={add}>
        <label htmlFor="goal-new" className="sr-only">
          New item
        </label>
        <Input
          ref={addRef}
          id="goal-new"
          name="goal"
          autoComplete="off"
          value={draft}
          maxLength={MAX_CHARS}
          disabled={full}
          onChange={(e) => setDraft(e.target.value)}
          placeholder={full ? `A day can have at most ${MAX_ITEMS} items` : day === today ? "Add something to get done today…" : "Add something to get done…"}
        />
        <Button type="submit" variant="secondary" disabled={full || adding || !draft.trim()}>
          <Plus aria-hidden="true" /> Add
        </Button>
      </form>
    </div>
  );
}

function ItemRow({
  item,
  day,
  today,
  editing,
  onEdit,
  onEndEdit,
  categoryOf,
  colorFor,
  onToggle,
  onRename,
  onDelete,
}: {
  item: GoalItem;
  day: string;
  today: string;
  editing: boolean;
  onEdit: () => void;
  onEndEdit: () => void;
  categoryOf: (cardId: number) => string | null;
  colorFor: (category: string) => string;
  onToggle: (item: GoalItem, done: boolean) => void;
  onRename: (item: GoalItem, text: string) => Promise<boolean>;
  onDelete: (item: GoalItem) => void;
}) {
  const [text, setText] = React.useState(item.text);
  const [saving, setSaving] = React.useState(false);
  const editRef = React.useRef<HTMLInputElement>(null);
  const editButton = React.useRef<HTMLButtonElement>(null);
  const cid = `goal-${item.id}`;

  React.useEffect(() => {
    if (editing) {
      setText(item.text);
      editRef.current?.focus();
      editRef.current?.select();
    }
    // Only when editing starts.
  }, [editing]);

  const end = () => {
    onEndEdit();
    requestAnimationFrame(() => editButton.current?.focus());
  };

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    const t = text.trim();
    if (!t) return;
    if (t === item.text) return end();
    setSaving(true);
    const ok = await onRename(item, t);
    setSaving(false);
    if (ok) end();
  };

  return (
    <li className="group/item flex flex-col gap-1.5 border-b py-2.5 first:pt-0.5 last:border-0 last:pb-0.5">
      <div className="flex min-h-7 items-center gap-3">
        <Checkbox id={cid} checked={item.done} onCheckedChange={(v) => onToggle(item, v === true)} aria-describedby={`${cid}-ev`} />
        {editing ? (
          <form className="flex min-w-0 flex-1 flex-wrap items-center gap-2" onSubmit={save}>
            <label htmlFor={`${cid}-edit`} className="sr-only">
              Item text
            </label>
            <Input
              ref={editRef}
              id={`${cid}-edit`}
              value={text}
              maxLength={MAX_CHARS}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  end();
                }
              }}
              className="h-7 min-w-40 flex-1"
            />
            <Button type="submit" size="sm" disabled={saving || !text.trim()}>
              {saving ? "Saving…" : "Save"}
            </Button>
            <Button type="button" size="sm" variant="ghost" onClick={end}>
              Cancel
            </Button>
          </form>
        ) : (
          <>
            <label
              htmlFor={cid}
              className={cn("min-w-0 flex-1 text-body [overflow-wrap:anywhere]", item.done && "text-muted-foreground line-through decoration-foreground/40")}
            >
              {item.text}
            </label>
            <div className="flex shrink-0 items-center opacity-0 transition-opacity duration-(--dur-fast) group-focus-within/item:opacity-100 group-hover/item:opacity-100">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button ref={editButton} variant="ghost" size="icon-sm" aria-label={`Edit “${item.text}”`} onClick={onEdit} className="text-muted-foreground">
                    <Pencil aria-hidden="true" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Edit</TooltipContent>
              </Tooltip>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Delete “${item.text}”`}
                    onClick={() => onDelete(item)}
                    className="text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 aria-hidden="true" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Delete</TooltipContent>
              </Tooltip>
            </div>
          </>
        )}
      </div>
      <Evidence id={`${cid}-ev`} item={item} day={day} today={today} categoryOf={categoryOf} colorFor={colorFor} />
    </li>
  );
}

function quoteList(words: string[]): string {
  const q = words.slice(0, 4).map((w) => `“${w}”`);
  if (words.length > 4) q.push("…");
  return q.join(", ");
}

/** The evidence check: which cards of the day mention the item, and for how long. */
function Evidence({
  id,
  item,
  day,
  today,
  categoryOf,
  colorFor,
}: {
  id: string;
  item: GoalItem;
  day: string;
  today: string;
  categoryOf: (cardId: number) => string | null;
  colorFor: (category: string) => string;
}) {
  const ev = item.evidence;
  if (ev.cardCount === 0) {
    return (
      <p id={id} className="pl-7 text-caption text-subtle-foreground">
        {day === today ? "Not on the timeline yet." : "Not found on the timeline of this day."}
      </p>
    );
  }
  const words = goalWords(item.text, [...new Set(ev.cards.flatMap((c) => c.matched))]);
  return (
    <div className="flex flex-col gap-1.5 pl-7">
      <p id={id} className="text-caption text-muted-foreground tnum">
        On the timeline: {ev.cardCount} {ev.cardCount === 1 ? "card" : "cards"}, {formatDuration(ev.totalSecs)}
        {words.length > 0 && <> · mentions {quoteList(words)}</>}
        {ev.cardCount > ev.cards.length && <> · the {ev.cards.length} closest:</>}
      </p>
      <ul className="flex flex-wrap gap-1.5" aria-label={`Cards for “${item.text}”`}>
        {ev.cards.map((c) => (
          <li key={c.cardId} className="max-w-full">
            <CardChip
              card={{ cardId: c.cardId, day, title: c.title, category: categoryOf(c.cardId), start: c.start, end: c.end, secs: c.secs }}
              today={today}
              colorFor={colorFor}
              showDay={false}
              className="max-w-[min(100%,24rem)]"
            />
          </li>
        ))}
      </ul>
    </div>
  );
}
