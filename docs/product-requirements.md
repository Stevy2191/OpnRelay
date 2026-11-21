# Product Requirements

## 1. Goal

Replace a commercial step-by-step guide tool for an organisation of roughly **30–40 users**
with a self-hosted, open-source product that is at least as convenient for everyday
documentation, and used by every department, including **HR**, **EMS (emergency
medical services)** and **auditors**.

**Principle:** authors are trusted to decide what they show. No department is restricted
by default. OpnRelay provides redaction tools and access controls, but it does not force
them on anyone.

## 2. Users and roles

| Persona | Needs |
|---|---|
| **Author** (any staff) | Record a process quickly, tidy it up, share it |
| **HR staff** | Document HR system procedures; same tools as everyone else; they choose when to redact and who can see a guide |
| **EMS staff** | Document ePCR / dispatch / records workflows; same tools as everyone else; they choose when to redact and who can see a guide |
| **Viewer** | Find and follow guides; export them for training |
| **Auditor** (internal or external) | Read-only access to guides plus the full audit trail; export evidence |
| **Administrator** | Manage users, SSO, roles, branding, policies, retention |

### Role model

Organisation-level roles:

| Role | Summary |
|---|---|
| **Owner** | Everything, including deleting the organisation and managing owners |
| **Admin** | Users, groups, SSO, policies, branding, all workspaces |
| **Member** | Normal user; access is governed by workspace roles |
| **Auditor** | Read-only access to all guides in workspaces they are granted (or all), plus the audit log and audit exports. Cannot edit, share publicly or delete. |

Workspace-level roles (per team, e.g. "HR", "EMS", "IT"):

| Role | Summary |
|---|---|
| **Manager** | Manage workspace members and settings, plus everything an Editor can do |
| **Editor** | Create, edit, publish, share and export guides |
| **Viewer** | View and export published guides (if allowed by policy) |

Groups (manually managed or synced from SSO) can be assigned workspace roles.

## 3. Functional requirements

### 3.1 Capture
- **FR-C1** Desktop recorder for Windows 10/11, macOS 13+, and Linux (X11 and Wayland, with documented Wayland limits).
- **FR-C2** Browser extension for Chrome, Edge and Firefox (Manifest V3).
- **FR-C3** Screenshot modes, selectable per recording and defaulted by admin policy:
  full screen, active window, region around the click (configurable size).
- **FR-C4** Recording types: **steps** (screenshot per action), **video** (continuous), or **both** (video with step markers). GIF can be produced from video or from steps.
- **FR-C5** Each step captures: action type (click, double-click, right-click, type, key shortcut, navigate, app switch), target label (button/field name where available), app/window title or URL, cursor position, timestamp.
- **FR-C6** Auto-generated step title, e.g. `Click "Save"`, `Type in "Employee ID"`, `Navigate to hr.example.com/onboarding`.
- **FR-C7** Typed text: by default steps say "Type in field X" without the text. The user can turn on "include typed text" per recording. Password fields are never captured. This is a fixed safety baseline, not a restriction on content.
- **FR-C8** Pause/resume and stop controls, a global hotkey, and an always-visible recording indicator.
- **FR-C9** App/site exclusion list (e.g. password managers, banking); captures are suppressed while these are focused.
- **FR-C10** Offline recording; drafts are stored encrypted locally and uploaded when the server is reachable.

