# "Needs review" Flag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Published guides get a review date; after the workspace's review period they are flagged "Needs review" for editors, readers see "Last reviewed <date>", and editors clear it with "Still accurate" or by publishing.

**Architecture:** Two new columns (`workspaces.review_period_days`, `guides.reviewed_at`). "Needs review" is computed in SQL from them, so no scheduler is needed. One new endpoint confirms a guide is still accurate; publish also resets the clock. The web app shows the flag, a filter and a count (current visual style; the Runbook restyle is a separate plan).

**Tech Stack:** Rust · Axum · sqlx/PostgreSQL; Svelte 5 + TypeScript.

**Spec:** `docs/superpowers/specs/2026-11-21-runbook-redesign-design.md` (section "Needs review" flag)

## Global Constraints

- Review period is a **workspace setting, default 90 days**; `NULL` = off; allowed range 1–3650 days.
- A guide needs review when: `status = 'published'` AND the workspace's period is not null AND `reviewed_at + period < now()`.
- `reviewed_at` is set to `now()` on every publish and on "Still accurate". "Still accurate" creates **no new version**.
- Both are audit-logged: publish already logs `guide.published`; "Still accurate" logs `guide.reviewed`.
- Only editors (workspace `edit` or higher, admins) can mark "Still accurate"; only workspace managers change the period (existing `PATCH /workspaces/{id}` permission).
- Readers (view-only) never see the "Needs review" flag; they see "Last reviewed <date>".
- Binding lock order from the previous plan: a transaction that writes rows and audits calls `audit::lock(&mut tx)` right after `begin()`.
- Commit messages end with the repo's two attribution lines (`Co-Authored-By: …`, `Claude-Session: …`).
- Tests need Postgres (`service postgresql start`; default `postgres://opnrelay:opnrelay@localhost/postgres`).

## Review Focus

1. **Draft or archived guide past the period** → never flagged (only published guides). (Task 1)
2. **Workspace period turned off, then on again** → guides reviewed long ago are flagged immediately when turned back on; turning it off hides every flag. (Task 1)
3. **Existing published guides at upgrade** → `reviewed_at` starts at their `published_at`, so long-published guides are flagged right after the upgrade, while unpublished guides have `reviewed_at` NULL and are never flagged. (Task 1)
4. **Viewer calls "Still accurate"** → 403, nothing changes, no audit. (Task 1)
5. **Period changed from 90 to 30 days** → a guide reviewed 45 days ago becomes flagged without anyone touching it. (Task 1)

---

### Task 1: Server — review dates, flag, "Still accurate", workspace period

**Files:**
- Create: `crates/opnrelay-server/migrations/0006_review_flag.sql`, `crates/opnrelay-server/tests/review.rs`
- Modify: `crates/opnrelay-server/src/routes/guides.rs` (`GuideSummary`, `SUMMARY_COLS`, `load_summary`, `ListQuery`/`list`, `publish`, new route), `crates/opnrelay-server/src/routes/workspaces.rs` (`WorkspaceRow`, `UpdateWorkspace`, `create`, `update`)

**Interfaces:**
- Produces:
  - Migration: `ALTER TABLE workspaces ADD COLUMN review_period_days integer DEFAULT 90 CHECK (review_period_days BETWEEN 1 AND 3650)`; `ALTER TABLE guides ADD COLUMN reviewed_at timestamptz`; `UPDATE guides SET reviewed_at = published_at WHERE published_at IS NOT NULL`. Existing workspaces end up with 90.
  - `GuideSummary` gains `reviewed_at: Option<OffsetDateTime>`, `review_due_at: Option<OffsetDateTime>` (`reviewed_at + period` when published and period set, else null) and `needs_review: bool`. Every query that selects `SUMMARY_COLS` must join the workspace so these compute.
  - `GET /api/v1/guides?needs_review=true` → only guides with `needs_review` (still filtered by visibility). Admins/auditors see all as today.
  - `POST /api/v1/guides/{id}/reviewed` (body `{}`), requires `Perm::Edit`, guide must be `published` (else 400 "Only published guides can be marked as reviewed"); sets `reviewed_at = now()`, writes `guide.reviewed` audit, does **not** bump `revision` or create a version; returns the full guide JSON like `GET /guides/{id}`.
  - `publish` sets `reviewed_at = now()` alongside `published_at`.
  - `WorkspaceRow` gains `review_period_days: Option<i32>`; `PATCH /workspaces/{id}` accepts `review_period_days: Option<Option<i32>>` (absent = unchanged; `null` = off; number = 1..=3650 else 400) and records it in the `workspace.updated` audit meta.

