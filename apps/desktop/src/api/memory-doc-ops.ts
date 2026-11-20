import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import type { PreviewBlock, PreviewBlockKind, PreviewInline, PreviewMarks } from "./contract.ts";

/** Operations over the page document layout (ADR 0006) used by templates, history and synced blocks in the in-memory core. */

const isMap = (value: unknown): value is LoroMap => value instanceof LoroMap;
const isText = (value: unknown): value is LoroText => value instanceof LoroText;

export function childList(node: LoroMap): LoroList | undefined {
  const list = node.get("children");
  return list instanceof LoroList ? list : undefined;
}

export const rootChildren = (doc: LoroDoc): LoroList => {
  const list = childList(doc.getMap("doc"));
  if (!list) throw new Error("page document has no children list");
  return list;
};

const nameOf = (node: LoroMap): string => String(node.get("nodeName") ?? "");

export function attrsOf(node: LoroMap): Record<string, unknown> {
  const attrs = node.get("attributes");
  return isMap(attrs) ? (attrs.toJSON() as Record<string, unknown>) : {};
}

/** Insert an empty node of `name` with the given attributes. */
export function insertNode(list: LoroList, index: number, name: string, attrs: Record<string, unknown> = {}): LoroMap {
  const holder = new LoroMap();
  holder.set("nodeName", name);
  const node = list.insertContainer(index, holder);
  const attributes = node.setContainer("attributes", new LoroMap());
  for (const [key, value] of Object.entries(attrs)) attributes.set(key, value as string);
  node.setContainer("children", new LoroList());
  return node;
}

/** Deep copy of a node (and its text with marks) into `list` at `index`. */
export function copyNode(source: LoroMap, list: LoroList, index: number = list.length): LoroMap {
  const copy = insertNode(list, index, nameOf(source), attrsOf(source));
  const target = childList(copy);
  const from = childList(source);
  if (!target || !from) return copy;
  for (const child of from.toArray()) {
    if (isMap(child)) copyNode(child, target);
    else if (isText(child)) {
      const run = target.insertContainer(target.length, new LoroText());
      run.applyDelta(child.toDelta());
    }
  }
  return copy;
}

export function copyChildren(from: LoroList, to: LoroList, start = 0, end = from.length - 1): void {
  for (let at = start; at <= end; at += 1) {
    const child = from.get(at);
    if (isMap(child)) copyNode(child, to);
  }
}

/** Copy every block of `from` into `to`, in order, starting at index `at`. */
export function copyChildrenAt(from: LoroList, to: LoroList, at: number): void {
  let index = at;
  for (const child of from.toArray()) {
    if (!isMap(child)) continue;
    copyNode(child, to, index);
    index += 1;
  }
}

/** The node at `path` (child indexes from the root); an empty path is the root. */
export function nodeAtPath(doc: LoroDoc, path: number[]): LoroMap | undefined {
  let node: LoroMap = doc.getMap("doc");
  for (const index of path) {
    const next = childList(node)?.get(index);
    if (!isMap(next)) return undefined;
    node = next;
  }
  return node;
}

/** Sync ids of the synced-block placeholders in a page, in document order. */
export function syncedRefs(doc: LoroDoc): string[] {
  const refs: string[] = [];
  const visit = (node: LoroMap) => {
    if (nameOf(node) === "synced_block") {
      const id = attrsOf(node).syncId;
      if (typeof id === "string" && id) refs.push(id);
    }
    for (const child of childList(node)?.toArray() ?? []) if (isMap(child)) visit(child);
  };
  visit(doc.getMap("doc"));
  return refs;
}

export function containsSynced(list: LoroList, start: number, end: number): boolean {
  const check = (node: LoroMap): boolean =>
    nameOf(node) === "synced_block" || (childList(node)?.toArray() ?? []).some((child) => isMap(child) && check(child));
  for (let at = start; at <= end; at += 1) {
    const child = list.get(at);
    if (isMap(child) && check(child)) return true;
  }
  return false;
}

/** A new shared document laid out like a page, tagged with the root map `ne` so a device knows what it is. */
export function createSharedDoc(): LoroDoc {
  const doc = new LoroDoc();
  doc.setRecordTimestamp(true);
  const root = doc.getMap("doc");
  root.set("nodeName", "doc");
  root.setContainer("attributes", new LoroMap());
  root.setContainer("children", new LoroList());
  doc.getMap("ne").set("role", "synced");
  return doc;
}

export function ensureNotEmpty(doc: LoroDoc): void {
  const list = rootChildren(doc);
  if (list.length === 0) insertNode(list, 0, "paragraph");
}

const NO_MARKS: PreviewMarks = { bold: false, italic: false, code: false, strike: false, link: null };

function marksOf(attributes: Record<string, unknown> | undefined): PreviewMarks {
  const link = attributes?.link as { href?: string } | undefined;
  return {
    bold: Boolean(attributes?.bold),
    italic: Boolean(attributes?.italic),
    code: Boolean(attributes?.code),
    strike: Boolean(attributes?.strike),
    link: link?.href ?? NO_MARKS.link,
  };
}

function kindOf(name: string, attrs: Record<string, unknown>): PreviewBlockKind {
  const text = (key: string) => String(attrs[key] ?? "");
  switch (name) {
    case "paragraph":
    case "bullet_list":
    case "ordered_list":
    case "list_item":
    case "todo_list":
    case "quote":
    case "divider":
      return { type: name };
    case "heading":
      return { type: "heading", level: Number(attrs.level ?? 1) };
    case "todo_item":
      return { type: "todo_item", checked: Boolean(attrs.checked) };
    case "toggle":
      return { type: "toggle", open: Boolean(attrs.open) };
    case "callout":
      return { type: "callout", icon: text("icon") };
    case "code_block":
      return { type: "code_block", language: text("language") };
    case "image":
      return { type: "image", src: text("src"), alt: text("alt") };
    case "child_page":
    case "page_link":
      return { type: name, pageId: text("pageId"), title: text("title") };
    case "synced_block":
      return { type: "synced_block", syncId: text("syncId") };
    default:
      return { type: "unknown", nodeName: name };
  }
}

function previewNode(node: LoroMap): PreviewBlock {
  const inline: PreviewInline[] = [];
  const children: PreviewBlock[] = [];
  for (const child of childList(node)?.toArray() ?? []) {
    if (isText(child)) {
      for (const op of child.toDelta() as { insert?: string; attributes?: Record<string, unknown> }[]) {
        if (typeof op.insert === "string") inline.push({ type: "text", text: op.insert, marks: marksOf(op.attributes) });
      }
    } else if (isMap(child)) {
      if (nameOf(child) === "hard_break") inline.push({ type: "break" });
      else children.push(previewNode(child));
    }
  }
  return { kind: kindOf(nameOf(node), attrsOf(node)), inline, children };
}

/** The block tree of a page document, as a read-only snapshot. */
export function toPreview(doc: LoroDoc): PreviewBlock[] {
  const blocks: PreviewBlock[] = [];
  for (const child of rootChildren(doc).toArray()) if (isMap(child)) blocks.push(previewNode(child));
  return blocks;
}

/** Plain text of a page, for matching in tests. */
export function previewText(blocks: PreviewBlock[]): string {
  return blocks
    .map((block) => [block.inline.map((part) => (part.type === "text" ? part.text : "\n")).join(""), previewText(block.children)].filter(Boolean).join("\n"))
    .join("\n");
}
