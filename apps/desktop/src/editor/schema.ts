import { Schema, type MarkSpec, type NodeSpec } from "prosemirror-model";
import { parseAttachmentSrc, parsePageHref } from "./page-links.ts";

/**
 * Page schema: the 13 block kinds of the v1 spec (15 counting heading levels
 * 1-3 separately), the synced-block placeholder, and the inline marks bold,
 * italic, code, strike and link.
 *
 * Node names are persisted in the Loro document (`nodeName`), so renaming one
 * is a data migration. Keep this file free of DOM and browser imports: the
 * headless spikes load it under plain Node.
 */

const textBlock = { group: "block", content: "inline*" } as const;

const nodes: Record<string, NodeSpec> = {
  doc: { content: "block+" },

  paragraph: { ...textBlock, toDOM: () => ["p", 0], parseDOM: [{ tag: "p" }] },

  heading: {
    ...textBlock,
    attrs: { level: { default: 1 } },
    defining: true,
    toDOM: (node) => [`h${node.attrs.level as number}`, 0],
    parseDOM: [1, 2, 3].map((level) => ({ tag: `h${level}`, attrs: { level } })),
  },

  bullet_list: {
    group: "block",
    content: "list_item+",
    toDOM: () => ["ul", 0],
    parseDOM: [{ tag: "ul" }],
  },

  ordered_list: {
    group: "block",
    content: "list_item+",
    toDOM: () => ["ol", 0],
    parseDOM: [{ tag: "ol" }],
  },

  list_item: {
    content: "paragraph block*",
    defining: true,
    toDOM: () => ["li", 0],
    parseDOM: [{ tag: "li" }],
  },

  todo_list: {
    group: "block",
    content: "todo_item+",
    toDOM: () => ["ul", { class: "ne-todo-list" }, 0],
    parseDOM: [{ tag: "ul.ne-todo-list" }],
  },

  todo_item: {
    content: "paragraph block*",
    attrs: { checked: { default: false } },
    defining: true,
    toDOM: (node) => [
      "li",
      { class: "ne-todo-item", "data-checked": node.attrs.checked ? "true" : "false" },
      0,
    ],
    parseDOM: [
      {
        tag: "li.ne-todo-item",
        getAttrs: (dom) => ({ checked: (dom as HTMLElement).dataset.checked === "true" }),
      },
    ],
  },

  // The first child paragraph is the always-visible summary; the rest is
  // revealed when `open` is true.
  toggle: {
    group: "block",
    content: "paragraph block*",
    attrs: { open: { default: true } },
    defining: true,
    toDOM: (node) => ["div", { class: "ne-toggle", "data-open": node.attrs.open ? "true" : "false" }, 0],
    parseDOM: [{ tag: "div.ne-toggle" }],
  },

  quote: {
    group: "block",
    content: "block+",
    defining: true,
    toDOM: () => ["blockquote", 0],
    parseDOM: [{ tag: "blockquote" }],
  },

  callout: {
    group: "block",
    content: "block+",
    attrs: { icon: { default: "" } },
    defining: true,
    toDOM: (node) => ["div", { class: "ne-callout", "data-icon": node.attrs.icon as string }, 0],
    parseDOM: [
      {
        tag: "div.ne-callout",
        getAttrs: (dom) => ({ icon: (dom as HTMLElement).dataset.icon ?? "" }),
      },
    ],
  },

  divider: {
    group: "block",
    atom: true,
    selectable: true,
    toDOM: () => ["hr"],
    parseDOM: [{ tag: "hr" }],
  },

  code_block: {
    group: "block",
    content: "text*",
    marks: "",
    code: true,
    defining: true,
    attrs: { language: { default: "" } },
    toDOM: () => ["pre", ["code", 0]],
    parseDOM: [{ tag: "pre", preserveWhitespace: "full" }],
  },

  image: {
    group: "block",
    atom: true,
    selectable: true,
    attrs: { src: { default: "" }, alt: { default: "" } },
    toDOM: (node) => ["img", { src: node.attrs.src as string, alt: node.attrs.alt as string }],
    parseDOM: [
      {
        tag: "img[src]",
        getAttrs: (dom) => ({
          src: (dom as HTMLElement).getAttribute("src") ?? "",
          alt: (dom as HTMLElement).getAttribute("alt") ?? "",
        }),
      },
    ],
  },

  // Reference to a page that is a child of this one in the workspace tree.
  child_page: {
    group: "block",
    atom: true,
    selectable: true,
    attrs: { pageId: { default: "" }, title: { default: "" } },
    toDOM: (node) => [
      "div",
      { class: "ne-child-page", "data-page-id": node.attrs.pageId as string },
      node.attrs.title as string,
    ],
    parseDOM: [
      {
        tag: "div.ne-child-page",
        getAttrs: (dom) => ({ pageId: (dom as HTMLElement).dataset.pageId ?? "", title: "" }),
      },
    ],
  },

  // Link to any page; it does not create a parent/child relationship.
  page_link: {
    group: "block",
    atom: true,
    selectable: true,
    attrs: { pageId: { default: "" }, title: { default: "" } },
    toDOM: (node) => [
      "div",
      { class: "ne-page-link", "data-page-id": node.attrs.pageId as string },
      node.attrs.title as string,
    ],
    parseDOM: [
      {
        tag: "div.ne-page-link",
        getAttrs: (dom) => ({ pageId: (dom as HTMLElement).dataset.pageId ?? "", title: "" }),
      },
    ],
  },

  // Placeholder for content shared with other pages (docs/decisions/0008). The
  // shared content is its own document, so the placeholder has no children.
  synced_block: {
    group: "block",
    atom: true,
    selectable: true,
    attrs: { syncId: { default: "" } },
    toDOM: (node) => ["div", { class: "ne-synced", "data-sync-id": node.attrs.syncId as string }],
    parseDOM: [
      {
        tag: "div.ne-synced",
        getAttrs: (dom) => ({ syncId: (dom as HTMLElement).dataset.syncId ?? "" }),
      },
    ],
  },

  text: { group: "inline" },

  hard_break: {
    group: "inline",
    inline: true,
    selectable: false,
    toDOM: () => ["br"],
    parseDOM: [{ tag: "br" }],
  },
};

