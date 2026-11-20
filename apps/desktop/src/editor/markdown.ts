import { Fragment, Slice, type Mark, type Node as PmNode } from "prosemirror-model";
import { pageSchema } from "./schema.ts";

/**
 * A small CommonMark/GFM subset reader for pasted text: headings, lists
 * (nested, ordered, to-do), quotes, fenced code, dividers, images, and the
 * inline marks the schema has. Anything it does not recognise stays text.
 */

const { nodes, marks } = pageSchema;

const FENCE = /^```\s*([\w+-]*)\s*$/;
const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const LIST_ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const TODO = /^\[([ xX])\]\s+(.*)$/;
const IMAGE_LINE = /^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)\s*$/;

const INLINE_SOURCE =
  /`([^`]+)`|\*\*([^*]+)\*\*|__([^_]+)__|~~([^~]+)~~|(?<![*\w])\*([^*\s][^*]*)\*(?!\*)|(?<![_\w])_([^_\s][^_]*)_(?![_\w])|!?\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/;

/** Inline Markdown to text nodes carrying marks. */
export function parseInline(text: string, active: readonly Mark[] = []): PmNode[] {
  const out: PmNode[] = [];
  const push = (value: string, extra: readonly Mark[]) => {
    if (value) out.push(pageSchema.text(value, [...active, ...extra]));
  };
  let last = 0;
  // A fresh regex per call: nested calls must not share lastIndex with this loop.
  const pattern = new RegExp(INLINE_SOURCE.source, "g");
  for (let match = pattern.exec(text); match; match = pattern.exec(text)) {
    push(text.slice(last, match.index), []);
    const [, code, bold, bold2, strike, italic, italic2, label, href] = match;
    if (code !== undefined) push(code, [marks.code.create()]);
    else if (bold !== undefined || bold2 !== undefined) out.push(...parseInline(bold ?? bold2 ?? "", [...active, marks.bold.create()]));
    else if (strike !== undefined) out.push(...parseInline(strike, [...active, marks.strike.create()]));
    else if (italic !== undefined || italic2 !== undefined) out.push(...parseInline(italic ?? italic2 ?? "", [...active, marks.italic.create()]));
    else if (label !== undefined && href !== undefined) {
      // An inline image has no place in a text run; keep its description.
      if (match[0].startsWith("!")) push(label, []);
      else out.push(...parseInline(label, [...active, marks.link.create({ href })]));
    }
    last = match.index + match[0].length;
  }
  push(text.slice(last), []);
  return out;
}

const paragraph = (text: string): PmNode => nodes.paragraph.create(null, parseInline(text));

function indentOf(line: string): number {
  return line.length - line.trimStart().length;
}

function parseList(lines: string[], start: number): { node: PmNode; next: number } {
  const first = LIST_ITEM.exec(lines[start] ?? "") as RegExpExecArray;
  const base = (first[1] ?? "").length;
  const ordered = /\d/.test(first[2] ?? "");
  const todo = !ordered && TODO.test(first[3] ?? "");
  const items: PmNode[] = [];
  let i = start;

  while (i < lines.length) {
    const match = LIST_ITEM.exec(lines[i] ?? "");
    if (!match || (match[1] ?? "").length !== base) break;
    if (/\d/.test(match[2] ?? "") !== ordered) break;
    const check = !ordered ? TODO.exec(match[3] ?? "") : null;
    if (Boolean(check) !== todo) break;
    const body = check ? (check[2] ?? "") : (match[3] ?? "");
    i += 1;
    const nested: string[] = [];
    while (i < lines.length && (lines[i] ?? "").trim() !== "" && indentOf(lines[i] ?? "") > base) {
      nested.push(lines[i] ?? "");
      i += 1;
    }
    const children = [paragraph(body), ...parseBlocks(nested)];
    items.push(
      todo
        ? nodes.todo_item.create({ checked: check?.[1]?.toLowerCase() === "x" }, children)
        : nodes.list_item.create(null, children),
    );
  }
  const kind = todo ? "todo_list" : ordered ? "ordered_list" : "bullet_list";
  return { node: nodes[kind].create(null, items), next: i };
}

const startsBlock = (line: string) =>
  FENCE.test(line) || HEADING.test(line) || RULE.test(line) || LIST_ITEM.test(line) || line.startsWith(">") || IMAGE_LINE.test(line);

export function parseBlocks(lines: string[]): PmNode[] {
  const out: PmNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (line.trim() === "") {
      i += 1;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const code: string[] = [];
      for (i += 1; i < lines.length && !FENCE.test(lines[i] ?? "") ; i += 1) code.push(lines[i] ?? "");
      i += 1;
      const text = code.join("\n");
      out.push(nodes.code_block.create({ language: fence[1] ?? "" }, text ? pageSchema.text(text) : null));
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      out.push(nodes.heading.create({ level: Math.min((heading[1] ?? "#").length, 3) }, parseInline(heading[2] ?? "")));
      i += 1;
      continue;
    }
    if (RULE.test(line)) {
      out.push(nodes.divider.create());
      i += 1;
      continue;
    }
    if (line.startsWith(">")) {
      const inner: string[] = [];
      for (; i < lines.length && (lines[i] ?? "").startsWith(">"); i += 1) inner.push((lines[i] ?? "").replace(/^>\s?/, ""));
      const children = parseBlocks(inner);
      out.push(nodes.quote.create(null, children.length > 0 ? children : paragraph("")));
      continue;
    }
    if (LIST_ITEM.test(line)) {
      const list = parseList(lines, i);
      out.push(list.node);
      i = list.next;
      continue;
    }
    const image = IMAGE_LINE.exec(line);
    if (image) {
      out.push(nodes.image.create({ src: image[2] ?? "", alt: image[1] ?? "" }));
      i += 1;
      continue;
    }
    const text: string[] = [];
    for (; i < lines.length && (lines[i] ?? "").trim() !== "" && (text.length === 0 || !startsBlock(lines[i] ?? "")); i += 1) {
      text.push((lines[i] ?? "").trim());
    }
    out.push(paragraph(text.join(" ")));
  }
  return out;
}

export function markdownToNodes(markdown: string): PmNode[] {
  return parseBlocks(markdown.replace(/\r\n?/g, "\n").split("\n"));
}

const BLOCK_MARKERS = /^\s*(#{1,6}\s|[-*+]\s|\d+[.)]\s|>\s?|```|-{3,}\s*$|!\[[^\]]*\]\()/m;
const INLINE_MARKERS = /\*\*[^*\n]+\*\*|`[^`\n]+`|\[[^\]\n]+\]\([^)\n]+\)|~~[^~\n]+~~/;

/** Plain text worth converting: it has Markdown block or inline syntax. */
export function looksLikeMarkdown(text: string): boolean {
  return BLOCK_MARKERS.test(text) || INLINE_MARKERS.test(text);
}

/** A slice ready for `tr.replaceSelection`. One plain paragraph pastes inline. */
export function markdownToSlice(markdown: string): Slice {
  const blocks = markdownToNodes(markdown);
  const only = blocks.length === 1 ? blocks[0] : undefined;
  if (only?.type === nodes.paragraph) return new Slice(only.content, 0, 0);
  return new Slice(Fragment.fromArray(blocks), 0, 0);
}
