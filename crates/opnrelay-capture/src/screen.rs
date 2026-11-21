// SPDX-License-Identifier: AGPL-3.0-only
//! Screenshots and active-window information.

use image::RgbaImage;
use opnrelay_core::Rect;

use crate::CaptureError;

/// A screenshot of one monitor, with the mapping from screen coordinates.
pub struct Shot {
    pub image: RgbaImage,
    /// Monitor origin in screen coordinates.
    pub origin: (i32, i32),
    /// Image pixels per screen coordinate unit (HiDPI scale).
    pub scale: f64,
}

impl Shot {
    /// Convert a screen point to image pixel coordinates.
    pub fn to_image(&self, x: i32, y: i32) -> (i32, i32) {
        (
            ((x - self.origin.0) as f64 * self.scale).round() as i32,
            ((y - self.origin.1) as f64 * self.scale).round() as i32,
        )
    }

    /// Convert a screen-space rectangle to image pixels.
    pub fn rect_to_image(&self, r: Rect) -> Rect {
        let (x, y) = self.to_image(r.x, r.y);
        Rect {
            x,
            y,
            w: (r.w as f64 * self.scale).round() as u32,
            h: (r.h as f64 * self.scale).round() as u32,
        }
    }
}

/// Capture the monitor that contains the screen point.
pub fn capture_at(x: i32, y: i32) -> Result<Shot, CaptureError> {
    let err = |e: xcap::XCapError| CaptureError::Screen(e.to_string());
    let monitor = xcap::Monitor::from_point(x, y).or_else(|_| {
        xcap::Monitor::all()
            .map_err(err)?
            .into_iter()
            .next()
            .ok_or_else(|| CaptureError::Screen("no monitor".into()))
    })?;
    let image = monitor.capture_image().map_err(err)?;
    let width = monitor.width().map_err(err)?.max(1);
    Ok(Shot {
        scale: image.width() as f64 / width as f64,
        origin: (monitor.x().map_err(err)?, monitor.y().map_err(err)?),
        image,
    })
}

/// The window that has keyboard focus.
#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    pub title: String,
    pub app: String,
    pub process_id: u64,
    /// Bounds in screen coordinates.
    pub bounds: Option<Rect>,
}

pub fn active_window() -> Option<WindowInfo> {
    let windows = xcap::Window::all().ok()?;
    let w = windows
        .into_iter()
        .find(|w| w.is_focused().unwrap_or(false))?;
    let (x, y, width, height) = (
        w.x().ok()?,
        w.y().ok()?,
        w.width().unwrap_or(0),
        w.height().unwrap_or(0),
    );
    Some(WindowInfo {
        title: w.title().unwrap_or_default(),
        app: w.app_name().unwrap_or_default(),
        process_id: u64::from(w.pid().unwrap_or(0)),
        bounds: (width > 0 && height > 0).then_some(Rect {
            x,
            y,
            w: width,
            h: height,
        }),
    })
}
