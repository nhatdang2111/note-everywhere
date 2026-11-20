import { setBlockType, toggleMark, wrapIn } from "prosemirror-commands";
import type { Node as PmNode } from "prosemirror-model";
import { Selection } from "prosemirror-state";
import type { Command } from "prosemirror-state";
import { BLOCK_NODE_NAMES, pageSchema } from "./schema.ts";

export type BlockKind = (typeof BLOCK_NODE_NAMES)[number];

const { nodes, marks } = pageSchema;
const paragraph = () => nodes.paragraph.create();
const listOf = (list: string, item: string, attrs?: Record<string, unknown>) =>
  nodes[list].create(null, nodes[item].create(attrs, paragraph()));

/** Build a fresh, empty instance of a block kind. */
function createBlock(kind: BlockKind): PmNode {
  switch (kind) {
    case "paragraph":
      return paragraph();
    case "heading":
      return nodes.heading.create({ level: 1 });
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
      return nodes.image.create({ src: "", alt: "" });
    case "child_page":
      return nodes.child_page.create({ pageId: "", title: "Untitled page" });
    case "page_link":
      return nodes.page_link.create({ pageId: "", title: "Untitled page" });
  }
}

/**
 * Insert a block after the top-level block holding the cursor, replacing it
 * when that block is an empty paragraph.
 */
export function insertBlock(kind: BlockKind): Command {
  return (state, dispatch) => {
    const { $from } = state.selection;
    const top = $from.node(1);
    const block = createBlock(kind);
    const replaceEmpty = top.type === nodes.paragraph && top.content.size === 0;
    const start = $from.before(1);
    const from = replaceEmpty ? start : start + top.nodeSize;
    const to = replaceEmpty ? start + top.nodeSize : from;
    if (dispatch) {
      const tr = state.tr.replaceWith(from, to, block);
      tr.setSelection(Selection.near(tr.doc.resolve(from + 1)));
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
}

export const setHeading = (level: 1 | 2 | 3): Command => setBlockType(nodes.heading, { level });
export const wrapInQuote: Command = wrapIn(nodes.quote);
export const toggleBold: Command = toggleMark(marks.bold);
export const toggleItalic: Command = toggleMark(marks.italic);
export const toggleCode: Command = toggleMark(marks.code);
export const toggleStrike: Command = toggleMark(marks.strike);

export const toggleLink =
  (href: string): Command =>
  (state, dispatch) =>
    toggleMark(marks.link, { href })(state, dispatch);
