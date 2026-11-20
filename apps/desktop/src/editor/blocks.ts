import type { Node as PmNode } from "prosemirror-model";
import { Selection } from "prosemirror-state";
import type { Command } from "prosemirror-state";
import { BLOCK_NODE_NAMES, pageSchema } from "./schema.ts";

export type BlockKind = (typeof BLOCK_NODE_NAMES)[number];

/** The 15 insertable block kinds of the product spec; heading levels count separately. */
export const BLOCK_CHOICES = [
  "paragraph",
  "heading1",
  "heading2",
  "heading3",
  "bullet_list",
  "ordered_list",
  "todo_list",
  "toggle",
  "quote",
  "callout",
  "code_block",
  "divider",
  "image",
  "child_page",
  "page_link",
] as const;

export type BlockChoice = (typeof BLOCK_CHOICES)[number];

/** Kinds a text block can be turned into; the rest are atoms with no text to carry over. */
export const TURN_INTO_CHOICES = BLOCK_CHOICES.slice(0, 11) as readonly BlockChoice[];

export interface BlockAttrs {
  pageId?: string;
  title?: string;
  src?: string;
  alt?: string;
}

const { nodes } = pageSchema;
const paragraph = () => nodes.paragraph.create();
const listOf = (list: string, item: string) => nodes[list].create(null, nodes[item].create(null, paragraph()));

/** A fresh, empty block of the given kind. */
export function createBlock(choice: BlockChoice, attrs: BlockAttrs = {}): PmNode {
  switch (choice) {
    case "paragraph":
      return paragraph();
    case "heading1":
    case "heading2":
    case "heading3":
      return nodes.heading.create({ level: Number(choice.slice(-1)) });
    case "bullet_list":
      return listOf("bullet_list", "list_item");
    case "ordered_list":
      return listOf("ordered_list", "list_item");
    case "todo_list":
      return listOf("todo_list", "todo_item");
    case "toggle":
      return nodes.toggle.create(null, paragraph());
    case "quote":
      return nodes.quote.create(null, paragraph());
    case "callout":
      return nodes.callout.create(null, paragraph());
    case "divider":
      return nodes.divider.create();
    case "code_block":
      return nodes.code_block.create();
    case "image":
      return nodes.image.create({ src: attrs.src ?? "", alt: attrs.alt ?? "" });
    case "child_page":
      return nodes.child_page.create({ pageId: attrs.pageId ?? "", title: attrs.title ?? "" });
    case "page_link":
      return nodes.page_link.create({ pageId: attrs.pageId ?? "", title: attrs.title ?? "" });
  }
}

/**
 * Insert a block after the top-level block holding the cursor, replacing it
 * when that block is an empty paragraph.
 */
export function insertNode(block: PmNode): Command {
  return (state, dispatch) => {
    const { $from } = state.selection;
    if ($from.depth < 1) return false;
    const top = $from.node(1);
    const replaceEmpty = top.type === nodes.paragraph && top.content.size === 0;
    const start = $from.before(1);
    const from = replaceEmpty ? start : start + top.nodeSize;
    const to = replaceEmpty ? start + top.nodeSize : from;
    if (dispatch) {
      const tr = state.tr.replaceWith(from, to, block);
      // Atoms have no text inside; land the cursor in the block after them.
      const after = block.isAtom ? from + block.nodeSize : from + 1;
      if (block.isAtom && after >= tr.doc.content.size) tr.insert(tr.doc.content.size, paragraph());
      tr.setSelection(Selection.near(tr.doc.resolve(Math.min(after, tr.doc.content.size))));
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
}

export const insertChoice = (choice: BlockChoice, attrs?: BlockAttrs): Command => insertNode(createBlock(choice, attrs));

const KIND_TO_CHOICE: Record<BlockKind, BlockChoice> = {
  paragraph: "paragraph",
  heading: "heading1",
  bullet_list: "bullet_list",
  ordered_list: "ordered_list",
  todo_list: "todo_list",
  toggle: "toggle",
  quote: "quote",
  callout: "callout",
  divider: "divider",
  code_block: "code_block",
  image: "image",
  child_page: "child_page",
  page_link: "page_link",
};

/** Insert by schema node name; heading inserts level 1. */
export const insertBlock = (kind: BlockKind): Command => insertChoice(KIND_TO_CHOICE[kind]);
