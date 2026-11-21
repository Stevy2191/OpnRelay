# "Runbook" web redesign: design notes

Status: approved · 2026-11-21

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

- **Guide editor** (mockup approved): one step at a time. Step list on
  the left (thumbnails, headings, tips, drag to reorder), selected step's
  screenshot in the middle with tools (highlight, arrow, box, text, blur, crop,
  replace), step type/title/details and the recorded action on the right.
  Autosave; Publish creates a new version and restarts the review clock.
- **Admin → People & groups → Groups** (mockup approved): sync status
  banner with Sync now; table of groups with source (Microsoft / OpnRelay),
  members, workspaces it opens, role it grants, last synced; "Add from Microsoft"
  search dialog; groups deleted in Microsoft shown in red with Unlink.

## Calls made (not explicitly decided; easy to change)

- **Dark mode**: kept (the app follows the system setting today). The builder
  derives a Runbook dark palette: the navy sidebar stays, the paper and cards go
  dark navy-grey, orange stays the accent; contrast checked to WCAG AA.
- **Recorder and extension popup**: restyled in a later step, after the web app,
  using the same colours and fonts where the toolkit allows.
- **Exports** (PDF, Word, HTML): unchanged for now; they keep using the company
  branding settings.
- **Editor tools not built yet** (arrows, boxes, text, blur, crop, split): the
  editor layout ships first with the tools that exist today (highlight, replace
  image); the new tools are a following step (roadmap M5).
- **Order**: Entra group sync (with leavers) and the empty-state fix first, then
  the "Needs review" flag, then the Runbook web redesign.
