
import { baseKeymap, chainCommands, exitCode } from "prosemirror-commands";
import { keymap } from "prosemirror-keymap";
import { liftListItem, sinkListItem, splitListItem } from "prosemirror-schema-list";
import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { bindingPlugins, createPageDoc, type PageDoc } from "./binding.ts";
import { toggleBold, toggleCode, toggleItalic, toggleStrike } from "./commands.ts";
import { pageInputRules } from "./input-rules.ts";
import { todoItemView, toggleView } from "./node-views.ts";
import { pageSchema } from "./schema.ts";

const { nodes } = pageSchema;

export interface PageEditor {
  view: EditorView;
  doc: PageDoc;
  destroy(): void;
}

export interface PageEditorOptions {
  /** An existing document, for example one loaded from the Rust core. */
  doc?: PageDoc;
  /** Called with Loro update bytes for every local edit; the IPC bridge hooks in here. */
  onLocalUpdate?: (update: Uint8Array) => void;
}

const hardBreak = chainCommands(exitCode, (state, dispatch) => {
  dispatch?.(state.tr.replaceSelectionWith(nodes.hard_break.create()).scrollIntoView());
  return true;
});

const editingKeymap = keymap({
  "Mod-b": toggleBold,
  "Mod-i": toggleItalic,
  "Mod-e": toggleCode,
  "Mod-Shift-s": toggleStrike,
  "Shift-Enter": hardBreak,
  Enter: chainCommands(splitListItem(nodes.list_item), splitListItem(nodes.todo_item)),
  Tab: chainCommands(sinkListItem(nodes.list_item), sinkListItem(nodes.todo_item)),
  "Shift-Tab": chainCommands(liftListItem(nodes.list_item), liftListItem(nodes.todo_item)),
});

export function createPageEditor(mount: HTMLElement, options: PageEditorOptions = {}): PageEditor {
  const doc = options.doc ?? createPageDoc();
  const unsubscribe = options.onLocalUpdate ? doc.subscribeLocalUpdates(options.onLocalUpdate) : undefined;
  const view = new EditorView(mount, {
    state: EditorState.create({
      schema: pageSchema,
      // Binding plugins first so its undo keys win over the base keymap.
      plugins: [...bindingPlugins(doc), pageInputRules(), editingKeymap, keymap(baseKeymap)],
    }),
    nodeViews: { todo_item: todoItemView, toggle: toggleView },
  });
  return {
    view,
    doc,
    destroy() {
      unsubscribe?.();
      view.destroy();
    },
  };
}
