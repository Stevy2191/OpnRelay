# Entra Group Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Link Microsoft Entra groups into OpnRelay, keep their members (and optional org roles) in step with Entra hourly and at sign-in, disable leavers within the hour, and fix the empty home page.

**Architecture:** A new `graph` module gets an app-only token (client credentials) and wraps the four Graph reads. A new `group_sync` module holds all sync logic (full sync, per-user sync at sign-in, roles, leavers, safety brake) and an hourly scheduler like the backup one. Linked groups are ordinary `groups` rows with `source='idp'`, so workspace permissions are unchanged. Admin APIs live in `routes/admin.rs` (groups) and `routes/sso.rs` (sync settings, search, sync now).

**Tech Stack:** Rust · Axum · sqlx/PostgreSQL · reqwest; Svelte 5 + TypeScript web app; integration tests against a fake Microsoft (axum) in `crates/opnrelay-server/tests`.

**Spec:** `docs/superpowers/specs/2026-11-21-entra-group-sync-design.md`

## Global Constraints

- Graph application permissions documented to admins: `GroupMember.Read.All`, `User.Read.All` (read-only).
- Token: `POST {ms_login_url}/{tenant}/oauth2/v2.0/token`, `grant_type=client_credentials`, `scope=https://graph.microsoft.com/.default`. Group sync is refused when the tenant is `organizations`, `common` or `consumers`.
- Graph base is `st.cfg.ms_graph_url` (already includes `/v1.0`).
- Sync cadence: scheduler checks every 5 minutes, runs a full sync when the last run is over **1 hour** old.
- Owners are never role-changed or disabled by sync.
- Safety brake: a run that would disable **more than 10** people **and more than 20%** of Microsoft-linked users disables no one.
- Any Graph error while reading accounts or a group means no change for that part of the run; sign-in still succeeds if the per-user check fails.
- No audit event for a sync that changed nothing; `idp.sync.failed` only on OK→failing and then at most once per day.
- Name-clash suffix for linked groups: `" (Microsoft)"`.
- Every commit message ends with the two attribution lines used in this repo (`Co-Authored-By: …` and `Claude-Session: …`).
- Tests need Postgres at `OPNRELAY_TEST_DATABASE_URL` (default `postgres://opnrelay:opnrelay@localhost/postgres`).

## Review Focus

