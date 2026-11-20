# Note Everywhere sync server

A small Cloudflare Worker that keeps your devices in sync. You deploy it into **your own** Cloudflare account, on the free plan, and nobody else uses it: one person, several devices.

The server never sees a password, a note, or an unwrapped key. Everything it stores is ciphertext, plus the metadata it cannot avoid (document ids, sizes, timing, your IP address). Details: `docs/security-model.md` and `docs/sync-protocol.md`.

What runs where:

- A front **Worker** (Rust, compiled to WebAssembly) checks each request and forwards it.
- One **Durable Object** with SQLite storage holds the account, sessions, wrapped keys, the encrypted update log, snapshots and the attachment index.
- An **R2 bucket** holds encrypted attachments. The Worker uses a binding, so there are no S3 keys to create or keep.

## What you need

- A Cloudflare account (free plan). R2 may ask for a payment method before you can create a bucket; staying inside the free allowance costs nothing, but the card can be required.
- Node.js 20 or newer. Always run Wrangler as `npx wrangler@latest`: older installed versions cannot bundle this project's output.
- The Rust toolchain from <https://rustup.rs>. This directory's `rust-toolchain.toml` adds the `wasm32-unknown-unknown` target automatically.

You do **not** need to log in to anything to read or test the code; the steps below are only for deploying.

## Deploy from a blank account

All commands run inside the `server/` directory. Commands that delete data are marked **DESTRUCTIVE**; none appears in this section.

1. **Log in.** This opens a browser and stores a token on your machine.

   ```sh
   npx wrangler@latest login
   ```

2. **Create the attachment bucket.** The name must match `bucket_name` in `wrangler.toml` (`note-everywhere-blobs` for production).

   ```sh
   npx wrangler@latest r2 bucket create note-everywhere-blobs
   ```

   If Cloudflare says R2 is not enabled, open the R2 page in the dashboard, complete the subscription flow (free allowance, card may be requested), and run the command again.

3. **Deploy.** The first deploy compiles the Rust code (a few minutes) and, if you have never used Workers, asks you to pick a `workers.dev` subdomain. It prints the Worker's address, `https://note-everywhere-sync.<your-subdomain>.workers.dev`.

   ```sh
   npx wrangler@latest deploy --env production
   ```

   Until step 4 is done nobody can create an account: registration is refused when no setup code exists.

4. **Set the setup code as a secret.** Make a long random code and keep it somewhere safe until you have created your account. It is read once, from the prompt, and never written to a file.

   ```sh
   openssl rand -base64 24            # prints a code to use
   npx wrangler@latest secret put SETUP_CODE --env production
   ```

   The code must be at least 12 characters.

5. **Check it answers.** The server only talks to the app, so it rejects a plain `curl`; give it an app-style User-Agent.

   ```sh
   curl -A "NoteEverywhere/check" https://note-everywhere-sync.<your-subdomain>.workers.dev/v1/health
   # {"status":"ok"}
   ```

