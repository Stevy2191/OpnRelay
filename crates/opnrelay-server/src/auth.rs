// SPDX-License-Identifier: AGPL-3.0-only
//! Session handling and request extractors.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method};
use sqlx::types::ipnetwork::IpNetwork;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::audit::AuditActor;
use crate::authz::OrgRole;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const CSRF_HEADER: &str = "x-csrf-token";
/// Lifetime of a session waiting for its MFA code.
pub const MFA_PENDING_MINUTES: i64 = 10;

/// The authenticated user making a request.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: OrgRole,
    pub session_id: Uuid,
    pub csrf_token: String,
    pub ip: Option<IpAddr>,
    pub mfa_enabled: bool,
    /// Org policy requires MFA and the user has not enrolled yet.
    pub mfa_setup_required: bool,
}

impl Actor {
    pub fn audit(&self) -> AuditActor {
        AuditActor {
            id: Some(self.user_id),
            label: self.email.clone(),
            ip: self.ip,
        }
    }
}

/// Determine the client IP, honouring `X-Forwarded-For` only when the
/// deployment says a trusted reverse proxy is in front.
pub fn client_ip(parts: &Parts, trust_proxy: bool) -> Option<IpAddr> {
    if trust_proxy
        && let Some(xff) = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        && let Some(ip) = xff.rsplit(',').next().and_then(|s| s.trim().parse().ok())
    {
        return Some(ip);
    }
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
}

pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Build a `Set-Cookie` header value for the session cookie.
pub fn session_cookie(state: &AppState, token: &str, max_age: Duration) -> HeaderValue {
    let secure = if state.cfg.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let v = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        state.cfg.session_cookie_name(),
        token,
        max_age.whole_seconds().max(0),
        secure
    );
    HeaderValue::from_str(&v).expect("cookie is ascii")
}

pub fn clear_session_cookie(state: &AppState) -> HeaderValue {
    session_cookie(state, "", Duration::ZERO)
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    session_id: Uuid,
    csrf_token: String,
    mfa_pending: bool,
    last_seen_at: OffsetDateTime,
    expires_at: OffsetDateTime,
    user_id: Uuid,
    org_id: Uuid,
    email: String,
    display_name: String,
    org_role: String,
    status: String,
    session_idle_minutes: i32,
    mfa_requirement: String,
    mfa_enabled: bool,
}

/// Create a session for `user_id` and return the raw token (for the cookie).
pub async fn create_session(
    state: &AppState,
    user_id: Uuid,
    mfa_pending: bool,
    ip: Option<IpAddr>,
    user_agent: Option<&str>,
) -> ApiResult<(String, Duration)> {
    let absolute_hours: i32 = sqlx::query_scalar(
        "SELECT p.session_absolute_hours FROM org_policies p JOIN users u ON u.org_id = p.org_id WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    let lifetime = if mfa_pending {
        Duration::minutes(MFA_PENDING_MINUTES)
    } else {
        Duration::hours(absolute_hours.into())
    };
    let token = opnrelay_crypto::random_token();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, csrf_token, mfa_pending, mfa_at, ip, user_agent, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now() + $9)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(opnrelay_crypto::hash_token(&token))
    .bind(opnrelay_crypto::random_token())
    .bind(mfa_pending)
    .bind(None::<OffsetDateTime>)
    .bind(ip.map(IpNetwork::from))
    .bind(user_agent.map(|u| u.chars().take(400).collect::<String>()))
    .bind(lifetime)
    .execute(&state.db)
    .await?;
    Ok((token, lifetime))
}

