// Diagnostic: does per-edit latency drift as edits accumulate on a large page?
// Finding: yes when the loop never yields to the event loop (NE_DIAG_YIELD unset),
// no when it does (NE_DIAG_YIELD=1). Cause is garbage collection of wasm-bindgen
// handles whose FinalizationRegistry callbacks never get to run, not the binding
// logic or Loro itself (see diag-loro-only.mjs).
//   NE_BENCH_BLOCKS=5000 NE_DIAG_EDITS=600 NE_DIAG_MODE=no-undo [NE_DIAG_YIELD=1] node spikes/diag-drift.mjs
import { performance } from "node:perf_hooks";
import { LoroDoc } from "loro-crdt";
import { LoroSyncPlugin, LoroUndoPlugin } from "loro-prosemirror";
import { createHeadlessEditor, pageSchema as s, settle } from "./headless-editor.mjs";
import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";

const BLOCKS = Number(process.env.NE_BENCH_BLOCKS ?? 2000);
const EDITS = Number(process.env.NE_DIAG_EDITS ?? 400);
const mode = process.env.NE_DIAG_MODE ?? "full"; // full | no-undo | paced

const base = new LoroDoc();
const bv = await createHeadlessEditor(base);
const blocks = Array.from({ length: BLOCKS }, (_, i) => s.node("paragraph", null, [s.text(`khối số ${i} ghi chú ngày hôm nay`)]));
bv.dispatch(bv.state.tr.replaceWith(0, bv.state.doc.content.size, blocks));
await settle();
const snap = base.export({ mode: "snapshot" });

const doc = new LoroDoc();
doc.import(snap);
const plugins = [LoroSyncPlugin({ doc })];
if (mode !== "no-undo") plugins.push(LoroUndoPlugin({ doc }));
const mount = document.createElement("div");
document.body.appendChild(mount);
const view = new EditorView(mount, { state: EditorState.create({ schema: s, plugins }) });
await settle();

const buckets = [];
let acc = 0;
let accApply = 0;
const bucketsApply = [];
for (let i = 0; i < EDITS; i++) {
  const target = process.env.NE_DIAG_FIXED ? 2500 : (i * 7919) % BLOCKS;
  let pos = 0;
  for (let k = 0; k < target; k++) pos += view.state.doc.child(k).nodeSize;
  const tr = view.state.tr.insertText("a", pos + 2);
  const t0 = performance.now();
  const next = view.state.apply(tr);
  const t1 = performance.now();
  view.updateState(next);
  const t2 = performance.now();
  acc += t2 - t0;
  accApply += t1 - t0;
  if (i % 100 === 99) {
    buckets.push((acc / 100).toFixed(1));
    bucketsApply.push((accApply / 100).toFixed(1));
    acc = 0;
    accApply = 0;
    if (process.env.NE_DIAG_GC && globalThis.gc) globalThis.gc();
  }
  if (mode === "paced" && i % 20 === 19) await new Promise((r) => setTimeout(r, 1100));
  if (process.env.NE_DIAG_YIELD) await new Promise((r) => setTimeout(r, Number(process.env.NE_DIAG_YIELD)));
  void t1;
}
console.log(`${mode} blocks=${BLOCKS}: mean ms per 100 edits (apply+updateState): ${buckets.join(" ")}\n  apply only: ${bucketsApply.join(" ")}`);
process.exit(0);
