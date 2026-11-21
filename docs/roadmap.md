# Roadmap

No fixed timeline. Each milestone ends with something usable and demoable.

## M0: Foundations
- [x] Product & technical design docs
- [x] Choose licence: AGPL-3.0 ([ADR 0006](adr/0006-license.md)); `LICENSE` added
- [x] Cargo workspace, crate skeletons, `apps/` skeletons
- [x] CI: fmt, clippy, tests, `cargo-deny` (licences + advisories), web type check/tests/build, image build
- [x] Dockerfile + `compose.yaml` (Caddy + server + Postgres), verified end to end

## M1: Server core & web viewer
- [x] Email/password auth, sessions, CSRF, rate limits, lockout
- [x] MFA: TOTP + recovery codes; org policy to require MFA
- [x] Org roles (owner/admin/member/auditor), workspaces, workspace roles, groups
- [x] Guide/step CRUD, publish + version history + restore, duplicate, search
- [x] Media upload (validated, re-encoded) with encryption at rest
- [x] Audit log (hash chain) for all actions so far, with verify and CSV/JSON export
- [x] Web UI: login, invites, workspaces/folders, guide viewer, editor (steps, screenshots via upload/paste, highlights), admin, audit log, account/MFA
- **Demo:** create a guide by uploading screenshots manually; view it in the browser.

## M2: Desktop recorder (Windows + macOS first)
- [ ] `opnrelay-capture`: input hooks, screenshots (full/window/region), element labels
- [ ] Step generation and title templates
- [ ] OAuth PKCE sign-in, keychain storage, encrypted local drafts
- [ ] Resumable upload → draft guide
- [ ] Auto-redaction (structural + patterns)
- **Demo:** record a workflow on Windows/macOS and get a guide on the server.

## M3: Exports & branding
- [x] Branding settings UI (logo, colours, header/footer, cover page, numbering, watermark)
- [x] PDF (Typst), HTML, Markdown, DOCX, PNG zip
- [x] Watermarking, audit events (synchronous rendering; background jobs come with M7)
- **Demo:** branded PDF/Word of a recorded guide. *(This milestone is the internal pilot point.)*

## M4: Browser extension
- [x] Chrome/Edge + Firefox builds (Manifest V3), accessible-name labels, DOM-level redaction (password/card fields always; emails, phone numbers, SSNs, card numbers and admin patterns when on)
- [x] OAuth + PKCE sign-in, upload as a draft guide, pause/resume/discard
- [x] Enterprise managed config (server URL)
- [ ] Store listings (Chrome Web Store, Edge Add-ons, Firefox AMO) and signed self-host packages
- [ ] Linux desktop (X11; Wayland portal mode)

## M5: Editor & sharing
- [ ] Annotations (arrows, boxes, text), crop, manual blur/box redaction
- [x] Tips/alerts/headings, insert steps (done in M1)
- [ ] Merge/split steps, record-into-existing-guide
- [x] Version history + restore, publish workflow (done in M1)
- [ ] Internal links, public links (password/expiry), embeds, admin sharing policies

## M6: Enterprise auth & compliance
- [ ] WebAuthn/passkeys
- [ ] OIDC SSO + group mapping, then SAML 2.0
- [x] Auditor role, audit log search/export/verify (done in M1)
- [x] Legal hold (done in M1)
- [ ] Retention policies, user data export/erasure

## M7: Video & GIF
- [ ] Desktop video recording (steps + video), extension tab recording
- [ ] Worker transcoding, GIF generation, MP4/WebM export

## M8: Release hardening → v1.0
- [ ] Free release signing: Tauri update key, macOS self-signed identity, GPG for Linux repos, Sigstore + checksums for all files ([ADR 0007](adr/0007-no-paid-code-signing.md))
- [ ] Apply to SignPath Foundation for free Windows signing (optional)
- [ ] Fuzzing, load test (40 concurrent users + bulk exports), restore drills
- [ ] Accessibility review (WCAG 2.1 AA)
- [ ] **External penetration test**
- [ ] Admin and user documentation

## Later
- Optional AI connector ([ai-integration.md](ai-integration.md))
- PowerPoint, Confluence, SharePoint, Notion export
- SCIM provisioning
- Desktop OCR redaction
- Translations / i18n of UI
- Multi-organisation hosting

## Cost notes
- **No yearly fees.** No paid Apple or Microsoft signing certificates ([ADR 0007](adr/0007-no-paid-code-signing.md)).
- Optional one-time US$5 for the Chrome Web Store (avoidable via enterprise force-install); Firefox AMO and Edge Add-ons are free.
- Hosting: an existing internal VM is typically sufficient.
