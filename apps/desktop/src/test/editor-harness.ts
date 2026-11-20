import { TextSelection } from "prosemirror-state";
import { createPageDoc, type PageDoc } from "../editor/binding.ts";
import { createPageEditor, type EditorHost, type PageEditor } from "../editor/index.ts";

/** Wait for the sync binding's deferred work (selection restore, queued remote updates). */
export const settle = (ms = 20): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

type View = PageEditor["view"];

/** Type text the way the browser does: through input rules first, then as plain input. */
export function typeInto(view: View, text: string): void {
  for (const char of text) {
    const { from, to } = view.state.selection;
    const handled = view.someProp("handleTextInput", (f) => f(view, from, to, char, () => view.state.tr.insertText(char, from, to)));
    if (!handled) view.dispatch(view.state.tr.insertText(char, from, to));
  }
}

export function pressKey(view: View, key: string, init: KeyboardEventInit = {}): boolean {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  return Boolean(view.someProp("handleKeyDown", (f) => f(view, event)));
}

/** Replace the document with one paragraph and put the caret inside it. */
export function setParagraph(view: View, text: string, caret = text.length): void {
  const { schema } = view.state;
  const paragraph = schema.node("paragraph", null, text ? schema.text(text) : undefined);
  view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, paragraph));
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 1 + caret)));
}

export interface TestEditor {
  editor: PageEditor;
  view: PageEditor["view"];
  doc: PageDoc;
  updates: Uint8Array[];
  type(text: string): void;
  press(key: string, init?: KeyboardEventInit): boolean;
  /** Replace the whole document with one paragraph holding `text` and put the caret at `caret`. */
  setText(text: string, caret?: number): void;
  topKinds(): string[];
  destroy(): void;
}

export async function createTestEditor(options: { host?: EditorHost; doc?: PageDoc } = {}): Promise<TestEditor> {
  const mount = document.createElement("div");
  document.body.appendChild(mount);
  const updates: Uint8Array[] = [];
  const doc = options.doc ?? createPageDoc();
  const editor = createPageEditor(mount, { doc, host: options.host, onLocalUpdate: (update) => updates.push(update) });
  await settle();
  const { view } = editor;

  const type = (text: string) => typeInto(view, text);

  return {
    editor,
    view,
    doc,
    updates,
    type,
    press: (key, init) => pressKey(view, key, init),
    setText: (text, caret) => setParagraph(view, text, caret),
    topKinds() {
      const kinds: string[] = [];
      view.state.doc.forEach((node) => kinds.push(node.type.name));
      return kinds;
    },
    destroy() {
      editor.destroy();
      mount.remove();
    },
  };
}
