# API

REST + JSON under `/api/v1`. The OpenAPI 3.1 spec is generated from code (`utoipa`) and
served at `/api/v1/openapi.json` (authenticated). Errors use RFC 9457 Problem Details.

## 1. Authentication

| Client | Mechanism |
|---|---|
| Web app | Session cookie (`__Host-opnrelay_session`) + CSRF header `X-CSRF-Token` |
| Desktop app | OAuth 2.0 authorisation code + PKCE via system browser → loopback redirect (`http://127.0.0.1:{port}/callback`); bearer access token (15 min) + rotating refresh token |
| Browser extension | Same as desktop, redirect via `identity.launchWebAuthFlow` |
| Automation (later) | Personal access tokens with scopes, admin-controllable |

### OAuth endpoints
- `GET /oauth/authorize`: standard parameters; `client_id` is `opnrelay-desktop` or `opnrelay-extension` (public clients, PKCE required)
- `POST /oauth/token`: `authorization_code` and `refresh_token` grants
- `POST /oauth/revoke`

## 2. Conventions
- IDs are UUIDv7 strings.
- Pagination is cursor-based: `?limit=50&cursor=…`, returning `next_cursor`.
- Optimistic concurrency on guides: `If-Match: "<version>"`, returning `412` on conflict.
- Rate limits per IP and per user; `429` with `Retry-After`.
- Every mutating endpoint emits an audit event.

## 3. Endpoints

### Auth & account
| Method | Path | Notes |
|---|---|---|
| POST | `/auth/login` | email + password → `mfa_required` challenge or session |
| POST | `/auth/mfa/verify` | TOTP / WebAuthn assertion / recovery code |
| POST | `/auth/logout` | |
| GET | `/auth/sso/{idp_id}/start` | OIDC/SAML redirect |
| GET/POST | `/auth/sso/{idp_id}/callback` | OIDC callback / SAML ACS |
| GET | `/auth/sso/{idp_id}/metadata` | SAML SP metadata |
| POST | `/auth/password/forgot`, `/auth/password/reset` | |
| POST | `/invitations/{token}/accept` | |
| GET | `/me` | profile, roles, effective policies |
| GET/POST/DELETE | `/me/mfa/...` | enrol/remove factors, regenerate recovery codes |
| GET/DELETE | `/me/sessions`, `/me/devices` | list/revoke |

### Workspaces, folders
| Method | Path |
|---|---|
| GET/POST | `/workspaces` |
| GET/PATCH/DELETE | `/workspaces/{id}` |
| GET/PUT/DELETE | `/workspaces/{id}/members` |
| GET/POST | `/workspaces/{id}/folders` |
| PATCH/DELETE | `/folders/{id}` |

### Guides & steps
| Method | Path | Notes |
|---|---|---|
| GET | `/guides?workspace=&folder=&q=&status=&tag=&needs_review=true` | search + filter. `needs_review=true` returns only published guides past their workspace's review period, in workspaces where the caller can edit or manage (admins: all; viewers and auditors: none), oldest due first |
| POST | `/guides` | empty or from template |
| GET | `/guides/{id}` | guide + steps |
| PATCH | `/guides/{id}` | metadata |
| PUT | `/guides/{id}/steps` | full ordered step list (editor save), `If-Match` |
| POST | `/guides/{id}/publish` | snapshot version; also restarts the review clock |
| POST | `/guides/{id}/reviewed` | "Still accurate": sets `reviewed_at = now()` on a published guide without a new version or revision bump; editor; audit `guide.reviewed` |
| GET | `/guides/{id}/versions`, `/guides/{id}/versions/{v}` | |
| POST | `/guides/{id}/versions/{v}/restore` | |
| POST | `/guides/{id}/duplicate`, `/guides/{id}/move` | |
| DELETE | `/guides/{id}` | soft delete; blocked by legal hold |

### Capture upload (desktop & extension)
| Method | Path | Notes |
|---|---|---|
| POST | `/captures` | `{workspace_id, folder_id?, guide_id? (append), mode, client}` → `capture_id`, upload limits, effective redaction policy |
| PUT | `/captures/{id}/media/{client_media_id}` | chunked (`Content-Range`), SHA-256 in header; resumable |
| POST | `/captures/{id}/complete` | steps JSON referencing `client_media_id`s → `guide_id` |
| DELETE | `/captures/{id}` | abort |

### Media
| Method | Path | Notes |
|---|---|---|
| POST | `/media` | manual image upload (editor) |
| GET | `/media/{id}?sig=&exp=` | signed, short-lived URL; decrypted stream |
| POST | `/media/{id}/redact` | apply manual redaction regions (creates new media, discards old per policy) |

### Sharing
| Method | Path | Notes |
|---|---|---|
| GET/POST | `/guides/{id}/share-links` | create `{scope, expires_at, password?, allow_download}` |
| DELETE | `/share-links/{id}` | revoke |
| GET | `/s/{token}` | public viewer page (HTML, not under `/api`) |
| GET | `/embed/{token}` | iframe view; `frame-ancestors` from embed origins |

### Exports
| Method | Path | Notes |
|---|---|---|
| GET | `/guides/{id}/export?format=pdf\|docx\|html\|md\|png[&images=false]` | Implemented. Streams the file as an attachment; requires read access; audit-logged |
| POST | `/exports/bulk` | *Planned:* `{guide_ids \| folder_id \| workspace_id, format}` → zip |

### Branding
| Method | Path | Notes |
|---|---|---|
| GET | `/branding` | Any signed-in user (the UI shows the logo and colours) |
| PUT | `/admin/branding` | Admin. Colours, header/footer templates, cover page, numbering, watermark |
| PUT / DELETE | `/admin/branding/logo` | Admin. Raw PNG/JPEG body (≤ 2 MB), validated and re-encoded |

### Administration
| Method | Path |
|---|---|
| GET/POST/PATCH/DELETE | `/admin/users`, `/admin/users/{id}` (incl. disable, reset MFA, revoke sessions) |
| GET/POST | `/admin/invitations` |
| GET/POST/PATCH/DELETE | `/admin/groups` |
| GET/POST/PATCH/DELETE | `/admin/identity-providers` |
| GET/PUT | `/admin/policies` |
| GET/PUT | `/admin/embed-origins` |
| GET | `/admin/system` (version, storage usage, job queue health) |
| POST | `/admin/users/{id}/export-data`, `/admin/users/{id}/erase` |

### Audit (Admin, Auditor)
| Method | Path | Notes |
|---|---|---|
| GET | `/audit/events?actor=&action=&target=&from=&to=` | |
| POST | `/audit/exports` | CSV/JSON with chain proof |
| GET | `/audit/verify?from_seq=&to_seq=` | chain verification result |

### Health
- `GET /healthz` (liveness), `GET /readyz` (DB + storage reachable). Unauthenticated, no details.

## 4. Capture payload (excerpt)

```json
{
  "client": { "kind": "desktop", "version": "0.1.0", "os": "windows-11" },
  "mode": "region",
  "steps": [
    {
      "client_step_id": "s1",
      "kind": "action",
      "action": {
        "type": "click",
        "target_label": "Save",
        "target_role": "button",
        "app": "HRIS",
        "window_title": "Employee Profile",
        "url": null,
        "x": 812, "y": 604,
        "ts": "2026-11-21T14:03:11.201Z"
      },
      "title": "Click \"Save\"",
      "media": "m1",
      "annotations": { "highlight": { "x": 760, "y": 585, "w": 104, "h": 38 } },
      "redactions_applied": ["password_field", "pattern:email"]
    }
  ]
}
```
