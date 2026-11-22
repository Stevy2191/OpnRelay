# Editor tools: annotations, crop, redaction, merge/split — design

Status: draft for approval · 2026-11-22

## Why

The Runbook editor shipped with only "move highlight" and "replace image". HR and IT
authors need to point at things (arrows, boxes, text), trim screenshots (crop), hide
sensitive data (blur), and fix step structure (merge/split). Product requirements
FR-G4 and FR-R2, roadmap M5.

## Decided with the user

- **Redaction is editable, but only by the guide's author.** "Author" = the user who
  created the guide (`guides.created_by`), plus organisation **owners** as break-glass.
  Everyone else — editors, managers, admins, auditors, readers, exports, future public
  links — only ever receives the redacted image.
- Other editors **can add** redactions; they **cannot move or remove** existing ones.
- Split = "Split step" duplicates the step right after it (same screenshot, same
  annotations, empty details). Merge = "Merge with next" combines two steps' text and
  keeps the first step's screenshot.

## Goals

1. Arrow, box and text annotations on a step's screenshot: add, move, resize, recolour
   (orange default, plus navy/red/green), delete. Shown in the reader and in every export.
2. Crop: non-destructive; the reader and exports show the cropped area; the highlight
   and annotations stay aligned.
3. Redaction regions with three styles (blur, pixelate, solid box). Non-authors can never
   obtain the unredacted pixels through any route.
4. Merge with next / Split step in the editor.

## Non-goals

- Freehand drawing, shapes beyond arrow/box/text, numbered callout badges.
- Automatic redaction changes (already exists for recorders).
- Public share links (not built yet; they will use the redacted image by construction).

## Anti-goals

- An unredacted original reachable by a non-author through any route: media URL,
  version history, duplicate, export, search, backup download (admin backups are
  encrypted and admin-only — accepted, see below).
- Annotations that drift away from what they point at after crop or image replacement.

## Approaches considered (redaction)

| | How | Trade-off |
|---|---|---|
| Permanent (burn in) | Redact pixels, delete original | Safest, but not editable — rejected by the user |
| **Editable, author-only (chosen)** | Keep original as restricted media; server renders a redacted derivative used everywhere else | Original exists on the server; access is enforced at the media endpoint |
| Overlay only | Blur drawn by the browser over the original | Original visible to anyone with the link — rejected (anti-goal) |

## Design

### Data

`annotations` (existing JSON per step) gains:
- `arrows: [{ from: {x,y}, to: {x,y}, color }]`
- `texts: [{ x, y, text (≤200 chars), color, size: "s"|"m"|"l" }]`
- `boxes` (existing `Rect`s) gain optional `color`
- `crop` (existing) unchanged
- `redactions: [{ id, x, y, w, h, style: "blur"|"pixelate"|"box", added_by }]`

All coordinates are in original-image pixels (as `highlight` today).

New columns:
- `steps.display_media_id uuid NULL` — the rendered redacted image (NULL when no
  redactions; then `media_id` is shown).
- `media.restricted boolean NOT NULL DEFAULT false` — set on any media that is the
  original behind a redaction.

### Rendering redactions (server)

On `PUT /guides/{id}/steps`, for each step whose `redactions` changed: decrypt the
original, apply regions (Gaussian blur radius ≥ 12px at the region's scale, pixelate
with block ≥ 12px, or solid fill), re-encode, store as new media (`kind = 'redacted'`,
same guide), set `display_media_id`, mark the original `restricted = true`. Old derived
media are deleted when replaced. Pixelate/blur strength must make text unreadable:
a test renders text and checks it no longer matches.

### Who gets what

- `media` fetch: a `restricted` media is served only to the guide's author or an owner;
  everyone else gets 404.
- Guide/step JSON: for non-authors, `media_id` is replaced by `display_media_id` (when
  set) and `redactions` is omitted except `count` (so the editor can show "3 hidden
  areas"). Authors get both ids and the full list.
- Version snapshots store both ids; `GET /versions/{v}` applies the same substitution.
  Restore keeps original + redactions and re-renders.
- **Duplicate** (by anyone): each redacted step's *display* image becomes the copy's
  `media_id` (redaction baked in, no redactions list) — the duplicator never gains the
  original.
- **Exports** always use `display_media_id ?? media_id`, then draw crop/highlight/boxes/
  arrows/text.
- Thumbnails (`thumb_media_id`) use the display image.
- Non-author edits: `PUT steps` from a non-author must keep every existing redaction
  unchanged (same id, same rect, same style); new ones may be added; violation → 403
  "Only the guide's author can change hidden areas". Replacing a step's screenshot by a
  non-author is allowed (the new image has no redactions; the old original stays
  restricted and is unlinked).
- Backups contain originals (encrypted, admin-only download) — accepted.
- Audit: `guide.redaction.added` / `.removed` with step id and count.

### Editor

Canvas toolbar: Select · Highlight · Arrow · Box · Text · Hide (blur/pixelate/box picker)
· Crop · Replace image. Selected object: drag to move, handles to resize, Delete key,
colour swatches in the right panel. Crop shows handles and Apply/Reset. Non-authors see
existing hidden areas as hatched, locked shapes ("Added by the guide's author"). Undo for
the current step's last change (one level per action, in-memory) — small but expected.
Step actions: Duplicate · Split step · Merge with next · Move up/down · Delete.

### Reader and exports

Reader draws crop (CSS viewport over the image), highlight, boxes, arrows (SVG), text
labels. Exports (`opnrelay-export/annotate.rs`) add arrows (thick line + head) and text
(bundled Inter via `ab_glyph`), and use the display image.

## Testing

- Server: non-author gets 404 for the original and the redacted id in JSON; author gets
  both; duplicate by a non-author never references the original; versions substitute;
  non-author cannot remove/move redactions (403) but can add; owner can; rendering makes
  text unreadable; exports use the display image.
- Export crate: arrows and text drawn at the right place after crop.
- Web: unit tests for coordinate maths (crop mapping, hit-testing); Playwright: draw each
  tool, crop, hide area, save/reload, reader shows them, non-author view.

## Left to the builder

Exact blur radius/pixel size (subject to the unreadability test), handle sizes, colours
beyond the four listed, undo depth beyond one level.
