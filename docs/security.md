# Security & Compliance Design

OpnRelay will hold screenshots from HR systems, EMS systems and audit work. The platform
protects all guides equally (encryption, access control, audit log). **What appears in a
guide is the author's decision**: redaction tools are offered, not forced, and no
department is restricted.

> OpnRelay provides technical controls. Compliance (HIPAA, GDPR, SOC 2 and others) is a
> property of how an organisation deploys and operates it, not of the software alone.

## 1. Security goals

1. When a user chooses redaction, it happens **before the screenshot leaves their device**.
2. Only authorised users can see a guide; every access to sensitive content is **auditable**.
3. A stolen disk, backup, or database dump does not expose screenshots (**encryption at rest**).
4. The audit trail is **tamper-evident**.
5. Minimal attack surface: few services, small images, no telemetry, no outbound calls by default.

## 2. Threat model (STRIDE summary)

| Asset | Threat | Mitigations |
|---|---|---|
| User accounts | Credential stuffing, phishing | Argon2id; rate limiting and progressive lockout; MFA (TOTP, WebAuthn); SSO-only mode; breached-password check against an **offline** k-anonymity list (optional) |
| Sessions | Hijacking, CSRF | `HttpOnly; Secure; SameSite=Lax` cookies, `__Host-` prefix; CSRF tokens on state-changing requests; idle + absolute timeouts; rotation on privilege change |
| Native client tokens | Theft from disk | OAuth PKCE; access token TTL 15 min; refresh tokens rotated, single-use, bound to device ID, stored in OS keychain; reuse detection revokes the token family |
| Screenshots (in transit) | Interception | TLS 1.2+ only (Caddy), HSTS; certificate pinning optional for desktop |
| Screenshots (at rest, server) | Disk/backup theft | Envelope encryption: AES-256-GCM per-object data key, wrapped by a master key (KEK) held outside the DB |
| Screenshots (at rest, client) | Laptop theft | Drafts encrypted with SQLCipher; key in OS keychain; drafts deleted after successful upload |
| Screenshots (content) | Accidental over-capture | Optional client-side auto-redaction; manual redaction; app/site exclusion list; password fields never captured; typed text off by default (user can enable) |
| Public links | Leakage, guessing | 256-bit random tokens (stored hashed); optional password; expiry; revocation; `noindex`; rate limits; every public view audited |
| Guide data | Unauthorised access (IDOR) | Central authorisation layer (policy checks in one module, tested per endpoint); UUIDv7 IDs (not enumerable); deny by default |
| Uploads | Malicious files | Magic-byte validation; allowlist (PNG, JPEG, WebP, MP4/WebM); decode and **re-encode** images; size limits; EXIF stripping; never serve uploads as HTML; `Content-Disposition` and `X-Content-Type-Options: nosniff` |
| Web UI | XSS, clickjacking | Strict CSP (no inline, no eval); Markdown sanitised server-side (`ammonia`); `frame-ancestors` limited to configured embed origins |
| Exports | Data exfiltration | Permission + policy checks, audit events, optional watermark ("Exported by X on DATE"), short-lived download links |
| Audit log | Tampering, repudiation | Append-only table (DB role without UPDATE/DELETE), SHA-256 hash chain, periodic signed checkpoints, export + verify tool |
| Server | RCE, supply chain | Rust (memory safe); `cargo-deny` + `cargo-audit` in CI; pinned dependencies; distroless non-root container, read-only root FS, dropped capabilities; SBOM (CycloneDX) per release |
| Desktop app | Malicious or tampered installer/update | Tauri updater with ed25519-signed bundles (free, project-held key); SHA-256 checksums + Sigstore signatures on every release file; downloads served from the company's own OpnRelay server; optional signing with the company's internal CA. No paid OS vendor certificates ([ADR 0007](adr/0007-no-paid-code-signing.md)). |
| Extension | Over-broad permissions | Minimal permissions (`activeTab`, `scripting`, `storage`); host permission only for the configured server; recording only on explicit user start |
| Admin | Insider abuse | All admin actions audited; Owner/Admin changes require re-authentication (and MFA); optional two-person rule for audit log purge (log purge is otherwise impossible via UI) |

## 3. Cryptography

