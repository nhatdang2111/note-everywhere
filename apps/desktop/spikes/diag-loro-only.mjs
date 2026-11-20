// Diagnostic: the same edits applied to the Loro document directly, without
// ProseMirror or the binding. Stays flat (~0.03 ms), so the cost measured in
// typing-latency.mjs is in the binding's JS-side tree comparison, not in Loro.
import { performance } from "node:perf_hooks";
import { LoroDoc } from "loro-crdt";
import { createHeadlessEditor, pageSchema as s, settle } from "./headless-editor.mjs";

const BLOCKS = 5000;
const base = new LoroDoc();
const bv = await createHeadlessEditor(base);
bv.dispatch(
  bv.state.tr.replaceWith(
    0,
    bv.state.doc.content.size,
    Array.from({ length: BLOCKS }, (_, i) => s.node("paragraph", null, [s.text(`khối số ${i} ghi chú ngày hôm nay`)])),
  ),
);
await settle();
const doc = new LoroDoc();
doc.import(base.export({ mode: "snapshot" }));
if (process.env.NE_DIAG_SUB) doc.subscribe(() => {});
const list = doc.getMap("doc").get("children");
const buckets = [];
let acc = 0;
for (let i = 0; i < 1000; i++) {
  const t0 = performance.now();
  const node = list.get((i * 7919) % BLOCKS);
  const text = node.get("children").get(0);
  text.insert(2, "a");
  doc.commit({ origin: "loroSyncPlugin" });
  acc += performance.now() - t0;
  if (i % 100 === 99) {
    buckets.push((acc / 100).toFixed(2));
    acc = 0;
  }
}
console.log(`loro only (no ProseMirror), ${BLOCKS} blocks: mean ms per 100 edits: ${buckets.join(" ")}`);
process.exit(0);
