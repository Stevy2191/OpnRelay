# ADR 0003: Svelte + TypeScript for the web/desktop UI

**Status:** Proposed

## Context
The guide editor (drag-and-drop steps, image annotation, rich text) is the most UI-heavy part. The UI is shared by the web app and the Tauri desktop app. The browser extension must be JS/TS regardless.

## Decision
Use **Svelte 5 + TypeScript** built with Vite to static assets embedded in the server binary. All business logic, security checks and rendering stay in Rust.

## Consequences
- Small bundles and a mature ecosystem for editors and canvas annotation.
- Introduces a Node toolchain for building (not at runtime).

## Alternatives considered
- **Leptos / Dioxus (Rust → WASM)**: all-Rust, but a less mature component ecosystem for rich editing; larger WASM bundles; it can't be shared with the extension. Revisit if the project strongly prefers a Rust-only codebase.
- **React**: larger bundles; no strong advantage here.
