# OpnRelay

**Open-source, self-hosted, step-by-step guide capture.**

OpnRelay records what you do on your computer or in your browser and turns it into a
clean, branded, shareable how-to guide: numbered steps, screenshots, highlighted click
targets, and optional video/GIF. Everything runs on infrastructure you control: no
third-party cloud, no data leaving your network.

> Status: **design phase**. This repository currently contains the product and technical
> design documents. Code scaffolding follows the [roadmap](docs/roadmap.md).

## Why

Commercial guide-capture tools are useful but are priced per seat, store sensitive
screenshots in a vendor's cloud, and are hard to approve for teams that handle HR records,
patient/EMS data, or audit evidence. OpnRelay aims to give the same everyday workflow with:

- **Self-hosting**: one `docker compose up` on a company server.
- **Security first**: MFA, SSO, encryption at rest, redaction before upload, a tamper-evident audit log.
- **Light weight**: Rust everywhere it matters; a desktop app of about 10 MB and a server image under 50 MB.
- **Open source**: no license fees and an auditable codebase.

## Components

| Component | What it does | Tech |
|---|---|---|
| **Server** (`opnrelay-server`) | Accounts, guides, media, sharing, exports, audit log, web UI hosting | Rust · Axum · PostgreSQL |
| **Web app** | Log in, browse, edit, brand, share and export guides | Svelte + TypeScript (served by the server) |
| **Desktop app** | Record desktop workflows on Windows, macOS and Linux | Rust · Tauri 2 |
| **Browser extension** | Record workflows in Chrome, Edge and Firefox with rich element labels | TypeScript · Manifest V3 |

## Feature summary (v1)

- Capture modes: **full screen**, **active window**, or **region around the click**; screenshots or **video/GIF**
- Auto-generated step titles ("Click **Submit**", "Type in **Employee ID**")
- Editor: reorder, merge, delete steps; annotate, crop, blur, and add tips, alerts and headings
- Redaction: **automatic** (password fields, emails, SSNs, card numbers and other patterns) **and manual** (blur/box tool), applied before upload
- Exports: **PDF, HTML, Markdown, Word (.docx), images (PNG zip)**, plus **MP4/GIF** for recordings
- Company branding (logo, colours, font, header/footer) on every guide and export
- Sharing: internal links, **public links** (with expiry and password), **embeds**; admins can disable public sharing
- Accounts: email + password with **MFA (TOTP / passkeys)**, **SSO (OIDC and SAML)**, teams and roles, and a dedicated **Auditor** role
- Tamper-evident audit log, retention policies, legal hold

## Documentation

| Doc | Contents |
|---|---|
| [Product requirements](docs/product-requirements.md) | Users, use cases, functional and non-functional requirements |
| [Architecture](docs/architecture.md) | System overview, components, data flows, repo layout |
| [Security & compliance](docs/security.md) | Threat model, controls, HIPAA/GDPR/audit considerations |
| [Data model](docs/data-model.md) | Database schema and storage layout |
| [API](docs/api.md) | REST API surface and auth flows |
| [Capture](docs/capture.md) | Desktop and browser-extension recording design per platform |
| [Exports & branding](docs/exports-and-branding.md) | Export formats, rendering pipeline, branding model |
| [Deployment](docs/deployment.md) | Docker Compose, configuration, TLS, backups, upgrades |
| [Roadmap](docs/roadmap.md) | Milestones and scope per phase |
| [AI integration (deferred)](docs/ai-integration.md) | Notes for a future, optional AI connector |
| [Architecture decision records](docs/adr/) | Key decisions and their rationale |

## Contributing & security

See [CONTRIBUTING.md](CONTRIBUTING.md). To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

To be finalised; see [ADR 0006](docs/adr/0006-license.md).
