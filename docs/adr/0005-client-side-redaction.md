# ADR 0005: Redaction happens on the client before upload

**Status:** Accepted

## Context
HR and EMS screenshots may contain PII/PHI. If redaction only happens on the server, unredacted data is transmitted and stored, even briefly.

## Decision
Automatic redaction runs in the desktop app / extension **before** a screenshot is written to disk or uploaded. In the extension, sensitive DOM elements are masked *before* the screenshot is taken. Redaction is destructive by default. Manual redaction in the editor replaces the stored image.

## Consequences
- Server, backups and admins never see the redacted data.
- Redaction logic (`opnrelay-redact`) must be shared and consistent across clients.
- Users cannot un-redact a mistakenly redacted area unless policy keeps originals (off by default; never in sensitive workspaces).
- Auto-redaction is best-effort, so users must review before publishing. The editor highlights auto-redactions and offers a "review sensitive content" checklist when publishing.
