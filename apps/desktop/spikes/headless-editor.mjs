// Shared headless harness: a real EditorView on jsdom, bound to a LoroDoc
// through the production schema and binding plugins. DOM paint is jsdom's, not
// a browser's; only the model/binding path is representative.
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><body><div id='editor'></div></body>", {
  pretendToBeVisual: true,
});
for (const key of ["window", "document", "navigator", "Node", "HTMLElement", "MutationObserver"]) {
  Object.defineProperty(globalThis, key, { value: dom.window[key], configurable: true, writable: true });
}
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
globalThis.requestAnimationFrame = (cb) => setTimeout(cb, 0);
globalThis.cancelAnimationFrame = (id) => clearTimeout(id);

const { EditorState } = await import("prosemirror-state");
const { EditorView } = await import("prosemirror-view");
const { pageSchema } = await import("../src/editor/schema.ts");
const { bindingPlugins } = await import("../src/editor/binding.ts");

export { pageSchema };

/** Wait for the sync plugin's deferred init and any queued remote updates. */
export const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

export async function createHeadlessEditor(loroDoc) {
  const mount = dom.window.document.createElement("div");
  dom.window.document.body.appendChild(mount);
  const view = new EditorView(mount, {
    state: EditorState.create({ schema: pageSchema, plugins: bindingPlugins(loroDoc) }),
  });
  await settle();
  return view;
}

/** Document node as plain JSON, used to compare editor states. */
export const pmJson = (view) => view.state.doc.toJSON();
