import * as React from "react";
import { ArrowDown, ArrowUp, Plus, RotateCcw, TriangleAlert, X } from "lucide-react";
import type { Category, CountsAs } from "@/api";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";
import { toast } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { categoryColor, isSystemCategory } from "@/lib/categories";
import { cn, errorText } from "@/lib/utils";
import { countsAsOf } from "@/views/insights/format";
import { colorName, hasNonRecommendedColors, sameHex, surfaceContrast, swatchFor, SWATCHES, unusedColor, withRecommendedColors } from "./category-colors";
import { FieldError, Group, Note } from "./fields";
import { useCommitField, useFieldSaves, useSave } from "./save";

const COUNTS_AS: { value: CountsAs; label: string }[] = [
  { value: "focus", label: "Focus" },
  { value: "distraction", label: "Distraction" },
  { value: "neither", label: "Neither" },
];

/**
 * Stable row keys for the category list. Names are unique but change on a rename, and the index
 * changes on a move or remove; a key by either would hand a row's unsaved typing to another
 * category. A rename made here gives the new name the row's key while it is saved, so the row
 * (and the focus in it) stays; names that are gone lose their key afterwards.
 */
function useRowKeys(names: string[]) {
  const ids = React.useRef(new Map<string, number>());
  const pending = React.useRef(new Set<string>());
  const next = React.useRef(0);
  const norm = (n: string) => n.trim().toLowerCase();
  const keyOf = (name: string) => {
    const k = norm(name);
    let id = ids.current.get(k);
    if (id === undefined) {
      id = next.current++;
      ids.current.set(k, id);
    }
    return id;
  };
  const renaming = (from: string, to: string) => {
    const id = ids.current.get(norm(from));
    if (id === undefined || norm(from) === norm(to)) return;
    ids.current.set(norm(to), id);
    pending.current.add(norm(to));
  };
  const renamed = (to: string) => pending.current.delete(norm(to));
  const present = names.map(norm).join("\n");
  React.useEffect(() => {
    const keep = new Set(present.split("\n"));
    for (const k of [...ids.current.keys()]) if (!keep.has(k) && !pending.current.has(k)) ids.current.delete(k);
  }, [present]);
  return { keyOf, renaming, renamed };
}