### 3.2 Redaction
- **FR-R1** **Automatic redaction** (the user turns it on or off per recording; the default is set in the user's preferences): password fields, elements tagged sensitive, and pattern matches (email, phone, SSN, credit-card numbers, dates of birth, MRN-like IDs), plus admin-defined regexes and CSS selectors (extension).
- **FR-R2** **Manual redaction** in the editor: blur, pixelate, solid box.
- **FR-R3** Automatic redaction happens **on the client before upload**.
- **FR-R4** Redaction is destructive once the guide is saved: original pixels are discarded. Undo is available while editing.
- **FR-R5** No per-department restrictions. HR, EMS and every other workspace get the same capture, video, sharing and export options. Who can *see* a guide is controlled by normal workspace membership.

### 3.3 Guides and editor
- **FR-G1** Guide = title, description, ordered steps, metadata (owner, workspace, folder, tags, classification, status).
- **FR-G2** Step types: action (with screenshot), text, heading/section, tip, warning/alert.
- **FR-G3** Edit step titles/bodies (Markdown subset); reorder, merge, split, delete, insert steps.
- **FR-G4** Image tools: crop, re-highlight click target, arrow/box/text annotations, blur.
- **FR-G5** Add screenshots manually (paste/upload) and record additional steps into an existing guide.
- **FR-G6** Draft → Published lifecycle; version history with restore.
- **FR-G7** Folders, tags, full-text search across titles, step text and labels.
- **FR-G8** Duplicate guide; move between workspaces (subject to permissions).

### 3.4 Accounts and access
- **FR-A1** Email + password (Argon2id) with **MFA**: TOTP and WebAuthn/passkeys; recovery codes.
- **FR-A2** **SSO**: OIDC (Entra ID, Okta, Google Workspace, Keycloak and others) and SAML 2.0.
- **FR-A3** Admin can require MFA, require SSO-only, and set session idle/absolute timeouts.
- **FR-A4** JIT user provisioning from SSO, plus group/role mapping from IdP claims. SCIM is a later phase.
- **FR-A5** Invitations by email (SMTP) or via a copyable invite link when SMTP is not configured.
- **FR-A6** Desktop app and extension sign in through the browser (OAuth 2.0 authorisation code + PKCE); tokens are kept in the OS keychain / extension storage.
- **FR-A7** Users can see and revoke their sessions and devices; admins can revoke any.

### 3.5 Sharing
- **FR-S1** Internal link (requires login and permission).
- **FR-S2** Public link: optional expiry, optional password, optional "allow download", revocable, view counter.
- **FR-S3** Embed (iframe) for intranet pages; allowed embed origins are configured by an admin.
- **FR-S4** Admin policies: disable public links globally or per workspace; maximum expiry.

### 3.6 Export
- **FR-E1** Formats: **PDF, HTML (single self-contained file), Markdown (zip with images), Word (.docx), PNG images (zip)**; recordings as **MP4 (H.264) / WebM and GIF**.
- **FR-E2** All exports carry company branding (see 3.7).
- **FR-E3** Options: include/exclude screenshots, step numbering, page size (A4/Letter), table of contents, guide metadata.
- **FR-E4** Exports are logged in the audit log (who, what, format).
- **FR-E5** Bulk export of a folder/workspace (zip) for Admins/Auditors.
- *Later:* PowerPoint, Confluence, SharePoint, Notion.

### 3.7 Branding (per company)
- **FR-B1** Company logo (SVG/PNG), primary and accent colours, font choice (bundled open fonts), header and footer text, optional cover page.
- **FR-B2** Branding applies to the web viewer, public/embedded views and all exports.
- **FR-B3** Optional "confidential" footer derived from guide classification.

### 3.8 Audit and compliance
- **FR-AU1** Append-only, hash-chained audit log covering: logins (success and failure), MFA changes, role changes, guide create/view/edit/publish/delete/export/share, link access, settings changes, and admin actions.
- **FR-AU2** Audit log search, filter and export (CSV/JSON) for Admin and Auditor roles; a verification tool checks chain integrity.
- **FR-AU3** Retention policies per workspace (e.g. auto-delete drafts after N days, archive after N years) and **legal hold** that blocks deletion.
- **FR-AU4** Data subject tools: export all of a user's data, and delete or anonymise a user while preserving audit integrity.

## 4. Non-functional requirements

| Area | Requirement |
|---|---|
| **Security** | OWASP ASVS Level 2 as the target; see [security.md](security.md) |
| **Footprint** | Server idle RAM < 100 MB (excluding Postgres); image < 50 MB; desktop installer ≈ 10–15 MB; desktop idle RAM < 80 MB |
| **Performance** | Step capture latency < 150 ms; guide with 50 steps opens in < 1 s on LAN; PDF export of 50 steps in < 5 s |
| **Scale** | Designed for 30–40 users and tens of thousands of steps; vertical scaling to roughly 500 users without redesign |
| **Availability** | Single node; documented backup/restore; RPO 24 h by default (configurable) |
| **Deployment** | `docker compose up` brings up everything; no external services required |
| **Accessibility** | Web UI and HTML exports meet WCAG 2.1 AA; PDFs tagged where feasible |
| **Portability** | amd64 and arm64 server images |
| **Privacy** | No telemetry. No outbound network calls except those configured by the admin (SMTP, IdP). |
| **Open source** | All dependencies must use OSI-approved licences compatible with the project licence |

## 5. Out of scope for v1

- AI features (see [ai-integration.md](ai-integration.md))
- PowerPoint / Confluence / SharePoint / Notion export
- Multi-tenant SaaS hosting
- Mobile capture apps
- SCIM provisioning (planned for after v1)
