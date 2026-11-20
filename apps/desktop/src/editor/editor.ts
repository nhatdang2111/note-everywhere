import { baseKeymap, chainCommands, exitCode } from "prosemirror-commands";
import { keymap } from "prosemirror-keymap";
import { liftListItem, sinkListItem, splitListItem } from "prosemirror-schema-list";
import { EditorState, NodeSelection, Plugin } from "prosemirror-state";
import type { Command } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { bindingPlugins, createPageDoc, type PageDoc } from "./binding.ts";
import { blockHandlePlugin, blockSelectionPlugin, extendBlockSelection, openBlockMenu, selectCurrentBlock } from "./block-handle.ts";
import { createBlock } from "./blocks.ts";
import { toggleBold, toggleCode, toggleItalic, toggleStrike } from "./commands.ts";
import { findPlugin } from "./find.ts";
import { DEFAULT_LABELS, type EditorHost } from "./host.ts";
import { pageInputRules } from "./input-rules.ts";
import { createRemoteCaret } from "./remote-caret.ts";
import { looksLikeMarkdown, markdownToSlice } from "./markdown.ts";
import { createNodeViews } from "./node-views.ts";
import { parsePageHref } from "./page-links.ts";
import { pageSchema } from "./schema.ts";
import { stashPlugin } from "./stash.ts";
import { createSuggestionController } from "./suggestion.ts";

const { nodes } = pageSchema;

export interface PageEditor {
  view: EditorView;
  doc: PageDoc;
  /** Re-read titles and states of page links after the tree changed. */
  refreshLinks(): void;
  /** Close an open slash or page menu without choosing anything. */
  dismissSuggestion(): void;
  /**
   * Apply an update from another device. Updates that arrive together are
   * imported as one batch so the editor re-renders once.
   */
  importRemote(update: Uint8Array): void;
  destroy(): void;
}

export interface PageEditorOptions {
  /** An existing document, for example one loaded from the Rust core. */
  doc?: PageDoc;
  /** Called with Loro update bytes for every local edit; the IPC bridge hooks in here. */
  onLocalUpdate?: (update: Uint8Array) => void;
  host?: EditorHost;
}

const hardBreak = chainCommands(exitCode, (state, dispatch) => {
  dispatch?.(state.tr.replaceSelectionWith(nodes.hard_break.create()).scrollIntoView());
  return true;
});

/** Enter on a selected page block opens that page. */
const openSelectedPage =
  (host: EditorHost): Command =>
  (state) => {
    const { selection } = state;
    if (!(selection instanceof NodeSelection)) return false;
    const { type, attrs } = selection.node;
    if (type !== nodes.child_page && type !== nodes.page_link) return false;
    const pageId = String(attrs.pageId ?? "");
    if (!pageId) return false;
    host.openPage?.(pageId, false);
    return true;
  };

function imageFiles(data: DataTransfer | null): File[] {
  return [...(data?.files ?? [])].filter((file) => file.type.startsWith("image/"));
}

function interactionPlugin(host: EditorHost): Plugin {
  /** Store each image and put an image block after the block at `pos` (or the cursor). */
  const insertImages = async (view: EditorView, files: File[], pos?: number): Promise<void> => {
    if (!host.addImage) return;
    for (const file of files) {
      try {
        const src = await host.addImage(file);
        const top = view.state.doc.resolve(Math.min(pos ?? view.state.selection.from, view.state.doc.content.size));
        const at = top.depth === 0 ? top.pos : top.after(1);
        view.dispatch(view.state.tr.insert(at, createBlock("image", { src, alt: file.name })).scrollIntoView());
      } catch (error) {
        host.onError?.(error instanceof Error ? error.message : String(error));
      }
    }
  };

  return new Plugin({
    props: {
      handlePaste(view, event) {
        const files = imageFiles(event.clipboardData);
        if (files.length > 0 && host.addImage) {
          void insertImages(view, files);
          return true;
        }
        const plain = event.clipboardData?.getData("text/plain") ?? "";
        const html = event.clipboardData?.getData("text/html") ?? "";
        const { $from } = view.state.selection;
        if (!html && plain && !$from.parent.type.spec.code && looksLikeMarkdown(plain)) {
          view.dispatch(view.state.tr.replaceSelection(markdownToSlice(plain)).scrollIntoView());
          return true;
        }
        return false;
      },
      handleDrop(view, event) {
        const files = imageFiles(event.dataTransfer);
        if (files.length === 0 || !host.addImage) return false;
        event.preventDefault();
        const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
        void insertImages(view, files, at?.pos);
        return true;
      },
      handleClick(_view, _pos, event) {
        const anchor = (event.target as HTMLElement | null)?.closest?.("a[href]");
        if (!anchor) return false;
        const href = anchor.getAttribute("href") ?? "";
        const pageId = parsePageHref(href);
        const modifier = event.metaKey || event.ctrlKey;
        if (pageId) {
          event.preventDefault();
          if (host.resolvePage?.(pageId)?.state === "ok" || !host.resolvePage) host.openPage?.(pageId, modifier);
          return true;
        }
        if (modifier) {
          event.preventDefault();
          host.openExternal?.(href);
          return true;
        }
        return false;
      },
    },
    view(view) {
      // Mark inline page links whose target is in the trash or gone.
      const decorate = () => {
        for (const anchor of view.dom.querySelectorAll<HTMLAnchorElement>("a.ne-inline-page")) {
          const pageId = parsePageHref(anchor.getAttribute("href") ?? "");
          const state = pageId ? (host.resolvePage?.(pageId)?.state ?? "ok") : "missing";
          if (anchor.dataset.state !== state) anchor.dataset.state = state;
        }
      };
      decorate();
      return { update: decorate };
    },
  });
}