6. **Create your account in the app.** Enter the Worker address, a username, a password and the setup code. The app shows your **recovery key** once: save it. The server cannot reset a password by itself; the recovery key is what lets you set a new one from any device (the app's "forgot password" flow), which also signs every other device out. Losing both the password and the recovery key loses your data for good. A recovery does not replace the recovery key, so keep it as safe as the password.

   After the first account exists, registration is closed for good, whatever the setup code is.

7. **Optional tidy-up.** Remove the setup code once you have your account:

   ```sh
   npx wrangler@latest secret delete SETUP_CODE --env production
   ```

   Sign each further device in with the username and password; no setup code is needed.

Nothing in `wrangler.toml` is secret and it contains no account id. `wrangler.toml` declares `dev` and `production` separately on purpose, so always pass `--env`.

### Settings

Set these under `[env.production.vars]` in `wrangler.toml` (plain values, not secrets) and deploy again.

| Variable | Default | Meaning |
|---|---|---|
| `BLOB_MAX_BYTES` | 67108864 (64 MiB) | Largest single attachment. Never above 90 MiB: the free plan limits request bodies to 100 MB. |
| `BLOB_QUOTA_BYTES` | 8589934592 (8 GiB) | Total attachment bytes the server accepts. R2's free allowance is 10 GB. |
| `SESSION_TTL_DAYS` | 30 | How long a sign-in lasts. |
| `LOCKOUT_FREE_ATTEMPTS` | 5 | Wrong passwords tolerated before a lockout starts. |
| `LOCKOUT_BASE_SECONDS` | 30 | First lockout length; it doubles with each further failure, up to an hour. Applies to password logins and, in buckets of their own, to recovery-key attempts. |
| `SESSION_TOUCH_SECONDS` | 600 | How often a session's "last seen" time is written at most. It is only written on requests that write anyway (pushes, part uploads) or when a connection opens, so it costs at most one row write per session per interval. |

### Free-plan limits this design respects

Worker and Durable Object requests: 100,000 a day each. Durable Object rows written: 100,000 a day. CPU: 10 ms per request. The app batches edits into one push, answers keepalives without waking the object, and lets the object hibernate. A simulated day (3 devices, 300 edits each, 10 attachments) costs under 5% of every limit; see `docs/decisions/0004-sync-server-cloudflare-free.md`. If a limit is reached the server answers "unavailable" and the app keeps its queue and retries later; no edit is lost.

## Operating it

Watch live logs (they contain request lines and errors, never passwords, tokens or note content):

```sh
npx wrangler@latest tail --env production
```

Update the server: pull the new code and run the deploy command again. Data stays.

### Back up

Your devices are the primary backup: every device keeps a full local copy, and the server only holds ciphertext. In addition:

- **Attachments** live in the R2 bucket. Copy one object with
  `npx wrangler@latest r2 object get note-everywhere-blobs/blobs/<sha256> --remote --file ./copy.bin`
  (the key is the SHA-256 the app stores next to the attachment). Wrangler cannot list a bucket; to copy everything use the dashboard or an S3-compatible tool such as `rclone` with an R2 API token you create for the purpose, and keep that token out of this repository.
- **The database** (account, sessions, log) has no export command: Durable Object SQLite storage is only reachable from the Worker. Cloudflare keeps point-in-time recovery for SQLite-backed objects for 30 days, as an API available to Worker code; this server does not expose it. If the database is lost, deploy a fresh server and, from a device that still has the data, sign out (local data is kept) and sign in again (or register) on the new server: the app queues its whole local workspace for re-upload from scratch. Any other device must also sign out and back in to the new server, or it stops with a "server was reset" status (see "Wipe" below and `docs/deployment-guide.md`, section 6).

Back up the **recovery key** and the password separately from the server. They are the only way back in.

### Wipe

**DESTRUCTIVE.** The commands below permanently delete data in your Cloudflare account. Before running any of them, say out loud which environment they target (`production` in these examples) and be sure you have your recovery key and a local copy of your data on at least one device. They cannot be undone.

1. See what would be deleted first:

   ```sh
   npx wrangler@latest delete --env production --dry-run
   ```

2. **DESTRUCTIVE: delete the Worker (production).** Wrangler describes this as deleting the Worker and its associated resources. It does not say precisely which are included, so afterwards open the Cloudflare dashboard and confirm that the Durable Object namespace is gone; if it is not, delete it there.

   ```sh
   npx wrangler@latest delete --env production
   ```

3. **DESTRUCTIVE: delete every attachment and the bucket (production).** R2 refuses to delete a bucket that still has objects. Empty it from the dashboard (or with `rclone`), then:

   ```sh
   npx wrangler@latest r2 bucket delete note-everywhere-blobs
   ```

4. Remove the `workers.dev` subdomain or the whole account from the dashboard if you want nothing left.

Wiping only the data but keeping the Worker is done by deploying under a new `name` in `wrangler.toml` (a new Worker has a new, empty object) and then removing the old one with the steps above. Devices that were signed in to the old server stop with a "server was reset" status because the server's history is behind theirs; they must sign in to the new server.

## Develop and test locally

Nothing here touches Cloudflare. `wrangler dev` simulates the Worker, the Durable Object and R2 on your machine, with state in `server/.wrangler/` (ignored by git).

Static checks of the server (it targets WebAssembly, so these are `check`-style commands):

```sh
cd server
cargo fmt --check
cargo clippy -- -D warnings
```

Start the local server on port 8787. The test-only variables **must never be set on a real deployment**: `TEST_INSPECT=1` enables routes that wipe all data and dump every stored byte.

```sh
npx wrangler@latest dev --env dev --port 8787 --ip 127.0.0.1 \
  --var SETUP_CODE:local-test-setup-code \
  --var TEST_INSPECT:1 \
  --var LOCKOUT_BASE_SECONDS:2 \
  --var BLOB_MAX_BYTES:1048576 \
  --var BLOB_QUOTA_BYTES:3145728 \
  --var SESSION_TOUCH_SECONDS:2
```

Then, from the repository root, in another terminal:

```sh
NE_SYNC_TEST_URL=http://127.0.0.1:8787 NE_SYNC_TEST_TOUCH_SECONDS=2 cargo test -p ne-sync
```

`NE_SYNC_TEST_TOUCH_SECONDS` must equal the server's `SESSION_TOUCH_SECONDS`; without it the one test that checks session activity skips itself. For the request-budget numbers leave `SESSION_TOUCH_SECONDS` out so the production default (600) applies.

Without `NE_SYNC_TEST_URL` the integration tests skip themselves and the rest of the suite still runs, so CI needs no server. The suite covers the account lifecycle, lockout, closed registration, session revocation, convergence after offline edits, duplicate and out-of-order pushes, dropped connections mid-push, snapshot compaction with a fresh device, attachments including a tampered download, hibernation, recovering a forgotten password with the recovery key (second device, lockout, unknown names, accounts without a verifier), updates and snapshots of 5 and 30 MiB that travel in staged parts and survive dropped connections, session activity, and a check that the server stores no known plaintext marker, password or key. The 30 MiB case needs no special settings: those objects go through the parts endpoint, not through R2.

Other helpers:

```sh
# One simulated day of use against the free-plan limits; prints a table.
NE_SYNC_TEST_URL=http://127.0.0.1:8787 cargo test -p ne-sync --test request_budget -- --nocapture

# Authenticated relay check with no cryptography: 200 pushes, acks, fan-out.
NE_RESET=1 node spikes/ws-relay-check.mjs http://127.0.0.1:8787 200
```

Stop the local server with Ctrl-C when you are done.

## What local tests cannot tell you

- **CPU time per request.** Cloudflare's free plan allows 10 ms of CPU for each request; a local run does not enforce or measure it. The code keeps each request small (no hashing or decryption of payloads in the Worker, single-statement inserts, bounded pages), but only a real account shows the truth. After the first real deploy, run `npx wrangler@latest tail --env production` while pushing 100 updates at once and while pulling a page of 200 rows, and look for "exceeded resource limits" errors or high CPU figures in the dashboard.
- **Hibernation on the free plan.** Confirm in the dashboard that the object's duration does not grow while devices are connected and idle.
- **R2 and the payment method**, and the exact behaviour when a daily limit is reached.

## Troubleshooting

- `Error 1010` or a 403 from the app: the User-Agent must start with `NoteEverywhere/`. The app does this; a custom client must too.
- `Source phase imports are not available` while bundling: you are using an old Wrangler. Use `npx wrangler@latest`.
- "Registration is closed": an account already exists. Sign in instead.
- A device shows a "session ended" status: it was signed out from another device, or the password was changed or reset with the recovery key. Sign in again.
- "Recovery is not available for this account": the account was created before the server could verify recovery keys. Sign in with the password, or create a new account on a fresh server.
