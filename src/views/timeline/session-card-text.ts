/* Words for a Claude Code session card (SessionCards.tsx). No imports, so node can test it. */

/** The last part of a folder path, for a short label. */
export function folderName(cwd: string): string {
  const parts = cwd.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? "";
}

/**
 * How a condition of the check ended. A "must" condition is good when it showed; a "fails"
 * condition names something that must not happen, so "met" there means it did not happen.
 */
export function conditionText(kind: string, state: string): string {
  if (state !== "met" && state !== "failed") return "Not checked";
  if (kind === "fails") return state === "met" ? "Did not happen" : "Happened";
  return state === "met" ? "Shown" : "Not shown";
}

/**
 * An SVG text as the address of an image. The drawing of a session card is shown this way, as an
 * <img>: a browser runs no script and loads no outside file for an SVG that is an image.
 */
export function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** The words that stand in for the drawing of a card: its grade, its outcome and the grade note. */
export function drawingAlt(grade: string, outcome: string, gradeNote: string): string {
  return `Session card, ${grade.toLowerCase()}: ${[outcome, gradeNote].filter(Boolean).join(" ")}`;
}

/** "2 session cards": the count for a row or a summary line. */
export function sessionCardCount(n: number): string {
  return `${n} session ${n === 1 ? "card" : "cards"}`;
}
