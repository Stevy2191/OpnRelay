// SPDX-License-Identifier: AGPL-3.0-only
//! Word (.docx) export.

use std::io::Cursor;

use docx_rs::{
    AlignmentType, Docx, Footer, Header, NumPages, PageMargin, PageNum, Paragraph, Pic, Run,
    RunFonts,
};
use opnrelay_core::StepKind;

use crate::blocks::Block;
use crate::model::{RenderModel, color_or};
use crate::{ExportError, Result, tokens};

const EMU_PER_PX: u64 = 9525; // at 96 dpi
const EMU_PER_CM: u64 = 360_000;
const MAX_W_EMU: u64 = 17 * EMU_PER_CM;
const MAX_H_EMU: u64 = 19 * EMU_PER_CM;

fn fit(w_px: u32, h_px: u32, max_w: u64, max_h: u64) -> (u32, u32) {
    let (mut w, mut h) = (w_px as u64 * EMU_PER_PX, h_px as u64 * EMU_PER_PX);
    if w > max_w {
        h = h * max_w / w;
        w = max_w;
    }
    if h > max_h {
        w = w * max_h / h;
        h = max_h;
    }
    (w.max(1) as u32, h.max(1) as u32)
}

fn text_runs(p: Paragraph, blocks_runs: &[crate::blocks::Run]) -> Paragraph {
    let mut p = p;
    for r in blocks_runs {
        for (i, line) in r.text.split('\n').enumerate() {
            let mut run = Run::new().add_text(line).size(21);
            if i > 0 {
                run = Run::new()
                    .add_break(docx_rs::BreakType::TextWrapping)
                    .add_text(line)
                    .size(21);
            }
            if r.bold {
                run = run.bold();
            }
            if r.italic {
                run = run.italic();
            }
            if r.code {
                run = run
                    .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas"))
                    .color("8a2b3a");
            }
            p = p.add_run(run);
        }
    }
    p
}

fn body(mut doc: Docx, blocks: &[Block]) -> Docx {
    for b in blocks {
        let p = if b.kind == "li" {
            Paragraph::new().add_run(Run::new().add_text("•  ").size(21))
        } else {
            Paragraph::new()
        };
        doc = doc.add_paragraph(text_runs(p, &b.runs));
    }
    doc
}

/// Footer paragraph with `{page}`/`{pages}` turned into Word fields.
fn footer_paragraph(text: &str) -> Paragraph {
    let mut p = Paragraph::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next_page = rest.find("{page}");
        let next_pages = rest.find("{pages}");
        let (idx, is_pages) = match (next_page, next_pages) {
            (Some(a), Some(b)) if b <= a => (Some(b), true),
            (Some(a), _) => (Some(a), false),
            (None, Some(b)) => (Some(b), true),
            (None, None) => (None, false),
        };
        match idx {
            Some(i) => {
                if i > 0 {
                    p = p.add_run(Run::new().add_text(&rest[..i]).size(16).color("5d6673"));
                }
                if is_pages {
                    p = p.add_num_pages(NumPages::new());
                    rest = &rest[i + "{pages}".len()..];
                } else {
                    p = p.add_page_num(PageNum::new());
                    rest = &rest[i + "{page}".len()..];
                }
            }
            None => {
                p = p.add_run(Run::new().add_text(rest).size(16).color("5d6673"));
                rest = "";
            }
        }
    }
    p
}

