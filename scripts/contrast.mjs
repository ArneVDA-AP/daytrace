// WCAG 2.2 contrast of the token pairs the design relies on, light and dark, read from
// src/design/tokens.css (oklch values, var() references and alpha resolved; alpha is composited
// on the background of the pair). `npm run contrast`; exits 1 when a pair is under its minimum.
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/design/tokens.css", import.meta.url), "utf8");

function block(re) {
  const m = css.match(re);
  if (!m) throw new Error(`block not found: ${re}`);
  const out = {};
  for (const d of m[1].matchAll(/--([\w-]+):\s*([^;]+);/g)) out[d[1]] = d[2].replace(/\/\*.*?\*\//g, "").trim();
  return out;
}
const light = block(/:root\s*\{([\s\S]*?)\n\}/);
const dark = { ...light, ...block(/:root\[data-theme="dark"\]\s*\{([\s\S]*?)\n\}/) };

function oklchToRgb(L, C, h) {
  const a = C * Math.cos((h * Math.PI) / 180);
  const b = C * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const lin = [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
  return lin.map((x) => Math.min(1, Math.max(0, x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055)));
}

/** [r, g, b, alpha] in 0..1 for a token value. */
function parse(vars, value, depth = 0) {
  const v = value.trim();
  const ref = v.match(/^var\(--([\w-]+)\)$/);
  if (ref) return parse(vars, vars[ref[1]], depth + 1);
  const ok = v.match(/^oklch\(([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\)$/);
  if (ok) return [...oklchToRgb(+ok[1], +ok[2], +ok[3]), ok[4] === undefined ? 1 : +ok[4]];
  const hex = v.match(/^#([0-9a-f]{6})$/i);
  if (hex) return [0, 2, 4].map((i) => parseInt(hex[1].slice(i, i + 2), 16) / 255).concat(1);
  throw new Error(`cannot parse ${value}`);
}

const lum = (c) => {
  const f = (x) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4);
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
};
const over = (fg, bg) => [0, 1, 2].map((i) => fg[i] * fg[3] + bg[i] * (1 - fg[3]));
function ratio(vars, fg, bg) {
  const b = parse(vars, `var(--${bg})`);
  const f = over(parse(vars, `var(--${fg})`), b);
  const [x, y] = [lum(f), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

// [foreground, background, minimum, what]
const PAIRS = [
  ["foreground", "background", 4.5, "body text on the canvas (rail)"],
  ["foreground", "surface-1", 4.5, "body text on the content layer"],
  ["foreground", "surface-2", 4.5, "text on hover / selected"],
  ["foreground", "popover", 4.5, "text in menus and dialogs"],
  ["muted-foreground", "surface-1", 4.5, "secondary text"],
  ["muted-foreground", "background", 4.5, "secondary text on the canvas"],
  ["muted-foreground", "surface-2", 4.5, "secondary text on selected"],
  ["subtle-foreground", "surface-1", 4.5, "tertiary text"],
  ["subtle-foreground", "background", 4.5, "tertiary text on the canvas"],
  ["primary-foreground", "primary", 4.5, "primary button label"],
  ["destructive", "surface-1", 4.5, "error text"],
  ["foreground", "destructive-surface", 4.5, "text on an error banner"],
  ["foreground", "status-warn-surface", 4.5, "text on a warning banner"],
  ["destructive-foreground", "destructive", 4.5, "destructive button label"],
  ["status-live-foreground", "surface-1", 4.5, "'Recording' as text"],
  ["status-warn-foreground", "surface-1", 4.5, "warning text"],
  ["input", "surface-1", 3, "control outline (1.4.11)"],
  ["input", "background", 3, "control outline on the canvas"],
  ["ring", "surface-1", 3, "focus outline"],
  ["ring", "background", 3, "focus outline on the canvas"],
  ["status-live", "surface-1", 3, "recording dot"],
  ["cat-work", "surface-1", 3, "Work mark"],
  ["cat-personal", "surface-1", 3, "Personal mark"],
  ["cat-distraction", "surface-1", 3, "Distraction mark"],
  ["cat-work", "background", 3, "Work mark on the canvas"],
  ["cat-personal", "background", 3, "Personal mark on the canvas"],
  ["cat-distraction", "background", 3, "Distraction mark on the canvas"],
  ["cat-work", "surface-3", 3, "Work mark on a track"],
  ["cat-personal", "surface-3", 3, "Personal mark on a track"],
  ["cat-distraction", "surface-3", 3, "Distraction mark on a track"],
  ["cat-idle", "surface-1", 3, "idle hatch stroke"],
  ["cat-idle", "surface-3", 3, "idle hatch on a track (texture plus label: may be under 3)"],
];

let failed = 0;
for (const [name, vars] of [["light", light], ["dark", dark]]) {
  console.log(`\n${name}`);
  for (const [fg, bg, min, what] of PAIRS) {
    const r = ratio(vars, fg, bg);
    const soft = what.includes("may be under");
    const ok = r >= min;
    if (!ok && !soft) failed++;
    console.log(`${ok ? "  ok " : soft ? "  -- " : " FAIL"} ${r.toFixed(2).padStart(6)}:1  (min ${min})  ${fg} on ${bg}: ${what}`);
  }
}
if (failed) {
  console.error(`\n${failed} pair(s) under the minimum.`);
  process.exit(1);
}