export function createPageEditor(mount: HTMLElement, options: PageEditorOptions = {}): PageEditor {
  const host = options.host ?? {};
  const doc = options.doc ?? createPageDoc();
  const unsubscribe = options.onLocalUpdate ? doc.subscribeLocalUpdates(options.onLocalUpdate) : undefined;
  const nodeViews = createNodeViews(host);
  const suggestions = createSuggestionController(host);
  const remoteCaret = createRemoteCaret();

  const editingKeymap = keymap({
    "Mod-b": toggleBold,
    "Mod-i": toggleItalic,
    "Mod-e": toggleCode,
    "Mod-Shift-s": toggleStrike,
    "Mod-/": openBlockMenu(host),
    "Mod-Shift-k": () => host.onLinkShortcut?.() ?? false,
    "Shift-Enter": hardBreak,
    "Shift-ArrowDown": extendBlockSelection(1),
    "Shift-ArrowUp": extendBlockSelection(-1),
    Escape: selectCurrentBlock,
    Enter: chainCommands(openSelectedPage(host), splitListItem(nodes.list_item), splitListItem(nodes.todo_item)),
    Tab: chainCommands(sinkListItem(nodes.list_item), sinkListItem(nodes.todo_item)),
    "Shift-Tab": chainCommands(liftListItem(nodes.list_item), liftListItem(nodes.todo_item)),
  });

  const view = new EditorView(mount, {
    state: EditorState.create({
      schema: pageSchema,
      // Binding plugins first so its undo keys win over the base keymap; the
      // suggestion plugin sits before the keymaps so an open menu gets the arrows.
      plugins: [
        ...bindingPlugins(doc),
        remoteCaret.plugin,
        suggestions.plugin,
        pageInputRules(),
        stashPlugin(),
        findPlugin(),
        blockSelectionPlugin(),
        blockHandlePlugin(host),
        interactionPlugin(host),
        editingKeymap,
        keymap(baseKeymap),
        new Plugin({
          view: () => ({
            update: (updated, previous) => host.onUpdate?.(updated, previous),
          }),
        }),
      ],
    }),
    nodeViews: nodeViews.views,
    attributes: {
      role: "textbox",
      "aria-multiline": "true",
      "aria-label": host.labels?.editorLabel ?? DEFAULT_LABELS.editorLabel,
    },
  });

  let pending: Uint8Array[] = [];
  let scheduled = false;
  const flushRemote = () => {
    scheduled = false;
    const batch = pending;
    pending = [];
    if (batch.length === 0) return;
    remoteCaret.begin(view);
    doc.importBatch(batch);
  };

  return {
    view,
    doc,
    refreshLinks() {
      nodeViews.refresh();
      view.dispatch(view.state.tr.setMeta("ne-refresh", true));
    },
    dismissSuggestion: () => suggestions.dismiss(),
    importRemote(update) {
      pending.push(update);
      if (!scheduled) {
        scheduled = true;
        queueMicrotask(flushRemote);
      }
    },
    destroy() {
      unsubscribe?.();
      view.destroy();
    },
  };
}

