# Contributing to OpnRelay

Thanks for your interest! The project is in its design phase; see the [roadmap](docs/roadmap.md).

## Ground rules
- Discuss significant changes in an issue first. Architectural changes need an ADR in `docs/adr/`.
- Security-sensitive changes (auth, crypto, redaction, authorisation, uploads) require review by a maintainer and tests.
- No telemetry, analytics or outbound network calls may be added without an admin-controlled setting that defaults to off.
- New dependencies must pass `cargo-deny` (licence + advisories). Prefer well-maintained, widely used crates.

## Development (once code lands)
- Rust stable (pinned in `rust-toolchain.toml`), Node LTS + pnpm for the UI/extension.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` must pass.
- `#![forbid(unsafe_code)]` everywhere except documented FFI modules in `opnrelay-capture`.
- Conventional commit messages (`feat:`, `fix:`, `docs:` …).

## Reporting security issues
Do **not** open a public issue. See [SECURITY.md](SECURITY.md).
