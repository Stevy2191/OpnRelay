// SPDX-License-Identifier: AGPL-3.0-only
//! Login, MFA challenge, logout and invitation / reset acceptance.

use std::time::Duration as StdDuration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use opnrelay_crypto::password;
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};
use crate::auth::{self, ClientInfo, CurrentUserLax, PendingMfa};
use crate::error::{ApiError, ApiResult};
use crate::mfa;
use crate::state::AppState;

const MAX_FAILED_LOGINS: i32 = 10;
const LOCKOUT_MINUTES: i64 = 15;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/mfa/verify", post(mfa_verify))
        .route("/logout", post(logout))
        .route("/invite/inspect", post(invite_inspect))
        .route("/invite/accept", post(invite_accept))
}

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

#[derive(sqlx::FromRow)]
struct LoginUser {
    id: Uuid,
    org_role: String,
    sso_only: bool,
    email: String,
    password_hash: Option<String>,
    status: String,
    locked_until: Option<OffsetDateTime>,
    mfa_enabled: bool,
}

fn with_cookie(cookie: axum::http::HeaderValue, body: serde_json::Value) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, cookie);
    (headers, Json(body)).into_response()
}

async fn login(
    State(st): State<AppState>,
    client: ClientInfo,
    Json(req): Json<LoginReq>,
) -> ApiResult<Response> {
    let email = req.email.trim().to_lowercase();
    let ip_key = format!("login-ip:{:?}", client.ip);
    let acct_key = format!("login-acct:{email}:{:?}", client.ip);
    let window = StdDuration::from_secs(15 * 60);
    if !st.limiter.check(&ip_key, 100, window) || !st.limiter.check(&acct_key, 10, window) {
        return Err(ApiError::TooManyRequests);
    }

    let user: Option<LoginUser> = sqlx::query_as(
        "SELECT u.id, u.org_role, p.sso_only, u.email::text AS email, u.password_hash, u.status, u.locked_until,
                EXISTS (SELECT 1 FROM mfa_factors f WHERE f.user_id = u.id AND f.confirmed_at IS NOT NULL) AS mfa_enabled
         FROM users u JOIN org_policies p ON p.org_id = u.org_id WHERE u.email = $1 AND u.deleted_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&st.db)
    .await?;

    let hash = user
        .as_ref()
        .and_then(|u| u.password_hash.clone())
        .unwrap_or_else(|| password::dummy_hash().to_string());
    let pw = req.password.clone();
    let ok = tokio::task::spawn_blocking(move || password::verify(&pw, &hash))
        .await
        .map_err(anyhow::Error::from)?;

    let anon = AuditActor {
        id: user.as_ref().map(|u| u.id),
        label: email.clone(),
        ip: client.ip,
    };
    let Some(user) = user else {
        audit::record_now(
            &st.db,
            &anon,
            Event::new("auth.login.failed").meta(json!({"reason": "unknown_user"})),
        )
        .await?;
        return Err(ApiError::Unauthorized);
    };
    let locked = user
        .locked_until
        .is_some_and(|t| t > OffsetDateTime::now_utc());
    if !ok || user.status != "active" || user.password_hash.is_none() || locked {
        let reason = if locked {
            "locked"
        } else if user.status != "active" {
            "inactive"
        } else {
            "bad_password"
        };
        if !ok {
            sqlx::query(
                "UPDATE users SET failed_logins = failed_logins + 1,
                    locked_until = CASE WHEN failed_logins + 1 >= $2 THEN now() + make_interval(mins => $3) ELSE locked_until END
                 WHERE id = $1",
            )
            .bind(user.id)
            .bind(MAX_FAILED_LOGINS)
            .bind(LOCKOUT_MINUTES as i32)
            .execute(&st.db)
            .await?;
        }
        audit::record_now(
            &st.db,
            &anon,
            Event::new("auth.login.failed")
                .target("user", user.id)
                .meta(json!({"reason": reason})),
        )
        .await?;
        return Err(ApiError::Unauthorized);
    }

    // Microsoft-only mode: passwords work only for owners (break-glass access).
    if user.sso_only && user.org_role != "owner" {
        audit::record_now(
            &st.db,
            &anon,
            Event::new("auth.login.failed")
                .target("user", user.id)
                .meta(json!({"reason": "sso_only"})),
        )
        .await?;
        return Err(ApiError::Forbidden(
            "sign in with Microsoft: password sign-in is turned off for your organisation",
        ));
    }
    sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1")
        .bind(user.id)
        .execute(&st.db)
        .await?;
    st.limiter.reset(&acct_key);

    let (token, lifetime) = auth::create_session(
        &st,
        user.id,
        user.mfa_enabled,
        client.ip,
        client.user_agent.as_deref(),
    )
    .await?;
    if user.mfa_enabled {
        audit::record_now(
            &st.db,
            &anon,
            Event::new("auth.login.password_ok").target("user", user.id),
        )
        .await?;
    } else {
        sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1")
            .bind(user.id)
            .execute(&st.db)
            .await?;
        audit::record_now(
            &st.db,
            &anon,
            Event::new("auth.login").target("user", user.id),
        )
        .await?;
    }
    let _ = user.email;
    Ok(with_cookie(
        auth::session_cookie(&st, &token, lifetime),
        json!({ "mfa_required": user.mfa_enabled }),
    ))
}

