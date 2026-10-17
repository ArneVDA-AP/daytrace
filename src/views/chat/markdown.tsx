import { Fragment, type ReactNode } from "react";
import { chartBody, chartFence, type ChartBlock } from "@/views/chat/chart-spec";

/*
 * A small Markdown renderer for chat answers. Everything becomes React elements and text nodes;
 * model output never reaches innerHTML. Supports headings, paragraphs, bullet and numbered lists
 * (nested by indentation), block quotes, fenced code, rules, pipe tables, and inline bold,
 * italic, strikethrough, code and links.
 *
 * Citations: a `[words](card:<id>)` link keeps its words as plain text and puts a card chip after
 * the sentence it belongs to (after the next ".", "!" or "?" that ends a sentence, or at the end
 * of the line, list item, table cell or heading).
 *
 * Charts: a fenced block opened with ```chart type=… (Dayflow's inline charts) goes to `chart`
 * with its JSON, which validates and draws it (chart.tsx); without `chart` it stays a code block.
 */

export interface MarkdownOptions {
  /** The chip for a cited card, or null to show only the link words (unknown card). */
  cardChip: (id: number, key: string) => ReactNode | null;
  onExternalLink: (url: string) => void;
  /** Draws a chart block (index counts the answer's chart blocks from 0). */
  chart?: (block: ChartBlock, key: string) => ReactNode;
  /**
   * Added to the number of `#` to get the heading level, which stays between h3 and h6 (the page
   * or dialog has its h1 and h2). Default 2: a chat answer's `#` is an h3. "What's new" passes 0,
   * so the changelog's `###` sections are h3 under the dialog's h2 title.
   */
  headingOffset?: number;
}

export interface RenderedMarkdown {
  nodes: ReactNode[];
  /** Card ids that got a chip. */
  cardIds: Set<number>;
}

// Every pattern here must stay linear on long lines (no two adjacent quantifiers that can match
// the same characters): answers are model output and can contain thousands of spaces or
// brackets. Trailing heading markers, table separators and sentence ends are handled with string
// operations. Lines are split on every line terminator, so `.` matches any character of a line.
const LIST_RE = /^(\s*)([-*+]|\d{1,3}[.)])\s+(.*)$/;
const FENCE_RE = /^\s*```/;
const HEADING_RE = /^\s{0,3}(#{1,6})\s+(.*)$/;
const RULE_RE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const TABLE_CELL_SEP_RE = /^:?-{2,}:?$/;
/** Quotes and inline markup nested deeper than this are shown as plain text. */
const MAX_DEPTH = 8;

interface Ctx extends MarkdownOptions {
  cardIds: Set<number>;
  charts: number;
}

/** One run of inline text (a line, list item, cell or heading) collects its chips here. */
interface Flow {
  queue: { id: number; node: ReactNode }[];
}

export function renderMarkdown(text: string, opts: MarkdownOptions): RenderedMarkdown {
  const ctx: Ctx = { ...opts, cardIds: new Set(), charts: 0 };
  const nodes = blocks(text.replace(/\r\n?|[\u2028\u2029]/g, "\n").split("\n"), ctx, "b");
  return { nodes, cardIds: ctx.cardIds };
}

/** Heading text without the optional closing run of `#` (which must follow a space) and trailing spaces. */
export function headingText(raw: string): string {
  const t = raw.trimEnd();
  let i = t.length;
  while (i > 0 && t[i - 1] === "#") i--;
  if (i < t.length && (i === 0 || t[i - 1] === " " || t[i - 1] === "\t")) return t.slice(0, i).trimEnd();
  return t;
}

/** A table's separator row, such as `| --- | :--: |`. */
export function isTableSep(line: string): boolean {
  const c = cells(line);
  return c.length > 0 && c.every((x) => TABLE_CELL_SEP_RE.test(x));
}

const CLOSERS = "\"')]”’»";

