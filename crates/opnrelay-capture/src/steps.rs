// SPDX-License-Identifier: AGPL-3.0-only
//! Turning an input event + screenshot into a guide step. Pure and testable.

use std::io::Cursor;

use image::{ImageFormat, RgbaImage, imageops};
use opnrelay_core::{Action, ActionType, Annotations, Rect, Step, StepKind, title};
use uuid::Uuid;

use crate::CaptureError;
use crate::input::{Button, InputEvent};
use crate::screen::{Shot, WindowInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    /// An area around the click (default).
    Region,
    /// The active window.
    Window,
    /// The whole monitor.
    Full,
}

impl CaptureMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "full" => CaptureMode::Full,
            "window" => CaptureMode::Window,
            _ => CaptureMode::Region,
        }
    }
}

/// Region size in screen units (scaled for HiDPI).
pub const REGION: (u32, u32) = (1000, 640);
/// Highlight drawn around a click when no element bounds are known.
pub const CLICK_BOX: (u32, u32) = (90, 44);

/// A recorded step waiting for upload.
#[derive(Debug, Clone)]
pub struct StepDraft {
    pub id: Uuid,
    pub title: String,
    pub action: Action,
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub highlight: Option<Rect>,
}

impl StepDraft {
    /// Convert to the API step model, given the uploaded media id.
    pub fn to_step(&self, media_id: Option<Uuid>) -> Step {
        Step {
            id: self.id,
            kind: StepKind::Action,
            title: self.title.clone(),
            body_md: String::new(),
            action: Some(self.action.clone()),
            media_id,
            annotations: Annotations {
                highlight: self.highlight,
                ..Default::default()
            },
        }
    }
}

fn clamp_rect(r: Rect, w: u32, h: u32) -> Option<Rect> {
    let x0 = r.x.clamp(0, w as i32);
    let y0 = r.y.clamp(0, h as i32);
    let x1 = (r.x + r.w as i32).clamp(0, w as i32);
    let y1 = (r.y + r.h as i32).clamp(0, h as i32);
    (x1 > x0 && y1 > y0).then(|| Rect {
        x: x0,
        y: y0,
        w: (x1 - x0) as u32,
        h: (y1 - y0) as u32,
    })
}

/// The crop rectangle (image pixels) for a step.
pub fn crop_rect(
    mode: CaptureMode,
    shot: &Shot,
    point: (i32, i32),
    window: Option<&WindowInfo>,
) -> Rect {
    let (iw, ih) = shot.image.dimensions();
    let full = Rect {
        x: 0,
        y: 0,
        w: iw,
        h: ih,
    };
    match mode {
        CaptureMode::Full => full,
        CaptureMode::Window => window
            .and_then(|w| w.bounds)
            .and_then(|b| clamp_rect(shot.rect_to_image(b), iw, ih))
            .unwrap_or(full),
        CaptureMode::Region => {
            let w = ((REGION.0 as f64 * shot.scale) as u32).min(iw);
            let h = ((REGION.1 as f64 * shot.scale) as u32).min(ih);
            let (cx, cy) = shot.to_image(point.0, point.1);
            let x = (cx - w as i32 / 2).clamp(0, (iw - w) as i32);
            let y = (cy - h as i32 / 2).clamp(0, (ih - h) as i32);
            Rect { x, y, w, h }
        }
    }
}

fn action_for(
    ev: &InputEvent,
    window: Option<&WindowInfo>,
) -> Option<(Action, Option<(i32, i32)>)> {
    let (t, point) = match ev {
        InputEvent::Click {
            button: Button::Left,
            x,
            y,
        } => (ActionType::Click, Some((*x, *y))),
        InputEvent::Click {
            button: Button::Right,
            x,
            y,
        } => (ActionType::RightClick, Some((*x, *y))),
        InputEvent::DoubleClick { x, y } => (ActionType::DoubleClick, Some((*x, *y))),
        InputEvent::TypingFinished { x, y } => (ActionType::Type, Some((*x, *y))),
        InputEvent::Shortcut { .. } => (ActionType::KeyShortcut, None),
    };
    let mut a = Action::new(t);
    if let InputEvent::Shortcut { keys } = ev {
        a.keys = Some(keys.clone());
    }
    if let Some(w) = window {
        a.window_title = (!w.title.trim().is_empty()).then(|| w.title.clone());
        a.app = (!w.app.trim().is_empty()).then(|| w.app.clone());
    }
    Some((a, point))
}

