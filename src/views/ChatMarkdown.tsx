import { Fragment, type ReactNode } from "react";

/**
 * A small Markdown renderer for chat answers. Everything becomes React elements and text nodes;
 * model output never reaches innerHTML. Supports headings, paragraphs, bullet and numbered lists
 * (nested by indentation), block quotes, fenced code, rules, pipe tables, and inline bold,
 * italic, strikethrough, code and links. `card:<id>` links become buttons via `onCard`.
 */
export interface MarkdownProps {
  text: string;
  /** Renders a `card:<id>` link; return null to show the label as plain text. */
  renderCard: (id: number, label: ReactNode, key: string) => ReactNode;
  onExternalLink: (url: string) => void;
}

const LIST_RE = /^(\s*)([-*+]|\d{1,3}[.)])\s+(.*)$/;
const FENCE_RE = /^\s*```/;
const HEADING_RE = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE_RE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const TABLE_SEP_RE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

type Ctx = Omit<MarkdownProps, "text">;

export function ChatMarkdown({ text, renderCard, onExternalLink }: MarkdownProps) {
  const ctx: Ctx = { renderCard, onExternalLink };
  return <div className="chat-md">{blocks(text.replace(/\r\n?/g, "\n").split("\n"), ctx, "b")}</div>;
}

function startsBlock(line: string, next: string | undefined): boolean {
  return (
    FENCE_RE.test(line) ||
    HEADING_RE.test(line) ||
    RULE_RE.test(line) ||
    LIST_RE.test(line) ||
    /^\s*>/.test(line) ||
    (line.includes("|") && next !== undefined && TABLE_SEP_RE.test(next))
  );
}

function indentOf(line: string): number {
  const m = /^(\s*)/.exec(line);
  return m ? m[1].replace(/\t/g, "    ").length : 0;
}

function blocks(lines: string[], ctx: Ctx, keyPrefix: string): ReactNode[] {
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
      const body: string[] = [];
      i++;
      while (i < lines.length && !FENCE_RE.test(lines[i])) body.push(lines[i++]);
      i++; // closing fence
      out.push(
        <pre key={key} className="chat-md-code">
          <code>{body.join("\n")}</code>
        </pre>,
      );
      continue;
    }
    const h = HEADING_RE.exec(line);
    if (h) {
      const level = Math.min(6, h[1].length + 2);
      const Tag = `h${level}` as "h3" | "h4" | "h5" | "h6";
      out.push(<Tag key={key}>{inline(h[2], ctx, key)}</Tag>);
      i++;
      continue;
    }
    if (RULE_RE.test(line)) {
      out.push(<hr key={key} />);
      i++;
      continue;
    }
    if (/^\s*>/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) body.push(lines[i++].replace(/^\s*>\s?/, ""));
      out.push(<blockquote key={key}>{blocks(body, ctx, key)}</blockquote>);
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && TABLE_SEP_RE.test(lines[i + 1])) {
      const header = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") rows.push(cells(lines[i++]));
      out.push(
        <div key={key} className="chat-md-table">
          <table>
            <thead>
              <tr>
                {header.map((c, j) => (
                  <th key={j}>{inline(c, ctx, `${key}-h${j}`)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, ri) => (
                <tr key={ri}>
                  {header.map((_, j) => (
                    <td key={j}>{inline(r[j] ?? "", ctx, `${key}-${ri}-${j}`)}</td>
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
            {inline(l, ctx, `${key}-${j}`)}
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
    <Tag key={key} start={startNum !== 1 ? startNum : undefined}>
      {items.map((it, j) => (
        <li key={j}>
          {it.text.map((t, k) => (
            <Fragment key={k}>
              {k > 0 && <br />}
              {inline(t, ctx, `${key}-${j}-${k}`)}
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

const INLINE: { re: RegExp; kind: "code" | "link" | "strong" | "em" | "del" }[] = [
  { re: /`([^`]+)`/, kind: "code" },
  { re: /\[([^\]\n]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/, kind: "link" },
  { re: /\*\*(.+?)\*\*|__(.+?)__/, kind: "strong" },
  { re: /~~(.+?)~~/, kind: "del" },
  { re: /\*([^*\s](?:[^*]*[^*\s])?)\*|(?<![\w])_([^_\s](?:[^_]*[^_\s])?)_(?![\w])/, kind: "em" },
];

function inline(text: string, ctx: Ctx, key: string): ReactNode[] {
  const out: ReactNode[] = [];
  let rest = text;
  let n = 0;
  while (rest) {
    let best: { m: RegExpExecArray; kind: (typeof INLINE)[number]["kind"] } | null = null;
    for (const p of INLINE) {
      const m = p.re.exec(rest);
      if (m && (!best || m.index < best.m.index)) best = { m, kind: p.kind };
    }
    if (!best) {
      out.push(rest);
      break;
    }
    const { m, kind } = best;
    if (m.index > 0) out.push(rest.slice(0, m.index));
    const k = `${key}-${n++}`;
    switch (kind) {
      case "code":
        out.push(<code key={k}>{m[1]}</code>);
        break;
      case "strong":
        out.push(<strong key={k}>{inline(m[1] ?? m[2], ctx, k)}</strong>);
        break;
      case "em":
        out.push(<em key={k}>{inline(m[1] ?? m[2], ctx, k)}</em>);
        break;
      case "del":
        out.push(<del key={k}>{inline(m[1], ctx, k)}</del>);
        break;
      case "link":
        out.push(link(m[1], m[2], ctx, k));
        break;
    }
    rest = rest.slice(m.index + m[0].length);
  }
  return out;
}

function link(label: string, target: string, ctx: Ctx, key: string): ReactNode {
  const children = inline(label, ctx, key);
  const card = /^card:\/{0,2}(\d+)$/.exec(target);
  if (card) {
    return ctx.renderCard(Number(card[1]), children, key) ?? <Fragment key={key}>{children}</Fragment>;
  }
  if (/^https?:\/\//i.test(target)) {
    return (
      <a
        key={key}
        href={target}
        onClick={(e) => {
          e.preventDefault();
          ctx.onExternalLink(target);
        }}
      >
        {children}
      </a>
    );
  }
  return <Fragment key={key}>{children}</Fragment>;
}
