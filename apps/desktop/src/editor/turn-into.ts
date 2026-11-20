import { Fragment, type Node as PmNode } from "prosemirror-model";
import { Selection, TextSelection } from "prosemirror-state";
import type { Command, EditorState } from "prosemirror-state";
import { blockOffsets, blockRange, rangeBounds, withStash, type BlockRange } from "./block-ops.ts";
import type { BlockChoice } from "./blocks.ts";
import { pageSchema } from "./schema.ts";
import { stashAt, type Stash, type StashEntry } from "./stash.ts";

const { nodes } = pageSchema;

interface Leaf {
  inline: Fragment;
  stash: Stash;
  /** Blocks nested under this leaf (sub-lists, toggle contents). */
  extras: PmNode[];
  /** Where the source text block sat in the document. */
  from: number;
  to: number;
}

function codeToInline(code: PmNode): Fragment {
  const lines = code.textContent.split("\n");
  const parts: PmNode[] = [];
  lines.forEach((line, index) => {
    if (index > 0) parts.push(nodes.hard_break.create());
    if (line) parts.push(pageSchema.text(line));
  });
  return Fragment.fromArray(parts);
}

function leaf(state: EditorState, block: PmNode, pos: number, own: Stash): Leaf {
  return {
    inline: block.type === nodes.code_block ? codeToInline(block) : block.content,
    stash: { ...stashAt(state, pos), ...own },
    extras: [],
    from: pos,
    to: pos + block.nodeSize,
  };
}

/** A list item or toggle: its first paragraph is the leaf, the remaining children are extras. */
function itemLeaf(state: EditorState, item: PmNode, pos: number, own: Stash): Leaf {
  const first = item.firstChild as PmNode;
  const result = leaf(state, first, pos + 1, own);
  item.forEach((child, _offset, index) => {
    if (index > 0) result.extras.push(child);
  });
  return result;
}

/** Break a block into text leaves; null when the block has no text to carry over (atoms). */
function leavesOf(state: EditorState, block: PmNode, pos: number): Leaf[] | null {
  switch (block.type.name) {
    case "paragraph":
      return [leaf(state, block, pos, {})];
    case "heading":
      return [leaf(state, block, pos, { level: block.attrs.level })];
    case "code_block":
      return [leaf(state, block, pos, { language: block.attrs.language })];
    case "bullet_list":
    case "ordered_list":
    case "todo_list": {
      const out: Leaf[] = [];
      let itemPos = pos + 1;
      block.forEach((item) => {
        out.push(itemLeaf(state, item, itemPos, item.type === nodes.todo_item ? { checked: item.attrs.checked } : {}));
        itemPos += item.nodeSize;
      });
      return out;
    }
    case "toggle":
      return [itemLeaf(state, block, pos, { open: block.attrs.open })];
    case "quote":
    case "callout": {
      const out: Leaf[] = [];
      let childPos = pos + 1;
      block.forEach((child) => {
        if (child.isTextblock) out.push(leaf(state, child, childPos, {}));
        else if (out.length > 0) out[out.length - 1]?.extras.push(child);
        else out.push({ inline: Fragment.empty, stash: {}, extras: [child], from: childPos, to: childPos });
        childPos += child.nodeSize;
      });
      const first = out[0];
      if (first && block.type === nodes.callout) first.stash = { ...first.stash, icon: block.attrs.icon };
      return out;
    }
    default:
      return null;
  }
}

interface Built {
  blocks: PmNode[];
  /** Text block produced for each leaf, and the attributes it should remember. */
  textblocks: { leaf: Leaf; node: PmNode }[];
}

