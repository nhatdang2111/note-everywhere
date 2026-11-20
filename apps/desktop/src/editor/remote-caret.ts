import type { Node as PmNode } from "prosemirror-model";
import { Plugin, PluginKey, TextSelection } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";

function blockStart(doc: PmNode, index: number): number {
  let pos = 0;
  for (let i = 0; i < index; i += 1) pos += doc.child(i).nodeSize;
  return pos;
}

/**
 * Where a position in `oldDoc` ends up in `newDoc` after a change made
 * elsewhere. Top-level blocks are matched from both ends, then the text of the
 * block holding the position is compared, so a caret stays with its words when
 * text is added before it, in other blocks, or later in the same block.
 */
export function mapPosAcrossDocs(oldDoc: PmNode, newDoc: PmNode, pos: number): number {
  const clamped = Math.min(Math.max(pos, 0), oldDoc.content.size);
  if (oldDoc.eq(newDoc)) return clamped;
  const n = oldDoc.childCount;
  const m = newDoc.childCount;

  let prefix = 0;
  while (prefix < n && prefix < m && oldDoc.child(prefix).eq(newDoc.child(prefix))) prefix += 1;
  let suffix = 0;
  while (suffix < n - prefix && suffix < m - prefix && oldDoc.child(n - 1 - suffix).eq(newDoc.child(m - 1 - suffix))) suffix += 1;

  const $pos = oldDoc.resolve(clamped);
  const sizeDelta = newDoc.content.size - oldDoc.content.size;
  if ($pos.depth === 0) {
    const index = $pos.index(0);
    if (index <= prefix) return clamped;
    if (index >= n - suffix) return Math.min(Math.max(clamped + sizeDelta, 0), newDoc.content.size);
    return Math.min(clamped, newDoc.content.size);
  }

  const i = $pos.index(0);
  let j: number;
  if (i < prefix) j = i;
  else if (i >= n - suffix) j = i + (m - n);
  else if (n === m) j = i;
  else {
    j = -1;
    for (let k = prefix; k < m - suffix; k += 1) {
      if (newDoc.child(k).eq(oldDoc.child(i))) {
        j = k;
        break;
      }
    }
    if (j < 0) j = Math.min(Math.max(i, prefix), m - 1);
  }

  const oldBlock = oldDoc.child(i);
  const newBlock = newDoc.child(j);
  const newStart = blockStart(newDoc, j);
  const rel = clamped - blockStart(oldDoc, i) - 1;
  const limit = newBlock.content.size;

  const start = oldBlock.content.findDiffStart(newBlock.content);
  let mapped = rel;
  if (start !== null && start !== undefined) {
    const end = oldBlock.content.findDiffEnd(newBlock.content);
    let endA = end ? end.a : start;
    let endB = end ? end.b : start;
    const overlap = start - Math.min(endA, endB);
    if (overlap > 0) {
      endA += overlap;
      endB += overlap;
    }
    if (rel <= start) mapped = rel;
    else if (rel >= endA) mapped = rel + (endB - endA);
    else mapped = start + Math.min(rel - start, endB - start);
  }
  return newStart + 1 + Math.min(Math.max(mapped, 0), limit);
}

interface Pending {
  doc: PmNode;
  anchor: number;
  head: number;
  /** Time after which the guard stops watching for the binding's stale selection reset. */
  until: number;
  /** The mapped caret has been applied; later edits must not remap it. */
  applied: boolean;
}

const GUARD_MS = 400;

export interface RemoteCaret {
  plugin: Plugin;
  /** Call right before importing a remote update: remembers the selection to carry over. */
  begin(view: EditorView): void;
}

/**
 * Keeps the caret on its text when an update from another device is applied.
 *
 * The Loro binding moves the selection with the document change, but then
 * restores the position the caret had before the update, which is the wrong
 * place whenever the update changed text before it. This plugin puts the
 * caret where its text went and ignores that late restore.
 */
export function createRemoteCaret(): RemoteCaret {
  let pending: Pending | null = null;
  const key = new PluginKey("ne-remote-caret");

  const plugin = new Plugin({
    key,
    appendTransaction(transactions, _old, state) {
      const active = pending;
      if (!active || active.applied || !transactions.some((tr) => tr.docChanged)) return null;
      active.applied = true;
      if (!(state.selection instanceof TextSelection)) return null;
      const anchor = mapPosAcrossDocs(active.doc, state.doc, active.anchor);
      const head = mapPosAcrossDocs(active.doc, state.doc, active.head);
      if (anchor === state.selection.anchor && head === state.selection.head) return null;
      return state.tr.setSelection(TextSelection.create(state.doc, anchor, head));
    },
    filterTransaction(tr) {
      const active = pending;
      if (!active || Date.now() > active.until) return true;
      // A late, selection-only reset to where the caret used to be.
      const stale = tr.selectionSet && !tr.docChanged && !tr.getMeta("pointer") && !tr.getMeta("uiEvent");
      if (!stale || !(tr.selection instanceof TextSelection)) return true;
      return !(tr.selection.anchor === active.anchor && tr.selection.head === active.head);
    },
  });

  return {
    plugin,
    begin(view) {
      const { selection, doc } = view.state;
      if (!(selection instanceof TextSelection)) return void (pending = null);
      pending = { doc, anchor: selection.anchor, head: selection.head, until: Date.now() + GUARD_MS, applied: false };
      setTimeout(() => {
        if (pending && Date.now() >= pending.until) pending = null;
      }, GUARD_MS + 20);
    },
  };
}
