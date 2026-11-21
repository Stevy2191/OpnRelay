// SPDX-License-Identifier: AGPL-3.0-only
//! Format-neutral render model.

use opnrelay_core::{Classification, Step, StepKind};

use crate::annotate;
use crate::blocks::{self, Block};
use crate::{ExportError, Result};

/// Hard cap to keep exports bounded in time and memory.
pub const MAX_IMAGE_STEPS: usize = 500;

/// Company branding applied to every export.
#[derive(Debug, Clone)]
pub struct Branding {
    pub company: String,
    /// Logo as PNG/JPEG bytes (already validated by the server).
    pub logo: Option<Vec<u8>>,
    pub primary_color: String,
    pub accent_color: String,
    pub header_text: String,
    pub footer_text: String,
    pub cover_page: bool,
    pub show_step_numbers: bool,
    /// e.g. "Exported by Jane Doe · 2026-11-21 14:03"
    pub watermark: Option<String>,
}

impl Default for Branding {
    fn default() -> Self {
        Self {
            company: String::new(),
            logo: None,
            primary_color: "#1f4fd1".into(),
            accent_color: "#f59e0b".into(),
            header_text: "{company}".into(),
            footer_text: "{guide_title} · Page {page} of {pages}".into(),
            cover_page: true,
            show_step_numbers: true,
            watermark: None,
        }
    }
}

/// Guide metadata.
#[derive(Debug, Clone)]
pub struct GuideMeta {
    pub title: String,
    pub description: String,
    pub classification: Classification,
    pub author: String,
    /// Human-readable date of last update, e.g. "21 Nov 2026".
    pub date: String,
    pub version: i32,
}

/// A screenshot ready for output (annotations burned in, PNG encoded).
#[derive(Debug, Clone)]
pub struct RenderImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct RenderStep {
    /// Display number for action steps (when numbering is on).
    pub number: Option<u32>,
    pub kind: StepKind,
    pub title: String,
    pub body_md: String,
    pub body: Vec<Block>,
    pub image: Option<RenderImage>,
}

#[derive(Debug, Clone)]
pub struct RenderModel {
    pub meta: GuideMeta,
    pub branding: Branding,
    pub steps: Vec<RenderStep>,
}

impl RenderModel {
    /// Build the model. `steps` pairs each step with its screenshot bytes
    /// (if it has one and images are wanted).
    pub fn build(
        meta: GuideMeta,
        branding: Branding,
        steps: Vec<(Step, Option<Vec<u8>>)>,
    ) -> Result<Self> {
        let with_images = steps.iter().filter(|(_, img)| img.is_some()).count();
        if with_images > MAX_IMAGE_STEPS {
            return Err(ExportError::TooLarge(MAX_IMAGE_STEPS));
        }
        let mut n = 0u32;
        let mut out = Vec::with_capacity(steps.len());
        for (step, img) in steps {
            let number = if step.kind.is_numbered() {
                n += 1;
                branding.show_step_numbers.then_some(n)
            } else {
                None
            };
            let image = match img {
                Some(bytes) => Some(annotate::prepare(
                    &bytes,
                    &step.annotations,
                    &branding.accent_color,
                )?),
                None => None,
            };
            out.push(RenderStep {
                number,
                kind: step.kind,
                body: blocks::parse(&step.body_md),
                body_md: step.body_md,
                title: step.title,
                image,
            });
        }
        Ok(Self {
            meta,
            branding,
            steps: out,
        })
    }

    pub fn classification_label(&self) -> Option<&'static str> {
        match self.meta.classification {
            Classification::Internal => None,
            Classification::Confidential => Some("CONFIDENTIAL"),
            Classification::Restricted => Some("RESTRICTED"),
        }
    }
}

/// Validate a `#rrggbb` colour, falling back to `default`.
pub fn color_or(c: &str, default: &'static str) -> String {
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit());
    if ok {
        c.to_ascii_lowercase()
    } else {
        default.to_string()
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use opnrelay_core::{Annotations, Rect};

    fn new_id(i: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(i)
    }

    pub fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, _| image::Rgb([(x % 255) as u8, 200, 220]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    pub fn model() -> RenderModel {
        let mk = |i: u128, kind: StepKind, title: &str, body: &str| Step {
            id: new_id(i),
            kind,
            title: title.into(),
            body_md: body.into(),
            action: None,
            media_id: None,
            annotations: Annotations::default(),
        };
        let mut s2 = mk(
            2,
            StepKind::Action,
            "Click \"Save\"",
            "Then **wait** for <b>it</b>.\n\n- one\n- two",
        );
        s2.annotations.highlight = Some(Rect {
            x: 10,
            y: 10,
            w: 40,
            h: 20,
        });
        RenderModel::build(
            GuideMeta {
                title: "Add a <new> hire & more".into(),
                description: "HR onboarding".into(),
                classification: Classification::Confidential,
                author: "Olive Owner".into(),
                date: "21 Nov 2026".into(),
                version: 3,
            },
            Branding {
                company: "Acme Health".into(),
                logo: Some(png(120, 40)),
                watermark: Some("Exported by Olive".into()),
                ..Branding::default()
            },
            vec![
                (mk(1, StepKind::Heading, "Before you start", ""), None),
                (s2, Some(png(320, 200))),
                (
                    mk(3, StepKind::Tip, "Ask for ID", "Use `HR-123` format"),
                    None,
                ),
                (
                    mk(4, StepKind::Action, "Close the form", ""),
                    Some(png(200, 1200)),
                ),
            ],
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_only_action_steps() {
        let m = testutil::model();
        let nums: Vec<_> = m.steps.iter().map(|s| s.number).collect();
        assert_eq!(nums, vec![None, Some(1), None, Some(2)]);
        assert_eq!(m.classification_label(), Some("CONFIDENTIAL"));
    }

    #[test]
    fn colors() {
        assert_eq!(color_or("#ABCDEF", "#000000"), "#abcdef");
        assert_eq!(color_or("red; x", "#000000"), "#000000");
    }
}
