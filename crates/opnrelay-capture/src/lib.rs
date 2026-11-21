// SPDX-License-Identifier: AGPL-3.0-only
//! Cross-platform desktop capture for the OpnRelay recorder.
//!
//! * [`input`]: polls mouse and keyboard state and turns it into clicks,
//!   double-clicks, shortcuts and typing bursts. Keystrokes are never stored.
//! * [`screen`]: screenshots of the monitor under the pointer plus the active
//!   window's title, app and bounds.
//! * [`steps`]: pure functions that turn an event + screenshot into a step
//!   (crop mode, highlight, title). Unit tested without a display.
//! * [`recorder`]: background thread tying it together.

pub mod input;
pub mod recorder;
pub mod screen;
pub mod steps;

pub use recorder::{Recorder, RecorderOptions};
pub use steps::{CaptureMode, StepDraft};

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("screen capture failed: {0}")]
    Screen(String),
    #[error("image encoding failed: {0}")]
    Encode(String),
}