/** Settings > Categories: color, name, what it counts as, description for the model, order, add and remove. */
export function CategoriesSection() {
  const app = useAppState();
  const cats = app.settings.settings.categories;
  const save = useSave();
  const keys = useRowKeys(cats.map((c) => c.name));
  const [error, setError] = React.useState<string | null>(null);
  const listRef = React.useRef<HTMLOListElement>(null);
  const focusLater = (selector: string) => requestAnimationFrame(() => listRef.current?.querySelector<HTMLElement>(selector)?.focus());

  const run = async (what: string, patch: Parameters<typeof save>[0]) => {
    setError(null);
    try {
      await save(patch);
      return true;
    } catch (e) {
      setError(`Couldn’t ${what}: ${errorText(e)}`);
      return false;
    }
  };

  async function move(i: number, d: -1 | 1, button: "up" | "down") {
    const name = cats[i].name;
    const ok = await run("move the category", (cur) => {
      const list = [...cur.categories];
      const at = list.findIndex((c) => c.name === name);
      const to = at + d;
      if (at < 0 || to < 0 || to >= list.length || list[to].isIdle) return {};
      [list[at], list[to]] = [list[to], list[at]];
      return { categories: list };
    });
    if (!ok) return;
    // Focus follows the moved category; at the end of the list, the other arrow.
    const to = i + d;
    const last = cats.filter((c) => !c.isIdle).length - 1;
    const which = (button === "up" && to === 0) || (button === "down" && to === last) ? (button === "up" ? "down" : "up") : button;
    focusLater(`[data-row="${to}"] [data-move="${which}"]`);
  }

  async function remove(i: number) {
    const removed = cats[i];
    const ok = await run("remove the category", (cur) => ({ categories: cur.categories.filter((c) => c.name !== removed.name) }));
    if (!ok) return;
    focusLater(`[data-row="${Math.min(i, cats.length - 3)}"] [data-remove]`);
    toast(`Removed ${removed.name}`, {
      description: "Cards already in it keep the name.",
      action: {
        label: "Undo",
        onClick: () =>
          void run("restore the category", (cur) => {
            if (cur.categories.some((c) => c.name.toLowerCase() === removed.name.toLowerCase())) return {};
            const list = [...cur.categories];
            list.splice(Math.min(i, list.length), 0, removed);
            return { categories: list };
          }),
      },
    });
  }

  async function add() {
    const names = new Set(cats.map((c) => c.name.toLowerCase()));
    let name = "New category";
    for (let n = 2; names.has(name.toLowerCase()); n++) name = `New category ${n}`;
    const fresh: Category = { name, color: unusedColor(cats), description: "", isIdle: false, countsAs: "focus" };
    const ok = await run("add a category", (cur) => {
      const list = [...cur.categories];
      const idle = list.findIndex((c) => c.isIdle);
      list.splice(idle < 0 ? list.length : idle, 0, fresh);
      return { categories: list };
    });
    if (!ok) return;
    requestAnimationFrame(() => {
      const input = listRef.current?.querySelector<HTMLInputElement>(`[data-name="${CSS.escape(name)}"]`);
      input?.focus();
      input?.select();
    });
  }

  async function resetColors() {
    const before = cats.map((c) => ({ name: c.name, color: c.color }));
    const ok = await run("reset the colors", (cur) => ({ categories: withRecommendedColors(cur.categories) }));
    if (!ok) return;
    toast("Recommended colors restored", {
      description: "For Work, Personal, Distraction and Idle. Other categories keep their color.",
      action: {
        label: "Undo",
        onClick: () =>
          void run("undo the reset", (cur) => ({
            categories: cur.categories.map((c) => ({ ...c, color: before.find((b) => b.name === c.name)?.color ?? c.color })),
          })),
      },
    });
  }

  const movable = cats.filter((c) => !c.isIdle).length;

  return (
    <>
      <Group
        title="Categories"
        description="The model sorts every card into one of these, using the descriptions. Counts as decides whether a category’s time is focus, distraction or neither in Daily, Weekly, Goals and Your day so far. The colors are used on the timeline and in the charts; the order sets the order of the timeline filters and charts."
        actions={
          <Button variant="ghost" size="sm" onClick={() => void resetColors()} disabled={!hasNonRecommendedColors(cats)}>
            <RotateCcw aria-hidden /> Reset to recommended colors
          </Button>
        }
      >
        <ol ref={listRef} aria-label="Categories" className="flex flex-col divide-y">
          {cats.map((c, i) => (
            <CategoryRow
              key={keys.keyOf(c.name)}
              index={i}
              category={c}
              all={cats}
              first={i === 0}
              last={i === movable - 1}
              onMove={(d, b) => void move(i, d, b)}
              onRemove={() => void remove(i)}
              onRename={keys}
              canRemove={movable > 1}
            />
          ))}
        </ol>
        <div className="flex flex-wrap items-center gap-2 px-4 py-3">
          <Button variant="secondary" onClick={() => void add()}>
            <Plus aria-hidden /> Add category
          </Button>
          <FieldError>{error}</FieldError>
        </div>
      </Group>
      <Note>
        Names and descriptions apply to cards written from now on. Existing cards keep the category name they were written with; after a rename they
        show it without a color until they are written again. Counts as applies to every day at once, past days included.
      </Note>
    </>
  );
}

