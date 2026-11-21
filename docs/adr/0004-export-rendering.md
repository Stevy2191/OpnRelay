# ADR 0004: Typst for PDF; format-neutral render model

**Status:** Accepted

## Context
Exports must be high quality, branded, and consistent across PDF, HTML, Markdown, DOCX and images, generated entirely on the server without external services.

## Decision
Build a format-neutral `RenderModel` once per export. Render PDF with **Typst** embedded as a library (branded templates), HTML with Askama, DOCX with `docx-rs`, images with `image`/`gif`.

## Consequences
- Pure Rust: no headless Chrome, LibreOffice or wkhtmltopdf in the image (smaller and more secure).
- Typst templates make branding (fonts, colours, header/footer) declarative.
- PDF/A support via Typst.

## Alternatives considered
- **Headless Chromium print-to-PDF**: pixel-perfect HTML parity, but adds ~300 MB and a large attack surface.
- **printpdf/genpdf**: too low-level for good layout.
- **LibreOffice for DOCX→PDF**: very heavy.
