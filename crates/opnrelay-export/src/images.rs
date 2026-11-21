// SPDX-License-Identifier: AGPL-3.0-only
//! Images export: a zip of annotated screenshots named after their steps.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use crate::markdown::zip_err;
use crate::model::RenderModel;
use crate::{Result, slug};

pub fn render(m: &RenderModel) -> Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut n = 0;
    for s in &m.steps {
        if let Some(img) = &s.image {
            n += 1;
            let name = format!("{n:02}-{}.png", slug(&s.title));
            zip.start_file(name, opts).map_err(zip_err)?;
            zip.write_all(&img.png).map_err(zip_err)?;
        }
    }
    Ok(zip.finish().map_err(zip_err)?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::model;

    #[test]
    fn names_follow_steps() {
        let z = zip::ZipArchive::new(Cursor::new(render(&model()).unwrap())).unwrap();
        let names: Vec<String> = z
            .file_names()
            .filter_map(|n| n.ok())
            .map(|n| n.to_string())
            .collect();
        assert!(names.iter().any(|n| n == "01-click-save.png"));
        assert!(names.iter().any(|n| n == "02-close-the-form.png"));
    }
}