function CategoryRow({
  index,
  category: c,
  all,
  first,
  last,
  onMove,
  onRemove,
  onRename,
  canRemove,
}: {
  index: number;
  category: Category;
  all: Category[];
  first: boolean;
  last: boolean;
  onMove: (d: -1 | 1, button: "up" | "down") => void;
  onRemove: () => void;
  onRename: { renaming: (from: string, to: string) => void; renamed: (to: string) => void };
  canRemove: boolean;
}) {
  const save = useSave();
  const fields = useFieldSaves();
  const id = React.useId();
  const others = all.filter((_, j) => j !== index);
  const patchFor = (patch: Partial<Category>) => (cur: { categories: Category[] }) => ({
    categories: cur.categories.map((x) => (x.name === c.name && x.isIdle === c.isIdle ? { ...x, ...patch } : x)),
  });
  const update = (patch: Partial<Category>) => save(patchFor(patch));

  const name = useCommitField(
    c.name,
    async (v) => {
      // The row keeps its key (and focus) through the rename.
      const to = v.trim();
      onRename.renaming(c.name, to);
      try {
        // Pin what it counts as: a choice derived from the old name must not follow the new one.
        await update({ name: to, countsAs: countsAsOf(c) });
      } finally {
        onRename.renamed(to);
      }
    },
    {
    validate: (v) => {
      const t = v.trim();
      if (!t) return "A category needs a name.";
      if (others.some((o) => o.name.trim().toLowerCase() === t.toLowerCase())) return `There is already a category called ${t}.`;
      if (isSystemCategory(t)) return "System is used for cards that couldn’t be analyzed. Pick another name.";
      return null;
    },
  });
  // One paragraph for the model: Enter saves, line breaks become spaces.
  const description = useCommitField(c.description, (v) => update({ description: v.replace(/\s*\n\s*/g, " ") }));
  const counts = fields.shown("counts", countsAsOf(c));
  const error = name.error ?? description.error ?? fields.error("counts");

  if (c.isIdle) {
    return (
      <li data-row={index} className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-x-3 gap-y-2 px-4 py-3">
        <span aria-hidden className="mt-0.5 size-7 rounded-md bg-surface-1 ring-1 ring-border-strong hatch-idle" />
        <div className="min-w-0">
          <p className="flex min-h-8 flex-wrap items-center gap-x-3 text-body">
            <span className="text-muted-foreground">{c.name}</span>
            <span className="text-meta text-muted-foreground">Counts as neither</span>
          </p>
          <p className="mt-1 max-w-[68ch] text-meta text-muted-foreground">
            Used when you were away for most of a period. Idle time is not focus, distraction or active time. Drawn as a hatch, so it has no color; it
            can’t be renamed, moved or removed.
          </p>
        </div>
      </li>
    );
  }

  return (
    <li data-row={index} className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-x-3 gap-y-2 px-4 py-3">
      <ColorButton category={c} onPick={(hex) => update({ color: hex })} />
      <div className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-2">
        <div className="min-w-0 flex-1 basis-44">
          <label htmlFor={`${id}-name`} className="sr-only">
            Category name
          </label>
          <Input id={`${id}-name`} data-name={c.name} {...name.bind} maxLength={60} autoComplete="off" aria-describedby={`${id}-err`} />
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <label htmlFor={`${id}-counts`} className="text-meta whitespace-nowrap text-muted-foreground">
            Counts as
          </label>
          <NativeSelect
            id={`${id}-counts`}
            value={counts}
            aria-describedby={`${id}-err`}
            className="w-36"
            onChange={(e) => {
              const v = e.target.value as CountsAs;
              void fields.apply("counts", v, patchFor({ countsAs: v }));
            }}
          >
            {COUNTS_AS.map((o) => (
              <NativeSelectOption key={o.value} value={o.value}>
                {o.label}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </div>
      </div>
      <div className="flex items-center gap-0.5">
        <IconAction label={`Move ${c.name} up`} tip="Move up" data-move="up" disabled={first} onClick={() => onMove(-1, "up")}>
          <ArrowUp aria-hidden />
        </IconAction>
        <IconAction label={`Move ${c.name} down`} tip="Move down" data-move="down" disabled={last} onClick={() => onMove(1, "down")}>
          <ArrowDown aria-hidden />
        </IconAction>
        <IconAction label={`Remove ${c.name}`} tip="Remove" data-remove disabled={!canRemove} onClick={onRemove}>
          <X aria-hidden />
        </IconAction>
      </div>
      <div className="col-start-2 col-end-4 min-w-0">
        <label htmlFor={`${id}-desc`} className="sr-only">
          Description of {c.name} for the model
        </label>
        <Textarea
          id={`${id}-desc`}
          {...description.bind}
          rows={2}
          maxLength={500}
          placeholder="What belongs in it, for the model"
          autoComplete="off"
          aria-describedby={`${id}-err`}
          className="min-h-14 resize-none text-meta"
        />
      </div>
      <div id={`${id}-err`} className="col-start-2 col-end-4 empty:hidden">
        {error && <FieldError>{error}</FieldError>}
      </div>
    </li>
  );
}

function IconAction({ label, tip, children, ...props }: { label: string; tip: string } & React.ComponentProps<typeof Button>) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label={label} {...props}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{tip}</TooltipContent>
    </Tooltip>
  );
}

/** The color swatch of a row; opens the offered colors and a custom color picker. */
function ColorButton({ category: c, onPick }: { category: Category; onPick: (hex: string) => Promise<unknown> }) {
  const [open, setOpen] = React.useState(false);
  const [custom, setCustom] = React.useState(c.color);
  const [error, setError] = React.useState<string | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const id = React.useId();
  React.useEffect(() => setCustom(c.color), [c.color]);

  const pick = async (hex: string) => {
    setError(null);
    try {
      await onPick(hex);
    } catch (e) {
      setError(errorText(e));
    }
  };

  // The Windows color dialog fires "input" while you drag and "change" once you confirm.
  React.useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    const onChange = () => void pick(el.value.toUpperCase());
    el.addEventListener("change", onChange);
    return () => el.removeEventListener("change", onChange);
  });

  const contrast = surfaceContrast(custom);
  const weak = contrast && Math.min(contrast.light, contrast.dark) < 3 ? (contrast.light < contrast.dark ? `light mode (${contrast.light.toFixed(1)}:1)` : `dark mode (${contrast.dark.toFixed(1)}:1)`) : null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={`Color of ${c.name}: ${colorName(c.color)}`}
          data-mark
          className="mt-0.5 size-7 rounded-md ring-1 ring-border-strong ring-offset-2 ring-offset-surface-1"
          style={{ background: categoryColor(c) }}
        />
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-72"
        onOpenAutoFocus={(e) => {
          // Start on the color in use, not on the first one.
          const on = (e.currentTarget as HTMLElement | null)?.querySelector<HTMLElement>("[data-swatch=on]");
          if (on) {
            e.preventDefault();
            on.focus();
          }
        }}
      >
        <p className="text-meta font-medium" id={`${id}-t`}>
          Color for {c.name}
        </p>
        <div role="group" aria-labelledby={`${id}-t`} className="mt-2 grid grid-cols-7 gap-1.5">
          {SWATCHES.map((s) => {
            const on = sameHex(s.hex, c.color);
            return (
              <button
                key={s.hex}
                type="button"
                aria-pressed={on}
                aria-label={s.forCategory ? `${s.name}, recommended for ${s.forCategory}` : s.name}
                data-swatch={on ? "on" : undefined}
                data-mark
                onClick={() => void pick(s.hex)}
                className={cn("size-7 rounded-md", on && "ring-2 ring-foreground ring-offset-2 ring-offset-popover forced-colors:outline-2 forced-colors:outline-offset-2 forced-colors:outline-[Highlight]")}
                style={{ background: categoryColor({ name: "", color: s.hex, description: "", isIdle: false }) }}
              />
            );
          })}
        </div>
        <p className="mt-2 text-meta" aria-live="polite">
          {swatchFor(c.color) ? (swatchFor(c.color)!.forCategory ? `${swatchFor(c.color)!.name}, recommended for ${swatchFor(c.color)!.forCategory}` : swatchFor(c.color)!.name) : "A custom color"}
        </p>
        <p className="mt-2 text-caption text-muted-foreground">
          Blue, green and plum adjust to light and dark mode. The others keep at least 3:1 contrast in both.
        </p>
        <div className="mt-3 flex items-center gap-2 border-t pt-3">
          <input
            ref={inputRef}
            id={`${id}-custom`}
            type="color"
            value={/^#[0-9a-f]{6}$/i.test(custom) ? custom.toLowerCase() : "#000000"}
            onChange={(e) => setCustom(e.target.value.toUpperCase())}
            className="h-7 w-10 cursor-default rounded-sm border border-input bg-surface-1 p-0.5"
          />
          <label htmlFor={`${id}-custom`} className="text-meta">
            Custom color <span className="font-mono text-caption text-muted-foreground">{custom.toUpperCase()}</span>
          </label>
        </div>
        {weak && (
          <p className="mt-2 flex items-start gap-1.5 text-caption text-warn-foreground">
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            This color is hard to see in {weak}. Marks in it may blend into the background.
          </p>
        )}
        <FieldError>{error && `Couldn’t save the color: ${error}`}</FieldError>
      </PopoverContent>
    </Popover>
  );
}