pub fn render(m: &RenderModel) -> Result<Vec<u8>> {
    let primary = color_or(&m.branding.primary_color, "#1f4fd1")[1..].to_string();
    let header_text = tokens::expand(&m.branding.header_text, m)
        .replace("{page}", "")
        .replace("{pages}", "");
    let footer_text = tokens::expand(&m.branding.footer_text, m);

    let mut header_p = Paragraph::new();
    if let Some(logo) = &m.branding.logo
        && let Ok(img) = image::load_from_memory(logo)
    {
        let (w, h) = fit(
            img.width(),
            img.height(),
            5 * EMU_PER_CM,
            (EMU_PER_CM * 9) / 10,
        );
        let mut png = Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| ExportError::Image(e.to_string()))?;
        header_p = header_p.add_run(Run::new().add_image(
            Pic::new_with_dimensions(png.into_inner(), img.width(), img.height()).size(w, h),
        ));
        header_p = header_p.add_run(Run::new().add_text("   "));
    }
    header_p = header_p.add_run(
        Run::new()
            .add_text(header_text.trim())
            .size(18)
            .color("5d6673"),
    );
    if let Some(c) = m.classification_label() {
        header_p = header_p.add_run(
            Run::new()
                .add_text(format!("   {c}"))
                .bold()
                .size(18)
                .color("c4314b"),
        );
    }

    let mut footer = Footer::new().add_paragraph(footer_paragraph(&footer_text));
    if let Some(w) = &m.branding.watermark {
        footer = footer.add_paragraph(
            Paragraph::new().add_run(Run::new().add_text(w).size(14).color("9aa5b1")),
        );
    }

    let mut doc = Docx::new()
        .page_size(11906, 16838) // A4 in twips
        .page_margin(
            PageMargin::new()
                .top(1134)
                .bottom(1134)
                .left(1134)
                .right(1134),
        )
        .header(Header::new().add_paragraph(header_p))
        .footer(footer);

    doc = doc.add_paragraph(
        Paragraph::new().add_run(
            Run::new()
                .add_text(&m.meta.title)
                .bold()
                .size(40)
                .color(primary.clone()),
        ),
    );
    doc = doc.add_paragraph(
        Paragraph::new().add_run(
            Run::new()
                .add_text(format!(
                    "{} · {} · version {}",
                    m.meta.author, m.meta.date, m.meta.version
                ))
                .size(18)
                .color("5d6673"),
        ),
    );
    if !m.meta.description.is_empty() {
        doc = doc.add_paragraph(
            Paragraph::new().add_run(Run::new().add_text(&m.meta.description).size(22)),
        );
    }

    for s in &m.steps {
        match s.kind {
            StepKind::Heading => {
                doc = doc.add_paragraph(
                    Paragraph::new().add_run(
                        Run::new()
                            .add_text(&s.title)
                            .bold()
                            .size(30)
                            .color(primary.clone()),
                    ),
                );
            }
            StepKind::Tip | StepKind::Alert => {
                let (label, color) = if s.kind == StepKind::Tip {
                    ("Tip", "18794e")
                } else {
                    ("Important", "c4314b")
                };
                let mut p = Paragraph::new().add_run(
                    Run::new()
                        .add_text(format!("{label}: "))
                        .bold()
                        .size(21)
                        .color(color),
                );
                p = p.add_run(Run::new().add_text(&s.title).bold().size(21));
                doc = doc.add_paragraph(p);
                doc = body(doc, &s.body);
            }
            StepKind::Action | StepKind::Text => {
                let title = match s.number {
                    Some(n) => format!("{n}.  {}", s.title),
                    None => s.title.clone(),
                };
                doc = doc.add_paragraph(
                    Paragraph::new().add_run(Run::new().add_text(title).bold().size(24)),
                );
                doc = body(doc, &s.body);
                if let Some(img) = &s.image {
                    let (w, h) = fit(img.width, img.height, MAX_W_EMU, MAX_H_EMU);
                    doc = doc.add_paragraph(
                        Paragraph::new().align(AlignmentType::Left).add_run(
                            Run::new().add_image(
                                Pic::new_with_dimensions(img.png.clone(), img.width, img.height)
                                    .size(w, h),
                            ),
                        ),
                    );
                }
            }
        }
    }
    let mut out = Cursor::new(Vec::new());
    doc.build()
        .pack(&mut out)
        .map_err(|e| ExportError::Render(format!("docx: {e}")))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::model;
    use std::io::Read;

    #[test]
    fn docx_is_a_valid_package_with_content() {
        let bytes = render(&model()).unwrap();
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        z.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("Add a &lt;new&gt; hire &amp; more"));
        assert!(xml.contains("1.  Click"));
        assert!(xml.contains("Tip: "));
        let media = z
            .file_names()
            .filter_map(|n| n.ok())
            .filter(|n| n.starts_with("word/media/"))
            .count();
        assert!(media >= 3, "logo + 2 screenshots, got {media}");
    }

    #[test]
    fn fit_keeps_aspect_ratio() {
        let (w, h) = fit(4000, 1000, MAX_W_EMU, MAX_H_EMU);
        assert_eq!(w as u64, MAX_W_EMU);
        assert_eq!(h as u64, MAX_W_EMU / 4);
        let (w, h) = fit(100, 4000, MAX_W_EMU, MAX_H_EMU);
        assert_eq!(h as u64, MAX_H_EMU);
        assert!(w < h);
    }
}
