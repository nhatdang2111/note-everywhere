# Note Everywhere

A local-first notes app for macOS and Windows: nested pages, a block editor, folders and search, with end-to-end encrypted sync through a server you host yourself on Cloudflare's free plan. Secrets live in a vault behind a second password.

**Status: early development.** The repository currently holds the project skeleton and the technical spikes that decide the architecture. Nothing here is usable as a notes app yet.

## Design in one paragraph

The core is written in Rust and does the storage, CRDT merging, encryption and sync. The desktop shell is Tauri 2 with a TypeScript UI. Each person deploys their own sync server (a Cloudflare Worker plus one Durable Object); the server only ever stores encrypted blobs. You sign in with an account and password, the password never leaves your device, and the server keeps only a hash of a key derived from it.

## Repository layout

| Path | What it is |
|---|---|
| `crates/ne-core` | Block, page and workspace-tree model on CRDT documents |
| `crates/ne-store` | Local SQLite storage, full-text search, sync outbox |
| `crates/ne-crypto` | Password key derivation, key wrapping, authenticated encryption |
| `crates/ne-sync` | Client for the sync protocol |
| `crates/ne-vault` | Secrets vault behind a second password |
| `crates/ne-voice` | Audio capture and offline speech-to-text (conditional on a quality spike) |
| `crates/ne-platform` | OS credential storage on macOS and Windows |
| `apps/desktop` | Tauri 2 app; UI in `src/`, Rust shell in `src-tauri/` |
| `server` | Sync server for Cloudflare Workers (separate Cargo project, compiles to WebAssembly) |
| `docs/decisions` | Short records of architecture decisions |
| `docs/research` | Feature research from public sources |

## Development

Prerequisites: [Rust](https://rustup.rs) (stable), Node.js 22+, [pnpm](https://pnpm.io) 10+.

```sh
# Core crates
cargo test --workspace

# Desktop app
cd apps/desktop
pnpm install
pnpm tauri dev

# Sync server, locally (needs no Cloudflare account)
cd server
npx wrangler@latest dev --var ALLOW_UNAUTHENTICATED_SPIKE:true
node spikes/ws-relay-check.mjs   # in a second terminal: two-device relay check
```

The sync route is switched off unless that variable is set, because the server has no authentication yet. Do not deploy it until accounts and sessions land.

## Installing unsigned releases

Releases will not be code-signed, because signing certificates cost money and this project has no fixed costs. macOS and Windows will warn the first time you open the app; instructions for that, and for compiling from source instead, will be added with the first release.

## Licence

[MIT](LICENSE)
