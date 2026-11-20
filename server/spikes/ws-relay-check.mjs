// Checks the authenticated relay against a locally running server, with no
// cryptography involved: the server only stores opaque bytes, so this script
// registers an account with a well-formed (but random) key record, opens two
// device connections, and verifies that pushed updates are acknowledged with
// increasing sequence numbers and relayed to the other device exactly once.
//
// Usage:
//   SETUP_CODE=<code> node spikes/ws-relay-check.mjs [http://127.0.0.1:8787] [count]
//
// The server keeps one account for good. Against a throwaway local server that
// runs with TEST_INSPECT=1, set NE_RESET=1 to wipe it first.

import { randomBytes } from "node:crypto";

const base = process.argv[2] ?? "http://127.0.0.1:8787";
const count = Number(process.argv[3] ?? 200);
const setupCode = process.env.SETUP_CODE ?? "local-test-setup-code";
const BATCH = 10;
const TIMEOUT_MS = 20_000;
const headers = { "user-agent": "NoteEverywhere/0.0 (relay check)" };

const b64 = (bytes) => Buffer.from(bytes).toString("base64url");
const json = async (path, method, body, extra = {}) => {
  const response = await fetch(base + path, {
    method,
    headers: { ...headers, "content-type": "application/json", ...extra },
    body: body ? JSON.stringify(body) : undefined,
  });
  return { status: response.status, body: await response.json().catch(() => null) };
};

if (process.env.NE_RESET === "1") {
  const reset = await fetch(base + "/v1/_test/reset", { method: "POST", headers });
  if (reset.status !== 204) throw new Error(`reset failed with ${reset.status}; is TEST_INSPECT=1 set?`);
}

const wrapped = () => b64(randomBytes(73));
const registered = await json("/v1/account/register", "POST", {
  setup_code: setupCode,
  username: "relay-check@example.test",
  device_label: "relay check",
  auth_key: b64(randomBytes(32)),
  record: {
    kdf_salt: b64(randomBytes(16)),
    kdf_params: { memory_kib: 65536, iterations: 2, parallelism: 1 },
    auth_verifier: b64(randomBytes(32)),
    root_key_by_password: wrapped(),
    root_key_by_recovery: wrapped(),
    workspace_key: wrapped(),
  },
});
if (registered.status !== 201) {
  console.error(`registration failed (${registered.status} ${registered.body?.error}).`);
  console.error("The account may exist already; use a fresh local state or NE_RESET=1.");
  process.exit(2);
}
const token = registered.body.token;

// --- frame helpers (version 1, big endian) ---------------------------------

const u32 = (n) => {
  const out = Buffer.alloc(4);
  out.writeUInt32BE(n);
  return out;
};
const u64 = (n) => {
  const out = Buffer.alloc(8);
  out.writeBigUInt64BE(BigInt(n));
  return out;
};
const id = (text) => Buffer.concat([Buffer.from([text.length]), Buffer.from(text)]);

const pullFrame = (req, cursor) => Buffer.concat([Buffer.from([1, 0x02]), u32(req), u64(cursor)]);
const pushFrame = (req, entries) =>
  Buffer.concat([
    Buffer.from([1, 0x01]),
    u32(req),
    Buffer.from([entries.length >> 8, entries.length & 255]),
    ...entries.map((e) => Buffer.concat([e.clientId, id(e.doc), u32(e.payload.length), e.payload])),
  ]);

function parse(buffer) {
  const data = Buffer.from(buffer);
  const kind = data[1];
  if (kind === 0x81) {
    const n = data.readUInt16BE(1 + 1 + 4 + 8);
    const seqs = [];
    for (let i = 0; i < n; i += 1) seqs.push(Number(data.readBigUInt64BE(2 + 4 + 8 + 2 + i * 8)));
    return { kind: "ack", req: data.readUInt32BE(2), seqs };
  }
  if (kind === 0x82) {
    return {
      kind: "updates",
      req: data.readUInt32BE(2),
      done: data[6] === 1,
      entries: data.readUInt16BE(6 + 1 + 8 + 8),
    };
  }
  return { kind: `other-${kind.toString(16)}` };
}

function connect(device) {
  const url = base.replace(/^http/, "ws") + `/v1/sync?device=${device}`;
  const socket = new WebSocket(url, { headers: { ...headers, authorization: `Bearer ${token}` } });
  socket.binaryType = "arraybuffer";
  const frames = [];
  const waiters = [];
  socket.addEventListener("message", (event) => {
    if (typeof event.data === "string") return;
    const frame = parse(event.data);
    const waiter = waiters.shift();
    if (waiter) waiter(frame);
    else frames.push(frame);
  });
  const next = () =>
    new Promise((resolve, reject) => {
      const ready = frames.shift();
      if (ready) return resolve(ready);
      const timer = setTimeout(() => reject(new Error(`${device}: timed out waiting for a frame`)), TIMEOUT_MS);
      waiters.push((frame) => {
        clearTimeout(timer);
        resolve(frame);
      });
    });
  const opened = new Promise((resolve, reject) => {
    socket.addEventListener("open", () => resolve());
    socket.addEventListener("error", () => reject(new Error(`${device} could not connect to ${url}`)));
  });
  return { socket, next, opened };
}

const sender = connect("relay-sender");
const receiver = connect("relay-receiver");
await Promise.all([sender.opened, receiver.opened]);
for (const peer of [sender, receiver]) {
  peer.socket.send(pullFrame(1, 0));
  const page = await peer.next();
  if (page.kind !== "updates" || !page.done) throw new Error(`unexpected first frame: ${JSON.stringify(page)}`);
}

let relayed = 0;
const relayedAll = new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error(`only ${relayed} of ${count} relayed`)), TIMEOUT_MS);
  (async () => {
    while (relayed < count) {
      const frame = await receiver.next();
      if (frame.kind === "updates") relayed += frame.entries;
    }
    clearTimeout(timer);
    resolve();
  })().catch(reject);
});

const started = performance.now();
const acks = [];
for (let sent = 0, req = 2; sent < count; req += 1) {
  const size = Math.min(BATCH, count - sent);
  const entries = Array.from({ length: size }, () => ({
    clientId: randomBytes(16),
    doc: "relay-doc",
    payload: randomBytes(512),
  }));
  sender.socket.send(pushFrame(req, entries));
  const ack = await sender.next();
  if (ack.kind !== "ack" || ack.req !== req || ack.seqs.length !== size) {
    throw new Error(`unexpected reply to push ${req}: ${JSON.stringify(ack)}`);
  }
  acks.push(...ack.seqs);
  sent += size;
}
await relayedAll;
const elapsed = performance.now() - started;

const increasing = acks.every((seq, i) => seq > 0 && (i === 0 || seq === acks[i - 1] + 1));
sender.socket.close();
receiver.socket.close();

console.log(`updates pushed: ${count} in batches of ${BATCH}`);
console.log(`acks: ${acks.length}, first seq ${acks[0]}, last seq ${acks.at(-1)}, contiguous: ${increasing}`);
console.log(`relayed to the second device: ${relayed}`);
console.log(`elapsed: ${elapsed.toFixed(0)} ms (local only; says nothing about Cloudflare latency or CPU)`);
if (!increasing || acks.length !== count || relayed !== count) process.exit(1);
