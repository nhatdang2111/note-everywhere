// Proves that an update written natively by the Rust core renders in the
// ProseMirror editor, both on a fresh mount and as a live remote update.
//
//   cargo test -p ne-core --test page_doc_fixture   # writes the Rust update
//   node spikes/roundtrip-check.mjs                 # imports it on the JS side
//
// The update path defaults to <tmpdir>/ne-page-doc-rust-update.loro and can be
// overridden with NE_PAGE_DOC_RUST_UPDATE (the Rust test honours the same variable).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LoroDoc } from "loro-crdt";
import { redo, undo } from "loro-prosemirror";
import { createHeadlessEditor, pmJson, settle } from "./headless-editor.mjs";

const fixtureDir = new URL("../../../crates/ne-core/tests/fixtures/", import.meta.url);
const rustUpdatePath =
  process.env.NE_PAGE_DOC_RUST_UPDATE ?? join(tmpdir(), "ne-page-doc-rust-update.loro");

const read = (url) => new Uint8Array(readFileSync(url));
const snapshot = read(new URL("page-doc-snapshot.loro", fixtureDir));
const editorUpdate = read(new URL("page-doc-update.loro", fixtureDir));
let rustUpdate;
try {
  rustUpdate = new Uint8Array(readFileSync(rustUpdatePath));
} catch (error) {
  console.error(`Cannot read ${rustUpdatePath}: run the Rust test first (see header).`);
  throw error;
}

const textOf = (node) => node.textContent;

function assertRustChanges(view, label) {
  const top = [];
  view.state.doc.forEach((node) => top.push(node));

  const bullet = top.find((n) => n.type.name === "bullet_list");
  assert.equal(
    textOf(bullet.child(1)),
    "Mục hai (sửa từ Rust)",
    `${label}: text inserted natively into a list item`,
  );

  const last = top[top.length - 1];
  assert.equal(last.type.name, "paragraph", `${label}: appended block kind`);
  assert.equal(textOf(last), "Thêm từ Rust: in đậm", `${label}: appended text`);
  const runs = [];
  last.forEach((child) => runs.push([child.text, child.marks.map((m) => m.type.name)]));
  assert.deepEqual(
    runs,
    [
      ["Thêm từ Rust: ", []],
      ["in đậm", ["bold"]],
    ],
    `${label}: appended marks`,
  );

  // Edits from the editor side must still be there.
  assert.ok(textOf(top[0]).startsWith("Tiêu đề trang (đã sửa)"), `${label}: editor edit survived`);
}

// A. Fresh mount from snapshot + editor update + Rust update.
const fresh = new LoroDoc();
fresh.import(snapshot);
fresh.import(editorUpdate);
fresh.import(rustUpdate);
const versionBeforeMount = JSON.stringify(fresh.frontiers());
const freshView = await createHeadlessEditor(fresh);
assertRustChanges(freshView, "fresh mount");
assert.equal(
  JSON.stringify(fresh.frontiers()),
  versionBeforeMount,
  "mounting the editor must not write to a natively written document",
);

// B. Editor already open, Rust update arrives as a remote update.
const live = new LoroDoc();
live.import(snapshot);
live.import(editorUpdate);
const liveView = await createHeadlessEditor(live);
live.import(rustUpdate);
await settle();
assertRustChanges(liveView, "live import");

assert.deepEqual(pmJson(liveView), pmJson(freshView), "live and fresh renderings must match");

// C. Undo reverts only this device's own edit and never the imported Rust changes.
const headingEnd = 1 + liveView.state.doc.firstChild.content.size;
liveView.dispatch(liveView.state.tr.insertText(" ZZ", headingEnd));
await settle();
assert.equal(textOf(liveView.state.doc.firstChild), "Tiêu đề trang (đã sửa) ZZ");
assert.ok(undo(liveView.state, liveView.dispatch), "undo command must run");
await settle();
assert.equal(textOf(liveView.state.doc.firstChild), "Tiêu đề trang (đã sửa)", "undo removed the local edit");
assertRustChanges(liveView, "after undo");
assert.ok(redo(liveView.state, liveView.dispatch), "redo command must run");
await settle();
assert.equal(textOf(liveView.state.doc.firstChild), "Tiêu đề trang (đã sửa) ZZ", "redo restored the local edit");
assertRustChanges(liveView, "after redo");

console.log("roundtrip ok: Rust-made changes render on mount and as live updates; undo/redo only touches local edits");
process.exit(0);
