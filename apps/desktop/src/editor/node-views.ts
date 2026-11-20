import type { Node as PmNode } from "prosemirror-model";
import type { EditorView, NodeView, NodeViewConstructor } from "prosemirror-view";
import { DEFAULT_LABELS, type EditorHost, type EditorLabels } from "./host.ts";

type GetPos = () => number | undefined;

export const CODE_LANGUAGES = ["", "bash", "c", "cpp", "css", "go", "html", "java", "javascript", "json", "markdown", "python", "rust", "sql", "toml", "typescript", "yaml"] as const;

const PAGE_ICON =
  '<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 1.75h5l3 3v9.5H4z"/><path d="M9 1.75v3h3"/></svg>';
const LINK_ICON =
  '<svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 1.75h5l3 3v9.5H4z"/><path d="M6.5 10.5l3-3M7.5 7.5h2v2"/></svg>';

/** Update one attribute of the node at `getPos()` without touching its content. */
function setAttr(view: EditorView, getPos: GetPos, name: string, value: unknown): void {
  const pos = getPos();
  if (pos === undefined || !view.state.doc.nodeAt(pos)) return;
  view.dispatch(view.state.tr.setNodeAttribute(pos, name, value));
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  return element;
}

export interface NodeViews {
  views: Record<string, NodeViewConstructor>;
  /** Re-read titles and states of page blocks, after the tree changed. */
  refresh(): void;
}

export function createNodeViews(host: EditorHost): NodeViews {
  const labels: EditorLabels = { ...DEFAULT_LABELS, ...host.labels };
  const refreshers = new Set<() => void>();

  const todoItem = (node: PmNode, view: EditorView, getPos: GetPos): NodeView => {
    const dom = el("li", "ne-todo-item");
    const box = el("input");
    box.type = "checkbox";
    box.contentEditable = "false";
    box.setAttribute("aria-label", labels.todoCheckbox);
    const contentDOM = el("div");
    dom.append(box, contentDOM);
    const sync = (checked: boolean) => {
      box.checked = checked;
      dom.dataset.checked = checked ? "true" : "false";
    };
    sync(Boolean(node.attrs.checked));
    box.addEventListener("change", () => setAttr(view, getPos, "checked", box.checked));
    return {
      dom,
      contentDOM,
      update(next) {
        if (next.type !== node.type) return false;
        sync(Boolean(next.attrs.checked));
        return true;
      },
      stopEvent: (event) => event.target === box,
      ignoreMutation: (mutation) => !contentDOM.contains(mutation.target),
    };
  };

  const toggle = (node: PmNode, view: EditorView, getPos: GetPos): NodeView => {
    const dom = el("div", "ne-toggle");
    const button = el("button", "ne-toggle-button");
    button.type = "button";
    button.contentEditable = "false";
    button.setAttribute("aria-label", labels.toggleBlock);
    button.addEventListener("click", () => setAttr(view, getPos, "open", dom.dataset.open !== "true"));
    const contentDOM = el("div", "ne-toggle-body");
    dom.append(button, contentDOM);
    const sync = (open: boolean) => {
      dom.dataset.open = open ? "true" : "false";
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
  };

  const codeBlock = (node: PmNode, view: EditorView, getPos: GetPos): NodeView => {
    const dom = el("div", "ne-code-block");
    const select = el("select", "ne-code-language");
    select.contentEditable = "false";
    select.setAttribute("aria-label", labels.codeLanguage);
    const known = new Set<string>(CODE_LANGUAGES);
    const addOption = (value: string) => {
      const option = el("option");
      option.value = value;
      option.textContent = value || "plain text";
      select.append(option);
    };
    CODE_LANGUAGES.forEach(addOption);
    select.addEventListener("change", () => setAttr(view, getPos, "language", select.value));
    const pre = el("pre");
    const contentDOM = el("code");
    pre.append(contentDOM);
    dom.append(select, pre);
    const sync = (language: string) => {
      if (!known.has(language)) (known.add(language), addOption(language));
      select.value = language;
    };
    sync(String(node.attrs.language ?? ""));
    return {
      dom,
      contentDOM,
      update(next) {
        if (next.type !== node.type) return false;
        sync(String(next.attrs.language ?? ""));
        return true;
      },
      stopEvent: (event) => event.target === select,
      ignoreMutation: (mutation) => !pre.contains(mutation.target),
    };
  };

  const image = (node: PmNode): NodeView => {
    const dom = el("figure", "ne-image");
    const img = el("img");
    const fallback = el("span", "ne-image-missing");
    fallback.textContent = labels.imageMissing;
    dom.append(img, fallback);
    let shown = "";
    const show = async (src: string, alt: string) => {
      img.alt = alt;
      if (src === shown) return;
      shown = src;
      dom.dataset.state = src ? "loading" : "empty";
      if (!src) return;
      try {
        const url = await (host.resolveImage?.(src) ?? src);
        if (shown !== src) return;
        img.src = url;
        dom.dataset.state = "ready";
      } catch {
        if (shown === src) dom.dataset.state = "error";
      }
    };
    img.addEventListener("error", () => void (dom.dataset.state = "error"));
    void show(String(node.attrs.src ?? ""), String(node.attrs.alt ?? ""));
    return {
      dom,
      update(next) {
        if (next.type !== node.type) return false;
        void show(String(next.attrs.src ?? ""), String(next.attrs.alt ?? ""));
        return true;
      },
    };
  };

  /** Card for `child_page` and `page_link`: shows the live title and opens the page on click. */
  const pageCard =
    (kind: "child" | "link"): NodeViewConstructor =>
    (node) => {
      let current = node;
      const dom = el("div", `ne-page-card ne-${kind === "child" ? "child-page" : "page-link"}`);
      dom.contentEditable = "false";
      dom.setAttribute("role", "link");
      dom.tabIndex = -1;
      const icon = el("span", "ne-page-card-icon");
      icon.innerHTML = kind === "child" ? PAGE_ICON : LINK_ICON;
      const title = el("span", "ne-page-card-title");
      const hint = el("span", "ne-page-card-hint");
      dom.append(icon, title, hint);

      const render = () => {
        const pageId = String(current.attrs.pageId ?? "");
        const info = pageId ? host.resolvePage?.(pageId) : null;
        const state = !pageId ? "unset" : (info?.state ?? "ok");
        dom.dataset.state = state;
        dom.dataset.pageId = pageId;
        title.textContent = state === "unset" ? labels.chooseAPage : info?.title || String(current.attrs.title ?? "") || "Untitled";
        hint.textContent =
          state === "trashed" ? labels.pageInTrash : state === "missing" ? labels.pageMissing : kind === "child" ? labels.subpage : labels.linkedPage;
        dom.setAttribute("aria-disabled", String(state !== "ok"));
      };
      dom.addEventListener("click", (event) => {
        const pageId = String(current.attrs.pageId ?? "");
        if (pageId && dom.dataset.state === "ok") host.openPage?.(pageId, event.metaKey || event.ctrlKey);
      });
      refreshers.add(render);
      render();
      return {
        dom,
        update(next) {
          if (next.type !== current.type) return false;
          current = next;
          render();
          return true;
        },
        stopEvent: (event) => event.type === "click",
        destroy: () => void refreshers.delete(render),
      };
    };

  return {
    views: {
      todo_item: todoItem,
      toggle,
      code_block: codeBlock,
      image,
      child_page: pageCard("child"),
      page_link: pageCard("link"),
    },
    refresh: () => refreshers.forEach((render) => render()),
  };
}
