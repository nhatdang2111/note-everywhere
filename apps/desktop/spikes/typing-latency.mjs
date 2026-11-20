// Latency benchmark for the ProseMirror <-> Loro binding on a 5,000-block page.
//
//   node spikes/typing-latency.mjs
//
// Measured per single-character transaction at a random position (1,000 samples
// after a warm-up):
//   local  : state.apply(tr) through LoroSyncPlugin (PM transaction -> Loro ops ->
//            commit) plus doc.export(update). This is the cost the binding adds.
//            Keystrokes are paced by NE_BENCH_PACE_MS (default 10 ms).
//   dom    : view.updateState(newState) on jsdom, reported separately.
//   remote : doc.import(update from another peer) until the editor state shows the
//            change, and until the binding's deferred work has run (Loro event ->
//            binding -> PM transaction -> jsdom DOM sync -> selection restore).
//            The idle figure is the one judged against the budget.
//
// Not included: browser layout and paint, IPC transfer, and WebKit's JavaScript
// engine (Tauri on macOS runs JavaScriptCore; this runs V8 in Node).
import { performance } from "node:perf_hooks";
import os from "node:os";
import { LoroDoc } from "loro-crdt";
import { createHeadlessEditor, pageSchema as s, settle } from "./headless-editor.mjs";

const BLOCKS = Number(process.env.NE_BENCH_BLOCKS ?? 5000);
const SAMPLES = Number(process.env.NE_BENCH_SAMPLES ?? 1000);
const WARMUP = Math.min(100, Math.floor(SAMPLES / 10));
const BUDGET_MS = 16;
// Gap between simulated keystrokes. The loop must yield to the event loop: the
// WASM binding allocates ~one handle per block per edit and relies on
// FinalizationRegistry callbacks to free them. A tight synchronous loop starves
// those callbacks and shows a latency drift that real typing never produces
// (see spikes/diag-drift.mjs). 10 ms is already faster than any human.
const PACE_MS = Number(process.env.NE_BENCH_PACE_MS ?? 10);
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Deterministic PRNG so runs differ only by machine noise.
function mulberry32(seed) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = mulberry32(20261121);
const pick = (n) => Math.floor(rand() * n);

const WORDS = "ghi chú ngày hôm nay họp dự án kế hoạch note everywhere sync offline đồng bộ tìm kiếm trang liên kết".split(" ");
const sentence = (min, max) => {
  const n = min + pick(max - min + 1);
  return Array.from({ length: n }, () => WORDS[pick(WORDS.length)]).join(" ");
};
const para = () => {
  const body = sentence(6, 16);
  const cut = Math.max(1, Math.floor(body.length / 3));
  const withBold = pick(4) === 0;
  const inline = withBold
    ? [s.text(body.slice(0, cut)), s.text(body.slice(cut, cut * 2), [s.mark("bold")]), s.text(body.slice(cut * 2))]
    : [s.text(body)];
  return s.node("paragraph", null, inline);
};
const item = () => s.node("list_item", null, [para()]);

function makeBlock() {
  const r = rand();
  if (r < 0.7) return para();
  if (r < 0.8) return s.node("heading", { level: 1 + pick(3) }, [s.text(sentence(2, 6))]);
  if (r < 0.9) return s.node("bullet_list", null, [item(), item()]);
  if (r < 0.95) return s.node("todo_list", null, [s.node("todo_item", { checked: pick(2) === 0 }, [para()])]);
  return s.node("quote", null, [para()]);
}

const stats = (samples) => {
  const sorted = [...samples].sort((x, y) => x - y);
  const at = (p) => sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)];
  const mean = samples.reduce((x, y) => x + y, 0) / samples.length;
  return { n: samples.length, p50: at(50), p95: at(95), p99: at(99), max: sorted.at(-1), mean };
};
const fmt = (n) => n.toFixed(3).padStart(8);
const row = (label, st) =>
  console.log(`${label.padEnd(30)} p50 ${fmt(st.p50)}  p95 ${fmt(st.p95)}  p99 ${fmt(st.p99)}  max ${fmt(st.max)}  mean ${fmt(st.mean)}  (ms, n=${st.n})`);

/** Mean per bucket of 100 samples, to expose latency drift as the edit history grows. */
const trend = (samples) => {
  const out = [];
  for (let i = 0; i < samples.length; i += 100) {
    const bucket = samples.slice(i, i + 100);
    out.push((bucket.reduce((x, y) => x + y, 0) / bucket.length).toFixed(2));
  }
  return out.join(" ");
};

const CHARS = "abcdeăâđêôơưáàảãạ ";
const randomChar = () => CHARS[pick(CHARS.length)];

/** Offsets of the top-level textual blocks, recomputed per sample (outside timing). */
function randomTextPos(doc) {
  const target = pick(doc.childCount);
  let pos = 0;
  for (let i = 0; i < target; i++) pos += doc.child(i).nodeSize;
  // Descend to the first textblock inside the chosen block.
  let node = doc.child(target);
  let inner = pos + 1;
  while (!node.isTextblock) {
    node = node.firstChild;
    inner += 1;
  }
  return inner + pick(node.content.size + 1);
}

