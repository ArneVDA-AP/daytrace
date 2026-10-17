import * as React from "react";
import { api, type Category } from "@/api";
import { useAppState } from "@/app/state";
import { cn, errorText } from "@/lib/utils";
import { withLiveRecordingState } from "@/views/platform-api";
import { keyOf, type Lane } from "./model";

/*
 * Category filter chips (Dayflow TabFilterBar): pressing a chip dims that category's cards on the
 * list and the trace (it does not hide them). The chips follow the order of the categories in
 * Settings and can be reordered here, which saves that order: drag a chip onto another, or press
 * Ctrl+← / Ctrl+→ on a focused chip. The idle category stays last.
 */

/** New settings order: `key` moved to the position of `targetKey` among the non-idle categories. */
function reorder(categories: Category[], key: string, targetKey: string): Category[] | null {
  const nonIdle = categories.filter((c) => !c.isIdle);
  const from = nonIdle.findIndex((c) => keyOf(c.name) === key);
  const to = nonIdle.findIndex((c) => keyOf(c.name) === targetKey);
  if (from < 0 || to < 0 || from === to) return null;
  const moved = [...nonIdle];
  const [item] = moved.splice(from, 1);
  moved.splice(to, 0, item);
  // Put the reordered non-idle categories back into the non-idle slots.
  let i = 0;
  return categories.map((c) => (c.isIdle ? c : moved[i++]));
}

export function CategoryChips({
  lanes,
  colorFor,
  muted,
  onToggle,
}: {
  lanes: Lane[];
  colorFor: (name: string) => string;
  muted: Set<string>;
  onToggle: (key: string) => void;
}) {
  const app = useAppState();
  const [order, setOrder] = React.useState<string[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [dragging, setDragging] = React.useState<string | null>(null);
  const refs = React.useRef(new Map<string, HTMLButtonElement>());
  const settingsKeys = new Set(app.categories.filter((c) => !c.isIdle).map((c) => keyOf(c.name)));

  // While a new order is being saved, show it at once.
  const shown = React.useMemo(() => {
    if (!order) return lanes;
    const pos = new Map(order.map((k, i) => [k, i]));
    const movable = lanes.filter((l) => pos.has(l.key)).sort((a, b) => pos.get(a.key)! - pos.get(b.key)!);
    let i = 0;
    return lanes.map((l) => (pos.has(l.key) ? movable[i++] : l));
  }, [lanes, order]);

  const save = async (key: string, targetKey: string) => {
    const next = reorder(app.categories, key, targetKey);
    if (!next) return;
    setError(null);
    setOrder(next.filter((c) => !c.isIdle).map((c) => keyOf(c.name)));
    try {
      // Start from the backend's copy so a pause made from the tray meanwhile is kept.
      const fresh = (await api.getSettings()).settings;
      const freshNext = reorder(fresh.categories, key, targetKey) ?? fresh.categories;
      const dto = await api.saveSettings(await withLiveRecordingState({ ...fresh, categories: freshNext }));
      app.setSettings(dto);
    } catch (e) {
      setError(`Couldn’t save the new order: ${errorText(e)}`);
    } finally {
      setOrder(null);
    }
  };

  const moveBy = (key: string, delta: number) => {
    const movable = shown.filter((l) => settingsKeys.has(l.key));
    const i = movable.findIndex((l) => l.key === key);
    const target = movable[i + delta];
    if (i < 0 || !target) return;
    void save(key, target.key).then(() => refs.current.get(key)?.focus());
  };

  return (
    <div className="min-w-0">
      <div role="group" aria-label="Categories shown on the timeline" className="flex flex-wrap items-center gap-1.5">
        {shown.map((l) => {
          const off = muted.has(l.key);
          const movable = settingsKeys.has(l.key);
          return (
            <button
              key={l.key}
              ref={(el) => {
                if (el) refs.current.set(l.key, el);
                else refs.current.delete(l.key);
              }}
              type="button"
              aria-pressed={!off}
              aria-keyshortcuts={movable ? "Control+ArrowLeft Control+ArrowRight" : undefined}
              title={`${off ? "Show" : "Dim"} ${l.name}${movable ? ". Drag, or Ctrl+← and Ctrl+→, to reorder" : ""}`}
              draggable={movable}
              onDragStart={(e) => {
                setDragging(l.key);
                e.dataTransfer.effectAllowed = "move";
                e.dataTransfer.setData("text/plain", l.key);
              }}
              onDragEnd={() => setDragging(null)}
              onDragOver={(e) => {
                if (dragging && movable && dragging !== l.key) {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                }
              }}
              onDrop={(e) => {
                e.preventDefault();
                const from = dragging ?? e.dataTransfer.getData("text/plain");
                setDragging(null);
                if (from && movable && from !== l.key) void save(from, l.key);
              }}
              onKeyDown={(e) => {
                if (!movable || !e.ctrlKey || e.altKey || e.metaKey) return;
                if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                  e.preventDefault();
                  moveBy(l.key, e.key === "ArrowLeft" ? -1 : 1);
                }
              }}
              onClick={() => onToggle(l.key)}
              className={cn(
                "inline-flex h-7 items-center gap-1.5 rounded-sm border px-2 text-meta transition-[background-color,border-color,opacity] duration-(--dur-fast)",
                off ? "border-border bg-transparent text-subtle-foreground hover:text-muted-foreground" : "border-border-strong bg-surface-1 text-foreground hover:bg-surface-2",
                dragging === l.key && "opacity-50",
              )}
            >
              <span
                aria-hidden="true"
                data-mark=""
                className={cn("size-2.5 shrink-0 rounded-full", l.isIdle && "hatch-idle border border-cat-idle bg-surface-1", off && "opacity-40")}
                style={l.isIdle ? undefined : { background: colorFor(l.name) }}
              />
              <span className={cn(off && "line-through")}>{l.name}</span>
            </button>
          );
        })}
      </div>
      {error && (
        <p role="alert" className="mt-1.5 text-caption text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
