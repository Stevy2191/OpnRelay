// SPDX-License-Identifier: AGPL-3.0-only
//! OAuth 2.0 authorization code flow with PKCE for native clients (desktop
//! recorder, browser extension). Access tokens live 15 minutes; refresh
//! tokens rotate on every use and expire after 30 days without use.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::types::ipnetwork::IpNetwork;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};
use crate::auth::{ClientInfo, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const ACCESS_MINUTES: i32 = 15;
pub const REFRESH_DAYS: i32 = 30;
const CODE_SECONDS: i32 = 120;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth/authorize", post(authorize))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/me/devices", get(list_devices))
        .route("/me/devices/{id}", delete(delete_device))
}

/// Is `uri` an allowed redirect for `client_id`?
pub fn valid_redirect(client_id: &str, uri: &str) -> bool {
    match client_id {
        // Loopback redirect for the desktop app (RFC 8252 §7.3).
        "opnrelay-desktop" => {
            let rest = uri
                .strip_prefix("http://127.0.0.1:")
                .or_else(|| uri.strip_prefix("http://localhost:"));
            match rest.and_then(|r| r.split_once('/')) {
                Some((port, path)) => {
                    port.parse::<u16>().is_ok_and(|p| p >= 1024) && path == "callback"
                }
                None => false,
            }
        }
        // Browser identity API redirect URLs (Chrome/Edge and Firefox).
        "opnrelay-extension" => {
            let Some(rest) = uri.strip_prefix("https://") else {
                return false;
            };
            let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
            let ok_host = match host.split_once('.') {
                Some((id, "chromiumapp.org")) => {
                    id.len() == 32 && id.chars().all(|c| ('a'..='p').contains(&c))
                }
                Some((id, "extensions.allizom.org")) => {
                    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit())
                }
                _ => false,
            };
            ok_host
                && path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))
        }
        _ => false,
    }
}

fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(opnrelay_crypto::sha256(verifier.as_bytes()))
}

#[derive(Deserialize)]
struct AuthorizeReq {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    device_name: String,
}

/// Called by the web app's consent page with the user's session.
async fn authorize(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<AuthorizeReq>,
) -> ApiResult<Json<Value>> {
    if !valid_redirect(&req.client_id, &req.redirect_uri) {
        return Err(ApiError::bad("unknown client or redirect address"));
    }
    if req.code_challenge_method != "S256"
        || req.code_challenge.len() != 43
        || !req
            .code_challenge
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::bad(
            "a valid S256 PKCE code challenge is required",
        ));
    }
    if req.state.len() > 200 {
        return Err(ApiError::bad("state is too long"));
    }
    let device: String = req.device_name.trim().chars().take(100).collect();
    let code = opnrelay_crypto::random_token();
    sqlx::query(
        "INSERT INTO oauth_codes (code_hash, user_id, client_id, redirect_uri, code_challenge, device_name, expires_at)
         VALUES ($1,$2,$3,$4,$5,$6, now() + make_interval(secs => $7))",
    )
    .bind(opnrelay_crypto::hash_token(&code))
    .bind(a.user_id)
    .bind(&req.client_id)
    .bind(&req.redirect_uri)
    .bind(&req.code_challenge)
    .bind(&device)
    .bind(CODE_SECONDS as f64)
    .execute(&st.db)
    .await?;
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("oauth.authorized").meta(json!({"client_id": req.client_id, "device": device})),
    )
    .await?;
    let sep = if req.redirect_uri.contains('?') {
        '&'
    } else {
        '?'
    };
    let state = url_encode(&req.state);
    Ok(Json(json!({
        "redirect_to": format!("{}{sep}code={code}&state={state}", req.redirect_uri)
    })))
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(tag = "grant_type", rename_all = "snake_case")]
enum TokenReq {
    AuthorizationCode {
        code: String,
        code_verifier: String,
        redirect_uri: String,
        client_id: String,
    },
    RefreshToken {
        refresh_token: String,
    },
}

#[derive(Serialize)]
struct TokenResp {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    refresh_token: String,
}

#[derive(sqlx::FromRow)]
struct CodeRow {
    user_id: Uuid,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    device_name: String,
    email: String,
}

fn new_tokens() -> (String, String) {
    (
        opnrelay_crypto::random_token(),
        opnrelay_crypto::random_token(),
    )
}

