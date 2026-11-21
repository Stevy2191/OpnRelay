// SPDX-License-Identifier: AGPL-3.0-only
//! The recording session: watches input, takes screenshots, collects steps.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use opnrelay_core::{ActionType, title};

use crate::input::{InputEvent, Watcher};
use crate::screen;
use crate::steps::{self, CaptureMode, StepDraft};

#[derive(Debug, Clone)]
pub struct RecorderOptions {
    pub mode: CaptureMode,
    /// Process ids whose windows are never recorded (the recorder itself,
    /// password managers...).
    pub excluded_pids: Vec<u64>,
    /// App names (case-insensitive substrings) that are never recorded.
    pub excluded_apps: Vec<String>,
}

impl Default for RecorderOptions {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Region,
            excluded_pids: vec![u64::from(std::process::id())],
            excluded_apps: [
                "1password",
                "bitwarden",
                "keepass",
                "lastpass",
                "dashlane",
                "keychain access",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

/// A running recording. Dropping it stops recording.
pub struct Recorder {
    steps: Arc<Mutex<Vec<StepDraft>>>,
    errors: Arc<Mutex<Vec<String>>>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

fn excluded(opts: &RecorderOptions, w: Option<&screen::WindowInfo>) -> bool {
    let Some(w) = w else { return false };
    if opts.excluded_pids.contains(&w.process_id) {
        return true;
    }
    let app = w.app.to_lowercase();
    opts.excluded_apps
        .iter()
        .any(|x| !x.is_empty() && app.contains(&x.to_lowercase()))
}

impl Recorder {
    pub fn start(opts: RecorderOptions) -> Result<Recorder, String> {
        let (tx, rx) = mpsc::channel::<InputEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let steps: Arc<Mutex<Vec<StepDraft>>> = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let watcher = Watcher::spawn(tx, stop.clone())?;

        let (s2, e2, p2, stop2) = (steps.clone(), errors.clone(), paused.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let Ok(ev) = rx.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };
                if p2.load(Ordering::Relaxed) {
                    continue;
                }
                // Let the UI react to the click before the screenshot, but
                // not so long that it navigates away.
                if matches!(
                    ev,
                    InputEvent::Click { .. } | InputEvent::DoubleClick { .. }
                ) {
                    std::thread::sleep(Duration::from_millis(60));
                }
                let window = screen::active_window();
                if excluded(&opts, window.as_ref()) {
                    continue;
                }
                if let InputEvent::DoubleClick { .. } = ev {
                    // Upgrade the click we already recorded.
                    let mut s = s2.lock().expect("lock");
                    if let Some(last) = s.last_mut()
                        && last.action.action_type == ActionType::Click
                    {
                        last.action.action_type = ActionType::DoubleClick;
                        last.title = title::generate(&last.action);
                        continue;
                    }
                }
                let (x, y) = match ev {
                    InputEvent::Click { x, y, .. }
                    | InputEvent::DoubleClick { x, y }
                    | InputEvent::TypingFinished { x, y } => (x, y),
                    InputEvent::Shortcut { .. } => window
                        .as_ref()
                        .and_then(|w| w.bounds)
                        .map(|b| (b.x + b.w as i32 / 2, b.y + b.h as i32 / 2))
                        .unwrap_or((0, 0)),
                };
                let result = screen::capture_at(x, y)
                    .and_then(|shot| steps::build(&ev, &shot, window.as_ref(), opts.mode));
                match result {
                    Ok(step) => s2.lock().expect("lock").push(step),
                    Err(e) => e2.lock().expect("lock").push(e.to_string()),
                }
            }
        });
        Ok(Recorder {
            steps,
            errors,
            paused,
            stop,
            threads: vec![watcher, worker],
        })
    }

    pub fn step_count(&self) -> usize {
        self.steps.lock().expect("lock").len()
    }

    pub fn last_error(&self) -> Option<String> {
        self.errors.lock().expect("lock").last().cloned()
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Remove the most recent step (e.g. a click on the wrong thing).
    pub fn undo_last(&self) -> bool {
        self.steps.lock().expect("lock").pop().is_some()
    }

    /// Stop and return the recorded steps.
    pub fn finish(mut self) -> Vec<StepDraft> {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        std::mem::take(&mut *self.steps.lock().expect("lock"))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions() {
        let o = RecorderOptions::default();
        let me = screen::WindowInfo {
            process_id: u64::from(std::process::id()),
            ..Default::default()
        };
        assert!(excluded(&o, Some(&me)));
        let pm = screen::WindowInfo {
            app: "Bitwarden".into(),
            process_id: 1,
            ..Default::default()
        };
        assert!(excluded(&o, Some(&pm)));
        let other = screen::WindowInfo {
            app: "Payroll".into(),
            process_id: 1,
            ..Default::default()
        };
        assert!(!excluded(&o, Some(&other)));
        assert!(!excluded(&o, None));
    }
}
