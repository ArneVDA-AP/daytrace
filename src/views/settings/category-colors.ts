import type { Category } from "@/api";
import { RECOMMENDED } from "@/lib/categories";

/*
 * Colors offered for categories. The three recommended ones are the backend's
 * default_categories (types.rs) and map to theme-aware tokens (lib/categories.ts). The extra four
 * are stored as given in both themes, so they were picked at oklch lightness ~0.6, where they keep
 * at least 3:1 against every surface of both themes (light: 3.13–3.95, dark: 3.39–4.86, measured
 * with the contrast formula of scripts/contrast.mjs), and away from the hues of the three defaults
 * and of the failure red. These are stored data values, not interface colors.
 */

export interface Swatch {
  name: string;
  hex: string;
  /** Category this is the recommended color of. */
  forCategory?: string;
}

const RECOMMENDED_NAMES: Record<string, string> = { Work: "Blue", Personal: "Green", Distraction: "Plum" };

export const SWATCHES: Swatch[] = [
  ...RECOMMENDED.map((r) => ({ name: RECOMMENDED_NAMES[r.name] ?? r.name, hex: r.hex, forCategory: r.name })),
  { name: "Amber", hex: "#BE6B0B" },
  { name: "Teal", hex: "#048F90" },
  { name: "Violet", hex: "#876CCA" },
  { name: "Olive", hex: "#8A8329" },
];

/** Idle's stored default (types.rs); Idle is always drawn as a hatch. */
export const IDLE_DEFAULT_HEX = "#A0AEC0";

export function sameHex(a: string, b: string): boolean {
  return a.trim().toUpperCase() === b.trim().toUpperCase();
}

export function swatchFor(hex: string): Swatch | undefined {
  return SWATCHES.find((s) => sameHex(s.hex, hex));
}

/** Words for a stored color: "Blue", or "custom color #12AB34". */
export function colorName(hex: string): string {
  return swatchFor(hex)?.name ?? `custom color ${hex.toUpperCase()}`;
}

/** The recommended color of a category, by name (Work, Personal, Distraction; Idle). */
export function recommendedHex(c: Category): string | null {
  if (c.isIdle) return IDLE_DEFAULT_HEX;
  return RECOMMENDED.find((r) => r.name.toLowerCase() === c.name.trim().toLowerCase())?.hex ?? null;
}

/** Categories with the recommended color put back on the default ones; others keep theirs. */
export function withRecommendedColors(categories: Category[]): Category[] {
  return categories.map((c) => {
    const hex = recommendedHex(c);
    return hex ? { ...c, color: hex } : c;
  });
}

export function hasNonRecommendedColors(categories: Category[]): boolean {
  return categories.some((c) => {
    const hex = recommendedHex(c);
    return hex !== null && !sameHex(hex, c.color);
  });
}

/** A color for a new category: the first offered one no category uses yet. */
export function unusedColor(categories: Category[]): string {
  const used = new Set(categories.map((c) => c.color.trim().toUpperCase()));
  return (SWATCHES.find((s) => !s.forCategory && !used.has(s.hex)) ?? SWATCHES.find((s) => !used.has(s.hex)) ?? SWATCHES[3]).hex;
}

// ---- contrast of a custom color against the content layer of each theme ----

function luminance(hex: string): number | null {
  const m = hex.trim().match(/^#?([0-9a-f]{6})$/i);
  if (!m) return null;
  const f = (x: number) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4);
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16) / 255);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

/** Relative luminance of --surface-1 in light (white) and dark (oklch 0.21 0.007 260) mode. */
const SURFACE_LUMINANCE = { light: 1, dark: 0.0091 };

/** Contrast of a color against the content layer in light and dark mode. */
export function surfaceContrast(hex: string): { light: number; dark: number } | null {
  const l = luminance(hex);
  if (l === null) return null;
  const ratio = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
  return { light: ratio(l, SURFACE_LUMINANCE.light), dark: ratio(l, SURFACE_LUMINANCE.dark) };
}