fn oauth_error(msg: &str) -> ApiError {
    ApiError::BadRequest(format!("invalid_grant: {msg}"))
}

async fn token(
    State(st): State<AppState>,
    client: ClientInfo,
    Json(req): Json<TokenReq>,
) -> ApiResult<Json<TokenResp>> {
    if !st.limiter.check(
        &format!("oauth-token:{:?}", client.ip),
        120,
        std::time::Duration::from_secs(600),
    ) {
        return Err(ApiError::TooManyRequests);
    }
    let ip = client.ip.map(IpNetwork::from);
    match req {
        TokenReq::AuthorizationCode {
            code,
            code_verifier,
            redirect_uri,
            client_id,
        } => {
            let mut tx = st.db.begin().await?;
            let row: Option<CodeRow> = sqlx::query_as(
                "UPDATE oauth_codes c SET used_at = now()
                 FROM users u
                 WHERE c.code_hash = $1 AND c.used_at IS NULL AND c.expires_at > now()
                   AND u.id = c.user_id AND u.status = 'active' AND u.deleted_at IS NULL
                 RETURNING c.user_id, c.client_id, c.redirect_uri, c.code_challenge, c.device_name, u.email::text AS email",
            )
            .bind(opnrelay_crypto::hash_token(&code))
            .fetch_optional(&mut *tx)
            .await?;
            let row = row.ok_or_else(|| oauth_error("the code is invalid or expired"))?;
            let mismatch = if row.client_id != client_id || row.redirect_uri != redirect_uri {
                Some("client or redirect does not match")
            } else if !opnrelay_crypto::ct_eq(
                pkce_s256(&code_verifier).as_bytes(),
                row.code_challenge.as_bytes(),
            ) {
                Some("PKCE verification failed")
            } else {
                None
            };
            if let Some(msg) = mismatch {
                // Keep the code burned: a failed attempt must not be retryable.
                tx.commit().await?;
                return Err(oauth_error(msg));
            }
            let (access, refresh) = new_tokens();
            let grant = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO device_grants (id, user_id, client_id, device_name, access_hash, access_expires_at,
                    refresh_hash, refresh_expires_at, ip)
                 VALUES ($1,$2,$3,$4,$5, now() + make_interval(mins => $6), $7, now() + make_interval(days => $8), $9)",
            )
            .bind(grant)
            .bind(row.user_id)
            .bind(&row.client_id)
            .bind(&row.device_name)
            .bind(opnrelay_crypto::hash_token(&access))
            .bind(ACCESS_MINUTES)
            .bind(opnrelay_crypto::hash_token(&refresh))
            .bind(REFRESH_DAYS)
            .bind(ip)
            .execute(&mut *tx)
            .await?;
            audit::record(
                &mut tx,
                &AuditActor {
                    id: Some(row.user_id),
                    label: row.email,
                    ip: client.ip,
                },
                Event::new("oauth.device.connected")
                    .target("device", grant)
                    .meta(json!({"client_id": row.client_id, "device": row.device_name})),
            )
            .await?;
            tx.commit().await?;
            Ok(Json(TokenResp {
                access_token: access,
                token_type: "Bearer",
                expires_in: ACCESS_MINUTES as i64 * 60,
                refresh_token: refresh,
            }))
        }
        TokenReq::RefreshToken { refresh_token } => {
            let h = opnrelay_crypto::hash_token(&refresh_token);
            let mut tx = st.db.begin().await?;
            // Reuse of an old refresh token: revoke the whole grant.
            let reused: Option<Uuid> = sqlx::query_scalar(
                "SELECT grant_id FROM device_refresh_history WHERE refresh_hash = $1",
            )
            .bind(&h)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(grant) = reused {
                sqlx::query("UPDATE device_grants SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
                    .bind(grant)
                    .execute(&mut *tx)
                    .await?;
                audit::record(
                    &mut tx,
                    &AuditActor::anonymous(client.ip),
                    Event::new("oauth.refresh_reuse_detected").target("device", grant),
                )
                .await?;
                tx.commit().await?;
                return Err(oauth_error("refresh token was already used; sign in again"));
            }
            let grant: Option<(Uuid, Uuid)> = sqlx::query_as(
                "SELECT g.id, g.user_id FROM device_grants g JOIN users u ON u.id = g.user_id
                 WHERE g.refresh_hash = $1 AND g.revoked_at IS NULL AND g.refresh_expires_at > now()
                   AND u.status = 'active' AND u.deleted_at IS NULL
                 FOR UPDATE OF g",
            )
            .bind(&h)
            .fetch_optional(&mut *tx)
            .await?;
            let (grant, _user) =
                grant.ok_or_else(|| oauth_error("the refresh token is invalid or expired"))?;
            let (access, refresh) = new_tokens();
            sqlx::query(
                "INSERT INTO device_refresh_history (refresh_hash, grant_id) VALUES ($1, $2)",
            )
            .bind(&h)
            .bind(grant)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE device_grants SET access_hash = $2, access_expires_at = now() + make_interval(mins => $3),
                    refresh_hash = $4, refresh_expires_at = now() + make_interval(days => $5),
                    last_used_at = now(), ip = COALESCE($6, ip)
                 WHERE id = $1",
            )
            .bind(grant)
            .bind(opnrelay_crypto::hash_token(&access))
            .bind(ACCESS_MINUTES)
            .bind(opnrelay_crypto::hash_token(&refresh))
            .bind(REFRESH_DAYS)
            .bind(ip)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(Json(TokenResp {
                access_token: access,
                token_type: "Bearer",
                expires_in: ACCESS_MINUTES as i64 * 60,
                refresh_token: refresh,
            }))
        }
    }
}

