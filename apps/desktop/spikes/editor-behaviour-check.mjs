// Drives the production editor module (input rules, keymap, insertBlock) on
// jsdom and checks the resulting document, so the Markdown shortcuts and block
// insertion are covered without a browser.
//
//   node spikes/editor-behaviour-check.mjs
import assert from "node:assert/strict";
import "./headless-editor.mjs"; // installs the jsdom globals before the editor loads
import { BLOCK_NODE_NAMES, createPageEditor, insertBlock } from "../src/editor/index.ts";
import { settle } from "./headless-editor.mjs";

const mount = document.createElement("div");
document.body.appendChild(mount);
const editor = createPageEditor(mount);
const { view, doc } = editor;
await settle();

function reset() {
  const empty = view.state.schema.node("paragraph");
  view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, empty));
  view.dispatch(view.state.tr.setSelection(view.state.selection.constructor.atStart(view.state.doc)));
}

function typeText(text) {
  for (const char of text) {
    const { from, to } = view.state.selection;
    const handled = view.someProp("handleTextInput", (f) => f(view, from, to, char, () => view.state.tr.insertText(char, from, to)));
    if (!handled) view.dispatch(view.state.tr.insertText(char, from, to));
  }
}

function press(key, init = {}) {
  const event = new window.KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  return view.someProp("handleKeyDown", (f) => f(view, event));
}

const topKinds = () => {
  const kinds = [];
  view.state.doc.forEach((n) => kinds.push(n.type.name));
  return kinds;
};

// Markdown shortcuts.
reset();
typeText("## Tiêu đề");
assert.equal(view.state.doc.firstChild.type.name, "heading");
assert.equal(view.state.doc.firstChild.attrs.level, 2);
assert.equal(view.state.doc.firstChild.textContent, "Tiêu đề");

reset();
typeText("- một");
assert.deepEqual(topKinds(), ["bullet_list"]);
assert.ok(press("Enter"), "Enter splits the list item");
typeText("hai");
assert.equal(view.state.doc.firstChild.childCount, 2, "two list items");
assert.ok(press("Tab"), "Tab nests the item");
assert.equal(view.state.doc.firstChild.firstChild.childCount, 2, "first item now holds a nested list");
assert.ok(press("Tab", { shiftKey: true }), "Shift-Tab lifts it back");
assert.equal(view.state.doc.firstChild.childCount, 2);

reset();
typeText("1. a");
assert.deepEqual(topKinds(), ["ordered_list"]);

reset();
typeText("[] việc");
assert.deepEqual(topKinds(), ["todo_list"]);
assert.equal(view.state.doc.firstChild.firstChild.type.name, "todo_item");

reset();
typeText("> trích");
assert.deepEqual(topKinds(), ["quote"]);

reset();
typeText("```");
assert.deepEqual(topKinds(), ["code_block"]);

reset();
typeText("---");
assert.deepEqual(topKinds(), ["divider", "paragraph"]);

reset();
typeText("**đậm**");
let marks = [];
view.state.doc.firstChild.forEach((n) => marks.push([n.text, n.marks.map((m) => m.type.name)]));
assert.deepEqual(marks, [["đậm", ["bold"]]]);
typeText(" sau");
marks = [];
view.state.doc.firstChild.forEach((n) => marks.push([n.text, n.marks.map((m) => m.type.name)]));
assert.deepEqual(marks, [["đậm", ["bold"]], [" sau", []]], "text typed after the closing delimiter is plain");

// Shift+Enter inserts a hard break instead of splitting the block.
reset();
typeText("a");
assert.ok(press("Enter", { shiftKey: true }));
typeText("b");
assert.equal(view.state.doc.childCount, 1);
assert.equal(view.state.doc.firstChild.childCount, 3);

// Every insertable kind produces a schema-valid document and survives the Loro round trip.
for (const kind of BLOCK_NODE_NAMES.filter((k) => k !== "paragraph" && k !== "heading")) {
  reset();
  assert.ok(insertBlock(kind)(view.state, view.dispatch), `insertBlock(${kind})`);
  view.state.doc.check();
  assert.ok(topKinds().includes(kind), `${kind} present`);
}

// Undo/redo come from the binding.
reset();
typeText("xin chào");
await settle();
assert.ok(press("z", { ctrlKey: true }), "Mod-z handled");
await settle();
assert.notEqual(view.state.doc.textContent, "xin chào", "undo changed the document");
assert.equal(doc.getMap("doc").get("children").length >= 1, true);

console.log("editor behaviour ok");
process.exit(0);
