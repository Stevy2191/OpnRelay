# UI screenshot tool

`ui-shots.mjs` seeds a throwaway OpnRelay server (owner, editor, viewer, three
workspaces with folders, eight guides including a 120-character title, a guide
without screenshots and one flagged needs-review) and saves
`NAME-light-desktop.png`, `NAME-dark-desktop.png` and `NAME-light-phone.png`
(390x844) for each page. Seeding is idempotent: re-running reuses existing data.

## 1. Start a throwaway server

```sh
service postgresql start                      # or any Postgres you can create a db in
cd apps/web && pnpm build && cd ../..         # the server embeds apps/web/dist
cargo build -p opnrelay-server
PGPASSWORD=opnrelay psql -h 127.0.0.1 -U opnrelay postgres -c "create database ui_shots"

S=/tmp/ui-shots-scratch; mkdir -p $S
export OPNRELAY_PUBLIC_URL=http://127.0.0.1:8092 OPNRELAY_BIND=127.0.0.1:8092
export OPNRELAY_MASTER_KEY=$(target/debug/opnrelay-server keys generate)
export OPNRELAY_DATABASE_URL=postgres://opnrelay:opnrelay@127.0.0.1/ui_shots
export OPNRELAY_STORAGE=fs://$S/media OPNRELAY_BACKUP_DIR=$S/backups
target/debug/opnrelay-server serve &
target/debug/opnrelay-server admin create-owner --email owner@example.com   # prints a one-time link
```

The variables are read in `crates/opnrelay-server/src/config.rs`
(`OPNRELAY_PUBLIC_URL`, `OPNRELAY_BIND`, `OPNRELAY_MASTER_KEY`,
`OPNRELAY_DATABASE_URL`, `OPNRELAY_STORAGE`, `OPNRELAY_BACKUP_DIR`, ...).

## 2. Run

Playwright must be resolvable from Node (global install with `NODE_PATH`, or
local). Browsers are not installed by this tool.

```sh
node apps/web/tools/ui-shots.mjs --base http://127.0.0.1:8092 --out /tmp/shots \
  --owner-link 'http://127.0.0.1:8092/#/invite/TOKEN' \
  --database-url postgres://opnrelay:opnrelay@127.0.0.1/ui_shots
```

Options:

- `--owner-link` / `--owner-token`: the one-time link/token from `create-owner` (only needed the first time).
- `--owner-email` (default `owner@example.com`), `--password` (default `Shots-Passw0rd-123!`, used for all three users).
- `--pages home,guide,edit,admin,...`: subset of `login home all-guides folder ws-cards ws-list ws-empty drafts review search search-none guide guide-empty edit admin account audit downloads ws-settings`.
- `--database-url`: lets the tool backdate the needs-review guide with `psql`. Without it, the command is printed.

## 3. Clean up

```sh
kill %1   # the server
PGPASSWORD=opnrelay psql -h 127.0.0.1 -U opnrelay postgres -c "drop database ui_shots"
```