#[derive(Deserialize)]
struct MfaReq {
    code: String,
}

#[derive(sqlx::FromRow)]
struct FactorRow {
    id: Uuid,
    secret_enc: Option<Vec<u8>>,
    last_used_step: Option<i64>,
}

/// Verify a TOTP or recovery code for a user. Returns the method used.
pub async fn check_second_factor(
    st: &AppState,
    user_id: Uuid,
    email: &str,
    code: &str,
) -> ApiResult<Option<&'static str>> {
    let factors: Vec<FactorRow> = sqlx::query_as(
        "SELECT id, secret_enc, last_used_step FROM mfa_factors
         WHERE user_id = $1 AND kind = 'totp' AND confirmed_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_all(&st.db)
    .await?;
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    for f in factors {
        let Some(enc) = f.secret_enc else { continue };
        let secret = opnrelay_crypto::open(&st.secrets_key, &enc, f.id.as_bytes())
            .map_err(anyhow::Error::from)?;
        let t = mfa::totp(secret, email)?;
        if let Some(step) = mfa::verify(&t, code, now, f.last_used_step) {
            // Conditional update guards against concurrent replay.
            let updated = sqlx::query(
                "UPDATE mfa_factors SET last_used_step = $2, last_used_at = now()
                 WHERE id = $1 AND (last_used_step IS NULL OR last_used_step < $2)",
            )
            .bind(f.id)
            .bind(step)
            .execute(&st.db)
            .await?;
            if updated.rows_affected() == 1 {
                return Ok(Some("totp"));
            }
        }
    }
    let used = sqlx::query(
        "UPDATE recovery_codes SET used_at = now() WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
    )
    .bind(user_id)
    .bind(mfa::hash_recovery(code))
    .execute(&st.db)
    .await?;
    if used.rows_affected() == 1 {
        return Ok(Some("recovery_code"));
    }
    Ok(None)
}

async fn mfa_verify(
    State(st): State<AppState>,
    pending: PendingMfa,
    Json(req): Json<MfaReq>,
) -> ApiResult<Response> {
    let key = format!("mfa:{}", pending.session_id);
    let actor = AuditActor {
        id: Some(pending.user_id),
        label: pending.email.clone(),
        ip: pending.ip,
    };
    if !st.limiter.check(&key, 5, StdDuration::from_secs(600)) {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(pending.session_id)
            .execute(&st.db)
            .await?;
        audit::record_now(
            &st.db,
            &actor,
            Event::new("auth.mfa.locked").target("user", pending.user_id),
        )
        .await?;
        return Err(ApiError::TooManyRequests);
    }
    let Some(method) = check_second_factor(&st, pending.user_id, &pending.email, &req.code).await?
    else {
        audit::record_now(
            &st.db,
            &actor,
            Event::new("auth.mfa.failed").target("user", pending.user_id),
        )
        .await?;
        return Err(ApiError::Unauthorized);
    };
    st.limiter.reset(&key);
    // Replace the pending session with a fresh, fully authenticated one.
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(pending.session_id)
        .execute(&st.db)
        .await?;
    let (token, lifetime) =
        auth::create_session(&st, pending.user_id, false, pending.ip, None).await?;
    sqlx::query("UPDATE sessions SET mfa_at = now() WHERE token_hash = $1")
        .bind(opnrelay_crypto::hash_token(&token))
        .execute(&st.db)
        .await?;
    sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1")
        .bind(pending.user_id)
        .execute(&st.db)
        .await?;
    audit::record_now(
        &st.db,
        &actor,
        Event::new("auth.login")
            .target("user", pending.user_id)
            .meta(json!({"mfa": method})),
    )
    .await?;
    Ok(with_cookie(
        auth::session_cookie(&st, &token, lifetime),
        json!({"ok": true}),
    ))
}

