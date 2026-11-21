// SPDX-License-Identifier: AGPL-3.0-only
//! Markdown export: a zip with `guide.md` and an `images/` folder.

use std::io::{Cursor, Write};

use opnrelay_core::StepKind;
use zip::write::SimpleFileOptions;

use crate::model::RenderModel;
use crate::{ExportError, Result, tokens};

pub(crate) fn zip_err(e: impl std::fmt::Display) -> ExportError {
    ExportError::Render(format!("zip: {e}"))
}

/// Escape characters that would start Markdown syntax in a title.
fn md_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|'
        ) {
            out.push('\\');
        }
        out.push(if c == '\n' { ' ' } else { c });
    }
    out
}

pub fn render(m: &RenderModel) -> Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut md = String::new();
    if let Some(logo) = &m.branding.logo {
        let ext = if logo.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            "png"
        };
        let name = format!("images/logo.{ext}");
        zip.start_file(&name, stored).map_err(zip_err)?;
        zip.write_all(logo).map_err(zip_err)?;
        md.push_str(&format!(
            "![{}]({name})\n\n",
            md_inline(&m.branding.company)
        ));
    }
    let header = tokens::without_pages(&tokens::expand(&m.branding.header_text, m));
    if !header.is_empty() && header != m.branding.company {
        md.push_str(&format!("_{}_\n\n", md_inline(&header)));
    }
    if let Some(c) = m.classification_label() {
        md.push_str(&format!("**{c}**\n\n"));
    }
    md.push_str(&format!("# {}\n\n", md_inline(&m.meta.title)));
    md.push_str(&format!(
        "_{} · {} · version {}_\n\n",
        md_inline(&m.meta.author),
        md_inline(&m.meta.date),
        m.meta.version
    ));
    if !m.meta.description.is_empty() {
        md.push_str(&format!("{}\n\n", m.meta.description.trim()));
    }
    let mut img_n = 0;
    for s in &m.steps {
        match s.kind {
            StepKind::Heading => md.push_str(&format!("## {}\n\n", md_inline(&s.title))),
            StepKind::Tip | StepKind::Alert => {
                let label = if s.kind == StepKind::Tip {
                    "Tip"
                } else {
                    "Important"
                };
                md.push_str(&format!("> **{label}:** {}\n", md_inline(&s.title)));
                for line in s.body_md.trim().lines() {
                    md.push_str(&format!("> {line}\n"));
                }
                md.push('\n');
            }
            StepKind::Action | StepKind::Text => {
                match s.number {
                    Some(n) => md.push_str(&format!("### {n}. {}\n\n", md_inline(&s.title))),
                    None => md.push_str(&format!("### {}\n\n", md_inline(&s.title))),
                }
                if !s.body_md.trim().is_empty() {
                    md.push_str(s.body_md.trim());
                    md.push_str("\n\n");
                }
                if let Some(img) = &s.image {
                    img_n += 1;
                    let name = format!("images/step-{img_n:03}.png");
                    zip.start_file(&name, stored).map_err(zip_err)?;
                    zip.write_all(&img.png).map_err(zip_err)?;
                    md.push_str(&format!("![{}]({name})\n\n", md_inline(&s.title)));
                }
            }
        }
    }
    let footer = tokens::without_pages(&tokens::expand(&m.branding.footer_text, m));
    md.push_str("---\n\n");
    md.push_str(&format!("_{}_\n", md_inline(&footer)));
    if let Some(w) = &m.branding.watermark {
        md.push_str(&format!("\n_{}_\n", md_inline(w)));
    }
    zip.start_file("guide.md", opts).map_err(zip_err)?;
    zip.write_all(md.as_bytes()).map_err(zip_err)?;
    Ok(zip.finish().map_err(zip_err)?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::model;
    use std::io::Read;

    #[test]
    fn zip_contains_markdown_and_images() {
        let bytes = render(&model()).unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = z
            .file_names()
            .filter_map(|n| n.ok())
            .map(|n| n.to_string())
            .collect();
        assert!(names.contains(&"guide.md".to_string()));
        assert!(names.contains(&"images/step-001.png".to_string()));
        assert!(names.contains(&"images/step-002.png".to_string()));
        assert!(names.contains(&"images/logo.png".to_string()));
        let mut md = String::new();
        z.by_name("guide.md")
            .unwrap()
            .read_to_string(&mut md)
            .unwrap();
        assert!(md.contains("# Add a \\<new\\> hire & more"));
        assert!(md.contains("### 1. Click \"Save\""));
        assert!(md.contains("> **Tip:** Ask for ID"));
        assert!(md.contains("**CONFIDENTIAL**"));
    }
}
