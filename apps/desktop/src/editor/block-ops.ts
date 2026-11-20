import type { Node as PmNode } from "prosemirror-model";
import { NodeSelection, Selection, TextSelection } from "prosemirror-state";
import type { Command, EditorState, Transaction } from "prosemirror-state";
import { pageSchema } from "./schema.ts";
import { stashEntriesIn, stashKey, type StashEntry } from "./stash.ts";

const { nodes } = pageSchema;

/** Top-level block indexes, inclusive. */
export interface BlockRange {
  start: number;
  end: number;
}

export function blockOffsets(doc: PmNode): number[] {
  const offsets: number[] = [];
  let pos = 0;
  doc.forEach((child) => {
    offsets.push(pos);
    pos += child.nodeSize;
  });
  return offsets;
}

/** The top-level blocks the selection touches. */
export function blockRange(state: EditorState): BlockRange {
  const { $from, $to } = state.selection;
  const start = $from.index(0);
  const end = $to.depth === 0 ? Math.max($to.index(0) - 1, start) : $to.index(0);
  return { start, end: Math.min(end, state.doc.childCount - 1) };
}

export function rangeBounds(doc: PmNode, range: BlockRange): { from: number; to: number } {
  const offsets = blockOffsets(doc);
  return { from: offsets[range.start] ?? 0, to: (offsets[range.end] ?? 0) + doc.child(range.end).nodeSize };
}

/** Put the selection on blocks `start..end` (a node selection for one atom, text otherwise). */
export function selectBlocks(tr: Transaction, start: number, end: number): Transaction {
  const offsets = blockOffsets(tr.doc);
  const first = tr.doc.child(start);
  if (start === end && !first.inlineContent && first.isAtom) {
    return tr.setSelection(NodeSelection.create(tr.doc, offsets[start] ?? 0));
  }
  const from = (offsets[start] ?? 0) + 1;
  const last = tr.doc.child(end);
  const to = (offsets[end] ?? 0) + last.nodeSize - 1;
  if (start === end) return tr.setSelection(Selection.near(tr.doc.resolve(from)));
  return tr.setSelection(TextSelection.between(tr.doc.resolve(from), tr.doc.resolve(to)));
}

export function withStash(tr: Transaction, entries: StashEntry[]): Transaction {
  return entries.length > 0 ? tr.setMeta(stashKey, entries) : tr;
}

/** Move blocks so they sit before the block that is currently at index `insertBefore`. */
export const moveBlocks =
  (range: BlockRange, insertBefore: number): Command =>
  (state, dispatch) => {
    const { doc } = state;
    if (insertBefore >= range.start && insertBefore <= range.end + 1) return false;
    const offsets = blockOffsets(doc);
    const { from, to } = rangeBounds(doc, range);
    const target = insertBefore >= doc.childCount ? doc.content.size : (offsets[insertBefore] ?? doc.content.size);
    if (dispatch) {
      const moved = doc.slice(from, to).content;
      const carried = stashEntriesIn(state, from, to);
      const tr = state.tr;
      let newStart: number;
      if (target > to) {
        tr.insert(target, moved);
        tr.delete(from, to);
        newStart = target - (to - from);
      } else {
        tr.delete(from, to);
        tr.insert(target, moved);
        newStart = target;
      }
      const count = range.end - range.start;
      const startIndex = blockOffsets(tr.doc).indexOf(newStart);
      withStash(tr, carried.map((entry) => ({ pos: newStart + (entry.pos - from), attrs: entry.attrs })));
      if (startIndex >= 0) selectBlocks(tr, startIndex, startIndex + count);
      dispatch(tr.scrollIntoView());
    }
    return true;
  };

/** Move the selected blocks one place up (-1) or down (1). */
export const moveBlocksBy =
  (delta: -1 | 1): Command =>
  (state, dispatch) => {
    const range = blockRange(state);
    if (delta < 0) return range.start > 0 && moveBlocks(range, range.start - 1)(state, dispatch);
    return range.end < state.doc.childCount - 1 && moveBlocks(range, range.end + 2)(state, dispatch);
  };

export const duplicateBlocks: Command = (state, dispatch) => {
  const range = blockRange(state);
  const { from, to } = rangeBounds(state.doc, range);
  if (dispatch) {
    const tr = state.tr.insert(to, state.doc.slice(from, to).content);
    selectBlocks(tr, range.end + 1, range.end + 1 + (range.end - range.start));
    dispatch(tr.scrollIntoView());
  }
  return true;
};

export const deleteBlocks: Command = (state, dispatch) => {
  const range = blockRange(state);
  const { from, to } = rangeBounds(state.doc, range);
  if (dispatch) {
    const tr = state.tr.delete(from, to);
    if (tr.doc.childCount === 0) tr.insert(0, nodes.paragraph.create());
    const index = Math.min(range.start, tr.doc.childCount - 1);
    dispatch(selectBlocks(tr, index, index).scrollIntoView());
  }
  return true;
};
