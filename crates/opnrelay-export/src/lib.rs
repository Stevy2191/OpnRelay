// SPDX-License-Identifier: AGPL-3.0-only
//! Export engine: turns a guide into PDF, HTML, Markdown, Word or images.
//!
//! Every renderer is a pure function of a [`RenderModel`], which is built
//! once per export: screenshots are decoded, annotations burned in, Markdown
//! parsed into a tiny safe block model, and branding resolved.

pub mod annotate;
pub mod blocks;
mod docx;
mod html;
mod images;
mod markdown;
pub mod model;
mod pdf;
mod tokens;

pub use model::{Branding, GuideMeta, RenderModel};

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("image could not be processed: {0}")]
    Image(String),
    #[error("rendering failed: {0}")]
    Render(String),
    #[error("too large to export (max {0} steps with images)")]
    TooLarge(usize),
}

pub type Result<T> = std::result::Result<T, ExportError>;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Html,
    Markdown,
    Docx,
    Images,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pdf" => Format::Pdf,
            "html" => Format::Html,
            "md" | "markdown" => Format::Markdown,
            "docx" | "word" => Format::Docx,
            "png" | "images" => Format::Images,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Html => "html",
            Format::Markdown => "md",
            Format::Docx => "docx",
            Format::Images => "png",
        }
    }
}

/// A finished export file.
pub struct Output {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    /// File extension without the dot.
    pub extension: &'static str,
}

/// Render `model` in `format`.
pub fn render(model: &RenderModel, format: Format) -> Result<Output> {
    Ok(match format {
        Format::Pdf => Output {
            bytes: pdf::render(model)?,
            mime: "application/pdf",
            extension: "pdf",
        },
        Format::Html => Output {
            bytes: html::render(model).into_bytes(),
            mime: "text/html; charset=utf-8",
            extension: "html",
        },
        Format::Markdown => Output {
            bytes: markdown::render(model)?,
            mime: "application/zip",
            extension: "zip",
        },
        Format::Docx => Output {
            bytes: docx::render(model)?,
            mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            extension: "docx",
        },
        Format::Images => Output {
            bytes: images::render(model)?,
            mime: "application/zip",
            extension: "zip",
        },
    })
}

/// A file-name-safe slug of a title.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 60 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "guide".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Log a new incident (CAD)!"), "log-a-new-incident-cad");
        assert_eq!(slug("ÄÖ"), "guide");
        assert!(slug(&"x".repeat(200)).len() <= 60);
    }

    #[test]
    fn formats() {
        for f in [
            Format::Pdf,
            Format::Html,
            Format::Markdown,
            Format::Docx,
            Format::Images,
        ] {
            assert_eq!(Format::parse(f.as_str()), Some(f));
        }
        assert_eq!(Format::parse("exe"), None);
    }
}

#[cfg(test)]
mod samples {
    /// `OPNRELAY_EXPORT_SAMPLES=/some/dir cargo test -p opnrelay-export write_samples`
    /// writes every format for manual inspection.
    #[test]
    fn write_samples() {
        let Ok(dir) = std::env::var("OPNRELAY_EXPORT_SAMPLES") else {
            return;
        };
        let m = crate::model::testutil::model();
        for f in [
            super::Format::Pdf,
            super::Format::Html,
            super::Format::Markdown,
            super::Format::Docx,
            super::Format::Images,
        ] {
            let out = super::render(&m, f).unwrap();
            std::fs::write(
                format!("{dir}/sample-{}.{}", f.as_str(), out.extension),
                out.bytes,
            )
            .unwrap();
        }
    }
}
