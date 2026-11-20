# Note Everywhere

A local-first notes app for macOS and Windows, for one person on several devices: nested pages, a block editor, folders and search, end-to-end encrypted sync through a server you host yourself on Cloudflare's free plan, and a vault for secrets behind a second password.

## Status

**Pre-release, built in the open. Not ready for your real notes.**

- The building blocks (storage, sync, encryption, vault, Markdown, speech-to-text) exist as Rust libraries with automated tests. The desktop window and the Rust core are still being connected, and the app has **not yet been smoke-tested end to end in a real window** on macOS or Windows.
- The sync server has been tested against a local simulation of Cloudflare (`wrangler dev`), not on a real Cloudflare account.
- The cryptographic code has **not had an independent security audit**. See [SECURITY.md](SECURITY.md) and [docs/threat-model.md](docs/threat-model.md).
- There is no published release yet. Installers will be unsigned (see below).

Do not rely on it as your only copy of anything.

## What it does

### Works today (as tested libraries)

- **Workspace tree.** Folders and pages nest without limit; moving, favourites, trash and restore; two devices that move pages into each other while offline converge without creating a cycle (`crates/ne-core`).
- **Page documents.** Fifteen block kinds (paragraph, three heading levels, bulleted, numbered and to-do lists, toggle, quote, callout, divider, code, image, child page, page link) stored as CRDT documents that merge character by character.
- **Markdown.** Import a folder of Markdown files into a page tree and export a page or a branch as CommonMark/GFM, with file names that are valid on Windows.
- **Local store.** One SQLite database per workspace, transactional writes, crash-safe, a durable sync outbox, content-addressed attachments (`crates/ne-store`).
- **Search.** Full-text search over titles and content that ignores Vietnamese diacritics ("ghi chu" finds "ghi chú"), plus backlinks. Vault content never appears in results.
- **Encryption.** Password-based key derivation (Argon2id), key wrapping, recovery keys, XChaCha20-Poly1305 envelopes for every update and chunked encryption for attachments (`crates/ne-crypto`).
- **Sync.** A client and a Cloudflare Worker server: account and sessions, login throttling, encrypted update log with snapshot compaction, attachments through R2. The server only stores ciphertext plus the metadata it cannot avoid (`crates/ne-sync`, `server/`, [protocol](docs/sync-protocol.md)).
- **Vault.** Logins, secure notes and API keys sealed behind a second password, lock policy, password generator, `secret://` references, CSV import and export (`crates/ne-vault`).
- **Offline speech-to-text.** Microphone capture, silence trimming, checksum-verified model download, and a whisper.cpp engine behind the `transcribe` feature. Measured only on synthetic text-to-speech audio, not on real voices (`crates/ne-voice`, [ADR 0007](docs/decisions/0007-offline-transcription.md)).
- **OS integration.** Session token and root key in the macOS login keychain or Windows Credential Manager, idle and sleep detection (`crates/ne-platform`).
- **Editor.** A ProseMirror editor bound to the page CRDT, with Markdown shortcuts, a `/` menu and the fifteen block kinds, checked headlessly (jsdom), not in a real window.

### In progress

- The desktop shell (`apps/desktop/src-tauri`) that exposes the Rust core to the UI.
- The desktop UI (`apps/desktop/src`): screens for the tree, editor, search, sign-in, sync status, vault and voice. They currently run against an in-memory backend used for development and tests.

### Planned

- Release installers and updates through GitHub Releases.
- Push-to-talk dictation inserted at the cursor, wired into the UI.
- Databases and views, templates, page history, reminders, import from a Notion export, automatic trash cleanup, syntax highlighting in code blocks (see the [product spec](docs/product-spec.md), section 11).

### Not planned

Mobile, web, Linux, multiple users, public sharing, comments, an AI assistant, a public API, telemetry of any kind.

## Architecture

```text
 Desktop app (Tauri 2)                                  Your Cloudflare account
┌──────────────────────────────────────────┐          ┌───────────────────────────────┐
│ UI: TypeScript, ProseMirror editor       │          │ Worker (Rust, WebAssembly)    │
│        │ IPC                             │          │   │                           │
│ Rust core                                │  HTTPS   │ Durable Object (SQLite)       │
│  ne-core   pages + tree (Loro CRDT)      │  + WSS   │   account, sessions,          │
│  ne-store  SQLite, search, outbox        │ <──────> │   encrypted update log        │
│  ne-crypto keys, XChaCha20-Poly1305      │ only     │ R2 bucket                     │
│  ne-sync   sync client                   │ cipher-  │   encrypted attachments       │
│  ne-vault  secrets behind 2nd password   │ text     └───────────────────────────────┘
│  ne-voice  offline speech-to-text        │
│  ne-platform keychain, idle/sleep        │
└──────────────────────────────────────────┘
 Every device keeps a full local copy and works offline.
```

