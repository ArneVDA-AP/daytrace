import type { Category } from "@/api";

/*
 * Category colors. Settings store one hex per category. The recommended defaults (designer-bespoke
 * palette, written by the backend for new installs) map back to theme-aware tokens, so they get the
 * right step in light and dark and the system color in Windows Contrast themes. Any other color
 * the user picked is used as given; every UI that shows a category color also shows its name.
 */

export interface Recommended {
  name: string;
  /** Hex stored in settings for new installs (the light step). */
  hex: string;
  /** Dark step, for display only. */
  darkHex: string;
  token: string;
}

export const RECOMMENDED: Recommended[] = [
  { name: "Work", hex: "#407BC5", darkHex: "#89B7FF", token: "var(--cat-work)" },
  { name: "Personal", hex: "#287C42", darkHex: "#6FC082", token: "var(--cat-personal)" },
  { name: "Distraction", hex: "#73176F", darkHex: "#B167AB", token: "var(--cat-distraction)" },
];

/** Idle default colors, old and new: idle is always drawn with the idle token (a hatch). */
const IDLE_DEFAULTS = new Set(["#A0AEC0", "#878D94", "#C0C4CB"]);

const TOKEN_BY_HEX = new Map<string, string>();
for (const r of RECOMMENDED) {
  TOKEN_BY_HEX.set(r.hex, r.token);
  TOKEN_BY_HEX.set(r.darkHex, r.token);
}

/** Color for "Processing failed" cards (category "System"): status, not a category. */
export const SYSTEM_COLOR = "var(--cat-system)";
/** Color for a category name that is not in the settings. */
export const UNKNOWN_COLOR = "var(--chart-5)";

export function isSystemCategory(name: string): boolean {
  return name.trim().toLowerCase() === "system";
}

/** CSS color for one category as stored in settings. */
export function categoryColor(c: Category): string {
  const hex = c.color.trim().toUpperCase();
  if (c.isIdle && (IDLE_DEFAULTS.has(hex) || hex === "")) return "var(--cat-idle)";
  return TOKEN_BY_HEX.get(hex) ?? c.color;
}

/**
 * Builds the `colorFor(name)` function every view receives: a CSS color for a card's category
 * name (case-insensitive), the status color for failed cards and a neutral for unknown names.
 */
export function makeColorFor(categories: Category[]): (name: string) => string {
  const map = new Map(categories.map((c) => [c.name.trim().toLowerCase(), categoryColor(c)]));
  return (name: string) => {
    if (isSystemCategory(name)) return SYSTEM_COLOR;
    return map.get(name.trim().toLowerCase()) ?? UNKNOWN_COLOR;
  };
}

/** The idle category of the settings, if any. */
export function idleCategory(categories: Category[]): Category | undefined {
  return categories.find((c) => c.isIdle);
}
