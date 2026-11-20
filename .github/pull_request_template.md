## What and why

<!-- What changes, and why. Link the issue if there is one. -->

## How it was checked

<!-- Commands you ran, manual steps, platforms (macOS, Windows). Say plainly what you could not check. -->

## Gates

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `pnpm typecheck`, `pnpm test` and `pnpm build` in `apps/desktop` (if the UI or desktop shell changed)
- [ ] `cargo fmt --check` and `cargo clippy -- -D warnings` in `server` (if the server changed)
- [ ] Sync integration suites with `NE_SYNC_TEST_URL` (if sync behaviour changed)
- [ ] `cargo deny check` (if a dependency was added or upgraded)

## Checklist

- [ ] New behaviour has tests, including failure cases
- [ ] Code builds on both macOS and Windows (platform code behind `cfg`)
- [ ] Docs updated where behaviour, setup, security or the wire format changed; a decision record added or updated if a decision changed
- [ ] Commit messages follow Conventional Commits
- [ ] Clean-room: no code, text or assets copied from other products
- [ ] No telemetry, no plaintext leaving the device, no new network destinations
- [ ] Dependencies are MIT-compatible (see `deny.toml`)
- [ ] No secrets, real personal data, real notes or absolute local paths in the diff
- [ ] This is not a security fix that should be reported privately first (see `SECURITY.md`)