1. **Tenant set to `organizations`/`common`** → enabling group sync returns 400 "Group sync needs your specific tenant ID…", sync never runs. (Task 3)
2. **Person in two linked groups granting different roles** → highest wins (admin > auditor); leaving the admin group drops them to auditor, not member. (Task 6)
3. **Member removed from a group while signed in** → their very next request no longer sees that workspace (permission checks are per request; no sign-out needed). (Task 4)
4. **Linked group name equals a manual group's name** → linked as "HR Staff (Microsoft)", manual group untouched. (Task 3)
5. **Safety-brake boundary** → exactly 10 disables proceed; 11 of 50 (22%) are blocked and nothing changes. (Task 7)

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/opnrelay-server/migrations/0004_group_sync.sql` (create) | New columns from the spec |
| `crates/opnrelay-server/src/graph.rs` (create) | App token cache + Graph reads; no DB access |
| `crates/opnrelay-server/src/group_sync.rs` (create) | Full sync, per-user sync, roles, leavers, brake, scheduler |
| `crates/opnrelay-server/src/state.rs` (modify) | `graph_token` cache field |
| `crates/opnrelay-server/src/lib.rs`, `main.rs` (modify) | Register modules; spawn scheduler in `Serve` |
| `crates/opnrelay-server/src/routes/sso.rs` (modify) | Settings fields, group search, sync now, sign-in hook, re-enable |
| `crates/opnrelay-server/src/routes/admin.rs` (modify) | Link/patch groups, role preview, refusals, `disabled_by='admin'` |
| `crates/opnrelay-server/tests/common/fake_ms.rs` (create) | Fake Microsoft moved out of `tests/microsoft.rs`, extended with Graph directory endpoints |
| `crates/opnrelay-server/tests/group_sync.rs` (create) | Integration tests for this plan |
| `apps/web/src/lib/api.ts` (modify) | Types for groups and sync settings |
| `apps/web/src/routes/GroupsTab.svelte` (create) | Groups tab (moved out of `Admin.svelte`) with Microsoft groups |
| `apps/web/src/routes/SsoSettings.svelte`, `Admin.svelte`, `Home.svelte` (modify) | Group sync section, role lock, empty states |
| `docs/microsoft.md`, `docs/roadmap.md` (modify) | Admin docs |

---

### Task 1: Fake Microsoft directory for tests

**Files:**
- Create: `crates/opnrelay-server/tests/common/fake_ms.rs`
- Modify: `crates/opnrelay-server/tests/common/mod.rs` (add `pub mod fake_ms;`), `crates/opnrelay-server/tests/microsoft.rs` (use the shared fake; delete its local copy)

**Interfaces:**
- Produces:
  - `pub async fn fake_microsoft() -> (String, S)` where `S = Arc<Mutex<Fake>>`, existing behaviour preserved.
  - `Fake` gains: `groups: HashMap<String, FakeGroup>` (`FakeGroup { name: String, kind: &'static str, members: Vec<String>, nested: Vec<String> }`), `users: HashMap<String, bool>` (oid → accountEnabled), `graph_fail: bool` (every Graph directory call returns 503), `throttle_once: bool` (next call returns 429 `Retry-After: 1`), `deleted_groups: HashSet<String>` (404), `page_size: usize` (default 2), `app_token_requests: u32`.
  - `pub fn next_user(s: &S, oid: &str, email: &str)`: the oid the next authorize signs in as (the existing fake uses email as subject; change it so `oid` claim = given oid).
  - Routes: token endpoint accepts `grant_type=client_credentials` → `{"access_token":"app-token","expires_in":3599}`; `GET /v1.0/groups` (`$search="displayName:x"`, requires `ConsistencyLevel: eventual` header else 400); `GET /v1.0/groups/{id}/transitiveMembers/microsoft.graph.user` (paged with `@odata.nextLink` using `$skiptoken`, resolves `nested` recursively); `GET /v1.0/users/{id}/transitiveMemberOf/microsoft.graph.group`; `GET /v1.0/users?$filter=id in ('a','b')&$select=id,accountEnabled` (missing ids simply absent). Directory routes require `Authorization: Bearer app-token`.

- [ ] **Step 1:** Move `Fake`, its handlers, `fake_microsoft`, `browser`, `location`, `microsoft_flow` into `common/fake_ms.rs` unchanged; make `tests/microsoft.rs` import them.
- [ ] **Step 2:** Run `cargo test -p opnrelay-server --test microsoft` — Expected: PASS (pure move).
- [ ] **Step 3:** Add the directory state and routes listed above.
- [ ] **Step 4:** Add a self-test `fake_directory_pages_and_nests` in `tests/group_sync.rs`: with `page_size = 2`, group `G` with members `[u1,u2,u3]` and nested group `N` with `[u4]`, fetching all pages returns `{u1,u2,u3,u4}`; a request without the bearer token gets 401.
- [ ] **Step 5:** Run `cargo test -p opnrelay-server --test group_sync --test microsoft` — Expected: PASS.
- [ ] **Step 6:** Commit `test: shared fake Microsoft with Graph directory endpoints`.

### Task 2: Migration and Graph client

**Files:**
- Create: `crates/opnrelay-server/migrations/0004_group_sync.sql`, `crates/opnrelay-server/src/graph.rs`
- Modify: `crates/opnrelay-server/src/state.rs`, `crates/opnrelay-server/src/lib.rs`
- Test: `crates/opnrelay-server/tests/group_sync.rs`

**Interfaces:**
- Consumes: `microsoft::AppConfig`, `st.cfg.ms_login_url`, `st.cfg.ms_graph_url`.
- Produces (in `graph.rs`):
  - `pub struct DirGroup { pub id: String, pub display_name: String, pub description: Option<String>, pub kind: String }` (`kind`: `"Security group"` / `"Microsoft 365 group"`, from `groupTypes` containing `"Unified"`; on-prem flag `onPremisesSyncEnabled` exposed as `pub on_prem: bool`)
  - `pub enum GraphError { NotFound, Failed(String) }`
  - `pub fn tenant_is_specific(tenant: &str) -> bool`
  - `pub async fn app_token(st: &AppState, app: &AppConfig) -> Result<String, GraphError>` (cached in `st.graph_token: Arc<tokio::sync::Mutex<Option<(String, std::time::Instant)>>>` until 5 minutes before expiry)
  - `pub async fn search_groups(st, app, q: &str) -> Result<Vec<DirGroup>, GraphError>` (top 20)
  - `pub async fn get_group(st, app, id: &str) -> Result<DirGroup, GraphError>`
  - `pub async fn group_user_ids(st, app, group_id: &str) -> Result<HashSet<String>, GraphError>` (transitive, all pages)
  - `pub async fn user_group_ids(st, app, oid: &str) -> Result<HashSet<String>, GraphError>` (transitive, all pages)
  - `pub async fn accounts_enabled(st, app, oids: &[String]) -> Result<HashMap<String, bool>, GraphError>` (chunks of 15; ids Graph doesn't return are absent = deleted)
  - All calls retry once on 429/503 honouring `Retry-After` (cap 10 s), then `Failed`.
- Migration adds exactly the spec's columns: `identity_providers.group_sync_enabled boolean NOT NULL DEFAULT false`, `group_sync_last_at`, `group_sync_last_ok_at timestamptz`, `group_sync_last_error text`, `group_sync_failed_logged_at timestamptz`, `group_sync_brake_pending integer`; `groups.grants_role text CHECK (grants_role IN ('admin','auditor'))`, `groups.missing_since timestamptz`, `groups.last_synced_at timestamptz`, unique index `groups_idp_external ON groups(org_id, external_id) WHERE source = 'idp'`; `users.role_source text NOT NULL DEFAULT 'manual' CHECK (role_source IN ('manual','idp'))`, `users.disabled_by text CHECK (disabled_by IN ('admin','microsoft'))`.

- [ ] **Step 1: Failing tests** in `tests/group_sync.rs` calling `opnrelay_server::graph` directly with an `AppState` built from the test app's config:
  - `graph_reads_directory`: `search_groups("hr")` returns `HR Staff` with `kind == "Security group"`; `group_user_ids` follows paging and nesting; `user_group_ids("u4")` contains `G` and `N`; `accounts_enabled(["u1","gone"])` → `{"u1": true}`; `get_group` of a deleted group → `Err(GraphError::NotFound)`.
  - `graph_caches_token_and_retries`: two calls → `app_token_requests == 1`; with `throttle_once` the call still succeeds; with `graph_fail` → `Err(GraphError::Failed(_))`.
  - `tenant_check`: `tenant_is_specific("organizations") == false`, `("common") == false`, `("consumers") == false`, a GUID and `"contoso.onmicrosoft.com"` are true.
- [ ] **Step 2:** Run `cargo test -p opnrelay-server --test group_sync graph_ tenant_check` — Expected: FAIL (module `graph` not found).
- [ ] **Step 3:** Write the migration; add `graph_token` to `AppState::new`; implement `graph.rs`; `pub mod graph;` in `lib.rs`.
- [ ] **Step 4:** Run the same tests — Expected: PASS. Also `cargo test -p opnrelay-server` — Expected: PASS (migration applies cleanly).
- [ ] **Step 5:** Commit `feat(server): Graph client for Entra groups (app-only token)`.

### Task 3: Sync settings, search and linking groups

**Files:**
- Modify: `crates/opnrelay-server/src/routes/sso.rs`, `crates/opnrelay-server/src/routes/admin.rs`
- Create: `crates/opnrelay-server/src/group_sync.rs` (stub `sync_group` used by link; full logic in Task 4)
- Test: `crates/opnrelay-server/tests/group_sync.rs`

**Interfaces:**
- Consumes: Task 2 `graph::*`.
- Produces:
  - `GET/PUT /api/v1/admin/sso` gain `group_sync_enabled: bool` (PUT) and read-only `group_sync_last_at`, `group_sync_last_ok_at`, `group_sync_last_error`, `group_sync_brake_pending`.
  - `GET /api/v1/admin/sso/groups/search?q=` → `[{id, display_name, description, kind, on_prem, linked: bool}]` (400 if sync off).
  - `POST /api/v1/admin/groups/link {external_id}` → `201 GroupRow`; 409 if already linked.
  - `GroupRow` (list and link) gains `source, external_id, grants_role, missing_since, last_synced_at`.
  - `PUT /admin/groups/{id}/members` on a `source='idp'` group → 400 "Members of Microsoft groups come from Microsoft".
  - `pub async fn sync_group(st: &AppState, p: &Provider, group_id: Uuid, actor: &AuditActor) -> Result<SyncCounts, GraphError>` in `group_sync.rs`; `pub struct SyncCounts { pub added: u32, pub removed: u32 }`.
  - Audit: `idp.group.linked`, `idp.sync.settings_changed`; the existing `DELETE /admin/groups/{id}` writes `idp.group.unlinked` (instead of its usual event) when the group is `source='idp'`.

- [ ] **Step 1: Failing tests:**
  - `enable_sync_requires_specific_tenant`: provider saved with tenant `organizations`; `PUT /admin/sso {group_sync_enabled:true,…}` → 400 and body detail contains `"specific tenant"`.
  - `search_and_link`: sync on; search `"hr"` lists `HR Staff` with `linked:false`; link it → 201, row has `source:"idp"`, members = the OpnRelay users whose oid is in the group (create two users by Microsoft sign-in first via `next_user` + `microsoft_flow`); search again shows `linked:true`; linking again → 409; audit has `idp.group.linked`.
  - `link_name_clash`: manual group `HR Staff` exists; linking Entra `HR Staff` creates `HR Staff (Microsoft)`; manual group unchanged.
  - `idp_members_read_only`: `PUT /admin/groups/{linked}/members` → 400.
  - `unlink_audited`: `DELETE /admin/groups/{linked}` → 204; group gone from workspace members; audit has `idp.group.unlinked`.
- [ ] **Step 2:** Run `cargo test -p opnrelay-server --test group_sync search_and_link link_name_clash idp_members_read_only enable_sync_requires_specific_tenant` — Expected: FAIL.
- [ ] **Step 3:** Implement settings fields, search, link (create group, then `sync_group`), refusal. `sync_group` replaces `group_members` for that group in one transaction with the oids → `user_identities(subject)` matches for the Microsoft provider, writes `group.member.added/removed` per change with `meta {"source":"microsoft"}`, sets `last_synced_at`, updates the name from Entra (keeping the clash suffix rule).
- [ ] **Step 4:** Run the tests — Expected: PASS.
- [ ] **Step 5:** Commit `feat(server): link Entra groups and search the directory`.

### Task 4: Full sync, scheduler, Sync now, failures

**Files:**
- Modify: `crates/opnrelay-server/src/group_sync.rs`, `crates/opnrelay-server/src/routes/sso.rs`, `crates/opnrelay-server/src/main.rs`
- Test: `crates/opnrelay-server/tests/group_sync.rs`

**Interfaces:**
- Consumes: `sync_group` (Task 3).
- Produces:
  - `pub struct RunSummary { pub groups: u32, pub added: u32, pub removed: u32, pub roles_changed: u32, pub disabled: u32, pub missing: u32, pub error: Option<String>, pub brake: Option<u32> }`
  - `pub async fn sync_all(st: &AppState, actor: &AuditActor, confirm_brake: bool) -> anyhow::Result<RunSummary>` — takes `pg_try_advisory_lock(0x6f70_6e72)`; if not acquired returns `error: Some("a sync is already running")`; skipped entirely when sync is off.
  - `pub fn due(last: Option<OffsetDateTime>, now: OffsetDateTime) -> bool` (true when `last` is None or older than 1 hour)
  - `pub fn spawn_scheduler(st: AppState)` (5-minute tick; calls `sync_all(.., false)` with actor label `"Microsoft sync"` when `due`)
  - `POST /api/v1/admin/sso/sync {confirm_brake?: bool}` → `RunSummary`.
- Rules from spec: per-group transaction; Graph failure stops the run, leaves remaining groups untouched, sets `group_sync_last_error`; 404 → `missing_since = now()`, members removed, audit `idp.group.missing` once; success clears `missing_since` if the group reappears; `group_sync_last_ok_at` set only on a run with no error; `idp.sync.failed` audit only when previous run was OK or `group_sync_failed_logged_at` is over 24 h old.

- [ ] **Step 1: Failing tests:**
  - `due_logic`: `due(None, t) == true`; `due(Some(t - 59min), t) == false`; `due(Some(t - 61min), t) == true`.
  - `removal_takes_effect_next_request`: user in linked `HR Staff`, group on workspace `HR` as viewer; user `GET /workspaces` lists `HR`; remove user from fake group; `POST /admin/sso/sync` → `removed == 1`; same user session `GET /workspaces` → empty, `GET /guides/{hr_guide}` → 404.
  - `nested_membership_counts`: user only in nested `N` inside linked `G` → member after sync.
  - `graph_failure_changes_nothing`: set `graph_fail`; sync → `error.is_some()`; memberships unchanged; `GET /admin/sso` shows `group_sync_last_error`; second failing sync immediately after writes no second `idp.sync.failed` audit event.
  - `deleted_group_emptied`: delete group in fake; sync → `missing == 1`; group row has `missing_since`, 0 members.
  - `noop_sync_writes_no_audit`: count `audit_events`, run sync twice with no changes, count unchanged.
  - `sync_off_freezes`: turn sync off; change fake membership; `POST /admin/sso/sync` → 400 "Group sync is off"; members unchanged.
- [ ] **Step 2:** Run `cargo test -p opnrelay-server --test group_sync` — Expected: the new tests FAIL.
- [ ] **Step 3:** Implement `sync_all`, `due`, `spawn_scheduler`, the route; call `group_sync::spawn_scheduler(state.clone())` next to the backup scheduler in `main.rs`.
- [ ] **Step 4:** Run the tests — Expected: PASS.
- [ ] **Step 5:** Commit `feat(server): hourly Entra group sync with safe failure handling`.

### Task 5: Sync at Microsoft sign-in

**Files:**
- Modify: `crates/opnrelay-server/src/group_sync.rs`, `crates/opnrelay-server/src/routes/sso.rs` (`handle_callback`, after `resolve_user`, before `create_session`)
- Test: `crates/opnrelay-server/tests/group_sync.rs`

**Interfaces:**
- Produces: `pub async fn sync_user(st: &AppState, p: &Provider, user_id: Uuid, oid: &str) -> Result<(), GraphError>` — sets membership of every linked (non-missing) group for this user from `graph::user_group_ids`, then `recompute_role` (Task 6; until then a no-op). On `Err`, the callback logs `tracing::warn!` and continues.

- [ ] **Step 1: Failing tests:**
  - `first_sign_in_lands_in_workspace`: JIT on, sync on, `HR Staff` linked and on workspace `HR`; a brand-new oid in `HR Staff` signs in via `microsoft_flow`; that browser's `GET /api/v1/workspaces` lists `HR`.
  - `sign_in_survives_graph_outage`: `graph_fail = true`; sign-in redirects to `/#/` with a session cookie (`GET /api/v1/me` 200); existing memberships unchanged.
- [ ] **Step 2:** Run — Expected: FAIL.
- [ ] **Step 3:** Implement `sync_user` and the callback hook (only when `group_sync_enabled`).
- [ ] **Step 4:** Run — Expected: PASS.
- [ ] **Step 5:** Commit `feat(server): apply Entra groups at Microsoft sign-in`.

### Task 6: Roles from groups

**Files:**
- Modify: `crates/opnrelay-server/src/group_sync.rs`, `crates/opnrelay-server/src/routes/admin.rs`
- Test: `crates/opnrelay-server/tests/group_sync.rs`

**Interfaces:**
- Produces:
  - `pub async fn recompute_roles(tx: &mut PgConnection, org: Uuid, only_user: Option<Uuid>, actor: &AuditActor) -> sqlx::Result<u32>` (returns number of role changes). Algorithm: if no linked group in the org has `grants_role`, set every `role_source='idp'` user back to `'manual'` keeping their role, return 0. Otherwise, for every non-owner user with a Microsoft identity: `role_source='idp'`, role = highest `grants_role` among their linked groups (`admin` > `auditor`), else `member`; audit `user.role.changed {from,to,source:"microsoft"}` on change.
  - `PATCH /api/v1/admin/groups/{id} {grants_role: null|"admin"|"auditor"}` (idp groups only; 400 otherwise) → `GroupRow`; audit `idp.group.role_changed`; then `recompute_roles` for the org.
  - `GET /api/v1/admin/groups/{id}/role-preview?grants_role=admin` → `{ "changes": n }` (dry run in a rolled-back transaction).
  - `PATCH /admin/users/{id}` with `org_role` for a `role_source='idp'` user → 400 "This person's role is set by a Microsoft group".
  - `GET /admin/users` rows gain `role_source` and `role_from: Option<String>` (name of the granting group).
  - `sync_all` and `sync_user` call `recompute_roles` after membership changes.

- [ ] **Step 1: Failing tests:**
  - `role_granted_and_revoked`: link `IT Admins`, PATCH `grants_role:"admin"`; member in it becomes `admin` with `role_source:"idp"`; remove from fake group + sync → `member`.
  - `highest_role_wins`: user in `IT Admins` (admin) and `Compliance` (auditor) → admin; leave `IT Admins` → auditor.
  - `owner_untouched`: owner linked to Microsoft and absent from all groups stays `owner`, `role_source:"manual"`.
  - `manual_change_refused_while_managed`: PATCH role on managed user → 400; after PATCH `grants_role:null` on the last mapping, user is `role_source:"manual"` with role unchanged and PATCH role succeeds.
  - `role_preview_counts`: Microsoft-linked users A (member, in `IT Admins`), B (member, not in it) and C (manual admin, not in it); preview `grants_role=admin` on `IT Admins` → `changes == 2` (A promoted, C demoted, B unchanged); nothing is actually changed afterwards.
- [ ] **Step 2:** Run — Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run — Expected: PASS.
- [ ] **Step 5:** Commit `feat(server): org roles from Entra groups`.

### Task 7: Leavers and the safety brake

**Files:**
- Modify: `crates/opnrelay-server/src/group_sync.rs`, `crates/opnrelay-server/src/routes/sso.rs` (`resolve_user` re-enable), `crates/opnrelay-server/src/routes/admin.rs` (admin disable sets `disabled_by='admin'`, enable clears it)
- Test: `crates/opnrelay-server/tests/group_sync.rs`

**Interfaces:**
- Produces:
  - `pub fn brake_trips(to_disable: usize, linked_users: usize) -> bool` → `to_disable > 10 && to_disable * 5 > linked_users`.
  - Leaver step inside `sync_all` (after groups, before roles): `graph::accounts_enabled` for all active users with a Microsoft identity; candidates = disabled or absent, excluding owners (owners listed in `group_sync_last_error` as "Owner <name> is disabled in Microsoft"); if `brake_trips` and not `confirm_brake` → store `group_sync_brake_pending = n`, error "Sync would have disabled N people; nothing was changed…", audit `idp.sync.failed {reason:"brake", count:n}`; else for each: `status='disabled'`, `disabled_by='microsoft'`, delete sessions, revoke device grants (same SQL as `admin.rs:253`), remove from all idp groups, audit `user.disabled {source:"microsoft", reason:"disabled"|"deleted"}`. Any Graph error here → no one disabled.
  - Re-enable: in `resolve_user`, a `disabled` user with `disabled_by='microsoft'` signing in with Microsoft (Microsoft only issues the token for enabled accounts) → `status='active'`, `disabled_by=NULL`, audit `user.enabled {source:"microsoft"}`; `disabled_by='admin'` keeps today's "your OpnRelay account is disabled" error.

- [ ] **Step 1: Failing tests:**
  - `brake_boundary`: `brake_trips(10, 20) == false`; `brake_trips(11, 50) == true`; `brake_trips(11, 60) == false`.
  - `leaver_disabled_and_signed_out`: user signed in (session + a device grant via the existing OAuth helper in `tests/api.rs` pattern); fake sets `users[oid] = false`; sync → `disabled == 1`; user's session `GET /me` → 401; device refresh fails; `audit_events` has `user.disabled` with `source:"microsoft"`.
  - `deleted_user_treated_as_leaver`: remove oid from fake users → disabled with `reason:"deleted"`.
  - `reenable_on_sign_in_only_if_microsoft_disabled`: re-enable in fake, sign in → active. Second user disabled by admin → Microsoft sign-in still refused.
  - `owner_never_disabled`: owner's oid disabled in fake → owner stays active; `group_sync_last_error` mentions the owner.
  - `brake_blocks_mass_disable`: 50 linked users, 11 disabled in fake → sync disables none, `group_sync_brake_pending == 11`; `POST /admin/sso/sync {confirm_brake:true}` → 11 disabled, `brake_pending` cleared.
  - `graph_error_disables_no_one`: users endpoint fails (set a `users_fail` flag in the fake) → nobody disabled.
- [ ] **Step 2:** Run — Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p opnrelay-server` — Expected: all PASS.
- [ ] **Step 5:** Commit `feat(server): disable Entra leavers within the hour, with a safety brake`.

### Task 8: Admin web UI for group sync

**Files:**
- Create: `apps/web/src/routes/GroupsTab.svelte` (move the current Groups tab markup from `Admin.svelte:159-179` here)
- Modify: `apps/web/src/lib/api.ts`, `apps/web/src/routes/Admin.svelte`, `apps/web/src/routes/SsoSettings.svelte`

**Interfaces:**
- Consumes: Task 3–7 endpoints.
- Produces: `api.ts` types `Group { id; name; source: 'manual'|'idp'; external_id: string|null; grants_role: 'admin'|'auditor'|null; missing_since: string|null; last_synced_at: string|null; member_ids: string[] }`, `DirGroup`, `SyncSummary`, and `User` gains `role_source`, `role_from`.
- UI (current visual style; the Runbook restyle is a separate plan):
  - **SsoSettings**: "Group sync" section: checkbox, the two permission names with Copy buttons, last run / last success / error, **Sync now** (shows summary), and the brake banner with **Disable N people** confirm button calling `{confirm_brake:true}`. Warning when no success for 24 h: "No successful sync for over 24 hours."
  - **GroupsTab**: when sync is off, linked groups show "Not syncing"; sync status banner (green OK / red error, Sync now); list shows a "Microsoft" tag, member count, workspaces, "Also make members: nothing/auditor/admin" select for idp groups (before saving, call role-preview and show "This changes the role of N people." with Confirm/Cancel in-page, no `confirm()`), red "Deleted in Microsoft" with Unlink (in-page confirmation naming the workspaces), members read-only for idp groups; "Add from Microsoft" panel: search box (debounced 300 ms), results with Linked / Link.
  - **Users tab**: role select disabled for `role_source==='idp'` with "Set by Microsoft group <role_from>".

- [ ] **Step 1:** Implement the components.
- [ ] **Step 2:** Run `cd apps/web && pnpm check && pnpm test && pnpm build` — Expected: no errors, tests pass.
- [ ] **Step 3:** Manual check against a local server with the fake directory is not available; instead run the server against Postgres, enable sync with a non-reachable Graph and confirm the error banner renders and Sync now reports the error (screenshot with Playwright, deviceScaleFactor 1).
- [ ] **Step 4:** Commit `feat(web): Microsoft group sync admin UI`.

### Task 9: Empty home page states

**Files:**
- Modify: `apps/web/src/routes/Home.svelte:146-150`

- [ ] **Step 1:** Replace the empty-state copy:
  - Admin/owner (`isAdmin(me)`) with zero workspaces: heading "Create your first workspace", text "Workspaces hold your guides and decide who can see them, for example one per department.", button **Create workspace** (calls the existing `newWorkspace`).
  - Member with zero workspaces: heading "You haven't been added to a workspace yet.", text "Ask your administrator to add you. Guides will appear here once you are."
  - Remove the "Pick a workspace on the left." branch; a member with workspaces but no guides keeps "Nothing has been published here yet."
- [ ] **Step 2:** Run `cd apps/web && pnpm check && pnpm build` — Expected: OK.
- [ ] **Step 3:** Screenshot both states with Playwright (a fresh owner; an invited member) and look at them.
- [ ] **Step 4:** Commit `fix(web): clear empty states for new admins and members`.

### Task 10: Documentation

**Files:**
- Modify: `docs/microsoft.md`, `docs/roadmap.md`

- [ ] **Step 1:** In `docs/microsoft.md`: add the two application permissions to step 1.4 (with "Grant admin consent"); new section "Group sync" covering linking, hourly/sign-in timing, nested groups, on-prem AD note (about 90 minutes worst case with Entra Connect's 30-minute cycle), roles, leavers, the safety brake, what happens when sync is off or Microsoft is unreachable; replace "Planned: mapping Entra groups…" line; add the "Assignment required?" tip for limiting who can sign in.
- [ ] **Step 2:** In `docs/roadmap.md` tick "Entra group → role mapping" (keep "SAML 2.0 for other providers" unticked).
- [ ] **Step 3:** Commit `docs: Microsoft group sync and leavers`.
