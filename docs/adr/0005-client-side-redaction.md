# ADR 0005: Redaction happens on the client before upload

**Status:** Accepted

## Context
Users decide what appears in their guides, and redaction is optional. When a user *does* choose redaction, it is only meaningful if the unredacted pixels never reach the server.

## Decision
When enabled by the user, automatic redaction runs in the desktop app / extension **before** a screenshot is written to disk or uploaded. In the extension, sensitive DOM elements are masked *before* the screenshot is taken. Redaction is destructive once saved. Manual redaction in the editor replaces the stored image.

## Consequences
- Server, backups and admins never see the redacted data.
- Redaction logic (`opnrelay-redact`) must be shared and consistent across clients.
- Users cannot un-redact an area after saving (undo works while editing).
- Redaction is never forced on any workspace or department.
- Auto-redaction is best-effort, so users must review before publishing. The editor highlights auto-redactions and offers a "review sensitive content" checklist when publishing.
