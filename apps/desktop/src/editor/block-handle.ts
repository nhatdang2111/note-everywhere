import { NodeSelection, Plugin, PluginKey } from "prosemirror-state";
import type { Command, EditorState } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";
import { blockOffsets, blockRange, moveBlocks, selectBlocks, type BlockRange } from "./block-ops.ts";
import { DEFAULT_LABELS, type EditorHost } from "./host.ts";

const BLOCK_MIME = "application/x-ne-block";
const HANDLE_DOTS =
  '<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><circle cx="6" cy="4" r="1.25"/><circle cx="10" cy="4" r="1.25"/><circle cx="6" cy="8" r="1.25"/><circle cx="10" cy="8" r="1.25"/><circle cx="6" cy="12" r="1.25"/><circle cx="10" cy="12" r="1.25"/></svg>';

/** True when whole blocks are selected: a node selection or a range over several blocks. */
export function isBlockSelection(state: EditorState): boolean {
  const { selection } = state;
  if (selection instanceof NodeSelection) return selection.$from.depth === 0;
  const range = blockRange(state);
  return range.end > range.start && !selection.empty;
}

const selectionKey = new PluginKey("ne-block-selection");

/** Highlights the blocks of a multi-block selection. */
export function blockSelectionPlugin(): Plugin {
  return new Plugin({
    key: selectionKey,
    props: {
      decorations(state) {
        if (!isBlockSelection(state)) return null;
        const range = blockRange(state);
        const offsets = blockOffsets(state.doc);
        const decorations: Decoration[] = [];
        for (let index = range.start; index <= range.end; index += 1) {
          const from = offsets[index] ?? 0;
          decorations.push(Decoration.node(from, from + state.doc.child(index).nodeSize, { class: "ne-block-selected" }));
        }
        return DecorationSet.create(state.doc, decorations);
      },
    },
  });
}

/** Select the whole block holding the cursor, so block commands and arrow-key growth apply to it. */
export const selectCurrentBlock: Command = (state, dispatch) => {
  if (state.selection.$from.depth === 0) return false;
  const { start } = blockRange(state);
  if (dispatch) dispatch(state.tr.setSelection(NodeSelection.create(state.doc, blockOffsets(state.doc)[start] ?? 0)));
  return true;
};

/** With whole blocks selected, grow the selection by one block in the arrow's direction. */
export const extendBlockSelection =
  (direction: -1 | 1): Command =>
  (state, dispatch) => {
    if (!isBlockSelection(state)) return false;
    const range = blockRange(state);
    const next: BlockRange =
      direction > 0
        ? { start: range.start, end: Math.min(range.end + 1, state.doc.childCount - 1) }
        : { start: Math.max(range.start - 1, 0), end: range.end };
    if (dispatch) dispatch(selectBlocks(state.tr, next.start, next.end));
    return true;
  };

/** Open the block menu for the current selection from the keyboard. */
export const openBlockMenu =
  (host: EditorHost): Command =>
  (state, dispatch, view) => {
    if (!view || state.selection.$from.depth === 0 && !(state.selection instanceof NodeSelection)) return false;
    const range = blockRange(state);
    if (dispatch) {
      const { left, top, bottom } = view.coordsAtPos(state.selection.from);
      host.onBlockMenu?.({ start: range.start, end: range.end, anchor: { left, top, bottom } });
    }
    return true;
  };

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = className;
  return element;
}

/**
 * A grip beside the block under the pointer. Dragging it moves the block (or
 * the selected blocks) with an insertion line; clicking it selects the block
 * and opens the block menu; Shift+click extends the selection.
 */
