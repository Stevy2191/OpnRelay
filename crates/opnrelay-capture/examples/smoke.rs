// SPDX-License-Identifier: AGPL-3.0-only
//! Manual/CI smoke test on a real (or virtual) display:
//! `cargo run -p opnrelay-capture --example smoke -- <seconds> <outdir>`
use std::time::Duration;

use opnrelay_capture::{CaptureMode, Recorder, RecorderOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let out = args.get(2).cloned().unwrap_or_else(|| ".".into());
    let rec = Recorder::start(RecorderOptions {
        mode: CaptureMode::Region,
        ..Default::default()
    })
    .expect("start recorder");
    println!("recording for {secs}s");
    std::thread::sleep(Duration::from_secs(secs));
    if let Some(e) = rec.last_error() {
        println!("last error: {e}");
    }
    let steps = rec.finish();
    for (i, s) in steps.iter().enumerate() {
        println!(
            "{i}: {} {}x{} highlight={:?}",
            s.title, s.width, s.height, s.highlight
        );
        std::fs::write(format!("{out}/desktop-step-{i}.png"), &s.png).unwrap();
    }
    println!("steps: {}", steps.len());
}
