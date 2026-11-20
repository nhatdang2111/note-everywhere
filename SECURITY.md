# Security policy

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue or pull request.

Use GitHub's private vulnerability reporting: open the repository's **Security** tab, choose **Advisories**, then **Report a vulnerability**. That creates a private advisory visible only to you and the maintainer.

Include, as far as you can:

- what is affected (a crate, the sync server, the desktop shell, the release workflow, a document),
- the version or commit,
- steps to reproduce or a proof of concept,
- what an attacker gains,
- whether you plan to publish, and when.

This is a personal open-source project maintained in spare time. Reports are handled on a best-effort basis: expect an acknowledgement within about a week, not a guaranteed response time. Please allow a reasonable period to fix a problem before publishing details, and say if you need a specific date. Reporters are credited in the advisory unless they ask not to be.

Please do **not**:

- test against a sync server that is not yours, or against anyone's real data;
- include real notes, passwords, recovery keys or personal data in a report. Use synthetic data;
- open a public issue describing the vulnerability.

## Supported versions

There is no stable release yet. While the project is pre-release, only the latest commit on `main` and the most recent published release (once there is one) receive fixes. After a stable release this section will list the supported versions.

## Scope

In scope:

- the cryptographic code (`crates/ne-crypto`) and everything that uses it to protect data: the vault (`crates/ne-vault`), the sync client (`crates/ne-sync`), attachment encryption;
- the sync server in `server/` and the protocol it implements ([docs/sync-protocol.md](docs/sync-protocol.md));
- the desktop shell and its IPC surface (`apps/desktop/src-tauri`) and the UI's handling of untrusted content (imported Markdown and CSV, attachments, data arriving from another device);
- the release workflow (`.github/workflows/release.yml`) and anything that could let someone else publish or alter a release;
- mistakes in [docs/security-model.md](docs/security-model.md) or [docs/threat-model.md](docs/threat-model.md) that overstate a protection.

Out of scope, because the design says so openly (see [docs/threat-model.md](docs/threat-model.md), section 4):

- malware or an attacker already running with the user's privileges on the user's computer;
- a stolen, unlocked computer;
- metadata the server necessarily sees (ids, sizes, timing, IP address);
- a malicious server withholding, deleting or replaying data, which the cryptography does not detect;
- the lack of key rotation and of per-device key revocation;
- macOS and Windows warning about unsigned installers;
- vulnerabilities in Cloudflare, GitHub, the operating system or the webview, unless this project's configuration makes them worse;
- denial of service against a free-plan Cloudflare deployment by someone who knows its address, and weak passwords chosen by users.

If you are unsure whether something is in scope, report it anyway.

## Honest statement about the cryptography

The cryptographic code **has not had an independent security audit**. It composes well-known RustCrypto primitives (XChaCha20-Poly1305, Argon2id, HKDF-SHA256, SHA-256) and contains no hand-written primitives, but the way they are combined, the key hierarchy, the key-wrapping contexts, the streaming attachment format and the vault design are this project's own. On 2026-11-21 the maintainer did an internal, AI-assisted read-through of `ne-crypto`, `ne-vault` and related code; it found no exploitable vulnerability and led to two small hardening changes (a constant-time recovery-key checksum comparison, and SVG attachments no longer served inline as images). This is **not** a substitute for an independent human audit; see [docs/security-model.md](docs/security-model.md), section 5. Argon2id parameters are provisional and have not been measured on Windows. Do not trust this software with information whose exposure would cause serious harm until an independent review has been done and published; this document will say so when it happens.

Release installers are unsigned. Verify the SHA-256 checksum that accompanies each release, or build from source.

## Handling secrets in this repository

No secrets belong in the repository. If you find one (a key, token or credential, or personal data), report it privately as above so it can be revoked before it is discussed publicly.
