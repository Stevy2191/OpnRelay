// SPDX-License-Identifier: AGPL-3.0-only
//! PDF export via the Typst typesetting engine, embedded as a library.
//!
//! All guide content is passed to the template as JSON data and images as
//! virtual files; nothing user-provided is ever parsed as Typst markup.

use std::collections::HashMap;
use std::sync::OnceLock;

use opnrelay_core::StepKind;
use serde_json::{Value, json};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::model::{RenderModel, color_or};
use crate::{ExportError, Result, tokens};

const TEMPLATE: &str = include_str!("guide.typ");

static INTER: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter_400Regular.ttf"),
    include_bytes!("../fonts/Inter_400Regular_Italic.ttf"),
    include_bytes!("../fonts/Inter_600SemiBold.ttf"),
    include_bytes!("../fonts/Inter_700Bold.ttf"),
];

struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let fonts: Vec<Font> = INTER
            .iter()
            .copied()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        Shared {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

fn file_id(path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("static path"),
    )
    .intern()
}

struct GuideWorld {
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl World for GuideWorld {
    fn library(&self) -> &LazyHash<Library> {
        &shared().library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &shared().book
    }
    fn main(&self) -> FileId {
        self.main.id()
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }
    fn font(&self, index: usize) -> Option<Font> {
        shared().fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// Usable page area for screenshots (A4 minus margins), in cm.
const MAX_IMG_W_CM: f64 = 16.0;
const MAX_IMG_H_CM: f64 = 19.0;

fn image_width_cm(w: u32, h: u32) -> f64 {
    let aspect = h as f64 / w.max(1) as f64;
    let natural_cm = w as f64 / 96.0 * 2.54;
    natural_cm
        .min(MAX_IMG_W_CM)
        .min(MAX_IMG_H_CM / aspect)
        .max(1.0)
}

fn build_data(m: &RenderModel, files: &mut HashMap<FileId, Bytes>) -> Value {
    let mut steps = Vec::new();
    for (i, s) in m.steps.iter().enumerate() {
        let image = s.image.as_ref().map(|img| {
            let path = format!("img/{i}.png");
            files.insert(file_id(&path), Bytes::new(img.png.clone()));
            json!({ "path": format!("/{path}"), "width_cm": image_width_cm(img.width, img.height) })
        });
        let kind = match s.kind {
            StepKind::Action => "action",
            StepKind::Text => "text",
            StepKind::Heading => "heading",
            StepKind::Tip => "tip",
            StepKind::Alert => "alert",
        };
        steps.push(json!({
            "kind": kind,
            "number": s.number,
            "title": s.title,
            "body": s.body,
            "image": image,
        }));
    }
    let logo = m.branding.logo.as_ref().map(|bytes| {
        let ext = if bytes.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            "png"
        };
        let path = format!("logo.{ext}");
        files.insert(file_id(&path), Bytes::new(bytes.clone()));
        format!("/{path}")
    });
    json!({
        "title": m.meta.title,
        "description": m.meta.description,
        "author": m.meta.author,
        "date": m.meta.date,
        "version": m.meta.version,
        "company": m.branding.company,
        "classification": m.classification_label(),
        "primary": color_or(&m.branding.primary_color, "#1f4fd1"),
        "accent": color_or(&m.branding.accent_color, "#f59e0b"),
        "header": tokens::expand(&m.branding.header_text, m).replace("{page}", "").replace("{pages}", ""),
        "footer": tokens::expand(&m.branding.footer_text, m),
        "cover": m.branding.cover_page,
        "watermark": m.branding.watermark,
        "logo": logo,
        "steps": steps,
    })
}

pub fn render(m: &RenderModel) -> Result<Vec<u8>> {
    let mut files = HashMap::new();
    let data = build_data(m, &mut files);
    files.insert(
        file_id("data.json"),
        Bytes::new(serde_json::to_vec(&data).map_err(|e| ExportError::Render(e.to_string()))?),
    );
    let world = GuideWorld {
        main: Source::new(file_id("main.typ"), TEMPLATE.to_string()),
        files,
    };
    let doc: PagedDocument = typst::compile(&world).output.map_err(|diags| {
        ExportError::Render(
            diags
                .iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()).map_err(|diags| {
        ExportError::Render(
            diags
                .iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::model;

    #[test]
    fn renders_a_pdf() {
        let pdf = render(&model()).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 5_000);
    }

    #[test]
    fn markup_in_content_is_not_interpreted() {
        let mut m = model();
        m.meta.title = "#panic(\"x\") *not bold* $math$ #import \"/etc/passwd\"".into();
        m.steps[1].title = "]) #read(\"/data.json\") [".into();
        assert!(render(&m).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn tall_images_are_limited_by_height() {
        assert!(image_width_cm(200, 4000) < 2.0);
        assert_eq!(image_width_cm(4000, 1000), MAX_IMG_W_CM);
    }
}
