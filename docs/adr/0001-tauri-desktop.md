# ADR 0001: Tauri 2 for desktop apps

**Status:** Superseded (November 2026). See *Revision* below.

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

## Revision (November 2026)
Guide editing, viewing and administration all happen in the web app, so the desktop
program only needs a small control panel (sign in, choose workspace and screenshot mode,
start/pause/undo/finish). The recorder is therefore a **native Rust app using egui**
(`apps/desktop`, crate `opnrelay-recorder`) instead of Tauri:

- No embedded web view (no WebView2/WebKitGTK dependency), smaller and simpler to ship.
- Capture logic lives in `crates/opnrelay-capture` (screenshots via `xcap`, input by
  polling with `device_query`, active window via `xcap`).
- Sign-in uses the system browser with OAuth + PKCE and a loopback redirect; the refresh
  token is stored in the OS keychain (file fallback with 0600 permissions).

Tauri stays an option if the desktop app ever needs a rich UI of its own.
