// SPDX-License-Identifier: AGPL-3.0-only
//! Burns vector annotations into a copy of a screenshot.

use std::io::Cursor;

use image::{ImageFormat, Rgba, RgbaImage, imageops};
use opnrelay_core::{Annotations, Rect};

use crate::model::{RenderImage, color_or};
use crate::{ExportError, Result};

/// Exported screenshots are scaled down to at most this width.
pub const MAX_WIDTH: u32 = 1600;

fn parse_color(hex: &str) -> Rgba<u8> {
    let h = color_or(hex, "#f59e0b");
    let v = u32::from_str_radix(&h[1..], 16).unwrap_or(0xf59e0b);
    Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

/// Clamp a rectangle to the image; `None` if nothing is left.
fn clamp(r: &Rect, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = r.x.max(0) as u32;
    let y0 = r.y.max(0) as u32;
    if x0 >= w || y0 >= h {
        return None;
    }
    let x1 = (x0 + r.w).min(w);
    let y1 = (y0 + r.h).min(h);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

fn stroke_rect(
    img: &mut RgbaImage,
    (x, y, w, h): (u32, u32, u32, u32),
    thickness: u32,
    color: Rgba<u8>,
) {
    let (iw, ih) = img.dimensions();
    for t in 0..thickness {
        // Draw just outside the target so it stays readable.
        let (left, top) = (x.saturating_sub(t + 1), y.saturating_sub(t + 1));
        let right = (x + w + t).min(iw - 1);
        let bottom = (y + h + t).min(ih - 1);
        for px in left..=right {
            img.put_pixel(px, top, color);
            img.put_pixel(px, bottom, color);
        }
        for py in top..=bottom {
            img.put_pixel(left, py, color);
            img.put_pixel(right, py, color);
        }
    }
}

/// Decode, crop, draw highlight/boxes, downscale and re-encode as PNG.
pub fn prepare(bytes: &[u8], ann: &Annotations, accent: &str) -> Result<RenderImage> {
    let mut img = image::load_from_memory(bytes)
        .map_err(|e| ExportError::Image(e.to_string()))?
        .to_rgba8();
    let (mut ox, mut oy) = (0i32, 0i32);
    if let Some(crop) = ann
        .crop
        .as_ref()
        .and_then(|c| clamp(c, img.width(), img.height()))
    {
        img = imageops::crop_imm(&img, crop.0, crop.1, crop.2, crop.3).to_image();
        ox = crop.0 as i32;
        oy = crop.1 as i32;
    }
    let shift = |r: &Rect| Rect {
        x: r.x - ox,
        y: r.y - oy,
        w: r.w,
        h: r.h,
    };
    let thickness = (img.width() / 300).clamp(3, 8);
    let accent = parse_color(accent);
    for b in &ann.boxes {
        if let Some(r) = clamp(&shift(b), img.width(), img.height()) {
            stroke_rect(&mut img, r, thickness, Rgba([196, 49, 75, 255]));
        }
    }
    if let Some(r) = ann
        .highlight
        .as_ref()
        .and_then(|h| clamp(&shift(h), img.width(), img.height()))
    {
        stroke_rect(&mut img, r, thickness, accent);
    }
    if img.width() > MAX_WIDTH {
        let h = (img.height() as u64 * MAX_WIDTH as u64 / img.width() as u64).max(1) as u32;
        img = imageops::resize(&img, MAX_WIDTH, h, imageops::FilterType::Triangle);
    }
    let (width, height) = img.dimensions();
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| ExportError::Image(e.to_string()))?;
    Ok(RenderImage {
        png: out.into_inner(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testutil::png;

    #[test]
    fn highlight_is_drawn_and_crop_applied() {
        let ann = Annotations {
            highlight: Some(Rect {
                x: 20,
                y: 20,
                w: 30,
                h: 10,
            }),
            crop: Some(Rect {
                x: 10,
                y: 10,
                w: 100,
                h: 50,
            }),
            ..Default::default()
        };
        let out = prepare(&png(200, 100), &ann, "#ff0000").unwrap();
        assert_eq!((out.width, out.height), (100, 50));
        let img = image::load_from_memory(&out.png).unwrap().to_rgba8();
        // Highlight at (20,20) in original = (10,10) after crop; border just outside.
        assert_eq!(img.get_pixel(9, 9), &Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn out_of_bounds_annotations_are_ignored_and_large_images_scaled() {
        let ann = Annotations {
            highlight: Some(Rect {
                x: 5000,
                y: 5000,
                w: 10,
                h: 10,
            }),
            ..Default::default()
        };
        let out = prepare(&png(3200, 100), &ann, "bad").unwrap();
        assert_eq!(out.width, MAX_WIDTH);
        assert!(prepare(b"not an image", &ann, "#000000").is_err());
    }
}