// ---- Build the 5,000-block page and a shared base snapshot -----------------
const tBuild = performance.now();
const baseDoc = new LoroDoc();
baseDoc.setPeerId(1n);
const baseView = await createHeadlessEditor(baseDoc);
const blocks = Array.from({ length: BLOCKS }, makeBlock);
baseView.dispatch(baseView.state.tr.replaceWith(0, baseView.state.doc.content.size, blocks));
await settle();
const buildMs = performance.now() - tBuild;
const baseSnapshot = baseDoc.export({ mode: "snapshot" });
let nodeCount = 0;
baseView.state.doc.descendants(() => {
  nodeCount += 1;
});
console.log(`machine: ${os.cpus()[0].model}, ${os.cpus().length} cores, ${Math.round(os.totalmem() / 2 ** 30)} GiB, node ${process.version} ${process.platform}/${process.arch}`);
console.log(`document: ${baseView.state.doc.childCount} top-level blocks, ${nodeCount} nodes, snapshot ${(baseSnapshot.length / 1024).toFixed(0)} KiB, built in ${buildMs.toFixed(0)} ms`);
baseView.destroy();

async function mountReplica(peer) {
  const doc = new LoroDoc();
  doc.setPeerId(peer);
  doc.import(baseSnapshot);
  const t = performance.now();
  const view = await createHeadlessEditor(doc);
  return { doc, view, mountMs: performance.now() - t };
}

// ---- Local typing: PM transaction -> Loro update bytes ---------------------
const typist = await mountReplica(2n);
console.log(`editor mount from snapshot: ${typist.mountMs.toFixed(0)} ms`);
const local = [];
const dom = [];
let updateBytes = 0;
for (let i = 0; i < WARMUP + SAMPLES; i++) {
  const { view, doc } = typist;
  const tr = view.state.tr.insertText(randomChar(), randomTextPos(view.state.doc));
  const before = doc.version();
  const t0 = performance.now();
  const next = view.state.apply(tr);
  const bytes = doc.export({ mode: "update", from: before });
  const t1 = performance.now();
  view.updateState(next);
  const t2 = performance.now();
  if (i % 100 === 99) console.error(`local ${i + 1}/${WARMUP + SAMPLES} ${((t1 - t0)).toFixed(1)} ms`);
  if (i >= WARMUP) {
    local.push(t1 - t0);
    dom.push(t2 - t1);
    updateBytes += bytes.length;
  }
  await pause(PACE_MS);
}
await settle();

// ---- Remote apply: update from another peer -> editor state ----------------
const author = await mountReplica(3n);
const receiver = await mountReplica(4n);
const remoteUpdates = [];
for (let i = 0; i < WARMUP + SAMPLES; i++) {
  const { view, doc } = author;
  const pos = randomTextPos(view.state.doc);
  const before = doc.version();
  const marker = randomChar();
  view.dispatch(view.state.tr.insertText(marker, pos));
  remoteUpdates.push(doc.export({ mode: "update", from: before }));
  if (i % 100 === 99) console.error(`author ${i + 1}/${WARMUP + SAMPLES}`);
  await pause(1);
}
const remote = [];
const remoteIdle = [];
let remoteLate = 0;
for (let i = 0; i < remoteUpdates.length; i++) {
  const { view, doc } = receiver;
  const sizeBefore = view.state.doc.content.size;
  const t0 = performance.now();
  doc.import(remoteUpdates[i]);
  // Loro delivers events in a microtask; spin on microtasks until the state changes.
  let spins = 0;
  while (view.state.doc.content.size === sizeBefore && spins < 1000) {
    await Promise.resolve();
    spins += 1;
  }
  const t1 = performance.now();
  if (view.state.doc.content.size === sizeBefore) remoteLate += 1;
  // The binding defers selection restoration with setTimeout(0); wait for it so the
  // full main-thread cost of one remote update is counted.
  await new Promise((resolve) => setTimeout(resolve, 0));
  const t2 = performance.now();
  if (i % 100 === 99) console.error(`remote ${i + 1}/${remoteUpdates.length} ${(t1 - t0).toFixed(1)} / ${(t2 - t0).toFixed(1)} ms`);
  if (i >= WARMUP) {
    remote.push(t1 - t0);
    remoteIdle.push(t2 - t0);
  }
}
await settle();

// ---- Report ---------------------------------------------------------------
console.log("");
const localStats = stats(local);
row("local: binding + export", localStats);
row("local: jsdom DOM update (info)", stats(dom));
row("local: binding+export+jsdom", stats(local.map((v, i) => v + dom[i])));
const remoteStats = stats(remoteIdle);
row("remote: import -> editor state", stats(remote));
row("remote: import -> idle", remoteStats);
console.log("");
console.log(`trend local (mean ms per 100 samples): ${trend(local)}`);
console.log(`trend remote idle (mean ms per 100 samples): ${trend(remoteIdle)}`);
console.log(`mean update size ${(updateBytes / SAMPLES).toFixed(0)} bytes per keystroke`);
if (remoteLate > 0) console.log(`warning: ${remoteLate} remote updates needed more than 1000 microtask turns`);
const verdict = (name, p95) => `${name}: p95 ${p95.toFixed(3)} ms ${p95 < BUDGET_MS ? "PASS" : "FAIL"} (budget ${BUDGET_MS} ms)`;
console.log(verdict("local", localStats.p95));
console.log(verdict("remote", remoteStats.p95));
console.log("DOM layout/paint, IPC transfer and JavaScriptCore speed are not measured.");
process.exit(0);