/** Index just after the first sentence end in `s` (punctuation, closing quotes, then a space or the end), or -1. */
export function sentenceEnd(s: string): number {
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch !== "." && ch !== "!" && ch !== "?") continue;
    let j = i + 1;
    while (j < s.length && CLOSERS.includes(s[j])) j++;
    if (j === s.length || s[j] === " " || s[j] === "\t" || s[j] === "\u00a0") return j;
    i = j - 1;
  }
  return -1;
}

/** True when `s` ends a sentence (ignoring trailing spaces and closing quotes). */
function endsSentence(s: string): boolean {
  let i = s.length - 1;
  while (i >= 0 && (s[i] === " " || s[i] === "\t")) i--;
  while (i >= 0 && CLOSERS.includes(s[i])) i--;
  return i >= 0 && (s[i] === "." || s[i] === "!" || s[i] === "?");
}

function startsBlock(line: string, next: string | undefined): boolean {
  return (
    FENCE_RE.test(line) ||
    HEADING_RE.test(line) ||
    RULE_RE.test(line) ||
    LIST_RE.test(line) ||
    /^\s*>/.test(line) ||
    (line.includes("|") && next !== undefined && isTableSep(next))
  );
}

function indentOf(line: string): number {
  const m = /^(\s*)/.exec(line);
  return m ? m[1].replace(/\t/g, "    ").length : 0;
}

const HEADING_CLASS: Record<number, string> = {
  3: "text-lead font-semibold",
  4: "text-body font-semibold",
  5: "text-body font-semibold",
  6: "text-body font-semibold text-muted-foreground",
};

