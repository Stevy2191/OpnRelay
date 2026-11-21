// SPDX-License-Identifier: AGPL-3.0-only
//! The signed-in user's own account: profile, password, MFA, sessions.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use opnrelay_crypto::password;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::{CurrentUser, CurrentUserLax};
use crate::error::{ApiError, ApiResult};
use crate::mfa;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(me))
        .route("/password", put(change_password))
        .route("/preferences", get(get_prefs).put(put_prefs))
        .route("/mfa/totp", post(totp_start))
        .route("/mfa/totp/confirm", post(totp_confirm))
        .route("/mfa/totp/{id}", delete(totp_delete))
        .route("/mfa/recovery-codes", post(regenerate_recovery))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", delete(revoke_session))
}

#[derive(sqlx::FromRow)]
struct OrgInfo {
    name: String,
    mfa_requirement: String,
    public_links_enabled: bool,
    default_capture_mode: String,
    auto_redaction_default: bool,
    redaction_patterns: Value,
}

async fn me(
    State(st): State<AppState>,
    CurrentUserLax(a): CurrentUserLax,
) -> ApiResult<Json<Value>> {
    let org: OrgInfo = sqlx::query_as(
        "SELECT o.name, p.mfa_requirement, p.public_links_enabled, p.default_capture_mode, p.auto_redaction_default,
                p.redaction_patterns
         FROM organizations o JOIN org_policies p ON p.org_id = o.id WHERE o.id = $1",
    )
    .bind(a.org_id)
    .fetch_one(&st.db)
    .await?;
    let recovery_left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(a.user_id)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(json!({
        "id": a.user_id,
        "email": a.email,
        "display_name": a.display_name,
        "org_role": a.role,
        "csrf_token": a.csrf_token,
        "mfa_enabled": a.mfa_enabled,
        "mfa_setup_required": a.mfa_setup_required,
        "recovery_codes_remaining": recovery_left,
        "organization": {
            "id": a.org_id,
            "name": org.name,
            "mfa_requirement": org.mfa_requirement,
            "public_links_enabled": org.public_links_enabled,
            "default_capture_mode": org.default_capture_mode,
            "auto_redaction_default": org.auto_redaction_default,
            "redaction_patterns": org.redaction_patterns,
        }
    })))
}

#[derive(Deserialize)]
struct PasswordChange {
    current_password: String,
    new_password: String,
}

async fn change_password(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<PasswordChange>,
) -> ApiResult<StatusCode> {
    password::check_policy(&req.new_password).map_err(ApiError::bad)?;
    verify_password(&st, a.user_id, &req.current_password).await?;
    let pw = req.new_password;
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw))
        .await
        .map_err(anyhow::Error::from)?
        .map_err(anyhow::Error::from)?;
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
        .bind(a.user_id)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    // Sign out other sessions and connected devices.
    sqlx::query("UPDATE device_grants SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL AND id <> $2")
        .bind(a.user_id)
        .bind(a.session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND id <> $2")
        .bind(a.user_id)
        .bind(a.session_id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.password.changed").target("user", a.user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Re-authentication check for sensitive changes.
pub async fn verify_password(st: &AppState, user_id: Uuid, pw: &str) -> ApiResult<()> {
    let key = format!("reauth:{user_id}");
    if !st
        .limiter
        .check(&key, 10, std::time::Duration::from_secs(900))
    {
        return Err(ApiError::TooManyRequests);
    }
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&st.db)
        .await?;
    let hash = hash.unwrap_or_else(|| password::dummy_hash().to_string());
    let pw = pw.to_string();
    let ok = tokio::task::spawn_blocking(move || password::verify(&pw, &hash))
        .await
        .map_err(anyhow::Error::from)?;
    if ok {
        Ok(())
    } else {
        Err(ApiError::Forbidden("current password is incorrect"))
    }
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Default)]
struct Prefs {
    capture_mode: Option<String>,
    auto_redaction: Option<bool>,
    include_typed_text: bool,
    region_size: Option<i32>,
}