| Use | Algorithm |
|---|---|
| Password hashing | Argon2id (m=64 MiB, t=3, p=1; tunable) |
| Media/object encryption | AES-256-GCM, unique 96-bit nonce, per-object random DEK |
| Key wrapping | AES-256-KW or XChaCha20-Poly1305 with KEK |
| KEK source | Docker secret / file (`OPNRELAY_MASTER_KEY_FILE`); optional external KMS (Vault Transit) later |
| Token storage | Opaque random tokens; only SHA-256 hashes stored in DB |
| Audit chain | SHA-256 over canonical JSON of each event + previous hash |
| TOTP secrets | Encrypted with KEK-derived key before storage |
| TLS | Caddy defaults (TLS 1.2/1.3, modern ciphers) |

**Key rotation:** the KEK can be rotated by re-wrapping DEKs (fast, no media rewrite).
Rotation is a CLI command (`opnrelay-server keys rotate`).

**Key loss = data loss.** The deployment guide requires the master key to be backed up separately from data backups.

## 4. Authentication details

- **Local accounts:** email + password (min 12 chars, no composition rules, breached-list check optional) + MFA.
- **MFA:** TOTP (RFC 6238) and WebAuthn/passkeys; 10 single-use recovery codes (hashed). Admin policy: *MFA required for all*, *for Admin/Auditor only*, or *optional*.
- **SSO:** OIDC (auth code + PKCE) and SAML 2.0 (SP-initiated; signed assertions required; encrypted assertions supported). Group claims map to workspace roles.
- **SSO-only mode** disables local passwords except for a break-glass Owner account (must have MFA).
- **Native clients:** the desktop app and extension open the system browser to `/oauth/authorize`, which redirects back to a loopback port (desktop) or the extension redirect URL (extension).

## 5. Authorisation

- Deny by default. One policy function per resource action, e.g. `can(actor, Action::ExportGuide, &guide)`.
- Inputs: org role, workspace role (direct or via group), guide status, workspace sensitivity flags, org policies.
- **Auditor** is read-only by construction: the policy layer rejects every mutating action for that role, regardless of other grants.
- Every endpoint has an authorisation test; CI fails if an endpoint is registered without a policy check (enforced by a router macro).

## 6. Privacy and data handling

- **No telemetry.** No crash reporting unless an admin configures a self-hosted Sentry-compatible endpoint.
- **Data minimisation:** keystrokes are not stored by default; only "typed into field" events. Users can turn on typed-text capture per recording. Password fields are always excluded.
- **Retention:** per-workspace rules (drafts, published guides, exports, audit logs). Exports auto-expire (default 24 h).
- **Legal hold** blocks deletion and retention purges for selected workspaces or guides.
- **User data export/erasure** for GDPR-style requests. Audit events keep a pseudonymous actor ID after erasure so the chain stays intact.

## 7. Compliance mapping (guidance)

### HIPAA Security Rule (relevant if EMS guides can contain PHI)

| Safeguard | OpnRelay control |
|---|---|
| Access control §164.312(a) | Unique user IDs, RBAC, automatic logoff (idle timeout), emergency (break-glass) Owner account, encryption at rest |
| Audit controls §164.312(b) | Tamper-evident audit log including guide **views** |
| Integrity §164.312(c) | Hash-chained audit log; SHA-256 checksums on media; version history |
| Authentication §164.312(d) | MFA, SSO |
| Transmission security §164.312(e) | TLS everywhere |

OpnRelay does not restrict what EMS staff capture. The controls above (access control,
encryption, audit of every view and export) apply to every guide regardless.

### Audit use
- Auditor role with read-only scope.
- Audit log export (CSV/JSON) with chain-verification output.
- Guide version history shows who changed what and when; published versions are immutable snapshots.

### GDPR / employee privacy (HR)
- Data subject export and erasure tools; retention policies; purpose limitation via workspace scoping.

## 8. Secure development lifecycle

- `#![forbid(unsafe_code)]` in all crates except narrowly scoped platform FFI modules in `opnrelay-capture` (reviewed and documented).
- CI: `clippy -D warnings`, `cargo test`, `cargo-deny` (licences + advisories), `cargo-audit`, `npm audit` / `pnpm audit`, Semgrep, Trivy image scan, SBOM generation.
- Fuzzing (`cargo-fuzz`) for upload parsing, Markdown sanitising, and capture-payload deserialisation.
- Dependency updates via Renovate/Dependabot, with weekly review.
- Release artifacts are signed; reproducible builds are a goal.
- An external penetration test before v1.0 (see roadmap).
- Vulnerability disclosure: see [`SECURITY.md`](../SECURITY.md).
