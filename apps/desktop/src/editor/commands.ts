import { setBlockType, toggleMark, wrapIn } from "prosemirror-commands";
import type { Mark, MarkType, ResolvedPos } from "prosemirror-model";
import { NodeSelection, Selection } from "prosemirror-state";
import type { Command, EditorState } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { pageSchema } from "./schema.ts";

const { nodes, marks } = pageSchema;

export const setHeading = (level: 1 | 2 | 3): Command => setBlockType(nodes.heading, { level });
export const wrapInQuote: Command = wrapIn(nodes.quote);
export const toggleBold: Command = toggleMark(marks.bold);
export const toggleItalic: Command = toggleMark(marks.italic);
export const toggleCode: Command = toggleMark(marks.code);
export const toggleStrike: Command = toggleMark(marks.strike);

export interface MarkRange {
  from: number;
  to: number;
  mark: Mark;
}

/** The full run of text around `$pos` that carries a mark of `type`. */
export function markRangeAt($pos: ResolvedPos, type: MarkType): MarkRange | null {
  const { parent, parentOffset } = $pos;
  const probe = (offset: number) => {
    const { node, index, offset: start } = parent.childAfter(offset);
    const mark = node?.marks.find((candidate) => candidate.type === type);
    return node && mark ? { node, index, start, mark } : null;
  };
  // At the end of a link the node after the cursor has no mark; look just before it too.
  const found = probe(parentOffset) ?? (parentOffset > 0 ? probe(parentOffset - 1) : null);
  if (!found) return null;

  const { node, index, start, mark } = found;
  let startIndex = index;
  let endIndex = index + 1;
  let from = $pos.start() + start;
  let to = from + node.nodeSize;
  while (startIndex > 0 && mark.isInSet(parent.child(startIndex - 1).marks)) {
    startIndex -= 1;
    from -= parent.child(startIndex).nodeSize;
  }
  while (endIndex < parent.childCount && mark.isInSet(parent.child(endIndex).marks)) {
    to += parent.child(endIndex).nodeSize;
    endIndex += 1;
  }
  return { from, to, mark };
}

/** The link the cursor or selection sits in, if any. */
export function linkAt(state: EditorState): MarkRange | null {
  const { selection } = state;
  return markRangeAt(selection.$from, marks.link);
}

/** Set `href` on the selection, or on the whole link when the cursor is inside one. */
export const setLink =
  (href: string): Command =>
  (state, dispatch) => {
    const existing = linkAt(state);
    const { from, to, empty } = state.selection;
    const range = existing && empty ? existing : { from, to };
    if (range.from === range.to) return false;
    if (dispatch) {
      const tr = state.tr.removeMark(range.from, range.to, marks.link).addMark(range.from, range.to, marks.link.create({ href }));
      dispatch(tr);
    }
    return true;
  };

export const removeLink: Command = (state, dispatch) => {
  const existing = linkAt(state);
  const { from, to, empty } = state.selection;
  const range = existing && empty ? existing : { from, to };
  if (range.from === range.to) return false;
  dispatch?.(state.tr.removeMark(range.from, range.to, marks.link));
  return true;
};

/**
 * Insert text at the cursor, replacing a text selection. A selected block
 * (divider, image) is kept: the text goes into a new paragraph after it.
 */
export function insertTextAtCursor(view: EditorView, text: string): void {
  if (!text) return;
  const { state } = view;
  if (state.selection instanceof NodeSelection) {
    const at = state.selection.to;
    const tr = state.tr.insert(at, nodes.paragraph.create(null, pageSchema.text(text)));
    view.dispatch(tr.setSelection(Selection.near(tr.doc.resolve(at + 1 + text.length))).scrollIntoView());
    return;
  }
  view.dispatch(state.tr.insertText(text).scrollIntoView());
}

/** Whether a mark applies across the whole selection (or at the cursor). */
export function isMarkActive(state: EditorState, type: MarkType): boolean {
  const { from, $from, to, empty } = state.selection;
  if (empty) return Boolean(type.isInSet(state.storedMarks ?? $from.marks()));
  return state.doc.rangeHasMark(from, to, type);
}