#[derive(Deserialize)]
struct RevokeReq {
    token: String,
}

/// RFC 7009 style: always 200, whether or not the token existed.
async fn revoke(State(st): State<AppState>, Json(req): Json<RevokeReq>) -> ApiResult<StatusCode> {
    let h = opnrelay_crypto::hash_token(&req.token);
    sqlx::query(
        "UPDATE device_grants SET revoked_at = now()
         WHERE (access_hash = $1 OR refresh_hash = $1) AND revoked_at IS NULL",
    )
    .bind(h)
    .execute(&st.db)
    .await?;
    Ok(StatusCode::OK)
}

#[derive(Serialize, sqlx::FromRow)]
struct DeviceRow {
    id: Uuid,
    client_id: String,
    device_name: String,
    #[serde(serialize_with = "crate::util::ser_ip")]
    ip: Option<IpNetwork>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    last_used_at: OffsetDateTime,
}

async fn list_devices(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Vec<DeviceRow>>> {
    let rows = sqlx::query_as(
        "SELECT id, client_id, device_name, ip, created_at, last_used_at FROM device_grants
         WHERE user_id = $1 AND revoked_at IS NULL AND refresh_expires_at > now() ORDER BY last_used_at DESC",
    )
    .bind(a.user_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

async fn delete_device(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let r = sqlx::query("UPDATE device_grants SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL")
        .bind(id)
        .bind(a.user_id)
        .execute(&st.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("oauth.device.revoked").target("device", id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_rules() {
        assert!(valid_redirect(
            "opnrelay-desktop",
            "http://127.0.0.1:53682/callback"
        ));
        assert!(valid_redirect(
            "opnrelay-desktop",
            "http://localhost:4000/callback"
        ));
        assert!(!valid_redirect(
            "opnrelay-desktop",
            "http://127.0.0.1:80/callback"
        ));
        assert!(!valid_redirect(
            "opnrelay-desktop",
            "http://127.0.0.1:5000/evil"
        ));
        assert!(!valid_redirect(
            "opnrelay-desktop",
            "http://127.0.0.1.evil.com:5000/callback"
        ));
        assert!(!valid_redirect(
            "opnrelay-desktop",
            "https://evil.com/callback"
        ));
        assert!(valid_redirect(
            "opnrelay-extension",
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/"
        ));
        assert!(valid_redirect(
            "opnrelay-extension",
            "https://0123abcdef.extensions.allizom.org/"
        ));
        assert!(!valid_redirect(
            "opnrelay-extension",
            "https://evil.com.chromiumapp.org/"
        ));
        assert!(!valid_redirect(
            "opnrelay-extension",
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org.evil.com/"
        ));
        assert!(!valid_redirect(
            "opnrelay-extension",
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/?x=<"
        ));
        assert!(!valid_redirect("other", "http://127.0.0.1:5000/callback"));
    }

    #[test]
    fn pkce_matches_rfc7636_example() {
        assert_eq!(
            pkce_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
