// Builds a page document containing one of every block kind through the real
// schema and binding, then writes a snapshot and an incremental update for the
// Rust test in crates/ne-core/tests/page_doc_fixture.rs.
//
//   node spikes/page-doc-fixture.mjs
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { LoroDoc } from "loro-crdt";
import { createHeadlessEditor, pageSchema as s, pmJson, settle } from "./headless-editor.mjs";

const fixtureDir = new URL("../../../crates/ne-core/tests/fixtures/", import.meta.url);

const text = (value, ...marks) => s.text(value, marks.map((m) => (typeof m === "string" ? s.mark(m) : m)));
const para = (...inline) => s.node("paragraph", null, inline);
const item = (...children) => s.node("list_item", null, children);

const content = [
  s.node("heading", { level: 1 }, [text("Tiêu đề trang")]),
  s.node("heading", { level: 2 }, [text("Mục lớn: Heading two")]),
  s.node("heading", { level: 3 }, [text("Mục nhỏ")]),
  para(
    text("Xin chào, "),
    text("thế giới", "bold"),
    text(" and "),
    text("English", "italic"),
    text(" "),
    text("code()", "code"),
    text(" "),
    text("bỏ đi", "strike"),
    text(" "),
    text("liên kết", s.mark("link", { href: "https://example.org/vi" })),
    text(" "),
    text("đậm nghiêng", "bold", "italic"),
  ),
  para(text("Dòng một"), s.node("hard_break"), text("Dòng hai")),
  s.node("bullet_list", null, [
    item(para(text("Mục một")), s.node("bullet_list", null, [item(para(text("Mục con ở trong")))])),
    item(para(text("Mục hai"))),
  ]),
  s.node("ordered_list", null, [item(para(text("Bước một"))), item(para(text("Step two")))]),
  s.node("todo_list", null, [
    s.node("todo_item", { checked: true }, [para(text("Đã xong"))]),
    s.node("todo_item", { checked: false }, [para(text("Chưa xong"))]),
  ]),
  s.node("toggle", { open: false }, [para(text("Toggle tóm tắt")), para(text("Nội dung ẩn bên trong"))]),
  s.node("quote", null, [para(text("Trích dẫn: người Việt nói “có công mài sắt”"))]),
  s.node("callout", { icon: "💡" }, [para(text("Lưu ý quan trọng / important note"))]),
  s.node("divider"),
  s.node("code_block", { language: "rust" }, [text('fn main() {\n    println!("Xin chào");\n}')]),
  s.node("image", { src: "assets/so-do.png", alt: "Sơ đồ kiến trúc" }),
  s.node("child_page", { pageId: "page-child-0001", title: "Trang con" }),
  s.node("page_link", { pageId: "page-target-0002", title: "Trang được liên kết" }),
  para(),
];

const doc = new LoroDoc();
// Fixed peer id keeps the fixture bytes reproducible between runs.
doc.setPeerId(1001n);
const view = await createHeadlessEditor(doc);
view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, content));
await settle();

const snapshot = doc.export({ mode: "snapshot" });
const versionAfterSnapshot = doc.version();

// Later edit: type at the end of the H1 and tick the second to-do.
const h1 = view.state.doc.firstChild;
view.dispatch(view.state.tr.insertText(" (đã sửa)", 1 + h1.content.size));
let todoPos = -1;
view.state.doc.descendants((node, pos) => {
  if (node.type.name === "todo_item" && !node.attrs.checked) todoPos = pos;
});
assert.ok(todoPos >= 0, "unchecked to-do not found");
view.dispatch(view.state.tr.setNodeAttribute(todoPos, "checked", true));
await settle();

const update = doc.export({ mode: "update", from: versionAfterSnapshot });
assert.ok(update.length > 0 && update.length < snapshot.length, "update must be incremental");

// Self-check: a fresh doc built from snapshot + update renders the same editor state.
const replica = new LoroDoc();
replica.import(snapshot);
replica.import(update);
const replicaView = await createHeadlessEditor(replica);
assert.deepEqual(pmJson(replicaView), pmJson(view), "replica editor state differs from the source");

mkdirSync(fixtureDir, { recursive: true });
writeFileSync(new URL("page-doc-snapshot.loro", fixtureDir), snapshot);
writeFileSync(new URL("page-doc-update.loro", fixtureDir), update);
console.log(`wrote ${fileURLToPath(fixtureDir)}`);
console.log(`snapshot ${snapshot.length} bytes, update ${update.length} bytes`);
process.exit(0);