/// Build a step from an event and a screenshot taken right after it.
pub fn build(
    ev: &InputEvent,
    shot: &Shot,
    window: Option<&WindowInfo>,
    mode: CaptureMode,
) -> Result<StepDraft, CaptureError> {
    let (mut action, point) = action_for(ev, window).expect("all events map to actions");
    let center = point.unwrap_or_else(|| {
        let (w, h) = shot.image.dimensions();
        (
            shot.origin.0 + (w as f64 / shot.scale / 2.0) as i32,
            shot.origin.1 + (h as f64 / shot.scale / 2.0) as i32,
        )
    });
    let crop = crop_rect(mode, shot, center, window);
    let img: RgbaImage =
        imageops::crop_imm(&shot.image, crop.x as u32, crop.y as u32, crop.w, crop.h).to_image();

    let highlight = point.and_then(|(x, y)| {
        if matches!(action.action_type, ActionType::Type) {
            return None;
        }
        let (px, py) = shot.to_image(x, y);
        let bw = (CLICK_BOX.0 as f64 * shot.scale) as u32;
        let bh = (CLICK_BOX.1 as f64 * shot.scale) as u32;
        clamp_rect(
            Rect {
                x: px - crop.x - bw as i32 / 2,
                y: py - crop.y - bh as i32 / 2,
                w: bw,
                h: bh,
            },
            crop.w,
            crop.h,
        )
    });
    if let Some((x, y)) = point {
        let (px, py) = shot.to_image(x, y);
        action.x = Some(px - crop.x);
        action.y = Some(py - crop.y);
    }

    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)
        .map_err(|e| CaptureError::Encode(e.to_string()))?;
    Ok(StepDraft {
        id: Uuid::now_v7(),
        title: title::generate(&action),
        action,
        png: buf.into_inner(),
        width: crop.w,
        height: crop.h,
        highlight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(w: u32, h: u32, scale: f64) -> Shot {
        Shot {
            image: RgbaImage::from_pixel(w, h, image::Rgba([200, 200, 200, 255])),
            origin: (0, 0),
            scale,
        }
    }

    fn win() -> WindowInfo {
        WindowInfo {
            title: "Payroll – Employee record".into(),
            app: "Payroll".into(),
            process_id: 1,
            bounds: Some(Rect {
                x: 100,
                y: 50,
                w: 800,
                h: 600,
            }),
        }
    }

    #[test]
    fn region_is_centered_and_clamped() {
        let s = shot(1920, 1080, 1.0);
        let r = crop_rect(CaptureMode::Region, &s, (960, 540), None);
        assert_eq!(
            r,
            Rect {
                x: 460,
                y: 220,
                w: 1000,
                h: 640
            }
        );
        let edge = crop_rect(CaptureMode::Region, &s, (5, 5), None);
        assert_eq!((edge.x, edge.y), (0, 0));
        // HiDPI: region is scaled.
        let hi = shot(3840, 2160, 2.0);
        let r2 = crop_rect(CaptureMode::Region, &hi, (960, 540), None);
        assert_eq!((r2.w, r2.h), (2000, 1280));
    }

    #[test]
    fn window_and_full_modes() {
        let s = shot(1920, 1080, 1.0);
        assert_eq!(
            crop_rect(CaptureMode::Window, &s, (0, 0), Some(&win())),
            Rect {
                x: 100,
                y: 50,
                w: 800,
                h: 600
            }
        );
        assert_eq!(
            crop_rect(CaptureMode::Window, &s, (0, 0), None),
            Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080
            }
        );
        assert_eq!(crop_rect(CaptureMode::Full, &s, (0, 0), None).w, 1920);
    }

    #[test]
    fn builds_click_step_with_highlight_and_title() {
        let s = shot(1920, 1080, 1.0);
        let d = build(
            &InputEvent::Click {
                button: Button::Left,
                x: 960,
                y: 540,
            },
            &s,
            Some(&win()),
            CaptureMode::Region,
        )
        .unwrap();
        assert_eq!(d.title, "Click in \"Payroll – Employee record\"");
        assert_eq!((d.width, d.height), (1000, 640));
        let h = d.highlight.unwrap();
        assert_eq!(
            (h.x + h.w as i32 / 2, h.y + h.h as i32 / 2),
            (500, 320),
            "highlight centred on the click"
        );
        assert_eq!((d.action.x, d.action.y), (Some(500), Some(320)));
        assert!(d.png.starts_with(b"\x89PNG"));
        let step = d.to_step(None);
        assert_eq!(step.annotations.highlight, Some(h));
    }

    #[test]
    fn shortcut_and_typing_steps() {
        let s = shot(800, 600, 1.0);
        let d = build(
            &InputEvent::Shortcut {
                keys: "Ctrl+S".into(),
            },
            &s,
            None,
            CaptureMode::Region,
        )
        .unwrap();
        assert_eq!(d.title, "Press Ctrl+S");
        assert!(d.highlight.is_none());
        let t = build(
            &InputEvent::TypingFinished { x: 10, y: 10 },
            &s,
            Some(&win()),
            CaptureMode::Full,
        )
        .unwrap();
        assert_eq!(t.title, "Type in \"Payroll – Employee record\"");
        assert!(
            t.action.typed_text.is_none(),
            "typed text is never captured on desktop"
        );
    }
}
