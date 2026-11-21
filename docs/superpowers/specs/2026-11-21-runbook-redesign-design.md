# "Runbook" web redesign: design notes

Status: in progress (decisions so far) · 2026-11-21

Mockups: published review page "OpnRelay Design Directions" (Runbook section and
"Inside a workspace").

## Why

The current web app is clean but generic; it could be any admin tool and reads
like a copy of other guide-capture products. The new look should be distinctive.
Main authors are HR and IT staff writing how-tos; readers are the rest of the
organisation.

## Decided

- **Direction A, "Runbook"**: an operations-manual feel. Dark navy sidebar
  (`#0f1a2b`), signal orange accent (`#ff6b1a`), light paper background; IBM Plex
  Sans for text and IBM Plex Mono for step numbers, versions, IDs and status tags.
  Chosen over "Relay" (teal/coral, route metaphor) and "Split view" (ink/amber,
  fixed screenshot stage).
- **App shell**: left sidebar with Library (All guides, Drafts, Recently viewed),
  Workspaces with counts, Admin links, and an always-visible "Record a guide"
  button. Slim top bar with search (`/` shortcut) and the user.
- **All guides**: dense table with summary numbers (published, drafts awaiting
  review, views, need updating).
- **Guide reader**: step index column beside the guide (section headings, step
  numbers, current step highlighted), large orange mono step numbers, click target
  outlined on the screenshot with a small label (e.g. "TYPE · Jordan Reyes"),
  tips as orange callouts.
- **Inside a workspace**: guides as **cards**, grouped into **sections by folder**
  (folder order set by workspace managers; "Unfiled" last). Folder chips with
  counts filter to one folder. Card: first screenshot as thumbnail, status tag
  with version, step count, title, author, last updated; orange outline on hover.
  "Cards / List" switch. Header shows who has access (linked Microsoft groups and
  their role). Editors get a dashed "Record into <folder>" card at the end of each
  section.
- **"Needs review" flag** (new feature):
  - Each published guide has a review date, starting at publish. After the
    workspace's review period it is flagged "Needs review".
  - Review period is a **workspace setting, default 90 days**; can be changed per
    workspace or turned off.
  - Editors/managers see an orange flag on the card, a "Needs review" filter and
    the count on the home page. Readers see a quiet "Last reviewed <date>" line.
  - Editors clear it with **"Still accurate"** (restarts the clock, no new
    version) or by publishing a new version. Both are audit-logged (who, when).

- **Guide editor** (mockup, awaiting feedback): one step at a time. Step list on
  the left (thumbnails, headings, tips, drag to reorder), selected step's
  screenshot in the middle with tools (highlight, arrow, box, text, blur, crop,
  replace), step type/title/details and the recorded action on the right.
  Autosave; Publish creates a new version and restarts the review clock.
- **Admin → People & groups → Groups** (mockup, awaiting feedback): sync status
  banner with Sync now; table of groups with source (Microsoft / OpnRelay),
  members, workspaces it opens, role it grants, last synced; "Add from Microsoft"
  search dialog; groups deleted in Microsoft shown in red with Unlink.

## Still to settle

- Dark mode palette for Runbook.
- Recorder (desktop) and browser-extension popup restyle.
- Whether exports adopt the Runbook look by default (branding colours still apply).
