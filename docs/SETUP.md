# Developer setup

How to get a working checkout on macOS or Windows, run the checks, and run the sync server and its integration suites locally. Nothing here needs a Cloudflare account.

Status note: the desktop shell and UI are wired to the Rust core, but the app has only been launched once in a real window and never used end to end by a person (see the [README](../README.md#status)).

## 1. Tools

| Tool | Version | Notes |
|---|---|---|
| Rust | stable, installed with [rustup](https://rustup.rs) | `rust-toolchain.toml` selects the channel and adds `rustfmt` and `clippy`. The minimum supported version is `rust-version` in the root `Cargo.toml`. |
| Node.js | 22 | The same major version CI uses. |
| pnpm | 10 | `corepack enable` then `corepack prepare pnpm@10 --activate`, or `npm install -g pnpm@10`. |
| Tauri prerequisites | see below | Needed to compile the desktop shell. |
| cmake and libclang | optional | Only for the speech-to-text engine (`transcribe` feature of the desktop and `ne-voice` crates). |

### macOS

```sh
xcode-select --install                    # compiler, linker, libclang
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node@22 pnpm                 # or use any Node manager
brew install cmake                        # only for the transcribe feature
```

### Windows

1. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload.
2. WebView2 runtime: already present on Windows 11; the Tauri docs link the installer for Windows 10.
3. Install Rust with `rustup-init.exe` and choose the MSVC toolchain.
4. Install Node.js 22 and pnpm 10.
5. Only for the transcribe feature: `winget install Kitware.CMake LLVM.LLVM`. If the build cannot find libclang, set `LIBCLANG_PATH` to the `bin` folder of the LLVM install. This path has not been verified on Windows yet.

Authoritative per-OS list: <https://v2.tauri.app/start/prerequisites/>.

## 2. First run

```sh
cargo test --workspace            # Rust crates and the desktop shell crate
cd apps/desktop
pnpm install
pnpm typecheck
pnpm test                         # UI tests (vitest, jsdom)
pnpm tauri dev                    # opens the desktop window (see status note)
```

The first `pnpm tauri dev` compiles Tauri and every crate and takes several minutes. The dev UI is served on port 1420 and Vite fails instead of picking another port if it is taken; stop the old dev server first.

## 3. The checks CI runs

Run these before opening a pull request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/desktop && pnpm typecheck && pnpm test && pnpm build
```

The sync server is a separate Cargo project that targets WebAssembly:

```sh
cd server
cargo fmt --check
cargo clippy -- -D warnings
```

Licence and advisory checks use [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) with `deny.toml`:

```sh
cargo install cargo-deny --locked
cargo deny check
```

### Optional features and slow checks

```sh
# Speech-to-text engine (needs cmake and libclang)
cargo test -p ne-voice --features transcribe

# Argon2id timing on your machine (informs ADR 0003)
cargo run --release -p ne-crypto --example argon2_bench
```

The transcription measurement needs model files and audio that are not in the repository; see [ADR 0007](decisions/0007-offline-transcription.md) for how to reproduce it.

## 4. Run the sync server locally

`wrangler dev` simulates the Worker, the Durable Object and R2 on your machine. State lives in `server/.wrangler/`, which git ignores.

```sh
cd server
npx wrangler@latest dev --env dev --port 8787 --ip 127.0.0.1 \
  --var SETUP_CODE:local-test-setup-code \
  --var TEST_INSPECT:1 \
  --var LOCKOUT_BASE_SECONDS:2 \
  --var BLOB_MAX_BYTES:1048576 \
  --var BLOB_QUOTA_BYTES:3145728
```

`TEST_INSPECT:1` enables routes that wipe everything and dump every stored byte. Use it only for local testing; never set it on a real deployment. The server README has the full explanation and the settings table: [server/README.md](../server/README.md#develop-and-test-locally).

Stop the server with Ctrl-C when you are done.

## 5. Integration suites that need the server

With the server above running, in another terminal at the repository root:

```sh
NE_SYNC_TEST_URL=http://127.0.0.1:8787 cargo test -p ne-sync
```

PowerShell:

```powershell
$env:NE_SYNC_TEST_URL = "http://127.0.0.1:8787"; cargo test -p ne-sync
```

- Without `NE_SYNC_TEST_URL` the server-backed tests skip themselves and the rest of the suite still passes, so CI needs no server.
- `NE_SYNC_TEST_SETUP_CODE` overrides the setup code the tests use (default `local-test-setup-code`, matching the command above).
- One simulated day against the free-plan limits, printing a table:

  ```sh
  NE_SYNC_TEST_URL=http://127.0.0.1:8787 cargo test -p ne-sync --test request_budget -- --nocapture
  ```

- Relay check with no cryptography (200 pushes, acks, fan-out):

  ```sh
  cd server && NE_RESET=1 node spikes/ws-relay-check.mjs http://127.0.0.1:8787 200
  ```

These runs say nothing about CPU time per request or hibernation on Cloudflare's free plan; only a real deployment shows that (see [server/README.md](../server/README.md#what-local-tests-cannot-tell-you)).

## 6. Editor checks that need the UI toolchain

The editor spikes live in `apps/desktop/spikes` and are described in [ADR 0006](decisions/0006-page-document-structure.md): `page-doc-fixture.mjs` writes fixtures that `cargo test -p ne-core --test page_doc_fixture` reads, `roundtrip-check.mjs` feeds a Rust-written update back into the editor, and `typing-latency.mjs` measures latency in Node. Run them from `apps/desktop` with `node spikes/<name>.mjs`.

## 7. Common pitfalls

- **An old global Wrangler cannot bundle the worker.** The error is `Source phase imports are not available`. Always run `npx wrangler@latest`, never a `wrangler` found on your PATH.
- **Port 8787 or 1420 is already in use.** Find and stop the old process (`lsof -i :8787` on macOS, `netstat -ano | findstr 8787` on Windows) instead of switching ports; the tests and Vite expect these ports.
- **403 or Cloudflare error 1010 from the server.** Requests need a `User-Agent` starting with `NoteEverywhere/` and no `Origin` header. The app does this; `curl` needs `-A "NoteEverywhere/check"`.
- **Server tests skipped silently.** That is the design when `NE_SYNC_TEST_URL` is unset. Check the variable if you expected them to run.
- **Server build errors about the wasm target.** Run commands inside `server/` so its own `rust-toolchain.toml` (which adds `wasm32-unknown-unknown`) applies. The repository root workspace excludes `server`.
- **Golden-file tests fail on Windows with only line-ending differences.** The expected files under `crates/ne-core/tests/golden` must be checked out with LF endings. Clone with `git config core.autocrlf false` or re-checkout the files.
- **`loro-crdt` and `loro` versions must match.** The browser-side and Rust-side CRDT libraries are checked against each other at the same minor version; bump both together and rerun the editor spikes (ADR 0006).
- **Never put real notes, passwords or recovery keys into tests or fixtures.** All test data is synthetic.