function build(choice: BlockChoice, leaves: Leaf[]): Built {
  const textblocks: Built["textblocks"] = [];
  const text = (source: Leaf, node: PmNode): PmNode => (textblocks.push({ leaf: source, node }), node);
  const paragraph = (source: Leaf) => text(source, nodes.paragraph.create(null, source.inline));

  switch (choice) {
    case "paragraph":
      return { blocks: leaves.flatMap((l) => [paragraph(l), ...l.extras]), textblocks };
    case "heading1":
    case "heading2":
    case "heading3": {
      const level = Number(choice.slice(-1));
      return { blocks: leaves.flatMap((l) => [text(l, nodes.heading.create({ level }, l.inline)), ...l.extras]), textblocks };
    }
    case "code_block":
      return {
        blocks: leaves.flatMap((l) => {
          const content = l.inline.size > 0 ? pageSchema.text(l.inline.textBetween(0, l.inline.size, "\n", "\n")) : null;
          return [text(l, nodes.code_block.create({ language: String(l.stash.language ?? "") }, content)), ...l.extras];
        }),
        textblocks,
      };
    case "bullet_list":
    case "ordered_list":
    case "todo_list": {
      const todo = choice === "todo_list";
      const items = leaves.map((l) => {
        const content = [paragraph(l), ...l.extras];
        return todo ? nodes.todo_item.create({ checked: Boolean(l.stash.checked) }, content) : nodes.list_item.create(null, content);
      });
      return { blocks: [nodes[choice].create(null, items)], textblocks };
    }
    case "toggle": {
      const [first, ...rest] = leaves;
      if (!first) return { blocks: [], textblocks };
      const content = [paragraph(first), ...first.extras, ...rest.flatMap((l) => [paragraph(l), ...l.extras])];
      return { blocks: [nodes.toggle.create({ open: first.stash.open ?? true }, content)], textblocks };
    }
    case "quote":
    case "callout": {
      const content = leaves.flatMap((l) => [paragraph(l), ...l.extras]);
      const attrs = choice === "callout" ? { icon: String(leaves[0]?.stash.icon ?? "") } : null;
      return { blocks: [nodes[choice].create(attrs, content)], textblocks };
    }
    default:
      return { blocks: [], textblocks };
  }
}

function collectLeaves(state: EditorState, range: BlockRange): Leaf[] | null {
  const offsets = blockOffsets(state.doc);
  const leaves: Leaf[] = [];
  for (let index = range.start; index <= range.end; index += 1) {
    const found = leavesOf(state, state.doc.child(index), offsets[index] ?? 0);
    if (!found) return null;
    leaves.push(...found);
  }
  return leaves;
}

/** True when every selected block is a text block that can change kind. */
export function canTurnInto(state: EditorState): boolean {
  return collectLeaves(state, blockRange(state)) !== null;
}

/**
 * Change the kind of the selected blocks. Text, marks and nested blocks are
 * kept; attributes of the old kind are stashed so changing back restores them.
 * Several selected blocks become one list, toggle, quote or callout.
 */
export const turnInto =
  (choice: BlockChoice): Command =>
  (state, dispatch) => {
    const range = blockRange(state);
    const leaves = collectLeaves(state, range);
    if (!leaves || leaves.length === 0) return false;
    const built = build(choice, leaves);
    if (built.blocks.length === 0) return false;
    if (dispatch) {
      const { from, to } = rangeBounds(state.doc, range);
      const tr = state.tr.replaceWith(from, to, built.blocks);
      const end = from + built.blocks.reduce((sum, block) => sum + block.nodeSize, 0);
      const byNode = new Map(built.textblocks.map((entry) => [entry.node, entry]));
      const positions = new Map<Leaf, number>();
      const entries: StashEntry[] = [];
      tr.doc.nodesBetween(from, end, (node, pos) => {
        const entry = byNode.get(node);
        if (!entry) return;
        positions.set(entry.leaf, pos);
        if (Object.keys(entry.leaf.stash).length > 0) entries.push({ pos, attrs: entry.leaf.stash });
      });
      withStash(tr, entries);

      const caret = state.selection.$from.pos;
      const source = leaves.find((l) => caret > l.from && caret < l.to) ?? leaves[0];
      const target = source ? positions.get(source) : undefined;
      if (target !== undefined && source) {
        const node = tr.doc.nodeAt(target);
        const offset = Math.max(0, Math.min(caret - source.from - 1, node?.content.size ?? 0));
        tr.setSelection(TextSelection.create(tr.doc, target + 1 + offset));
      } else {
        tr.setSelection(Selection.near(tr.doc.resolve(from + 1)));
      }
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
