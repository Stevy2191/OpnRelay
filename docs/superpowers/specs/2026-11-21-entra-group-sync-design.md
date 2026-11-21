# Microsoft Entra group sync: design

Status: approved with leaver handling · 2026-11-21

## Why

Organisations using OpnRelay keep their departments in Microsoft Entra ID
already: *HR Staff*, *IT Support*, *Compliance*. Today an admin has to recreate
those groups by hand in OpnRelay, and keep them in step whenever someone joins,
moves or leaves. When that slips, someone who has moved out of HR can still read
HR guides.

The goal: **who belongs to which Entra group decides what they can see in
OpnRelay, and changes take effect within the hour, without anyone touching
OpnRelay.**

Most guide authors will be HR and IT staff writing how-tos for the rest of the
organisation.

## What exists today

- Access is per **workspace**. Members see a workspace only if they, or a group
  they belong to, are listed on it as viewer, editor or manager. Owners and admins
  see everything; auditors read everything. A new member in no workspace sees
  nothing (verified on a running server: no workspaces, no guides, direct guide
  links return 404).
- **Groups** (Admin → Groups) can be added to workspaces. The `groups` table
  already has `source ('manual'|'idp')` and `external_id`, unused so far.
- **Microsoft sign-in** (OIDC + PKCE) links accounts by the Entra object ID
  (`oid`), stored in `user_identities.subject`. That is the same ID Graph returns
  for group members, so no extra matching is needed.
- The server already holds the app's client ID, tenant ID and (encrypted) client
  secret, and talks to Graph for OneDrive backups. `OPNRELAY_MS_GRAPH_URL` /
  `OPNRELAY_MS_LOGIN_URL` let tests point at a fake Microsoft
  (`crates/opnrelay-server/tests/microsoft.rs`).

## Goals

1. Admins **link Entra groups** into OpnRelay by searching for them by name. Each
   linked group appears in Admin → Groups as a *Microsoft* group and is added to
   workspaces exactly like a manual group.
2. A linked group's **members follow Entra**, including members of nested groups.
   They are read-only in OpnRelay.
3. Membership is refreshed **every hour**, **at each Microsoft sign-in** (for that
   person), **when a group is first linked**, and **on demand** ("Sync now").
4. Optionally a linked group can **grant an organisation role** (*admin* or
   *auditor*), so for example *IT Admins* become OpnRelay admins.
5. **Leavers lose access within the hour**: when a person's Entra account is
   disabled or deleted (in a hybrid setup, usually by disabling them in
   on-premises AD), OpnRelay disables their account and ends all their sessions.
6. Every change made by sync is in the **audit log**.

## Non-goals

- Full SCIM provisioning (creating accounts ahead of first sign-in, syncing
  profile fields). Leavers *are* handled; see "Leavers" below.
- Creating OpnRelay accounts for group members who have never signed in.
  Accounts are still created on first sign-in (or by invitation); the new account
  gets its groups at that first sign-in.
- Mapping groups straight to workspaces without an OpnRelay group in between.
- SAML or other identity providers.

## Anti-goals (would make this a failure even if it works)

- A Microsoft outage or an expired client secret **locking everyone out**, or
  silently **removing everyone's access**.
- Sync ever changing or removing an **owner**.
- Admins having to paste long group IDs.
- Hourly sync filling the audit log with "nothing changed" entries.
- A misconfiguration (wrong tenant, broken permission) **mass-disabling** accounts.

## Approaches considered

| | How | Trade-off |
|---|---|---|
| **Mirror Entra groups as OpnRelay groups** *(chosen)* | Linked group = row in `groups` with `source='idp'`; sync maintains `group_members`. | Reuses the existing group → workspace permission system, the UI, and backups; no new permission paths to audit. |
| Map Entra group → workspace + role directly | New mapping table checked by the permission code. | A second way to grant access, so two code paths to keep correct and explain. |

How quickly removals take effect (the user chose **B**):

| | How | Trade-off |
|---|---|---|
| A. At next sign-in | Read groups from the sign-in only. | No new Microsoft permission, but someone moved out of HR keeps access for up to 7 days (session limit). |
| **B. Hourly + at sign-in** *(chosen)* | Server signs in as the app itself (client credentials) and re-reads linked groups hourly. | Needs read-only application permissions with admin consent. Also enables searching groups by name. |

## Design

### Microsoft side

Add **application** permissions (admin consent) to the existing app registration:

- `GroupMember.Read.All`: list and search groups, read their (transitive) members.
- `User.Read.All`: read whether each linked person's account is enabled
  (`accountEnabled`) for leaver handling, and the per-user group check at sign-in
  (`/users/{oid}/transitiveMemberOf`). Read-only. *Builder: confirm against current
  Graph docs that this pair is the least privileged that covers both.*

The server obtains an app-only token with the client-credentials grant against
`{login}/{tenant}/oauth2/v2.0/token`, scope `https://graph.microsoft.com/.default`,
using the client secret it already stores. The tenant must be a specific tenant
(GUID or domain), not `organizations`/`common`; enabling group sync is refused
otherwise with a clear message. Tokens are cached in memory until shortly before
expiry.

