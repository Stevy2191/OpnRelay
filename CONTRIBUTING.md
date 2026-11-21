# Contributing to OpnRelay

Thanks for your interest! The project is in its design phase; see the [roadmap](docs/roadmap.md).

## Ground rules
- Discuss significant changes in an issue first. Architectural changes need an ADR in `docs/adr/`.
- Security-sensitive changes (auth, crypto, redaction, authorisation, uploads) require review by a maintainer and tests.
- No telemetry, analytics or outbound network calls may be added without an admin-controlled setting that defaults to off.
- New dependencies must pass `cargo-deny` (licence + advisories). Prefer well-maintained, widely used crates.

## Development setup
Requirements: Rust stable, Node 22 + pnpm, PostgreSQL 16.

```bash
# Database for development and tests
createuser -s opnrelay && psql -c "ALTER ROLE opnrelay PASSWORD 'opnrelay'" && createdb -O opnrelay opnrelay_dev

# Web UI (rebuild after UI changes; or use `pnpm dev` for hot reload on :5173)
pnpm --dir apps/web install && pnpm --dir apps/web build

# Server
export OPNRELAY_PUBLIC_URL=http://127.0.0.1:8080
export OPNRELAY_DATABASE_URL=postgres://opnrelay:opnrelay@localhost/opnrelay_dev
export OPNRELAY_MASTER_KEY=$(cargo run -q -p opnrelay-server -- keys generate)
export OPNRELAY_STORAGE=fs://$PWD/.data/media
cargo run -p opnrelay-server -- admin create-owner --email you@example.com
cargo run -p opnrelay-server -- serve
```

Tests create and drop their own databases. They connect with
`OPNRELAY_TEST_DATABASE_URL` (default `postgres://opnrelay:opnrelay@localhost/postgres`).

Before sending changes:
- `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all`
- `pnpm --dir apps/web check && pnpm --dir apps/web test`
- `cargo deny check` (licences + advisories)
- `#![forbid(unsafe_code)]` everywhere except documented FFI modules in `opnrelay-capture`.
- Conventional commit messages (`feat:`, `fix:`, `docs:` …).

## Reporting security issues
Do **not** open a public issue. See [SECURITY.md](SECURITY.md).
