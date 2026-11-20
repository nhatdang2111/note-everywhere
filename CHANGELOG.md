# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project intends to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a first release.

## [Unreleased]

Nothing has been released yet. The entries below summarise what exists in the repository. The desktop application is not yet usable end to end: the shell that connects the Rust core to the UI is in progress and the app has not been smoke-tested in a real window.

### Added

- Workspace scaffold: Rust workspace with seven crates, a Tauri 2 desktop shell with a TypeScript UI, and a separate Cargo project for the sync server; MIT licence; CI for macOS and Windows (format, clippy, tests), the server (wasm target) and dependency licences and advisories (`cargo deny`).
- `ne-crypto`: password key derivation (Argon2id with HKDF), key wrapping with context binding, recovery keys with a checksum, XChaCha20-Poly1305 sync envelopes, chunked attachment streams, and account and vault unlock flows. Provisional Argon2id parameters.
- `ne-core`: workspace tree of folders and pages on a movable-tree CRDT with trash and favourites, concurrency tests showing convergence without cycles; typed page documents (fifteen block kinds) compatible with the editor binding; CommonMark and GFM Markdown import and export with Windows-safe file names.
- `ne-store`: one SQLite database per workspace with transactional writes and migrations, derived tree and link indexes, diacritic-insensitive full-text search, backlinks, content-addressed attachments, and a durable sync outbox.
- `ne-vault`: vault document with sealed items behind a second password, lock state machine and policy, password and passphrase generator, `secret://` references, and CSV import and export.
- `ne-platform`: OS credential storage (macOS keychain, Windows Credential Manager), idle and sleep detection, application directories.
- `ne-voice`: microphone capture, resampling, silence trimming, checksum-verified model download, word error rate tooling, and an opt-in whisper.cpp engine (`transcribe` feature, with `metal` and `vulkan` variants). Measured on synthetic text-to-speech audio only.
- Sync server for Cloudflare Workers: single-tenant Worker with one SQLite-backed Durable Object holding the account, hashed sessions, login throttling and the encrypted update log with snapshot compaction, plus attachments through an R2 binding. Tested locally with `wrangler dev`; not deployed to Cloudflare.
- `ne-sync`: sync client and engine with account flows, batching, retry and resume, attachment streaming, a shared wire-protocol crate, and integration tests that run against a local server.
- Desktop editor foundation: ProseMirror editor bound to Loro with the fifteen block kinds, headless interoperability and latency checks.
- Documentation: product spec, security model, sync protocol, architecture decision records (Tauri shell, CRDT choice, key derivation, Cloudflare sync server, licence and distribution, page document structure, offline transcription), feature research, setup, deployment and release guides, threat model, release checklist.
- Open-source project files: `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, issue and pull request templates, and a release workflow that builds unsigned macOS (universal) and Windows installers into a draft GitHub release with SHA-256 checksums. The release workflow has never been run.

### Known limitations

- The cryptographic code has not been independently audited.
- Release installers will be unsigned and, on macOS, only ad-hoc signed; there is no automatic update yet.
- Sync has not run on a real Cloudflare account, so CPU time per request and Durable Object hibernation on the free plan are unmeasured.
- Speech-to-text has not been tried with real voices or on a Windows laptop.
