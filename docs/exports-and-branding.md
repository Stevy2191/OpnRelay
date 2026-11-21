# Exports & Branding

## 1. Branding model (per company)

Configured by Admins under **Settings → Branding** and stored in `branding`
([data-model.md](data-model.md#settings)).

| Setting | Notes |
|---|---|
| Logo (light) / Logo (dark) | SVG (sanitised with `usvg`, scripts stripped) or PNG ≤ 2 MB |
| Primary / accent colour | Hex; contrast checked against white/black text for WCAG AA |
| Font | Bundled open-licence fonts (Inter, Source Sans 3, IBM Plex Sans, Noto Sans for wide script coverage). Custom font upload (OFL/TTF) later. |
| Header text / footer text | Plain text with tokens: `{company}`, `{guide_title}`, `{date}`, `{page}`, `{pages}`, `{classification}`, `{author}` |
| Cover page | On/off: logo, title, description, author, date, version |
| Step numbers | On/off |
| Export watermark | On/off: "Exported by {user} · {datetime}" (recommended for HR/EMS) |
| Classification banner | Automatic from guide classification (e.g. "CONFIDENTIAL") |

Branding applies to: the web viewer, public/embedded share pages, and **every** export format.

## 2. Export formats

| Format | Engine | Contents |
|---|---|---|
| **PDF** | Typst (embedded `typst` crate), with a branded template compiled at runtime | Cover page, optional TOC, numbered steps with screenshots, header/footer, page numbers; A4/Letter; PDF/A-2b option for archiving |
| **HTML** | Askama templates | Single self-contained `.html` (inline CSS, images as data URIs); responsive; print stylesheet; no scripts |
| **Markdown** | Native | `.zip` with `guide.md` + `images/`; CommonMark; logo included as image |
| **Word (.docx)** | `docx-rs` | Styled headings, numbered steps, images scaled to page width, header/footer with logo |
| **Images** | `image` crate | `.zip` of PNGs with annotations and highlights burned in, named `01-click-save.png` |
| **GIF** | `gif` crate (from steps) / ffmpeg (from video) | Animated walkthrough |
| **Video** | ffmpeg | MP4 (H.264/AAC) or WebM (VP9) with optional branded intro card and step chapter markers |

Planned later: PowerPoint (.pptx), Confluence, SharePoint, Notion.

### Options (all formats where applicable)
- Include screenshots: yes/no; image size: small/medium/full
- Include step descriptions, tips and alerts
- Include metadata block (owner, version, last updated)
- Page size and orientation (PDF/DOCX)
- Table of contents (PDF/DOCX/HTML)
- Language of generated labels (UI locale)

## 3. Rendering pipeline

```
Guide + steps (DB) ──► opnrelay-core::RenderModel  (format-neutral, already permission-filtered)
                         │  - steps with resolved text
                         │  - images: decrypted, annotations burned in (opnrelay-redact/annotate)
                         │  - branding resolved (colours, fonts, logo bytes, header/footer tokens)
                         ▼
             ┌───────────┼────────────┬──────────┬──────────┐
           Typst       Askama       Markdown    docx-rs    image/gif
            PDF         HTML          MD         DOCX       PNG/GIF
```

- The `RenderModel` is built once; each renderer is a pure function `RenderModel → bytes`. This makes golden-file snapshot tests easy (`insta`).
- Annotations are stored as vectors and burned in at export time, so editing a highlight never degrades the original screenshot.
- Rendering runs in the worker with resource limits (max pages, max image dimensions, timeout) to prevent export-based DoS.
- Output is encrypted like any media object and expires after 24 h (configurable).

## 4. Public / embed views
- Server-rendered HTML (same Askama templates as the HTML export, plus a light viewer script for image zoom).
- Respects `allow_download`; shows the branding; `noindex, nofollow`; no analytics.