async fn load_session(parts: &Parts, state: &AppState) -> ApiResult<(SessionRow, Option<IpAddr>)> {
    let token = read_cookie(&parts.headers, state.cfg.session_cookie_name())
        .filter(|t| !t.is_empty())
        .ok_or(ApiError::Unauthorized)?;
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT s.id AS session_id, s.csrf_token, s.mfa_pending, s.last_seen_at, s.expires_at,
                u.id AS user_id, u.org_id, u.email::text AS email, u.display_name, u.org_role, u.status,
                p.session_idle_minutes, p.mfa_requirement,
                EXISTS (SELECT 1 FROM mfa_factors f WHERE f.user_id = u.id AND f.confirmed_at IS NOT NULL) AS mfa_enabled
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         JOIN org_policies p ON p.org_id = u.org_id
         WHERE s.token_hash = $1 AND u.deleted_at IS NULL",
    )
    .bind(opnrelay_crypto::hash_token(&token))
    .fetch_optional(&state.db)
    .await?;
    let row = row.ok_or(ApiError::Unauthorized)?;
    let now = OffsetDateTime::now_utc();
    let idle = Duration::minutes(row.session_idle_minutes.into());
    let expired = now >= row.expires_at || (!row.mfa_pending && now >= row.last_seen_at + idle);
    if expired || row.status != "active" {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(row.session_id)
            .execute(&state.db)
            .await?;
        return Err(ApiError::Unauthorized);
    }
    Ok((row, client_ip(parts, state.cfg.trust_proxy)))
}

fn check_csrf(parts: &Parts, expected: &str) -> ApiResult<()> {
    if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(());
    }
    let sent = parts
        .headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if opnrelay_crypto::ct_eq(sent.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Forbidden("missing or invalid CSRF token"))
    }
}

async fn actor_from_parts(parts: &Parts, state: &AppState) -> ApiResult<Actor> {
    if let Some(actor) = parts.extensions.get::<Actor>() {
        return Ok(actor.clone());
    }
    let (row, ip) = load_session(parts, state).await?;
    if row.mfa_pending {
        return Err(ApiError::Unauthorized);
    }
    check_csrf(parts, &row.csrf_token)?;
    let now = OffsetDateTime::now_utc();
    if now - row.last_seen_at > Duration::minutes(1) {
        sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE id = $1")
            .bind(row.session_id)
            .execute(&state.db)
            .await?;
    }
    let role = OrgRole::parse(&row.org_role).ok_or_else(|| anyhow::anyhow!("bad role in db"))?;
    let mfa_required = match row.mfa_requirement.as_str() {
        "all" => true,
        "admins" => role.is_admin(),
        _ => false,
    };
    Ok(Actor {
        user_id: row.user_id,
        org_id: row.org_id,
        email: row.email,
        display_name: row.display_name,
        role,
        session_id: row.session_id,
        csrf_token: row.csrf_token,
        ip,
        mfa_enabled: row.mfa_enabled,
        mfa_setup_required: mfa_required && !row.mfa_enabled,
    })
}

/// Authenticated user; rejects users who still must enrol MFA.
pub struct CurrentUser(pub Actor);

/// Authenticated user; allowed while MFA enrolment is pending
/// (used by `/me`, MFA enrolment and logout).
pub struct CurrentUserLax(pub Actor);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let actor = actor_from_parts(parts, state).await?;
        if actor.mfa_setup_required {
            return Err(ApiError::Forbidden(
                "mfa_setup_required: your organisation requires two-factor authentication; enrol it first",
            ));
        }
        Ok(CurrentUser(actor))
    }
}

impl FromRequestParts<AppState> for CurrentUserLax {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        Ok(CurrentUserLax(actor_from_parts(parts, state).await?))
    }
}

/// A session that has passed the password step and awaits its MFA code.
pub struct PendingMfa {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub ip: Option<IpAddr>,
}

impl FromRequestParts<AppState> for PendingMfa {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let (row, ip) = load_session(parts, state).await?;
        if !row.mfa_pending {
            return Err(ApiError::bad("no MFA challenge pending"));
        }
        Ok(PendingMfa {
            session_id: row.session_id,
            user_id: row.user_id,
            email: row.email,
            ip,
        })
    }
}

/// Client IP + user agent, for unauthenticated endpoints.
pub struct ClientInfo {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
}

impl FromRequestParts<AppState> for ClientInfo {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        Ok(ClientInfo {
            ip: client_ip(parts, state.cfg.trust_proxy),
            user_agent: parts
                .headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(
            "cookie",
            HeaderValue::from_static("a=1; opnrelay_session=tok; b=2"),
        );
        assert_eq!(read_cookie(&h, "opnrelay_session").as_deref(), Some("tok"));
        assert_eq!(read_cookie(&h, "missing"), None);
    }
}