Graph calls:

- Search: `GET /groups?$search="displayName:{q}"&$select=id,displayName,description&$top=20`
  with `ConsistencyLevel: eventual`.
- Group members: `GET /groups/{id}/transitiveMembers/microsoft.graph.user?$select=id`
  following `@odata.nextLink`.
- One person: `GET /users/{oid}/transitiveMemberOf/microsoft.graph.group?$select=id`
  (paged), intersected with the linked groups.
- Honour `429`/`503` `Retry-After` with a bounded retry.

### Data model (new migration `0004_group_sync.sql`)

- `identity_providers`: `group_sync_enabled boolean NOT NULL DEFAULT false`,
  `group_sync_last_at timestamptz`, `group_sync_last_error text`,
  `group_sync_last_ok_at timestamptz`.
- `groups`: `grants_role text NULL CHECK (grants_role IN ('admin','auditor'))`,
  `missing_since timestamptz NULL` (set when Entra says the group no longer
  exists), and a unique index on `(org_id, external_id) WHERE source = 'idp'`.
- `users`: `role_source text NOT NULL DEFAULT 'manual' CHECK (role_source IN
  ('manual','idp'))`.
- The unused `identity_providers.group_mapping` column is left alone.

All new columns live in tables the backup already includes.

### Linking groups

