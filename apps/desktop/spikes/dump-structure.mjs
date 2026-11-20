// Prints the Loro container layout the binding produces and probes three
// assumptions the page-document ADR relies on:
//   1. a natively written node without an `attributes` map makes the editor write;
//   2. a second root container next to `doc` is left alone;
//   3. loro-crdt (JS) counts text indices in UTF-16 units.
//
//   node spikes/dump-structure.mjs
import assert from "node:assert/strict";
import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import { createHeadlessEditor, pageSchema as s, settle } from "./headless-editor.mjs";

const t = (text, ...marks) => s.text(text, marks.map((m) => s.mark(m)));

// --- Layout ---------------------------------------------------------------
const doc = new LoroDoc();
const view = await createHeadlessEditor(doc);
view.dispatch(
  view.state.tr.replaceWith(0, view.state.doc.content.size, [
    s.node("heading", { level: 1 }, [t("Tiêu đề")]),
    s.node("paragraph", null, [t("Xin "), t("chào", "bold"), s.node("hard_break"), t("sau")]),
    s.node("todo_list", null, [s.node("todo_item", null, [s.node("paragraph", null, [t("việc")])])]),
    s.node("paragraph"),
  ]),
);
await settle();
console.log("root container id:", doc.getMap("doc").id);
console.log(JSON.stringify(doc.toJSON(), null, 1));
const richText = doc.getMap("doc").get("children").get(1).get("children").get(0);
console.log("text delta:", JSON.stringify(richText.toDelta()));

// --- 1. Missing `attributes` ------------------------------------------------
const native = new LoroDoc();
const root = native.getMap("doc");
root.set("nodeName", "doc");
root.setContainer("attributes", new LoroMap());
const children = root.setContainer("children", new LoroList());
const node = children.insertContainer(0, new LoroMap());
node.set("nodeName", "paragraph");
node.setContainer("children", new LoroList()).insertContainer(0, new LoroText()).insert(0, "không có attributes");
native.commit();
const before = JSON.stringify(native.frontiers());
const nativeView = await createHeadlessEditor(native);
assert.equal(nativeView.state.doc.textContent, "không có attributes");
console.log("missing attributes -> editor wrote to the doc:", JSON.stringify(native.frontiers()) !== before);

// --- 2. Extra root container ----------------------------------------------
doc.getMap("meta").set("title", "ngoài binding");
doc.commit();
await settle();
view.dispatch(view.state.tr.insertText("!", 2));
await settle();
assert.equal(doc.getMap("meta").get("title"), "ngoài binding");
assert.equal(view.state.doc.firstChild.textContent, "T!iêu đề");
console.log("extra root container: untouched, editor still works");

// --- 3. UTF-16 indices -----------------------------------------------------
const probe = new LoroDoc();
const text = probe.getText("t");
text.insert(0, "😀a"); // U+1F600 is two UTF-16 units
text.insert(2, "X");
console.log("JS insert at index 2 after an emoji ->", JSON.stringify(text.toString()), "(UTF-16 units)");
process.exit(0);
