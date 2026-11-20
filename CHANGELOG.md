# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project intends to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once it has a first release.

## [Unreleased]

Nothing has been released yet. The entries below summarise what exists in the repository. The desktop shell that connects the Rust core to the UI is complete; the app has been launched once in a real window with no visual verification beyond that it opened, and has not yet been used end to end by a person.

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
- Account recovery: a forgotten password is reset with the account recovery key from any device (server verifier, client flow and desktop command); recovery signs every other device out and does not rotate the recovery key.
- Large-object sync: updates and snapshots above the server's row limit travel in staged parts, with resumable uploads, tombstoning and compaction that cover them, tested with 5 MiB and 30 MiB documents.
- Session activity: the device list shows when each session was last used, with a throttled update on the server.
- Databases: tables, boards, lists and calendars over pages-as-rows, with typed properties (text, number, select, multi-select, date, checkbox, URL, relations with two-way backlinks), filters, sorts, grouping, saved views, and conflict-free concurrent edits of cells, options and card positions.
- Templates: page templates and per-database row templates with default values.
- Page history: versions grouped from the change log, read-only preview of any version, and restore as a new forward change that syncs.
- Structured import: another app's Markdown and CSV export becomes folders, pages, databases with rows, attachments and rewritten page links, with a report of what was skipped.
- Synced blocks: a block whose content is a shared document shown in several pages, with unsync, occurrence listing and clean-up of blocks no page shows any more.
- Reminders: reminders on pages or on a database row's date property, snooze and reschedule, synced between devices, fired once per device by a scheduler.
- Notifications: a due reminder shows a native notification on macOS and Windows. Permission is requested when the first reminder is created, never at launch. The `reminder:due` event carries `nativeShown` so the UI does not show a second notification, and without permission the event is all there is.
- Updater: the app checks the newest GitHub release for an update on launch (at most once a day) and on demand, downloads it with progress and verifies its signature against a public key built into the app, and installs only when the person asks. A build without the key reports updates as unavailable. The release workflow produces signed updater packages and `latest.json` for macOS (universal) and Windows when the signing secret and public key variable exist, and still succeeds without them.
- Sync coverage: the first full upload of a local workspace and server log compaction now include databases, templates, reminders and synced blocks explicitly; end-to-end tests cover two-device sync, upload at sign-up and a new device bootstrapping from a compacted log.

### Known limitations

- The cryptographic code has not been independently audited.
- Release installers will be unsigned and, on macOS, only ad-hoc signed. Updates are authenticated by the project's own updater signature, not by the operating system. The updater key pair has not been generated yet, so no release can update itself until the maintainer creates it, and the update flow has only been tested against a local server with throwaway keys.
- Native reminder notifications cannot tell whether the person turned the app's notifications off in system settings on macOS and Windows; the app then reports the notification as shown.
- Sync has not run on a real Cloudflare account, so CPU time per request and Durable Object hibernation on the free plan are unmeasured.
- Speech-to-text has not been tried with real voices or on a Windows laptop.
