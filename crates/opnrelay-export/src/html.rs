// SPDX-License-Identifier: AGPL-3.0-only
//! Single-file HTML export: inline CSS and images, no scripts.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use opnrelay_core::StepKind;

use crate::blocks::Block;
use crate::model::{RenderModel, color_or};
use crate::tokens;

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn blocks_html(blocks: &[Block]) -> String {
    let mut out = String::new();
    let mut in_list = false;
    for b in blocks {
        let mut inner = String::new();
        for r in &b.runs {
            let mut t = esc(&r.text).replace('\n', "<br>");
            if r.code {
                t = format!("<code>{t}</code>");
            }
            if r.italic {
                t = format!("<em>{t}</em>");
            }
            if r.bold {
                t = format!("<strong>{t}</strong>");
            }
            inner.push_str(&t);
        }
        if b.kind == "li" {
            if !in_list {
                out.push_str("<ul>");
                in_list = true;
            }
            out.push_str(&format!("<li>{inner}</li>"));
        } else {
            if in_list {
                out.push_str("</ul>");
                in_list = false;
            }
            out.push_str(&format!("<p>{inner}</p>"));
        }
    }
    if in_list {
        out.push_str("</ul>");
    }
    out
}

fn data_uri(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", STANDARD.encode(png))
}

fn logo_uri(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else {
        "image/png"
    };
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

pub fn render(m: &RenderModel) -> String {
    let primary = color_or(&m.branding.primary_color, "#1f4fd1");
    let header = esc(&tokens::without_pages(&tokens::expand(
        &m.branding.header_text,
        m,
    )));
    let footer = esc(&tokens::without_pages(&tokens::expand(
        &m.branding.footer_text,
        m,
    )));
    let mut h = String::new();
    h.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'\">");
    h.push_str("<meta name=\"robots\" content=\"noindex\">");
    h.push_str(&format!("<title>{}</title>", esc(&m.meta.title)));
    h.push_str(&format!(
        "<style>:root{{--p:{primary}}}\
body{{font-family:Inter,system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:#1b1f24;max-width:860px;margin:0 auto;padding:24px;line-height:1.5}}\
header.brand{{display:flex;align-items:center;gap:12px;border-bottom:3px solid var(--p);padding-bottom:10px;margin-bottom:24px;color:#5d6673}}\
header.brand img{{max-height:40px;max-width:200px}}\
.cls{{display:inline-block;background:#c4314b;color:#fff;font-weight:700;font-size:12px;padding:2px 8px;border-radius:4px;letter-spacing:.05em}}\
h1{{margin:.2em 0}} .meta{{color:#5d6673;font-size:14px}}\
.step{{margin:28px 0;break-inside:avoid}} .step h3{{display:flex;gap:10px;align-items:center;margin:0 0 6px}}\
.num{{flex:none;width:28px;height:28px;border-radius:50%;background:var(--p);color:#fff;display:inline-grid;place-items:center;font-size:14px}}\
.step img{{max-width:100%;border:1px solid #dde1e7;border-radius:6px;margin-top:6px}}\
.callout{{padding:12px 16px;border-radius:8px;margin:20px 0}} .tip{{background:#e7f3ec}} .alert{{background:#fdecec}}\
h2.section{{border-bottom:1px solid #dde1e7;padding-bottom:4px;margin-top:36px}}\
code{{background:#f0f2f5;padding:1px 4px;border-radius:4px}}\
footer{{margin-top:40px;border-top:1px solid #dde1e7;padding-top:8px;color:#5d6673;font-size:12px;display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap}}\
@media print{{body{{max-width:none}}}}</style></head><body>"
    ));
    h.push_str("<header class=\"brand\">");
    if let Some(logo) = &m.branding.logo {
        h.push_str(&format!(
            "<img src=\"{}\" alt=\"{}\">",
            logo_uri(logo),
            esc(&m.branding.company)
        ));
    }
    h.push_str(&format!("<span>{header}</span>"));
    if let Some(c) = m.classification_label() {
        h.push_str(&format!("<span class=\"cls\">{c}</span>"));
    }
    h.push_str("</header><main>");
    h.push_str(&format!("<h1>{}</h1>", esc(&m.meta.title)));
    h.push_str(&format!(
        "<p class=\"meta\">{} · {} · version {}</p>",
        esc(&m.meta.author),
        esc(&m.meta.date),
        m.meta.version
    ));
    if !m.meta.description.is_empty() {
        h.push_str(&format!("<p>{}</p>", esc(&m.meta.description)));
    }
    for s in &m.steps {
        match s.kind {
            StepKind::Heading => {
                h.push_str(&format!("<h2 class=\"section\">{}</h2>", esc(&s.title)))
            }
            StepKind::Tip | StepKind::Alert => {
                let (cls, label) = if s.kind == StepKind::Tip {
                    ("tip", "Tip")
                } else {
                    ("alert", "Important")
                };
                h.push_str(&format!("<div class=\"callout {cls}\"><strong>{label}"));
                if !s.title.is_empty() {
                    h.push_str(&format!(": {}", esc(&s.title)));
                }
                h.push_str("</strong>");
                h.push_str(&blocks_html(&s.body));
                h.push_str("</div>");
            }
            StepKind::Action | StepKind::Text => {
                h.push_str("<section class=\"step\"><h3>");
                if let Some(n) = s.number {
                    h.push_str(&format!("<span class=\"num\">{n}</span>"));
                }
                h.push_str(&format!("<span>{}</span></h3>", esc(&s.title)));
                h.push_str(&blocks_html(&s.body));
                if let Some(img) = &s.image {
                    h.push_str(&format!(
                        "<img src=\"{}\" width=\"{}\" height=\"{}\" alt=\"{}\" loading=\"lazy\">",
                        data_uri(&img.png),
                        img.width,
                        img.height,
                        esc(&s.title)
                    ));
                }
                h.push_str("</section>");
            }
        }
    }
    h.push_str("</main><footer>");
    h.push_str(&format!("<span>{footer}</span>"));
    if let Some(w) = &m.branding.watermark {
        h.push_str(&format!("<span>{}</span>", esc(w)));
    }
    h.push_str("</footer></body></html>\n");
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::model;

    #[test]
    fn html_is_escaped_branded_and_self_contained() {
        let h = render(&model());
        assert!(h.contains("Add a &lt;new&gt; hire &amp; more"));
        assert!(!h.contains("<b>it</b>"), "user HTML must be escaped");
        assert!(h.contains("&lt;b&gt;it&lt;/b&gt;"));
        assert!(h.contains("<strong>wait</strong>"));
        assert!(h.contains("<ul><li>one</li><li>two</li></ul>"));
        assert!(h.contains("CONFIDENTIAL"));
        assert!(h.contains("Exported by Olive"));
        assert!(h.contains("data:image/png;base64,"));
        assert!(!h.contains("<script"));
        assert!(!h.contains("{page}"));
        assert_eq!(h.matches("class=\"num\"").count(), 2);
    }
}
