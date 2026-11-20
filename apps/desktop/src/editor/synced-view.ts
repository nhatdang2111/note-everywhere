import type { Node as PmNode } from "prosemirror-model";
import type { EditorView, NodeView } from "prosemirror-view";
import type { PageDoc } from "./binding.ts";
import type { PageEditor } from "./editor.ts";
import type { EditorHost, EditorLabels, SyncedSession } from "./host.ts";

type GetPos = () => number | undefined;

/** Builds the editor that lives inside a synced block, bound to the shared document. */
export type NestedEditorFactory = (mount: HTMLElement, doc: PageDoc) => PageEditor;

const SYNC_ICON =
  '<svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13 6.5A5 5 0 004 4.5M3 9.5a5 5 0 009 2M3.5 2v3h3M12.5 14v-3h-3"/></svg>';
const MENU_ICON =
  '<svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="M3.5 8h.01M8 8h.01M12.5 8h.01"/></svg>';

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = className;
  return element;
}

/** Child indexes from the document root down to the node at `pos`. */
export function pathOf(doc: PmNode, pos: number): number[] {
  const $pos = doc.resolve(pos);
  return Array.from({ length: $pos.depth + 1 }, (_, depth) => $pos.index(depth));
}

const isNotFound = (error: unknown): boolean => typeof error === "object" && error !== null && (error as { code?: unknown }).code === "not_found";

/**
 * Node view of a synced block: a labelled frame around an editor on the shared
 * document, so the content is shown and edited in place. Edits flow through the
 * host's session; the same content in another page is another view of the same
 * shared document. Returns the view plus a hook for the editor's refresh.
 */
export function createSyncedView(host: EditorHost, labels: EditorLabels, createNested: NestedEditorFactory, refreshers: Set<() => void>) {
  return (node: PmNode, view: EditorView, getPos: GetPos): NodeView => {
    let current = node;
    const dom = el("div", "ne-synced");
    dom.contentEditable = "false";
    dom.setAttribute("role", "group");
    dom.setAttribute("aria-label", labels.syncedBlock);

    const header = el("div", "ne-synced-header");
    const badge = el("span", "ne-synced-badge");
    badge.innerHTML = SYNC_ICON;
    const badgeText = el("span", "ne-synced-count");
    badge.append(badgeText);
    const menu = el("button", "ne-synced-menu");
    menu.type = "button";
    menu.innerHTML = MENU_ICON;
    menu.title = labels.syncedMenu;
    menu.setAttribute("aria-label", labels.syncedMenu);
    header.append(badge, menu);
    const body = el("div", "ne-synced-body");
    const status = el("p", "ne-synced-status");
    status.setAttribute("role", "status");
    dom.append(header, body, status);

    let nested: PageEditor | null = null;
    let session: SyncedSession | null = null;
    let stopWaiting: (() => void) | null = null;
    let destroyed = false;
    let opening = false;

    const syncId = () => String(current.attrs.syncId ?? "");

    const showStatus = (state: "loading" | "ready" | "missing") => {
      dom.dataset.state = state;
      status.textContent = state === "loading" ? labels.syncedLoading : state === "missing" ? labels.syncedMissing : "";
      status.hidden = state === "ready";
    };

    const countText = (count: number | null) => (count === null ? labels.syncedBlock : labels.syncedCount.replace("{count}", String(count)));
    badgeText.textContent = labels.syncedBlock;

    const refreshCount = async () => {
      const id = syncId();
      if (!id || !host.synced) return;
      const count = await host.synced.occurrences(id).catch(() => null);
      if (!destroyed && id === syncId()) badgeText.textContent = countText(count);
    };

    const open = async () => {
      const id = syncId();
      if (opening || nested || !id || !host.synced) return;
      opening = true;
      showStatus("loading");
      try {
        const opened = await host.synced.open(id);
        if (destroyed || id !== syncId()) {
          opened.close();
          return;
        }
        session = opened;
        nested = createNested(body, opened.doc);
        opened.setDeliver((update) => nested?.importRemote(update));
        showStatus("ready");
        void refreshCount();
      } catch (error) {
        if (destroyed) return;
        showStatus("missing");
        // The content may still be on its way from another device.
        if (isNotFound(error) && host.synced) {
          stopWaiting?.();
          stopWaiting = host.synced.retryWhenAvailable(id, () => void open());
        } else {
          host.onError?.(error instanceof Error ? error.message : String((error as { message?: string })?.message ?? error));
        }
      } finally {
        opening = false;
      }
    };

    const release = () => {
      stopWaiting?.();
      stopWaiting = null;
      nested?.destroy();
      nested = null;
      session?.close();
      session = null;
      body.replaceChildren();
    };

    menu.addEventListener("click", () => {
      const pos = getPos();
      if (pos === undefined) return;
      const rect = menu.getBoundingClientRect();
      host.onSyncedMenu?.({ syncId: syncId(), path: pathOf(view.state.doc, pos), anchor: { left: rect.left, top: rect.top, bottom: rect.bottom } });
    });

    const refresh = () => {
      void refreshCount();
      if (!nested) void open();
    };
    refreshers.add(refresh);

    if (host.synced) void open();
    else showStatus("missing");

    return {
      dom,
      update(next) {
        if (next.type !== current.type) return false;
        if (next.attrs.syncId !== current.attrs.syncId) return false;
        current = next;
        return true;
      },
      selectNode: () => dom.classList.add("ne-synced-selected"),
      deselectNode: () => dom.classList.remove("ne-synced-selected"),
      // Everything inside is the nested editor's business.
      stopEvent: (event) => !(event.target instanceof Node) || !header.contains(event.target) || event.type !== "mousedown",
      ignoreMutation: () => true,
      destroy() {
        destroyed = true;
        refreshers.delete(refresh);
        release();
      },
    };
  };
}
