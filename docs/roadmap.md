# Roadmap

No fixed timeline. Each milestone ends with something usable and demoable.

## M0: Foundations
- [x] Product & technical design docs
- [x] Choose licence: AGPL-3.0 ([ADR 0006](adr/0006-license.md)); `LICENSE` added
- [ ] Cargo workspace, crate skeletons, `apps/` skeletons
- [ ] CI: fmt, clippy, tests, `cargo-deny`, `cargo-audit`, web lint
- [ ] Dockerfile + `compose.yaml` that boots an empty server with Postgres

## M1: Server core & web viewer
- [ ] Email/password auth, sessions, CSRF, rate limits
- [ ] MFA: TOTP + recovery codes
- [ ] Org roles, workspaces, workspace roles, groups
- [ ] Guide/step CRUD, media upload with encryption at rest
- [ ] Audit log (hash chain) for all actions so far
- [ ] Web UI: login, workspace/folder browsing, guide viewer, basic editor (titles, reorder, delete)
- **Demo:** create a guide by uploading screenshots manually; view it in the browser.

## M2: Desktop recorder (Windows + macOS first)
- [ ] `opnrelay-capture`: input hooks, screenshots (full/window/region), element labels
- [ ] Step generation and title templates
- [ ] OAuth PKCE sign-in, keychain storage, encrypted local drafts
- [ ] Resumable upload → draft guide
- [ ] Auto-redaction (structural + patterns)
- **Demo:** record a workflow on Windows/macOS and get a guide on the server.

## M3: Exports & branding
- [ ] Branding settings UI
- [ ] PDF (Typst), HTML, Markdown, DOCX, PNG zip
- [ ] Export jobs, watermarking, audit events
- **Demo:** branded PDF/Word of a recorded guide. *(This milestone is the internal pilot point.)*

## M4: Browser extension
- [ ] Chrome/Edge + Firefox builds, DOM labels, DOM-level redaction
- [ ] Enterprise force-install + managed config
- [ ] Linux desktop (X11; Wayland portal mode)

## M5: Editor & sharing
- [ ] Annotations (arrows, boxes, text), crop, manual blur/box redaction
- [ ] Tips/alerts/headings, insert/merge/split steps, record-into-existing-guide
- [ ] Version history + restore, publish workflow
- [ ] Internal links, public links (password/expiry), embeds, admin sharing policies
- [ ] Full-text search

## M6: Enterprise auth & compliance
- [ ] WebAuthn/passkeys
- [ ] OIDC SSO + group mapping, then SAML 2.0
- [ ] Auditor role UI, audit log search/export/verify
- [ ] Retention policies, legal hold, user data export/erasure

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