- [ ] **Step 1: Failing tests** in `tests/review.rs` (use `common::TestApp`, `owner()`, `user()`; move time by `UPDATE guides SET reviewed_at = now() - interval 'N days'` on `app.db`):
  - `publish_sets_reviewed_and_flag_appears_after_period`: publish → `reviewed_at` set, `needs_review == false`, `review_due_at ≈ reviewed_at + 90d`; set reviewed_at 91 days ago → `needs_review == true` on `GET /guides/{id}` and in `GET /guides?needs_review=true`.
  - `drafts_and_archived_never_flagged`: draft with old `reviewed_at`/archived guide → `needs_review == false`, absent from the filter.
  - `still_accurate_clears_without_new_version`: flagged guide; editor `POST /guides/{id}/reviewed` → 200, `needs_review == false`, `current_version` and `revision` unchanged, `GET /guides/{id}/versions` length unchanged, audit has `guide.reviewed`.
  - `viewer_cannot_mark_reviewed`: viewer → 403; `reviewed_at` unchanged; no `guide.reviewed` audit.
  - `draft_cannot_be_marked_reviewed`: → 400.
  - `period_off_and_shorter`: PATCH `review_period_days: null` → flagged guide not flagged, `review_due_at` null; PATCH `30` with guide reviewed 45 days ago → flagged; PATCH `0` or `4000` → 400; member with edit (not manage) → 403.
  - `new_workspace_defaults_to_90`.
  - The upgrade backfill (`reviewed_at = published_at`) cannot be exercised by tests (test databases start empty); the reviewer checks the migration SQL instead.
- [ ] **Step 2:** Run `cargo test -p opnrelay-server --test review` — Expected: FAIL (columns/route missing).
- [ ] **Step 3:** Implement as listed. Keep the existing lock order rule for the new write path.
- [ ] **Step 4:** Run `cargo test -p opnrelay-server` (full), `cargo fmt --all`, `cargo clippy -p opnrelay-server --tests -- -D warnings` — Expected: all PASS / clean.
- [ ] **Step 5:** Commit `feat(server): needs-review flag with per-workspace review period`.

### Task 2: Web — flag, filter, count, "Still accurate", workspace setting, docs

**Files:**
- Modify: `apps/web/src/lib/api.ts`, `apps/web/src/routes/Home.svelte`, `apps/web/src/routes/GuideView.svelte`, `apps/web/src/routes/WorkspaceSettings.svelte`, `README.md` (feature list), `docs/roadmap.md`

**Interfaces:**
- Consumes: Task 1 fields and endpoints.
- UI (current style):
  - **Home**: on each guide card, when `needs_review` and the user can edit that guide's workspace (`workspaces[].my_permission` is `edit` or `manage`, or user is admin): an orange "Needs review" tag. Above the list, for editors with at least one flagged guide: "N guides need review" link that toggles a "Needs review" filter (`?needs_review=true`); the filter can be cleared.
  - **GuideView**: everyone sees "Last reviewed <date>" (date only, local format) when `reviewed_at` is set. Editors of a flagged guide see a notice "This guide is due for review." with a **Still accurate** button (calls `POST /guides/{id}/reviewed`, updates the page, shows "Marked as reviewed"); the notice also says publishing a new version counts as a review.
  - **WorkspaceSettings** (managers): "Review guides every [90] days" number input with an "Off" checkbox; saves via `PATCH /workspaces/{id}`; validation message for out-of-range values.
  - Readers never see the flag, the filter or the button.
  - Docs: one README feature bullet; roadmap line under M5 "Needs-review flag (per-workspace review period)" ticked.

- [ ] **Step 1:** Implement.
- [ ] **Step 2:** Run `cd apps/web && pnpm check && pnpm test && pnpm build` — Expected: no errors.
- [ ] **Step 3:** Run the real server against Postgres (fresh database), create a workspace and a published guide, set its `reviewed_at` 100 days back with `psql`, and screenshot with Playwright (deviceScaleFactor 1) as an editor (Home with the tag and count; GuideView with the notice) and as a viewer (GuideView with only "Last reviewed"). Screenshots go in the session scratchpad, not the repo. Describe them in the report.
- [ ] **Step 4:** Commit `feat(web): needs-review flag, filter and "Still accurate"`.