Decisions and their evidence are in [docs/decisions](docs/decisions).

## Install from a release

There is no release yet. When there is, installers are on the repository's Releases page. They are **not code-signed or notarised**: that costs money and this project has no fixed costs. Your operating system will warn you the first time. That is expected; the checksums below are how you check that the file is the one the build produced. They do not prove who built it, so if that matters to you, build from source.

### 1. Verify the checksum

Each release has a `SHA256SUMS.txt`. Download it next to the installer.

macOS (Terminal):

```sh
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

Windows (PowerShell), compare the output with the matching line in `SHA256SUMS.txt`:

```powershell
Get-FileHash .\Note-Everywhere_<version>_x64-setup.exe -Algorithm SHA256
```

### 2. First launch on macOS

The app is ad-hoc signed only. Open the `.dmg`, drag the app to Applications, then:

1. Open the app once. macOS says it cannot be opened. Click **Done**.
2. Open **System Settings → Privacy & Security**, scroll to the Security section and click **Open Anyway** next to the app's name. Confirm with your password.
3. Open the app again and choose **Open**.

Alternative from Terminal, which removes the "downloaded from the internet" flag:

```sh
xattr -dr com.apple.quarantine "/Applications/Note Everywhere.app"
```

### 3. First launch on Windows

Run the `-setup.exe` installer. SmartScreen shows "Windows protected your PC": click **More info**, then **Run anyway**.

## Build from source

Prerequisites are in [docs/SETUP.md](docs/SETUP.md): Rust (rustup), Node.js 22, pnpm 10 and the Tauri prerequisites for your OS.

```sh
git clone https://github.com/nhatdang2111/note-everywhere.git
cd note-everywhere/apps/desktop
pnpm install
pnpm tauri build
```

The installer ends up under `target/release/bundle/` at the repository root. Speech-to-text is off by default and needs `cmake` plus the `transcribe` feature of the desktop crate (see SETUP).

## Host your own sync server

Sync is optional: the app works without it. To sync, you deploy the server into your own Cloudflare account (free plan; R2 may ask for a payment method but staying inside the free allowance costs nothing). Step by step instructions, settings, backup and wipe are in [server/README.md](server/README.md).

## Development

```sh
# Rust crates
cargo test --workspace

# UI and desktop shell
cd apps/desktop
pnpm install
pnpm typecheck
pnpm test
pnpm tauri dev

# Sync server, locally (no Cloudflare account needed): see server/README.md
```

Full setup, the sync integration suites and common pitfalls: [docs/SETUP.md](docs/SETUP.md). How to contribute: [CONTRIBUTING.md](CONTRIBUTING.md).

## Repository layout

| Path | What it is |
|---|---|
| `crates/ne-core` | Block, page and workspace-tree model on CRDT documents; Markdown import and export |
| `crates/ne-store` | Local SQLite storage, full-text search, links, attachments, sync outbox |
| `crates/ne-crypto` | Password key derivation, key wrapping, authenticated encryption |
| `crates/ne-sync` | Client for the sync protocol (`protocol/` holds the shared wire formats) |
| `crates/ne-vault` | Secrets vault behind a second password |
| `crates/ne-voice` | Audio capture and offline speech-to-text |
| `crates/ne-platform` | OS credential storage, idle and sleep signals, app directories |
| `apps/desktop` | Tauri 2 app; UI in `src/`, Rust shell in `src-tauri/` |
| `server` | Sync server for Cloudflare Workers (separate Cargo project, compiles to WebAssembly) |
| `docs` | Product spec, security model, threat model, sync protocol, setup, deployment and release guides |
| `docs/decisions` | Short records of architecture decisions |
| `docs/research` | Feature research from public sources |

Most documents under `docs/` are written in Vietnamese with English technical terms (`docs/SETUP.md` is in English); the files at the repository root and the server README are in English.

## Security

Report vulnerabilities privately; see [SECURITY.md](SECURITY.md). The design is described in [docs/security-model.md](docs/security-model.md) and [docs/threat-model.md](docs/threat-model.md).

## Licence

[MIT](LICENSE)