async fn get_prefs(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Prefs>> {
    let p: Option<Prefs> = sqlx::query_as(
        "SELECT capture_mode, auto_redaction, include_typed_text, region_size FROM user_preferences WHERE user_id = $1",
    )
    .bind(a.user_id)
    .fetch_optional(&st.db)
    .await?;
    Ok(Json(p.unwrap_or_default()))
}

async fn put_prefs(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(p): Json<Prefs>,
) -> ApiResult<Json<Prefs>> {
    if let Some(m) = &p.capture_mode
        && !["full", "window", "region"].contains(&m.as_str())
    {
        return Err(ApiError::bad("capture_mode must be full, window or region"));
    }
    if let Some(r) = p.region_size
        && !(100..=4000).contains(&r)
    {
        return Err(ApiError::bad("region_size must be between 100 and 4000"));
    }
    sqlx::query(
        "INSERT INTO user_preferences (user_id, capture_mode, auto_redaction, include_typed_text, region_size)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (user_id) DO UPDATE SET capture_mode = EXCLUDED.capture_mode, auto_redaction = EXCLUDED.auto_redaction,
            include_typed_text = EXCLUDED.include_typed_text, region_size = EXCLUDED.region_size",
    )
    .bind(a.user_id)
    .bind(&p.capture_mode)
    .bind(p.auto_redaction)
    .bind(p.include_typed_text)
    .bind(p.region_size)
    .execute(&st.db)
    .await?;
    Ok(Json(p))
}

async fn totp_start(
    State(st): State<AppState>,
    CurrentUserLax(a): CurrentUserLax,
) -> ApiResult<Json<Value>> {
    let secret = mfa::new_secret();
    let t = mfa::totp(secret.clone(), &a.email)?;
    let id = Uuid::now_v7();
    let enc = opnrelay_crypto::seal(&st.secrets_key, &secret, id.as_bytes())
        .map_err(anyhow::Error::from)?;
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "DELETE FROM mfa_factors WHERE user_id = $1 AND kind = 'totp' AND confirmed_at IS NULL",
    )
    .bind(a.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO mfa_factors (id, user_id, kind, label, secret_enc) VALUES ($1, $2, 'totp', 'Authenticator app', $3)")
        .bind(id)
        .bind(a.user_id)
        .bind(enc)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let url = t.get_url();
    let qr = qrcode::QrCode::new(url.as_bytes())
        .map_err(|e| anyhow::anyhow!("qr: {e}"))?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .build();
    Ok(Json(json!({
        "factor_id": id,
        "secret": t.get_secret_base32(),
        "otpauth_url": url,
        "qr_svg": qr,
    })))
}

#[derive(Deserialize)]
struct TotpConfirm {
    factor_id: Uuid,
    code: String,
}

#[derive(sqlx::FromRow)]
struct PendingFactor {
    secret_enc: Option<Vec<u8>>,
}

async fn totp_confirm(
    State(st): State<AppState>,
    CurrentUserLax(a): CurrentUserLax,
    Json(req): Json<TotpConfirm>,
) -> ApiResult<Json<Value>> {
    if !st.limiter.check(
        &format!("totp-confirm:{}", a.user_id),
        10,
        std::time::Duration::from_secs(600),
    ) {
        return Err(ApiError::TooManyRequests);
    }
    let f: PendingFactor = sqlx::query_as(
        "SELECT secret_enc FROM mfa_factors WHERE id = $1 AND user_id = $2 AND kind = 'totp' AND confirmed_at IS NULL",
    )
    .bind(req.factor_id)
    .bind(a.user_id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let enc = f.secret_enc.ok_or(ApiError::NotFound)?;
    let secret = opnrelay_crypto::open(&st.secrets_key, &enc, req.factor_id.as_bytes())
        .map_err(anyhow::Error::from)?;
    let t = mfa::totp(secret, &a.email)?;
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    let step = mfa::verify(&t, &req.code, now, None).ok_or_else(|| {
        ApiError::bad("that code is not correct; check your device clock and try again")
    })?;

    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE mfa_factors SET confirmed_at = now(), last_used_step = $2 WHERE id = $1")
        .bind(req.factor_id)
        .bind(step)
        .execute(&mut *tx)
        .await?;
    let codes = replace_recovery_codes(&mut tx, a.user_id).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.mfa.enrolled")
            .target("user", a.user_id)
            .meta(json!({"kind": "totp"})),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "recovery_codes": codes })))
}

async fn replace_recovery_codes(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> ApiResult<Vec<String>> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    let codes = mfa::recovery_codes(10);
    for c in &codes {
        sqlx::query("INSERT INTO recovery_codes (id, user_id, code_hash) VALUES ($1, $2, $3)")
            .bind(Uuid::now_v7())
            .bind(user_id)
            .bind(mfa::hash_recovery(c))
            .execute(&mut *conn)
            .await?;
    }
    Ok(codes)
}

#[derive(Deserialize)]
struct WithPassword {
    password: String,
}

async fn regenerate_recovery(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<WithPassword>,
) -> ApiResult<Json<Value>> {
    if !a.mfa_enabled {
        return Err(ApiError::bad("enable two-factor authentication first"));
    }
    verify_password(&st, a.user_id, &req.password).await?;
    let mut tx = st.db.begin().await?;
    let codes = replace_recovery_codes(&mut tx, a.user_id).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.mfa.recovery_regenerated").target("user", a.user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "recovery_codes": codes })))
}

async fn totp_delete(
    State(st): State<AppState>,
    CurrentUserLax(a): CurrentUserLax,
    Path(id): Path<Uuid>,
    Json(req): Json<WithPassword>,
) -> ApiResult<StatusCode> {
    verify_password(&st, a.user_id, &req.password).await?;
    let mut tx = st.db.begin().await?;
    let r = sqlx::query("DELETE FROM mfa_factors WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mfa_factors WHERE user_id = $1 AND confirmed_at IS NOT NULL",
    )
    .bind(a.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if remaining == 0 {
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
            .bind(a.user_id)
            .execute(&mut *tx)
            .await?;
    }
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.mfa.removed").target("user", a.user_id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
struct SessionInfo {
    id: Uuid,
    #[serde(serialize_with = "crate::util::ser_ip")]
    ip: Option<sqlx::types::ipnetwork::IpNetwork>,
    user_agent: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    last_seen_at: OffsetDateTime,
    #[sqlx(skip)]
    current: bool,
}

async fn list_sessions(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Vec<SessionInfo>>> {
    let mut rows: Vec<SessionInfo> = sqlx::query_as(
        "SELECT id, ip, user_agent, created_at, last_seen_at FROM sessions
         WHERE user_id = $1 AND mfa_pending = false AND expires_at > now() ORDER BY last_seen_at DESC",
    )
    .bind(a.user_id)
    .fetch_all(&st.db)
    .await?;
    for r in &mut rows {
        r.current = r.id == a.session_id;
    }
    Ok(Json(rows))
}

async fn revoke_session(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let r = sqlx::query("DELETE FROM sessions WHERE id = $1 AND user_id = $2")
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
        Event::new("user.session.revoked").target("session", id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
