# Runbook Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the OpnRelay web app (and then the recorders) the approved "Runbook" look: navy sidebar shell, signal-orange accent, IBM Plex type, workspace card view, step-indexed reader and a one-step-at-a-time editor.

**Architecture:** Design tokens and base components live in `apps/web/src/app.css`; a new app shell in `App.svelte` replaces the top-bar layout; each screen is restyled in its own task so every task ships a working app. One small server addition (a thumbnail id on guide summaries) supports the cards. A committed screenshot tool lets every task and review see the result in light and dark, desktop and phone width.

**Tech Stack:** Svelte 5 + TypeScript + Vite; `@fontsource/ibm-plex-sans`, `@fontsource/ibm-plex-mono` (OFL, bundled locally); Rust/Axum (one field); Playwright for screenshots; egui (desktop recorder colours).

**Spec:** `docs/superpowers/specs/2026-11-21-runbook-redesign-design.md` (mockups: "OpnRelay Design Directions" page, Runbook, workspace, editor and Microsoft groups sections)

## Global Constraints

- Light tokens (exact): sidebar ink `#0f1a2b`, ink-2 `#17243a`, paper `#f5f6f8`, card `#ffffff`, line `#dfe3ea`, text `#182235`, muted `#627089`, signal orange `#ff6b1a`, signal soft `#fff0e6`, signal text `#b14708`, ok `#138a5e` (soft `#eef9f4`, line `#b5dfcd`), warn `#9a5a00` (soft `#fff7ea`, line `#f0d3a8`), danger `#b42318` (soft `#fdecec`).
- Dark tokens (exact): sidebar ink `#0b1424`, ink-2 `#16233a`, paper `#0e1626`, card `#152036`, line `#26344d`, text `#e6ebf3`, muted `#93a1b8`, signal orange `#ff7a2e`, signal soft `#3a2414`, signal text `#ffb27f`, ok `#3ccf91` (soft `#123327`), warn `#f0b45a` (soft `#33280f`), danger `#ff7b72` (soft `#3a1717`). The app follows the system light/dark setting as today.
- Fonts: IBM Plex Sans 400/500/600/700 for text; IBM Plex Mono 400/500 for step numbers, versions, IDs, status tags and keyboard hints. Bundled via `@fontsource/*` (the server's CSP is `font-src 'self'`; no external font hosts).
- Status tags are mono uppercase: `PUBLISHED v3`, `DRAFT`, `NEEDS REVIEW`, `INTERNAL` / `CONFIDENTIAL` / `RESTRICTED`.
- Contrast: body text and controls meet WCAG 2.1 AA in both themes; keyboard focus always visible (2px orange outline).
- Phone width (390px): no horizontal page scroll; sidebar becomes a drawer below 900px.
- Company branding: the app chrome always uses the Runbook palette. The company logo and name appear in the sidebar header. Branding colours keep applying to exports (unchanged) — `session.svelte.ts` stops overriding `--primary`.
- No browser `alert/confirm/prompt` in any screen touched by this plan; confirmations are in-page.
- No behaviour regressions: every existing action stays reachable (search, new workspace/folder/guide, workspace settings, export, duplicate, delete, publish, versions/restore, highlight, paste/upload screenshots, admin tabs, audit, account/MFA, downloads, Microsoft group sync UI, needs-review flag).
- Each task ends with `cd apps/web && pnpm check && pnpm test && pnpm build` clean and screenshots from `apps/web/tools/ui-shots.mjs` (Task 1) looked at in light and dark, desktop and phone.
- Commit messages end with the repo's two attribution lines (`Co-Authored-By: …`, `Claude-Session: …`).

## Review Focus

1. **Long names** (workspace "Human Resources & Benefits Administration", 120-char guide titles, long folder names) → wrap or truncate with ellipsis and a title tooltip; never overflow cards, sidebar or table cells. (Tasks 2, 3)
2. **Empty states in the new shell** (no workspaces, no guides, empty folder, member with no access, search with no results) → each has its own message in the new style, matching the copy already in `Home.svelte`. (Task 3)
3. **Guide with no screenshots / steps without media** (headings, tips, text-only steps) → reader, editor and card thumbnail handle them (card shows "No screenshots yet" pattern). (Tasks 3, 4, 5)
4. **Keyboard use** → `/` focuses search (not while typing in an input), sidebar and step index reachable by Tab, editor step list operable with arrow keys and Enter. (Tasks 2, 4, 5)
5. **Dark mode** → every new surface uses tokens; no hard-coded light colours (screenshots in dark for every task). (All)

---

### Task 1: Foundation — tokens, fonts, base components, screenshot tool

**Files:**
- Modify: `apps/web/package.json` (+ lockfile), `apps/web/src/main.ts`, `apps/web/src/app.css`, `apps/web/src/lib/session.svelte.ts`
- Create: `apps/web/tools/ui-shots.mjs`, `apps/web/tools/README.md`

**Interfaces:**
- Produces CSS custom properties (both themes): `--ink --ink-2 --paper --card --line --text --muted --sig --sig-soft --sig-text --ok --ok-soft --ok-line --warn --warn-soft --warn-line --danger --danger-soft --radius (6px) --radius-lg (8px) --font-sans --font-mono --shadow-pop`. Keep the old names (`--bg --surface --surface-2 --border --muted --primary --accent --danger --ok --tip --alert`) as aliases to the new tokens so untouched screens keep working until restyled.
- Base classes in `app.css`: `.btn` (+ `.primary` navy, `.signal` orange, `.danger`, `.small`, `.ghost`), inputs/selects/textarea, `.card`, `.tag` (+ `.pub .draft .review .class`), `.mono`, `.crumb`, `table.data`, `.callout` (+ `.tip .alert .warning .error`), `.kbd`, focus ring. Existing class names used by screens (`button.primary`, `.small`, `.card`, `.muted`, `.error`, `.success`, `.row`, `.stack`) keep working.
- `tools/ui-shots.mjs`: Node script. Args: `--base http://127.0.0.1:PORT --out DIR [--pages home,guide,edit,admin,...]`. It creates (idempotently) an owner + editor + viewer, 3 workspaces with folders, 6 guides (one with a 120-char title, one with no screenshots, one flagged needs-review via the API + a printed `psql` hint if it can't backdate), signs in with Playwright and saves `NAME-light-desktop.png`, `NAME-dark-desktop.png`, `NAME-light-phone.png` (390×844) for each page. `tools/README.md` says how to start a throwaway server (env vars from `crates/opnrelay-server/src/config.rs`) and run it.

- [ ] **Step 1:** `pnpm add @fontsource/ibm-plex-sans @fontsource/ibm-plex-mono`; import the needed weights in `main.ts`.
- [ ] **Step 2:** Rewrite `app.css` tokens and base components per Interfaces; remove the `--primary` override in `session.svelte.ts` (keep loading branding for the logo).
- [ ] **Step 3:** Write `tools/ui-shots.mjs` + README; run it against a throwaway server; look at Home/Guide/Admin in light and dark: existing screens must still be usable with the new base styles.
- [ ] **Step 4:** `pnpm check && pnpm test && pnpm build` clean; commit `feat(web): Runbook design tokens, fonts and base components`.

### Task 2: App shell — sidebar, top bar, drawer on phones

**Files:**
- Create: `apps/web/src/lib/Shell.svelte`, `apps/web/src/lib/RecordPanel.svelte`
- Modify: `apps/web/src/App.svelte`, `apps/web/src/routes/Home.svelte` (remove its own workspace sidebar), `apps/web/src/lib/session.svelte.ts` (shared workspace list store)

**Interfaces:**
- Produces `workspacesStore` in `session.svelte.ts`: `{ list: Workspace[], loaded: boolean, refresh(): Promise<void> }`, used by the shell and Home (Task 3) so the list is fetched once and refreshed after create/rename/delete.
- Shell layout (mockup "Runbook"): left sidebar 232px navy: brand (company logo or OpnRelay mark + org name), **Library** (All guides, Drafts, Needs review — the last only for users who can edit somewhere, with count), **Workspaces** (each with guide count; `+ New` for admins), **Admin** (People & groups → `#/admin`, Audit log, Downloads, Settings → `#/admin?tab=policies`) shown per role as today, and a bottom orange **Record a guide** button opening `RecordPanel` (desktop recorder + browser extension explanation, link to Downloads for admins or "ask your administrator" for others, and **New guide from screenshots** which runs the existing create-guide flow). Top bar 56px: search (placeholder "Search guides, steps, apps…", `/` shortcut hint), user menu (name → Account, Sign out). Active item: ink-2 background + 2px orange inset.
- Below 900px: sidebar hidden behind a menu button in the top bar; opens as an overlay drawer; closes on navigation and Escape.
- Routes unchanged (hash routes in `App.svelte`). Login/Invite/Connect pages stay outside the shell.

- [ ] **Step 1:** Implement the store, Shell, RecordPanel; move Home's workspace list into the shell.
- [ ] **Step 2:** `pnpm check && pnpm test && pnpm build`; run `tools/ui-shots.mjs`; check Review Focus 1, 4, 5 and the phone drawer.
- [ ] **Step 3:** Commit `feat(web): Runbook app shell with sidebar and phone drawer`.

### Task 3: Guide lists — All guides table, workspace cards by folder

**Files:**
- Modify: `crates/opnrelay-server/src/routes/guides.rs` (`GuideSummary` gains `thumb_media_id: Option<Uuid>`, the media of the first step with media by position), `crates/opnrelay-server/tests/api.rs` (assert it), `apps/web/src/lib/api.ts`, `apps/web/src/routes/Home.svelte`
- Create: `apps/web/src/lib/GuideCard.svelte`, `apps/web/src/lib/GuideTable.svelte`

**Interfaces:**
- Server test `guide_summary_has_thumbnail`: guide with steps [heading, action(media A), action(media B)] → `thumb_media_id == A`; guide without media → null; list and get both include it.
- **All guides** (`#/`, no workspace): summary numbers row (Published, Drafts, Need review — the last only for editors) then `GuideTable`: columns Guide (title + description, ellipsis), Workspace, Status tag, Class., Steps (mono), Updated (relative, mono). Row click opens the guide.
- **Drafts** (`#/?status=draft`) and **Needs review** (`#/?needs_review=true`) reuse `GuideTable` with a heading.
- **Workspace** (`#/?ws=ID`): header with crumb, title, "N guides · M folders", **Access:** chips listing the workspace's members/groups with role (Microsoft groups get the Microsoft mark), buttons Workspace settings (managers) and + New guide (editors). Folder chips (All, each folder with count, Unfiled) filter; **Cards / List** switch remembered in localStorage (try/catch). Card view: a section per folder (in folder order, Unfiled last) with heading + count + rule; `GuideCard` grid (4 columns desktop, 2 tablet, 1 phone): thumbnail 92px (`/api/v1/media/{thumb}` cropped top, or striped "No screenshots yet"), status tag top-left, step count bottom-right, title (2 lines max), author avatar initials + name, updated or orange `NEEDS REVIEW · N mo` for editors; hover/focus orange outline. Editors get a dashed "+ New guide in {folder}" card ending each section (creates the guide in that folder). List view = `GuideTable`.
- Search results (`?q=`) use `GuideTable` with "N results for …".
- All existing empty-state copy retained in the new style (Review Focus 2); create workspace/folder flows unchanged but without `prompt()` (inline input).

- [ ] **Step 1:** Server: failing test, add `thumb_media_id`, pass; `cargo test -p opnrelay-server`, fmt, clippy.
- [ ] **Step 2:** Web: implement; `pnpm check && pnpm test && pnpm build`; `tools/ui-shots.mjs`; check Review Focus 1, 2, 3, 5.
- [ ] **Step 3:** Commit `feat: Runbook guide lists — table, workspace cards by folder`.

### Task 4: Guide reader — step index and numbered steps

**Files:**
- Modify: `apps/web/src/routes/GuideView.svelte`, `apps/web/src/lib/Screenshot.svelte`

**Interfaces:**
- Layout: step index column 260px (sticky; "STEPS · N", section headings bold, rows `01 Click "Employees"` mono numbers, current step highlighted with orange inset + soft fill, steps above it marked done in ok colour) + content. Below 1100px the index collapses into a "Steps" dropdown at the top.
- Header: crumb `workspace / folder`, title, buttons Export ▾ / Share (hidden until sharing exists — omit) / Duplicate / Delete / Edit as today by permission; meta row: status tag, classification tag, author · updated, "Last reviewed <date>", source ("Recorded in Chrome · host" when `source` and first action url/app exist).
- Steps: big orange mono number (`01`) with a 3px orange top rule, title with the target label in a soft-orange mono chip, screenshot (max 620px wide) with orange highlight box and a small orange label at the box (`CLICK`, or `TYPE · <typed text>` when typed text exists). Tips/alerts as `.callout.tip/.alert`, headings as section titles. Needs-review notice + Still accurate keep working.
- Current step tracked with IntersectionObserver; clicking an index row scrolls to the step; ↑/↓ keys (when not typing) move between steps.

- [ ] **Step 1:** Implement; `pnpm check && pnpm test && pnpm build`; `tools/ui-shots.mjs`; check Review Focus 3, 4, 5.
- [ ] **Step 2:** Commit `feat(web): Runbook guide reader with step index`.

### Task 5: Editor — one step at a time

**Files:**
- Modify: `apps/web/src/routes/GuideEdit.svelte` (split into components)
- Create: `apps/web/src/lib/editor/StepList.svelte`, `apps/web/src/lib/editor/StepCanvas.svelte`, `apps/web/src/lib/editor/StepProps.svelte`, `apps/web/src/lib/editor/autosave.ts`, `apps/web/src/lib/editor/autosave.test.ts`

**Interfaces:**
- Layout (mockup "Editing a guide"): top bar (← Done, guide title, status tag `DRAFT · editing vN+1` or `PUBLISHED vN`, save state text, History, Preview (opens reader), **Publish vN+1** orange). Three columns: **StepList** 250px (section headings, `01` mono numbers, 46×28 thumbnails, tips/alerts rows, selected row soft orange; reorder by drag-and-drop **and** Alt+↑/↓; ↑/↓ moves selection; add row "+ Step · Heading · Tip · Alert · Screenshots"; paste/upload screenshots still adds steps); **StepCanvas** (toolbar: Highlight (active), Replace image, Remove screenshot; the screenshot with the existing click-to-place highlight; note when auto-redaction ran if the guide came from a recorder — show only if data exists, otherwise omit); **StepProps** 270px (type segmented control Step/Heading/Text/Tip/Alert, Title, Details (markdown, as today), Recorded action read-only mono block from `action` (type, label, typed text, app/url) when present, Duplicate / Delete step with in-page confirm). Guide title/description/classification/folder move to a "Guide details" panel opened from the title in the top bar.
- Tools not built yet (arrow, box, text, blur, crop, split) are **not** shown (spec: following step).
- Autosave: `createAutosave(save: () => Promise<boolean>, delayMs = 1500)` in `autosave.ts` → `{ touch(), flush(): Promise<void>, state: 'saved'|'pending'|'saving'|'error' }`; debounce, never two saves at once, re-save if touched during a save, `flush()` before Publish/Done/route change. The existing `save()` (PATCH + PUT with If-Match) is reused; a 409/412 shows "Someone else changed this guide — reload to see their changes" and stops autosaving.
- `autosave.test.ts` (vitest, fake timers): debounces bursts into one save; no overlapping saves; touch during save triggers one more; flush saves immediately; error state after a failed save and recovers on next success.
- Versions panel (History) keeps restore with in-page confirm.

- [ ] **Step 1:** Write `autosave.test.ts`; run `pnpm test` — FAIL (module missing).
- [ ] **Step 2:** Implement `autosave.ts`; `pnpm test` — PASS.
- [ ] **Step 3:** Implement the components and wire them; `pnpm check && pnpm test && pnpm build`; `tools/ui-shots.mjs` (edit page); manually exercise in the browser via Playwright: add step, reorder, set highlight, change type, autosave indicator reaches "Saved", publish.
- [ ] **Step 4:** Commit `feat(web): Runbook editor — one step at a time with autosave`.

### Task 6: Admin, account and sign-in screens

**Files:**
- Modify: `apps/web/src/routes/Admin.svelte`, `GroupsTab.svelte`, `SsoSettings.svelte`, `BackupSettings.svelte`, `BrandingSettings.svelte`, `Audit.svelte`, `Account.svelte`, `Downloads.svelte`, `WorkspaceSettings.svelte`, `Login.svelte`, `Invite.svelte`, `Connect.svelte`

**Interfaces:**
- Admin becomes "People & groups" + "Settings" per the mockup: underline tabs (People, Groups, Microsoft sign-in) on `#/admin`, and (Security & defaults, Branding, Backups) on `#/admin?tab=policies|branding|backups` under the sidebar's Settings. Existing tab query values keep working.
- Tables use `table.data`; Microsoft source shown as the mono Microsoft chip; role pills mono (`ADMIN` in signal soft); sync status as the green/red/amber banner; the "Add from Microsoft" search as a popover panel.
- Login/Invite/Connect: centred card on navy background with the OpnRelay mark; Microsoft button styled as a secondary button with the Microsoft mark.
- Replace remaining `confirm()` calls in these files with in-page confirmations.
- Audit log: mono time/action columns, filter row, Verify/Export buttons.

- [ ] **Step 1:** Implement; `pnpm check && pnpm test && pnpm build`; `tools/ui-shots.mjs` for every admin tab, audit, account, downloads, login; check Review Focus 1, 5 and phone width.
- [ ] **Step 2:** Commit `feat(web): Runbook admin, account and sign-in screens`.

### Task 7: Recorders — extension popup and desktop recorder colours

**Files:**
- Modify: `apps/extension/static/popup.css` (+ `popup.html` if markup classes are needed), `apps/desktop/src/app.rs` (egui visuals)

**Interfaces:**
- Extension popup: Runbook tokens (light and dark via `prefers-color-scheme`), navy header strip with the mark, orange "● Start recording" button, mono step count while recording, system font stack with IBM Plex if present (no font bundling in the extension).
- Desktop recorder: set egui `Visuals` (light and dark following the OS) to the Runbook palette: navy panel header, orange primary button for Start recording, muted greys from the tokens; no new dependencies, no bundled fonts.
- Behaviour unchanged.

- [ ] **Step 1:** Implement; `cd apps/extension && pnpm check && pnpm test && pnpm build`; `cargo build -p opnrelay-recorder` and `cargo clippy -p opnrelay-recorder -- -D warnings` (Linux deps per `.github/workflows/ci.yml`).
- [ ] **Step 2:** Screenshot the popup states (stub the `chrome` API as in a test page) and the recorder's first screen under `xvfb-run` if available; look at them.
- [ ] **Step 3:** Commit `feat: Runbook colours for the extension popup and desktop recorder`.
