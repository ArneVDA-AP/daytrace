/*
 * Weekly charts as PNG images (Dayflow saves each Weekly chart as an image). The charts are HTML
 * rows around hand-drawn SVG, so the picture is rebuilt as one SVG: every <svg> of the chart is
 * copied in place with its colors resolved from the current theme, every visible text becomes an
 * SVG <text> at its spot, and small colored boxes (legend keys) become rectangles. That SVG is
 * drawn on a canvas from a data: URL (CSP img-src allows data:, nothing is fetched) and read back
 * as PNG. Fonts are the system fonts the app uses (Segoe UI, Cascadia Mono or Consolas).
 */

const SVG_NS = "http://www.w3.org/2000/svg";
/** Presentation properties copied from the computed style of every SVG element. */
const SVG_PROPS = ["fill", "fill-opacity", "stroke", "stroke-width", "stroke-opacity", "opacity", "font-family", "font-size", "font-weight", "text-anchor", "dominant-baseline"];

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** A CSS color (or var()) as the browser resolves it in the current theme. */
export function resolveColor(css: string, within: Element = document.body): string {
  const probe = document.createElement("span");
  probe.style.color = css;
  probe.style.display = "none";
  within.appendChild(probe);
  const c = getComputedStyle(probe).color;
  probe.remove();
  return c;
}

function hidden(el: Element): boolean {
  if (el.closest("[data-export-skip]")) return true;
  if ((el as HTMLElement).classList?.contains("sr-only")) return true;
  const cs = getComputedStyle(el);
  return cs.display === "none" || cs.visibility === "hidden" || el.getClientRects().length === 0;
}

function copySvg(src: SVGSVGElement, x: number, y: number): string {
  const clone = src.cloneNode(true) as SVGSVGElement;
  const a = [src, ...src.querySelectorAll("*")];
  const b = [clone, ...clone.querySelectorAll("*")];
  a.forEach((el, i) => {
    const out = b[i] as SVGElement;
    out.removeAttribute("class");
    out.removeAttribute("role");
    out.removeAttribute("aria-label");
    out.removeAttribute("aria-hidden");
    if (el.tagName.toLowerCase() === "title") return;
    const cs = getComputedStyle(el);
    const style = SVG_PROPS.map((p) => `${p}:${cs.getPropertyValue(p)}`).join(";");
    out.setAttribute("style", style);
  });
  // Tooltips are not part of the picture.
  clone.querySelectorAll("title").forEach((t) => t.remove());
  clone.setAttribute("x", String(x));
  clone.setAttribute("y", String(y));
  clone.setAttribute("overflow", "visible");
  return new XMLSerializer().serializeToString(clone);
}

function textOf(node: Text, origin: DOMRect): string {
  const text = (node.textContent ?? "").replace(/\s+/g, " ").trim();
  if (!text) return "";
  const parent = node.parentElement;
  if (!parent || hidden(parent)) return "";
  const range = document.createRange();
  range.selectNodeContents(node);
  const r = range.getBoundingClientRect();
  if (!r.width || !r.height) return "";
  const cs = getComputedStyle(parent);
  return `<text x="${(r.left - origin.left).toFixed(1)}" y="${(r.top - origin.top + r.height / 2).toFixed(1)}" dominant-baseline="central" style="fill:${cs.color};font-family:${esc(cs.fontFamily)};font-size:${cs.fontSize};font-weight:${cs.fontWeight}">${esc(text)}</text>`;
}

function boxOf(el: HTMLElement, origin: DOMRect): string {
  const r = el.getBoundingClientRect();
  if (!r.width || !r.height || r.width > 64 || r.height > 64) return "";
  const cs = getComputedStyle(el);
  const rx = parseFloat(cs.borderTopLeftRadius) || 0;
  const at = `x="${(r.left - origin.left).toFixed(1)}" y="${(r.top - origin.top).toFixed(1)}" width="${r.width.toFixed(1)}" height="${r.height.toFixed(1)}" rx="${Math.min(rx, r.height / 2)}"`;
  if (el.classList.contains("hatch-idle")) return `<rect ${at} fill="url(#xp-idle)"/>`;
  if (el.classList.contains("stripes-failed")) return `<rect ${at} fill="url(#xp-failed)"/>`;
  const bg = cs.backgroundColor;
  if (!bg || bg === "transparent" || /rgba\([^)]*,\s*0\)$/.test(bg)) return "";
  return `<rect ${at} style="fill:${bg}"/>`;
}

/** The chart in `el` as one SVG document, with a title line above it. */
export function chartSvg(el: HTMLElement, title: string, subtitle: string): { svg: string; width: number; height: number } {
  const origin = el.getBoundingClientRect();
  const pad = 24;
  const head = 52;
  const width = Math.ceil(origin.width) + pad * 2;
  const height = Math.ceil(origin.height) + pad * 2 + head;
  const bg = resolveColor("var(--surface-1)", el);
  const fg = resolveColor("var(--foreground)", el);
  const muted = resolveColor("var(--muted-foreground)", el);
  const idle = resolveColor("var(--cat-idle)", el);
  const failed = resolveColor("var(--destructive)", el);
  const sans = getComputedStyle(el).fontFamily;

  const parts: string[] = [];
  const walk = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      parts.push(textOf(node as Text, origin));
      return;
    }
    if (!(node instanceof Element) || hidden(node)) return;
    if (node instanceof SVGSVGElement) {
      const r = node.getBoundingClientRect();
      parts.push(copySvg(node, r.left - origin.left, r.top - origin.top));
      return;
    }
    if (node instanceof HTMLElement) parts.push(boxOf(node, origin));
    node.childNodes.forEach(walk);
  };
  el.childNodes.forEach(walk);

  const svg = [
    `<svg xmlns="${SVG_NS}" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`,
    `<defs><pattern id="xp-idle" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="5" height="5" style="fill:${bg}"/><rect width="1.5" height="5" style="fill:${idle}"/></pattern>`,
    `<pattern id="xp-failed" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="6" height="6" style="fill:${bg}"/><rect width="2" height="6" style="fill:${failed}"/></pattern></defs>`,
    `<rect width="${width}" height="${height}" style="fill:${bg}"/>`,
    `<text x="${pad}" y="${pad + 8}" dominant-baseline="central" style="fill:${fg};font-family:${esc(sans)};font-size:17px;font-weight:600">${esc(title)}</text>`,
    `<text x="${pad}" y="${pad + 30}" dominant-baseline="central" style="fill:${muted};font-family:${esc(sans)};font-size:13px">${esc(subtitle)}</text>`,
    `<g transform="translate(${pad} ${pad + head})">${parts.join("")}</g>`,
    `</svg>`,
  ].join("");
  return { svg, width, height };
}

/** The chart in `el` as a PNG, base64 without the data: prefix (2× for sharp text). */
export async function chartPng(el: HTMLElement, title: string, subtitle: string): Promise<string> {
  const { svg, width, height } = chartSvg(el, title, subtitle);
  const img = new Image();
  img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  await img.decode();
  const scale = 2;
  const canvas = document.createElement("canvas");
  canvas.width = width * scale;
  canvas.height = height * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("The image could not be drawn.");
  ctx.scale(scale, scale);
  ctx.drawImage(img, 0, 0, width, height);
  return canvas.toDataURL("image/png").replace(/^data:image\/png;base64,/, "");
}
