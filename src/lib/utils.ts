import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// Daytrace adds named type sizes (text-caption, text-meta, ...). Tell tailwind-merge they are font
// sizes; otherwise it treats them as colours and drops text-muted-foreground next to them.
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      "font-size": [{ text: ["axis", "caption", "meta", "body", "lead", "title", "display"] }],
    },
  },
});

/** Joins class names and resolves Tailwind conflicts (the last one wins). */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Error text from a rejected invoke (Tauri rejects with a string). */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Something went wrong.";
}

/** True when a key press belongs to a text field, a slider or an open dialog/menu/grid. */
export function isTypingTarget(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  return !!el.closest(
    'input, textarea, select, [contenteditable=""], [contenteditable="true"], [role="slider"], [role="dialog"], [role="menu"], [role="listbox"], [role="grid"]',
  );
}
