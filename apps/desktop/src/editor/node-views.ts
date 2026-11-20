import type { Node as PmNode } from "prosemirror-model";
import type { EditorView, NodeView } from "prosemirror-view";

/** Update one attribute of the node at `getPos()` without touching its content. */
function setAttr(view: EditorView, getPos: () => number | undefined, name: string, value: unknown): void {
  const pos = getPos();
  if (pos === undefined) return;
  const node = view.state.doc.nodeAt(pos);
  if (!node) return;
  view.dispatch(view.state.tr.setNodeAttribute(pos, name, value));
}

/** To-do item: a checkbox that flips the `checked` attribute. */
export function todoItemView(
  node: PmNode,
  view: EditorView,
  getPos: () => number | undefined,
): NodeView {
  const dom = document.createElement("li");
  dom.className = "ne-todo-item";
  const box = document.createElement("input");
  box.type = "checkbox";
  box.contentEditable = "false";
  box.checked = Boolean(node.attrs.checked);
  box.addEventListener("change", () => setAttr(view, getPos, "checked", box.checked));
  const contentDOM = document.createElement("div");
  dom.append(box, contentDOM);
  return {
    dom,
    contentDOM,
    update(next) {
      if (next.type !== node.type) return false;
      box.checked = Boolean(next.attrs.checked);
      dom.dataset.checked = box.checked ? "true" : "false";
      return true;
    },
    stopEvent: (event) => event.target === box,
    ignoreMutation: (mutation) => !contentDOM.contains(mutation.target),
  };
}

/** Toggle: a disclosure button that flips the `open` attribute. */
export function toggleView(node: PmNode, view: EditorView, getPos: () => number | undefined): NodeView {
  const dom = document.createElement("div");
  dom.className = "ne-toggle";
  const button = document.createElement("button");
  button.type = "button";
  button.contentEditable = "false";
  button.addEventListener("click", () => setAttr(view, getPos, "open", dom.dataset.open !== "true"));
  const contentDOM = document.createElement("div");
  dom.append(button, contentDOM);
  const sync = (open: boolean) => {
    dom.dataset.open = open ? "true" : "false";
    button.textContent = open ? "▾" : "▸";
    button.setAttribute("aria-expanded", String(open));
  };
  sync(Boolean(node.attrs.open));
  return {
    dom,
    contentDOM,
    update(next) {
      if (next.type !== node.type) return false;
      sync(Boolean(next.attrs.open));
      return true;
    },
    stopEvent: (event) => event.target === button,
    ignoreMutation: (mutation) => !contentDOM.contains(mutation.target),
  };
}
