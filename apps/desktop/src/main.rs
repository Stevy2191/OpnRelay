// SPDX-License-Identifier: AGPL-3.0-only
//! OpnRelay desktop recorder.
//!
//! With no arguments it opens the small recorder window. Command-line use:
//!
//! ```text
//! opnrelay-recorder login  --server https://guides.company.com [--no-browser]
//! opnrelay-recorder record --seconds 60 --workspace <id> --title "..." [--mode region|window|full]
//! ```
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod session;
mod store;

use std::time::Duration;

use opnrelay_capture::{CaptureMode, Recorder, RecorderOptions};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn cli(args: &[String]) {
    let mut settings = store::Settings::load();
    if let Some(s) = arg(args, "--server") {
        settings.server_url =
            Some(opnrelay_client::normalize_server(&s).unwrap_or_else(|| fail("invalid --server")));
        settings.save();
    }
    let server = settings
        .server_url
        .clone()
        .unwrap_or_else(|| fail("no server configured; pass --server"));
    match args[1].as_str() {
        "login" => {
            let (url, wait) = session::begin_sign_in(&server).unwrap_or_else(|e| fail(e));
            println!("Open this address to sign in:\n{url}");
            if !args.iter().any(|a| a == "--no-browser") {
                let _ = webbrowser::open(&url);
            }
            let client = wait().unwrap_or_else(|e| fail(e));
            let me = client
                .lock()
                .expect("lock")
                .me()
                .unwrap_or_else(|e| fail(e));
            println!("Signed in as {} ({})", me.display_name, me.email);
        }
        "record" => {
            let client = session::resume(&server)
                .unwrap_or_else(|| fail("not signed in; run `opnrelay-recorder login` first"));
            let secs: u64 = arg(args, "--seconds")
                .and_then(|s| s.parse().ok())
                .unwrap_or(30);
            let workspace =
                arg(args, "--workspace").unwrap_or_else(|| fail("--workspace is required"));
            let title = arg(args, "--title").unwrap_or_else(|| "New guide".into());
            let mode = CaptureMode::parse(&arg(args, "--mode").unwrap_or_default());
            let rec = Recorder::start(RecorderOptions {
                mode,
                ..Default::default()
            })
            .unwrap_or_else(|e| fail(e));
            println!("Recording for {secs} seconds…");
            std::thread::sleep(Duration::from_secs(secs));
            let steps = rec.finish();
            println!("Recorded {} steps; uploading…", steps.len());
            if steps.is_empty() {
                fail("no steps were recorded");
            }
            let url = session::upload(&client, &workspace, &title, &steps, |_, _| {})
                .unwrap_or_else(|e| fail(e));
            println!("{url}");
        }
        other => fail(format!("unknown command {other:?}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("opnrelay-recorder {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.len() > 1 {
        cli(&args);
        return;
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("OpnRelay Recorder")
            .with_inner_size([340.0, 420.0])
            .with_min_inner_size([300.0, 300.0])
            .with_always_on_top(),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "OpnRelay Recorder",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    ) {
        fail(e);
    }
}
