# ADR 0001: Tauri 2 for desktop apps

**Status:** Accepted

## Context
We need Windows, macOS and Linux desktop recorders. They must be lightweight, written in Rust, and able to call low-level OS capture/accessibility APIs.

## Decision
Use **Tauri 2**: a Rust core (capture, redaction, local storage, upload) with the UI rendered in the OS webview.

## Consequences
- Installer ≈ 10–15 MB, low idle memory; all capture code is native Rust.
- The UI can share components with the web app.
- Webview differences (WebView2 / WKWebView / WebKitGTK) require cross-platform UI testing.
- Built-in signed updater and per-OS bundlers (MSI, DMG, AppImage/deb/rpm).

## Alternatives considered
- **Electron**: ~150 MB installers, higher memory; would need native Node modules for capture anyway.
- **Pure-Rust GUI (egui, Iced, Slint)**: smallest footprint, but a rich guide editor would be much more work, and none can be reused for the web UI.
