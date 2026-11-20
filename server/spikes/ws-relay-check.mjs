// Checks the Durable Object relay against a locally running server:
// two devices connect, one pushes binary updates, the other receives them,
// and the sender gets increasing sequence numbers back.
//
// Usage: node spikes/ws-relay-check.mjs [ws://127.0.0.1:8787/sync] [count]

const url = process.argv[2] ?? "ws://127.0.0.1:8787/sync";
const count = Number(process.argv[3] ?? 200);
const TIMEOUT_MS = 20_000;

function open(name) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.binaryType = "arraybuffer";
    socket.addEventListener("open", () => resolve(socket));
    socket.addEventListener("error", () => reject(new Error(`${name} could not connect to ${url}`)));
  });
}

const sender = await open("sender");
const receiver = await open("receiver");

const acks = [];
let received = 0;
let receivedBytes = 0;

const finished = new Promise((resolve, reject) => {
  const timer = setTimeout(
    () => reject(new Error(`timed out: ${acks.length} acks, ${received} relayed of ${count}`)),
    TIMEOUT_MS,
  );
  const check = () => {
    if (acks.length === count && received === count) {
      clearTimeout(timer);
      resolve();
    }
  };
  sender.addEventListener("message", (event) => {
    if (typeof event.data !== "string" || !event.data.startsWith("ack ")) {
      reject(new Error(`unexpected reply to sender: ${String(event.data)}`));
      return;
    }
    acks.push(Number(event.data.slice(4)));
    check();
  });
  receiver.addEventListener("message", (event) => {
    received += 1;
    receivedBytes += event.data.byteLength;
    check();
  });
});

const payload = new Uint8Array(512).fill(7);
const started = performance.now();
for (let i = 0; i < count; i += 1) sender.send(payload);
await finished;
const elapsed = performance.now() - started;

const increasing = acks.every((seq, i) => i === 0 || seq === acks[i - 1] + 1);
sender.close();
receiver.close();

console.log(`updates sent: ${count}`);
console.log(`acks: ${acks.length}, first seq ${acks[0]}, last seq ${acks.at(-1)}, strictly increasing: ${increasing}`);
console.log(`relayed to second device: ${received} messages, ${receivedBytes} bytes`);
console.log(`elapsed: ${elapsed.toFixed(0)} ms (${(elapsed / count).toFixed(2)} ms per update, local only)`);
if (!increasing || receivedBytes !== count * payload.length) process.exit(1);