function blocks(lines: string[], ctx: Ctx, keyPrefix: string, depth = 0): ReactNode[] {
  const out: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const key = `${keyPrefix}-${i}`;
    if (line.trim() === "") {
      i++;
      continue;
    }
    if (FENCE_RE.test(line)) {
      const fence = ctx.chart ? chartFence(line.trim().slice(3)) : null;
      if (fence && ctx.chart) {
        // Up to the closing fence (or a data line that ends with one); linear in the block.
        const { body, next } = chartBody(lines, i + 1, fence.inline);
        out.push(ctx.chart({ type: fence.type, body, raw: lines.slice(i, next).join("\n"), index: ctx.charts++ }, key));
        i = next;
        continue;
      }
      const body: string[] = [];
      i++;
      while (i < lines.length && !FENCE_RE.test(lines[i])) body.push(lines[i++]);
      i++; // closing fence
      out.push(
        <pre key={key} className="scroll-thin overflow-x-auto rounded-md border bg-surface-2 px-3 py-2 font-mono text-meta">
          <code>{body.join("\n")}</code>
        </pre>,
      );
      continue;
    }
    const h = HEADING_RE.exec(line);
    if (h) {
      const level = Math.max(3, Math.min(6, h[1].length + (ctx.headingOffset ?? 2)));
      const Tag = `h${level}` as "h3" | "h4" | "h5" | "h6";
      out.push(
        <Tag key={key} className={`mt-1 ${HEADING_CLASS[level]}`}>
          {flow(headingText(h[2]), ctx, key)}
        </Tag>,
      );
      i++;
      continue;
    }
    if (RULE_RE.test(line)) {
      out.push(<hr key={key} className="border-t" />);
      i++;
      continue;
    }
    if (/^\s*>/.test(line) && depth < MAX_DEPTH) {
      const body: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) body.push(lines[i++].replace(/^\s*>\s?/, ""));
      out.push(
        <blockquote key={key} className="flex flex-col gap-2 border-l-2 border-border-strong pl-3 text-muted-foreground">
          {blocks(body, ctx, key, depth + 1)}
        </blockquote>,
      );
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && isTableSep(lines[i + 1])) {
      const header = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") rows.push(cells(lines[i++]));
      out.push(
        <div key={key} className="scroll-thin max-w-full overflow-x-auto">
          <table className="border-collapse text-meta">
            <thead>
              <tr>
                {header.map((c, j) => (
                  <th key={j} scope="col" className="border-b border-border-strong px-3 py-1.5 text-left font-semibold first:pl-0">
                    {flow(c, ctx, `${key}-h${j}`)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, ri) => (
                <tr key={ri}>
                  {header.map((_, j) => (
                    <td key={j} className="tnum border-b px-3 py-1.5 align-top first:pl-0">
                      {flow(r[j] ?? "", ctx, `${key}-${ri}-${j}`)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }
    if (LIST_RE.test(line)) {
      const [node, next] = list(lines, i, ctx, key);
      out.push(node);
      i = next;
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && (para.length === 0 || !startsBlock(lines[i], lines[i + 1]))) {
      para.push(lines[i++].trim());
    }
    out.push(
      <p key={key}>
        {para.map((l, j) => (
          <Fragment key={j}>
            {j > 0 && <br />}
            {flow(l, ctx, `${key}-${j}`)}
          </Fragment>
        ))}
      </p>,
    );
  }
  return out;
}

/** A list starting at `start`; deeper-indented items become nested lists. */
function list(lines: string[], start: number, ctx: Ctx, key: string): [ReactNode, number] {
  const first = LIST_RE.exec(lines[start])!;
  const base = indentOf(lines[start]);
  const ordered = /\d/.test(first[2]);
  const items: { text: string[]; children: ReactNode[] }[] = [];
  let i = start;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trim() === "") {
      // A blank line ends the list unless the next line continues it.
      const next = lines[i + 1];
      if (next !== undefined && LIST_RE.test(next) && indentOf(next) >= base) {
        i++;
        continue;
      }
      break;
    }
    const m = LIST_RE.exec(line);
    const ind = indentOf(line);
    if (m && ind === base) {
      if (/\d/.test(m[2]) !== ordered) break; // a different kind of list starts here
      items.push({ text: [m[3]], children: [] });
      i++;
    } else if (m && ind > base && items.length) {
      const [node, next] = list(lines, i, ctx, `${key}-${i}`);
      items[items.length - 1].children.push(node);
      i = next;
    } else if (!m && ind > base && items.length) {
      items[items.length - 1].text.push(line.trim());
      i++;
    } else if (!m && items.length && !startsBlock(line, lines[i + 1]) && ind >= base) {
      // Lazy continuation of the item's paragraph.
      items[items.length - 1].text.push(line.trim());
      i++;
    } else {
      break;
    }
  }
  const Tag = ordered ? "ol" : "ul";
  const startNum = ordered ? parseInt(first[2], 10) : undefined;
  return [
    <Tag
      key={key}
      start={startNum !== 1 ? startNum : undefined}
      className={`flex flex-col gap-1 pl-5 marker:text-subtle-foreground [&_ol]:mt-1 [&_ul]:mt-1 ${ordered ? "list-decimal" : "list-disc"}`}
    >
      {items.map((it, j) => (
        <li key={j} className="pl-0.5">
          {it.text.map((t, k) => (
            <Fragment key={k}>
              {k > 0 && <br />}
              {flow(t, ctx, `${key}-${j}-${k}`)}
            </Fragment>
          ))}
          {it.children}
        </li>
      ))}
    </Tag>,
    i,
  ];
}

function cells(line: string): string[] {
  let t = line.trim();
  if (t.startsWith("|")) t = t.slice(1);
  if (t.endsWith("|")) t = t.slice(0, -1);
  return t.split("|").map((c) => c.trim());
}

// Link labels and targets stop at the next bracket, so a run of `[` or `[a](` cannot make every
// start position scan to the end of the line.
const INLINE: { re: RegExp; kind: "code" | "link" | "strong" | "em" | "del" }[] = [
  { re: /`([^`]+)`/g, kind: "code" },
  { re: /\[([^[\]\n]+)\]\(([^\s)[\]]+)(?:\s+"[^"]*")?\)/g, kind: "link" },
  { re: /\*\*(.+?)\*\*|__(.+?)__/g, kind: "strong" },
  { re: /~~(.+?)~~/g, kind: "del" },
  { re: /\*([^*\s](?:[^*]*[^*\s])?)\*|(?<![\w])_([^_\s](?:[^_]*[^_\s])?)_(?![\w])/g, kind: "em" },
];

/** Inline text of one line, cell or heading, with the chips of its citations after each sentence. */
function flow(text: string, ctx: Ctx, key: string): ReactNode[] {
  const f: Flow = { queue: [] };
  const out = inline(text, ctx, key, 0, f);
  flush(out, f);
  return out;
}

function flush(out: ReactNode[], f: Flow) {
  const seen = new Set<number>();
  for (const q of f.queue) {
    if (seen.has(q.id)) continue;
    seen.add(q.id);
    out.push(q.node);
  }
  f.queue = [];
}

function inline(text: string, ctx: Ctx, key: string, depth: number, f: Flow): ReactNode[] {
  if (depth >= MAX_DEPTH) return [text];
  const out: ReactNode[] = [];
  const root = depth === 0;
  // At the top level, chips waiting for the end of their sentence go in after it.
  const pushText = (s: string) => {
    if (!root || !f.queue.length) {
      out.push(s);
      return;
    }
    const j = sentenceEnd(s);
    if (j < 0) {
      out.push(s);
      return;
    }
    out.push(s.slice(0, j));
    flush(out, f);
    if (j < s.length) out.push(s.slice(j));
  };
  // Each pattern's next match is searched once and reused until the text before it is consumed,
  // so a pattern that does not occur again is not searched again for every other match.
  const next = INLINE.map((p) => ({ kind: p.kind, re: new RegExp(p.re.source, "g"), m: undefined as RegExpExecArray | null | undefined }));
  let pos = 0;
  let n = 0;
  while (pos < text.length) {
    let best: (typeof next)[number] | null = null;
    for (const p of next) {
      if (p.m === undefined || (p.m !== null && p.m.index < pos)) {
        p.re.lastIndex = pos;
        p.m = p.re.exec(text);
      }
      if (p.m && (!best || p.m.index < best.m!.index)) best = p;
    }
    if (!best || !best.m) {
      pushText(text.slice(pos));
      break;
    }
    const m = best.m;
    if (m.index > pos) pushText(text.slice(pos, m.index));
    const k = `${key}-${n++}`;
    let inner: string | null = null;
    switch (best.kind) {
      case "code":
        out.push(
          <code key={k} className="rounded-sm bg-surface-2 px-1 py-px font-mono text-meta">
            {m[1]}
          </code>,
        );
        break;
      case "strong":
        inner = m[1] ?? m[2];
        out.push(
          <strong key={k} className="font-semibold">
            {inline(inner, ctx, k, depth + 1, f)}
          </strong>,
        );
        break;
      case "em":
        inner = m[1] ?? m[2];
        out.push(<em key={k}>{inline(inner, ctx, k, depth + 1, f)}</em>);
        break;
      case "del":
        inner = m[1];
        out.push(<del key={k}>{inline(inner, ctx, k, depth + 1, f)}</del>);
        break;
      case "link":
        out.push(link(m[1], m[2], ctx, k, depth + 1, f));
        inner = m[1];
        break;
    }
    // "**Done.** Next": the sentence ended inside the markup.
    if (root && f.queue.length && inner !== null && endsSentence(inner)) flush(out, f);
    pos = m.index + m[0].length;
  }
  return out;
}

function link(label: string, target: string, ctx: Ctx, key: string, depth: number, f: Flow): ReactNode {
  const children = inline(label, ctx, key, depth, f);
  const card = /^card:\/{0,2}(\d+)$/.exec(target);
  if (card) {
    const id = Number(card[1]);
    const chip = ctx.cardChip(id, `${key}-chip`);
    if (chip) {
      ctx.cardIds.add(id);
      f.queue.push({ id, node: chip });
    }
    return <Fragment key={key}>{children}</Fragment>;
  }
  if (/^https?:\/\//i.test(target)) {
    return (
      <a
        key={key}
        href={target}
        title={target}
        className="underline decoration-border-strong underline-offset-2 hover:decoration-foreground"
        onClick={(e) => {
          e.preventDefault();
          ctx.onExternalLink(target);
        }}
        // A middle click would ask the web view to open the address itself, without the question.
        onAuxClick={(e) => e.preventDefault()}
      >
        {children}
      </a>
    );
  }
  return <Fragment key={key}>{children}</Fragment>;
}
