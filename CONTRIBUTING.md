# Contributing

Thanks for taking an interest. Note Everywhere is a personal open-source project in early development, so small, focused contributions that match the existing direction are the easiest to review. For anything large, open an issue first and describe the problem before writing code.

Security problems do not go in issues: see [SECURITY.md](SECURITY.md).

## Ground rules

- **Clean-room.** Do not copy code, text, strings, icons, layouts or other assets from Notion or any other product, and do not paste from decompiled or proprietary sources. Features are described from public documentation in [docs/research](docs/research); implement them in your own words and design. If you are unsure about the origin of something, leave it out.
- **No telemetry, ever.** No analytics, crash reporting, usage pings, update-check beacons or "anonymous" statistics. The only network traffic is to the sync server the user configures and to the model download the user asks for.
- **Local-first and private by default.** Nothing leaves the device unencrypted. Do not add features that send plaintext notes, titles, keys or passwords anywhere.
- **Synthetic data only.** Tests, fixtures, examples and screenshots must use invented data. Never commit real notes, real passwords or recovery keys, personal data or secrets (`.env` files, tokens, keys). Never commit absolute paths from your machine.
- **Scope.** Mobile, web, Linux, multi-user features, sharing, comments and an AI assistant are out of scope (see [docs/product-spec.md](docs/product-spec.md), section 13).

## Workflow

1. Fork the repository and create a branch from `main` (for example `feat/short-description` or `fix/short-description`).
2. Make a focused change. Keep unrelated cleanups out of the same pull request.
3. Run the gates below.
4. Open a pull request using the template. Describe what changed and why, and how you checked it.

Review may ask for changes. Please do not rewrite shared history on a branch under review without saying so.

## Gates to run before opening a pull request

Setup is in [docs/SETUP.md](docs/SETUP.md).

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd apps/desktop
pnpm typecheck
pnpm test
pnpm build
```

If you touched `server/`:

```sh
cd server
cargo fmt --check
cargo clippy -- -D warnings
```

and, when the change affects sync behaviour, run the integration suites against a local server (`NE_SYNC_TEST_URL`, see SETUP). If you added or upgraded a dependency, run `cargo deny check`.

Code expectations:

- `unsafe` code is forbidden by workspace lint; `unwrap` warns and warnings fail CI. Use proper error types in libraries and `expect("reason")` in tests.
- Code compiles on macOS and Windows; gate platform code with `cfg`.
- New behaviour comes with tests, including failure cases. Do not weaken a test to make it pass.
- Keep files reasonably small and meaningful; comment invariants and non-obvious choices, not the obvious.
- All cryptography lives in `crates/ne-crypto` and composes existing RustCrypto primitives. Do not write your own primitives or add another crypto library without an accepted decision record.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `perf:`, `ci:`, with an optional scope, for example `feat(store): add attachment reference counts`. Write the subject in the imperative, keep it under about 72 characters, and explain the why in the body when it is not obvious. Do not put ticket numbers or internal planning labels in code comments, test names or commit messages; describe the behaviour or the invariant directly.

## Dependencies and licences

The project is MIT-licensed and every dependency must be compatible with that.

- Allowed licences are the ones listed in [deny.toml](deny.toml) (MIT, Apache-2.0 including the LLVM exception, BSD-2 and BSD-3, ISC, Zlib, Unicode-3.0, MPL-2.0, CC0-1.0, BSL-1.0, CDLA-Permissive-2.0, Unlicense). Copyleft licences (GPL, AGPL, LGPL, SSPL and similar) are not acceptable.
- Add Rust dependencies with `cargo add` so the version is current, and check the licence before adding. CI runs `cargo deny` for licences and security advisories; do not widen `deny.toml` without explaining why in the pull request.
- JavaScript dependencies are checked by hand: confirm the licence, commit the lockfile change, and prefer few, well-maintained packages.
- Keep the dependency tree small. A new dependency needs a reason that a short piece of local code does not meet.

## Where decisions are recorded

Architecture decisions are short records in [docs/decisions](docs/decisions): the decision, why, the evidence behind it, and what is still unverified. If your change makes or reverses such a decision, add or update a record in the same pull request. Product behaviour is in [docs/product-spec.md](docs/product-spec.md), the cryptographic design in [docs/security-model.md](docs/security-model.md), the threats considered in [docs/threat-model.md](docs/threat-model.md), and the wire format in [docs/sync-protocol.md](docs/sync-protocol.md). Update the document that owns a fact when you change that fact, and be honest about what has and has not been verified.

## Languages

Code, code comments, commit messages and files at the repository root are in English. Documents under `docs/` that already exist in Vietnamese stay in Vietnamese, with English technical terms; `docs/SETUP.md` is in English. The UI is localised (English and Vietnamese strings live in `apps/desktop/src/i18n`).

## Licence of contributions

By submitting a contribution you agree that it is licensed under the repository's [MIT licence](LICENSE).