Admin → Groups gets **"Add from Microsoft"** (shown when group sync is on):
search box → results list → **Link**. Linking creates the group with the Entra
display name (renamed if the name clashes with a manual group: "HR Staff
(Microsoft)"), and syncs it immediately so admins see the members right away.

A Microsoft group in the list shows a *Microsoft* badge, its member count, last
sync time, and **read-only** members (the "edit members" control is hidden and
the API refuses `PUT /admin/groups/{id}/members` for `source='idp'`). It has one
setting: **"Also make members: nothing / auditor / admin"**. **Unlink** deletes the
OpnRelay group (same as deleting a manual group today; it drops it from every
workspace) after an in-page confirmation that names the workspaces it is on.

The Entra display name is refreshed on each sync (keeping any "(Microsoft)"
suffix logic), so renames in Entra show up.

### Sync

One function, `sync_groups(st, scope)`, where `scope` is *all linked groups* or
*one user*:

- **All groups** (hourly, on "Sync now", on link): for each linked group, fetch
  transitive user members; map Entra IDs to OpnRelay users via `user_identities`
  for the Microsoft provider (users without an OpnRelay account are skipped);
  replace that group's `group_members` with the result in one transaction. Then
  recompute roles (below). A scheduler task, like the backup scheduler, checks
  every 5 minutes and runs when the last run is over an hour old. Only one sync
  runs at a time (Postgres advisory lock), so several server instances are safe.
- **One user** (each Microsoft sign-in, inside `handle_callback` before the session
  is created): fetch that user's transitive groups, set their membership in every
  linked group accordingly, recompute their role. A new account created on first
  sign-in therefore lands in the right workspaces immediately.

**Turning group sync off** stops all syncing (hourly and at sign-in). Linked
groups stay, with their last memberships, but are marked "Not syncing" in Admin →
Groups. Admins can then unlink them or turn sync back on. *(Builder-check call:
freezing is the least surprising choice; emptying groups would cut everyone off
by flipping one switch.)*

**Failure rules (fail safe, never fail open or wide):**

- If the token request or any Graph call fails during a full sync, **nothing is
  changed** for the groups not yet synced; record `group_sync_last_error`, keep
  existing memberships. A group already synced in the same run keeps its new
  membership (each group is its own transaction).
- If Entra answers **404 for a group**, set `missing_since`, remove all its
  members, and show it in red ("Deleted in Microsoft") so the admin can unlink it.
- If the per-user check fails at sign-in, **sign-in still succeeds** with the
  person's existing memberships, and a warning is logged.
- Admin → Microsoft sign-in and Admin → Groups show a warning banner when the last
  sync failed, with the error and the time of the last successful sync. If there
  has been no successful sync for 24 hours, the banner says so plainly.

### Leavers

Context: this organisation's accounts are synced from on-premises AD to Entra
(Entra Connect, ~30-minute cycle). The normal leaver process is disabling the AD
account; the disabled state reaches Entra, but group memberships usually stay.

- Part of each full sync (only while group sync is on): read `accountEnabled` for
  every OpnRelay user with a linked Microsoft identity (batched; `$batch` or
  `id in (...)` filters, builder's choice). Users Graph reports as **not found**
  count as deleted.
- For each such user who is **disabled or deleted in Entra** and active in
  OpnRelay: set `status = 'disabled'`, `disabled_by = 'microsoft'`, revoke all
  sessions and device grants (desktop recorder, browser extension), and remove
  them from all Microsoft-linked groups. Audit `user.disabled` with source
  `microsoft` and the reason (disabled / deleted).
- **Owners are never disabled** by sync; instead the sync warning banner says
  "Owner *name* is disabled in Microsoft".
- **Re-enabling**: if the person is enabled in Entra again, their next Microsoft
  sign-in re-activates them, but only when `disabled_by = 'microsoft'`. Accounts
  an admin disabled by hand (`disabled_by = 'admin'`) stay disabled. Audit
  `user.enabled` with source `microsoft`.
- **Safety brake**: if one run would disable more than 10 people *and* more than
  20% of Microsoft-linked users, it disables no one, records `idp.sync.failed`
  with the reason, and shows the banner "Sync would have disabled N people;
  nothing was changed. Check the Microsoft settings, then run Sync now and
  confirm." Admins can confirm that one run from the banner.
- Any Graph error while reading accounts means no one is disabled in that run.
- Data model: `users.disabled_by text NULL CHECK (disabled_by IN
  ('admin','microsoft'))`; the existing admin disable action sets `'admin'`.

Worst case from disabling someone in AD to losing OpnRelay access: about 90
minutes (Entra Connect cycle plus the hourly sync). Hourly was chosen over 15
minutes.

### Roles

- Only users with `role_source = 'idp'` have their role set by sync. Owners are
  never touched.
- A Microsoft-linked, non-owner user becomes `role_source = 'idp'` as soon as any
  linked group in the org has `grants_role` set. Their role is then the highest
  role granted by the linked groups they are in (admin > auditor > member), or
  **member** if none.
- While `role_source = 'idp'`, the role picker in Admin → Users is disabled with
  the note "Set by Microsoft group *IT Admins*" (or "Set by Microsoft groups:
  member"). The API refuses manual role changes for them.
- If the last role-granting mapping is removed, those users go back to
  `role_source = 'manual'` and **keep their current role**.
- When an admin first sets "Also make members: …" on any group, the confirmation
  says how many existing Microsoft-linked users would change role, so nobody is
  surprised by an admin being demoted.

### Audit

Recorded with the actor "Microsoft sync" (or the admin, for link/unlink/settings):

- `idp.group.linked`, `idp.group.unlinked`, `idp.group.role_changed`,
  `idp.sync.settings_changed`
- `group.member.added` / `group.member.removed` (one per person, with group and
  source `microsoft`)
- `user.role.changed` with source `microsoft`
- `idp.sync.failed` (only when the state goes from OK to failing, and once per day
  while it keeps failing), `idp.group.missing`
- No event for a sync that changed nothing.

### Admin UI

- **Admin → Microsoft sign-in**: new "Group sync" section: on/off, the extra
  permissions to grant in Entra (copyable names), last sync time and result,
  **Sync now**.
- **Admin → Groups**: "Add from Microsoft" (search and link), Microsoft badge,
  read-only members, "Also make members", Unlink, red state for missing groups,
  sync warning banner.
- **Admin → Users**: role picker disabled for Microsoft-managed roles, with the
  reason.

### API (all admin-only, CSRF as usual)

- `GET /admin/sso/groups/search?q=` → `[{id, display_name, description, linked}]`
- `POST /admin/groups/link` `{external_id}` → group
- `PATCH /admin/groups/{id}` `{grants_role}` (Microsoft groups only)
- `POST /admin/sso/sync` → runs a full sync now, returns the summary
- `GET/PUT /admin/sso` gain `group_sync_enabled` and the last-sync fields
- `GET /admin/groups` rows gain `source`, `external_id`, `grants_role`,
  `missing_since`, `last_synced_at`

### Related small fix (same change set)

The home page's empty state, which today always says "Pick a workspace on the
left":

- Owner/admin with no workspaces: "Create your first workspace" with the button.
- Member with no workspaces: "You haven't been added to a workspace yet. Ask your
  administrator." No "Pick a workspace" wording.

### Docs

Update `docs/microsoft.md` (permissions, how sync works, failure behaviour, roles)
and tick "Entra group → role mapping" in `docs/roadmap.md`.

## Testing

Extend the fake Microsoft in `tests/microsoft.rs` with the client-credentials
token endpoint, group search, transitive members (with paging), per-user
transitive groups, 404 and 429 responses. Integration tests cover:

- linking a group syncs its members; members then see the group's workspace
- removing someone from the Entra group removes access after a sync
- nested group membership counts
- first Microsoft sign-in of a new person lands them in the right workspace
- Graph failure leaves memberships unchanged and records the error
- group deleted in Entra empties the group and marks it missing
- leaver: disabled in Entra → OpnRelay account disabled, sessions and device
  grants revoked, audit written; deleted (404) treated the same; re-enabled in
  Entra → next sign-in re-activates; admin-disabled accounts stay disabled;
  owners never disabled; the safety brake stops a mass-disable
- role mapping: admin/auditor granted and revoked; owners untouched; manual role
  edits refused while managed; removing the last role mapping returns users to
  manual with their role kept
- members of a Microsoft group can't be edited through the API
- audit events are written for changes and not for no-op syncs

Plus the web type check, and the empty-state change covered by a quick manual
check.

## Left to the builder

- Exact Graph permission pair (see above), page sizes, retry counts and back-off.
- How the token cache and advisory lock are structured.
- Wording of banners and confirmations, within the rules above.
