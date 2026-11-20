import { LoroDoc, LoroList, LoroMap, LoroText, type Container } from "loro-crdt";

/** Helpers over the page document layout from ADR 0006, used by the in-memory core. */

export const PAGE_HREF_PREFIX = "ne-page:";

function emptyNode(name: string): LoroMap {
  const node = new LoroMap();
  return node.set("nodeName", name), node;
}

function attach(parent: LoroList, name: string): LoroMap {
  const node = parent.insertContainer(parent.length, emptyNode(name));
  node.setContainer("attributes", new LoroMap());
  node.setContainer("children", new LoroList());
  return node;
}

/** A new page document with the root skeleton and one empty paragraph, as the core creates it. */
export function createSkeleton(): LoroDoc {
  const doc = new LoroDoc();
  doc.setRecordTimestamp(true);
  const root = doc.getMap("doc");
  root.set("nodeName", "doc");
  root.setContainer("attributes", new LoroMap());
  const children = root.setContainer("children", new LoroList());
  attach(children, "paragraph");
  doc.commit();
  return doc;
}

function childrenOf(node: LoroMap): LoroList | undefined {
  const list = node.get("children");
  return list instanceof LoroList ? list : undefined;
}

/** Append a paragraph with `text` to the end of the page. */
export function appendParagraph(doc: LoroDoc, text: string): void {
  const root = doc.getMap("doc");
  const list = childrenOf(root);
  if (!list) return;
  const paragraph = attach(list, "paragraph");
  if (text) {
    const run = childrenOf(paragraph)?.insertContainer(0, new LoroText());
    run?.insert(0, text);
  }
  doc.commit();
}

interface Visitor {
  node?(node: LoroMap, name: string): void;
  text?(text: LoroText, owner: LoroMap): void;
}

function isMap(value: unknown): value is LoroMap {
  return value instanceof LoroMap;
}
function isText(value: unknown): value is LoroText {
  return value instanceof LoroText;
}

function walk(node: LoroMap, visitor: Visitor): void {
  const name = String(node.get("nodeName") ?? "");
  visitor.node?.(node, name);
  const list = childrenOf(node);
  if (!list) return;
  for (const child of list.toArray() as (Container | unknown)[]) {
    if (isMap(child)) walk(child, visitor);
    else if (isText(child)) visitor.text?.(child, node);
  }
}

export function walkPage(doc: LoroDoc, visitor: Visitor): void {
  walk(doc.getMap("doc"), visitor);
}

/** Plain text of the page: blocks separated by newlines. */
export function plainText(doc: LoroDoc): string {
  const lines: string[] = [];
  walkPage(doc, {
    text(text, owner) {
      const name = String(owner.get("nodeName") ?? "");
      if (name === "hard_break") return;
      lines.push(text.toString());
    },
  });
  return lines.join("\n");
}

export interface PageRef {
  pageId: string;
  /** Text of the block that holds the reference, for a backlink snippet. */
  context: string;
}

/** Pages this page links to with `page_link` blocks or inline `ne-page:<id>` links. */
export function pageLinks(doc: LoroDoc): PageRef[] {
  const refs: PageRef[] = [];
  walkPage(doc, {
    node(node, name) {
      if (name !== "page_link") return;
      const attrs = node.get("attributes");
      const id = isMap(attrs) ? attrs.get("pageId") : undefined;
      if (typeof id === "string" && id) refs.push({ pageId: id, context: String(isMap(attrs) ? attrs.get("title") ?? "" : "") });
    },
    text(text) {
      const plain = text.toString();
      for (const op of text.toDelta() as { attributes?: Record<string, unknown> }[]) {
        const link = op.attributes?.link as { href?: string } | undefined;
        if (link?.href?.startsWith(PAGE_HREF_PREFIX)) {
          refs.push({ pageId: link.href.slice(PAGE_HREF_PREFIX.length), context: plain });
        }
      }
    },
  });
  return refs;
}