const marks: Record<string, MarkSpec> = {
  bold: { toDOM: () => ["strong", 0], parseDOM: [{ tag: "strong" }, { tag: "b" }] },
  italic: { toDOM: () => ["em", 0], parseDOM: [{ tag: "em" }, { tag: "i" }] },
  code: { toDOM: () => ["code", 0], parseDOM: [{ tag: "code" }] },
  strike: { toDOM: () => ["s", 0], parseDOM: [{ tag: "s" }, { tag: "del" }] },
  link: {
    attrs: { href: { default: "" } },
    // A link must not grow when the user types right after it.
    inclusive: false,
    toDOM: (mark) => {
      const href = mark.attrs.href as string;
      if (parsePageHref(href)) return ["a", { href, class: "ne-inline-page" }, 0];
      return parseAttachmentSrc(href) ? ["a", { href, class: "ne-inline-attachment" }, 0] : ["a", { href }, 0];
    },
    parseDOM: [
      {
        tag: "a[href]",
        getAttrs: (dom) => ({ href: (dom as HTMLElement).getAttribute("href") ?? "" }),
      },
    ],
  },
};

export const pageSchema = new Schema({ nodes, marks });

/** Every block kind a user can insert, in the order of the product spec. */
export const BLOCK_NODE_NAMES = [
  "paragraph",
  "heading",
  "bullet_list",
  "ordered_list",
  "todo_list",
  "toggle",
  "quote",
  "callout",
  "divider",
  "code_block",
  "image",
  "child_page",
  "page_link",
] as const;