async fn logout(
    State(st): State<AppState>,
    CurrentUserLax(actor): CurrentUserLax,
) -> ApiResult<Response> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(actor.session_id)
        .execute(&st.db)
        .await?;
    audit::record_now(
        &st.db,
        &actor.audit(),
        Event::new("auth.logout").target("user", actor.user_id),
    )
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, auth::clear_session_cookie(&st));
    Ok((StatusCode::NO_CONTENT, headers).into_response())
}

#[derive(Deserialize)]
struct InviteInspect {
    token: String,
}

#[derive(sqlx::FromRow)]
struct InviteRow {
    id: Uuid,
    user_id: Uuid,
    purpose: String,
    email: String,
    display_name: String,
}

async fn find_invite(st: &AppState, token: &str) -> ApiResult<InviteRow> {
    sqlx::query_as(
        "SELECT i.id, i.user_id, i.purpose, u.email::text AS email, u.display_name
         FROM invitations i JOIN users u ON u.id = i.user_id
         WHERE i.token_hash = $1 AND i.used_at IS NULL AND i.expires_at > now() AND u.deleted_at IS NULL",
    )
    .bind(opnrelay_crypto::hash_token(token.trim()))
    .fetch_optional(&st.db)
    .await?
    .ok_or_else(|| ApiError::bad("this link is invalid or has expired"))
}

async fn invite_inspect(
    State(st): State<AppState>,
    client: ClientInfo,
    Json(req): Json<InviteInspect>,
) -> ApiResult<Json<serde_json::Value>> {
    if !st.limiter.check(
        &format!("invite:{:?}", client.ip),
        30,
        StdDuration::from_secs(600),
    ) {
        return Err(ApiError::TooManyRequests);
    }
    let inv = find_invite(&st, &req.token).await?;
    Ok(Json(
        json!({ "email": inv.email, "display_name": inv.display_name, "purpose": inv.purpose }),
    ))
}

#[derive(Deserialize)]
struct InviteAccept {
    token: String,
    password: String,
    display_name: Option<String>,
}

async fn invite_accept(
    State(st): State<AppState>,
    client: ClientInfo,
    Json(req): Json<InviteAccept>,
) -> ApiResult<Json<serde_json::Value>> {
    if !st.limiter.check(
        &format!("invite:{:?}", client.ip),
        30,
        StdDuration::from_secs(600),
    ) {
        return Err(ApiError::TooManyRequests);
    }
    password::check_policy(&req.password).map_err(ApiError::bad)?;
    let inv = find_invite(&st, &req.token).await?;
    let pw = req.password.clone();
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(anyhow::Error::from)?;
    let name = req
        .display_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && n.chars().count() <= 100)
        .unwrap_or(inv.display_name);

    let mut tx = st.db.begin().await?;
    let used =
        sqlx::query("UPDATE invitations SET used_at = now() WHERE id = $1 AND used_at IS NULL")
            .bind(inv.id)
            .execute(&mut *tx)
            .await?;
    if used.rows_affected() != 1 {
        return Err(ApiError::bad("this link is invalid or has expired"));
    }
    sqlx::query(
        "UPDATE users SET password_hash = $2, display_name = $3, failed_logins = 0, locked_until = NULL,
            status = CASE WHEN status = 'invited' THEN 'active' ELSE status END, updated_at = now()
         WHERE id = $1",
    )
    .bind(inv.user_id)
    .bind(&hash)
    .bind(&name)
    .execute(&mut *tx)
    .await?;
    // A password reset signs the user out everywhere, including devices.
    sqlx::query(
        "UPDATE device_grants SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(inv.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(inv.user_id)
        .execute(&mut *tx)
        .await?;
    let action = if inv.purpose == "invite" {
        "user.invite.accepted"
    } else {
        "user.password.reset"
    };
    audit::record(
        &mut tx,
        &AuditActor {
            id: Some(inv.user_id),
            label: inv.email.clone(),
            ip: client.ip,
        },
        Event::new(action).target("user", inv.user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "email": inv.email })))
}
