# Roadmap

No fixed timeline. Each milestone ends with something usable and demoable.

## M0: Foundations
- [x] Product & technical design docs
- [ ] Choose licence ([ADR 0006](adr/0006-license.md)); add `LICENSE`
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
- [ ] Sensitive workspace enforcement

## M7: Video & GIF
- [ ] Desktop video recording (steps + video), extension tab recording
- [ ] Worker transcoding, GIF generation, MP4/WebM export

## M8: Release hardening → v1.0
- [ ] Code signing (Windows Authenticode, Apple notarisation), signed auto-updates
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
- Apple Developer Program: about US$99/year (macOS signing/notarisation)
- Windows code-signing certificate: about US$200–500/year (or Azure Trusted Signing, which is cheaper)
- Chrome Web Store one-time US$5; Firefox AMO and Edge Add-ons are free
- Hosting: an existing internal VM is typically sufficient
