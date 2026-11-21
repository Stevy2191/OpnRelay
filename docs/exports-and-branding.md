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
| Export watermark | On/off: "Exported by {user} · {datetime}" |
| Classification banner | Automatic from guide classification (e.g. "CONFIDENTIAL") |

Branding applies to: the web viewer, public/embedded share pages, and **every** export format.

## 2. Export formats

| Format | Engine | Contents |
|---|---|---|
| **PDF** | Typst (embedded `typst` crate), with a branded template | Cover page, numbered steps with screenshots, header/footer with logo, page numbers, classification badge, watermark; A4. *Planned:* TOC, Letter size, PDF/A |
| **HTML** | Built-in renderer | Single self-contained `.html` (inline CSS, images as data URIs, strict CSP); responsive; print stylesheet; no scripts |
| **Markdown** | Native | `.zip` with `guide.md` + `images/`; CommonMark; logo included as image |
| **Word (.docx)** | `docx-rs` | Styled headings, numbered steps, images scaled to page width, header/footer with logo |
| **Images** | `image` crate | `.zip` of PNGs with annotations and highlights burned in, named `01-click-save.png` |
| **GIF** *(M7)* | `gif` crate (from steps) / ffmpeg (from video) | Animated walkthrough |
| **Video** *(M7)* | ffmpeg | MP4 (H.264/AAC) or WebM (VP9) with optional branded intro card and step chapter markers |

Planned later: PowerPoint (.pptx), Confluence, SharePoint, Notion.

### Options
- Implemented: include screenshots yes/no; cover page, step numbering and watermark (from branding).
- Planned: image size; page size and orientation; table of contents; label language.

## 3. Rendering pipeline

```
Guide + steps (DB) ──► opnrelay_export::RenderModel  (format-neutral, already permission-checked)
                         │  - screenshots decrypted, cropped, highlight/boxes burned in, ≤1600 px wide
                         │  - step Markdown parsed into a tiny safe block model (text runs only)
                         │  - branding resolved (colours, logo, header/footer tokens, watermark)
                         ▼
             ┌───────────┬────────────┬──────────┬──────────┐
           Typst       HTML        Markdown    docx-rs     PNG zip
            PDF      (1 file)       (zip)       DOCX
```

- The `RenderModel` is built once; each renderer is a pure function `RenderModel → bytes`.
- **No user content is ever interpreted as markup.** Step text reaches Typst as JSON
  strings, HTML output is escaped, Markdown links/raw HTML are reduced to text. Typst runs
  in a sandboxed "world" that can only read the guide's data and images (no file system,
  no packages, no network).
- The [Inter](../crates/opnrelay-export/fonts/OFL.txt) font (SIL OFL 1.1) is bundled, so
  output looks the same on every server. Inter covers Latin, Greek and Cyrillic; add a
  font (e.g. Noto Sans CJK) to `crates/opnrelay-export/fonts/` for other scripts.
- Annotations are stored as vectors and burned in at export time, so editing a highlight
  never degrades the original screenshot.
- v1 renders **synchronously** (`GET /api/v1/guides/{id}/export?format=…`) in a blocking
  thread pool with limits (max 500 screenshots per export, images capped at 1600 px wide,
  60 exports per user per 10 minutes). Nothing is stored; each export is audit-logged.
  A background job queue for bulk/video exports is planned with M7.

## 4. Public / embed views
- Server-rendered HTML (same Askama templates as the HTML export, plus a light viewer script for image zoom).
- Respects `allow_download`; shows the branding; `noindex, nofollow`; no analytics.