export function blockHandlePlugin(host: EditorHost): Plugin {
  const label = host.labels?.blockHandle ?? DEFAULT_LABELS.blockHandle;
  return new Plugin({
    view(view) {
      const root = view.dom.parentElement;
      if (!root) return {};

      const handle = el("button", "ne-block-handle");
      handle.type = "button";
      handle.draggable = true;
      handle.tabIndex = -1;
      handle.innerHTML = HANDLE_DOTS;
      handle.title = label;
      handle.setAttribute("aria-label", label);
      handle.hidden = true;
      const indicator = el("div", "ne-drop-indicator");
      indicator.hidden = true;
      root.append(handle, indicator);

      let hovered = -1;
      let dragging: BlockRange | null = null;
      let dropIndex = -1;

      const blockDom = (index: number): HTMLElement | null => {
        const pos = blockOffsets(view.state.doc)[index];
        return pos === undefined ? null : (view.nodeDOM(pos) as HTMLElement | null);
      };

      const place = (index: number) => {
        const dom = blockDom(index);
        if (!dom) return void (handle.hidden = true);
        const rootRect = root.getBoundingClientRect();
        const rect = dom.getBoundingClientRect();
        handle.style.top = `${rect.top - rootRect.top + root.scrollTop}px`;
        handle.style.left = `${view.dom.offsetLeft - 30}px`;
        handle.hidden = false;
        hovered = index;
      };

      const blockAt = (event: MouseEvent): number => {
        const rect = view.dom.getBoundingClientRect();
        const left = Math.min(Math.max(event.clientX, rect.left + 8), rect.right - 8);
        const found = view.posAtCoords({ left, top: event.clientY });
        if (!found) return -1;
        const index = view.state.doc.resolve(found.pos).index(0);
        return Math.min(index, view.state.doc.childCount - 1);
      };

      const onMove = (event: MouseEvent) => {
        if (dragging) return;
        const index = blockAt(event);
        if (index >= 0 && index !== hovered) place(index);
      };
      const onLeave = () => {
        if (!dragging) handle.hidden = true;
        hovered = -1;
      };

      const rangeFor = (index: number): BlockRange => {
        const current = blockRange(view.state);
        const inside = isBlockSelection(view.state) && index >= current.start && index <= current.end;
        return inside ? current : { start: index, end: index };
      };

      const insertionIndex = (clientY: number): number => {
        const count = view.state.doc.childCount;
        for (let index = 0; index < count; index += 1) {
          const rect = blockDom(index)?.getBoundingClientRect();
          if (rect && clientY < rect.top + rect.height / 2) return index;
        }
        return count;
      };

      const showIndicator = (index: number) => {
        const rootRect = root.getBoundingClientRect();
        const target = blockDom(Math.min(index, view.state.doc.childCount - 1));
        if (!target) return;
        const rect = target.getBoundingClientRect();
        const y = index >= view.state.doc.childCount ? rect.bottom : rect.top;
        indicator.style.top = `${y - rootRect.top + root.scrollTop - 1}px`;
        indicator.style.left = `${view.dom.offsetLeft}px`;
        indicator.style.width = `${view.dom.offsetWidth}px`;
        indicator.hidden = false;
      };

      const onDragStart = (event: DragEvent) => {
        if (hovered < 0 || !event.dataTransfer) return event.preventDefault();
        dragging = rangeFor(hovered);
        event.dataTransfer.setData(BLOCK_MIME, JSON.stringify(dragging));
        event.dataTransfer.effectAllowed = "move";
        const dom = blockDom(dragging.start);
        if (dom) event.dataTransfer.setDragImage(dom, 0, 0);
      };
      const onDragEnd = () => {
        dragging = null;
        indicator.hidden = true;
        handle.hidden = true;
      };
      const isBlockDrag = (event: DragEvent) => Boolean(dragging) && Boolean(event.dataTransfer?.types.includes(BLOCK_MIME));
      const onDragOver = (event: DragEvent) => {
        if (!isBlockDrag(event)) return;
        event.preventDefault();
        event.stopPropagation();
        if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
        dropIndex = insertionIndex(event.clientY);
        showIndicator(dropIndex);
      };
      const onDrop = (event: DragEvent) => {
        if (!isBlockDrag(event) || !dragging) return;
        event.preventDefault();
        event.stopPropagation();
        const range = dragging;
        onDragEnd();
        moveBlocks(range, insertionIndex(event.clientY))(view.state, view.dispatch);
        view.focus();
      };
      const onDragLeave = (event: DragEvent) => {
        if (!root.contains(event.relatedTarget as Node | null)) indicator.hidden = true;
      };

      const onClick = (event: MouseEvent) => {
        if (hovered < 0) return;
        const range = event.shiftKey
          ? { start: Math.min(blockRange(view.state).start, hovered), end: Math.max(blockRange(view.state).end, hovered) }
          : { start: hovered, end: hovered };
        view.dispatch(selectBlocks(view.state.tr, range.start, range.end));
        view.focus();
        if (!event.shiftKey) {
          const { left, top, bottom } = handle.getBoundingClientRect();
          host.onBlockMenu?.({ ...range, anchor: { left, top, bottom } });
        }
      };

      root.addEventListener("mousemove", onMove);
      root.addEventListener("mouseleave", onLeave);
      root.addEventListener("dragover", onDragOver, true);
      root.addEventListener("drop", onDrop, true);
      root.addEventListener("dragleave", onDragLeave);
      handle.addEventListener("dragstart", onDragStart);
      handle.addEventListener("dragend", onDragEnd);
      handle.addEventListener("click", onClick);

      return {
        update() {
          if (hovered >= view.state.doc.childCount) hovered = -1;
        },
        destroy() {
          root.removeEventListener("mousemove", onMove);
          root.removeEventListener("mouseleave", onLeave);
          root.removeEventListener("dragover", onDragOver, true);
          root.removeEventListener("drop", onDrop, true);
          root.removeEventListener("dragleave", onDragLeave);
          handle.remove();
          indicator.remove();
        },
      };
    },
  });
}

