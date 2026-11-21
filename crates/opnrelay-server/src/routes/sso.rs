// SPDX-License-Identifier: AGPL-3.0-only
//! "Sign in with Microsoft" (Entra ID) and the shared Microsoft callback,
//! which also completes the OneDrive backup connection.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};
use crate::auth::{self, ClientInfo, CurrentUser, CurrentUserLax};
use crate::authz::{OrgRole, require_admin};
use crate::error::{ApiError, ApiResult};
use crate::graph;
use crate::group_sync;
use crate::microsoft::{self, AppConfig};
use crate::state::AppState;

const STATE_COOKIE: &str = "opnrelay_sso_state";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/providers", get(providers))
        .route("/auth/sso/microsoft/start", get(start_login))
        .route("/auth/sso/microsoft/callback", get(callback))
        .route("/admin/sso", get(get_settings).put(put_settings))
        .route("/admin/sso/groups/search", get(search_groups))
        .route("/admin/sso/sync", post(sync_now))
}

async fn org_id(st: &AppState) -> ApiResult<Uuid> {
    sqlx::query_scalar("SELECT id FROM organizations ORDER BY created_at LIMIT 1")
        .fetch_optional(&st.db)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn sso_only(st: &AppState, org: Uuid) -> ApiResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT sso_only FROM org_policies WHERE org_id = $1")
            .bind(org)
            .fetch_one(&st.db)
            .await?,
    )
}

/// Public: which sign-in methods the login page should show.
async fn providers(State(st): State<AppState>) -> ApiResult<Json<Value>> {
    let Ok(org) = org_id(&st).await else {
        return Ok(Json(json!({"microsoft": false, "password": true})));
    };
    let ms = microsoft::load(&st, &st.db, org)
        .await?
        .is_some_and(|p| p.enabled);
    let only = ms && sso_only(&st, org).await?;
    Ok(Json(json!({"microsoft": ms, "password": !only})))
}

fn state_cookie(st: &AppState, value: &str, max_age: i64) -> HeaderValue {
    let secure = if st.cfg.cookie_secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{STATE_COOKIE}={value}; Path=/api/v1/auth/sso; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("ascii cookie")
}

/// Create a state record and return the redirect to Microsoft plus the
/// browser-binding cookie.
pub async fn begin(
    st: &AppState,
    app: &AppConfig,
    purpose: &str,
    scope: &str,
    return_to: &str,
    user_id: Option<Uuid>,
    login_hint: Option<&str>,
) -> ApiResult<Response> {
    let state = opnrelay_crypto::random_token();
    let nonce = opnrelay_crypto::random_token();
    let verifier = opnrelay_crypto::random_token();
    let challenge = {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(opnrelay_crypto::sha256(verifier.as_bytes()))
    };
    sqlx::query("DELETE FROM sso_states WHERE expires_at < now()")
        .execute(&st.db)
        .await?;
    sqlx::query(
        "INSERT INTO sso_states (state_hash, purpose, nonce, verifier, return_to, user_id, expires_at)
         VALUES ($1,$2,$3,$4,$5,$6, now() + interval '10 minutes')",
    )
    .bind(opnrelay_crypto::hash_token(&state))
    .bind(purpose)
    .bind(&nonce)
    .bind(&verifier)
    .bind(return_to)
    .bind(user_id)
    .execute(&st.db)
    .await?;
    let url = microsoft::authorize_url(st, app, scope, &state, &nonce, &challenge, login_hint);
    let mut h = HeaderMap::new();
    h.insert(header::SET_COOKIE, state_cookie(st, &state, 600));
    Ok((h, Redirect::to(&url)).into_response())
}

#[derive(Deserialize)]
struct StartQuery {
    return_to: Option<String>,
}

/// Only same-app hash routes like `/connect?...` are allowed as return paths.
fn safe_return(r: Option<&str>) -> String {
    match r {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && p.len() <= 2000
                && !p.contains(['\\', '\r', '\n']) =>
        {
            p.to_string()
        }
        _ => "/".into(),
    }
}

async fn start_login(
    State(st): State<AppState>,
    Query(q): Query<StartQuery>,
) -> ApiResult<Response> {
    let org = org_id(&st).await?;
    let p = microsoft::load(&st, &st.db, org)
        .await?
        .filter(|p| p.enabled)
        .ok_or_else(|| ApiError::bad("Microsoft sign-in is not set up"))?;
    begin(
        &st,
        &p.app,
        "login",
        microsoft::LOGIN_SCOPE,
        &safe_return(q.return_to.as_deref()),
        None,
        None,
    )
    .await
}

fn login_error(msg: &str) -> Response {
    let enc: String = msg
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    Redirect::to(&format!("/#/login?error={enc}")).into_response()
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(sqlx::FromRow)]
struct StateRow {
    purpose: String,
    nonce: String,
    verifier: String,
    return_to: String,
    user_id: Option<Uuid>,
}

async fn callback(
    State(st): State<AppState>,
    client: ClientInfo,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    match handle_callback(&st, &client, &headers, q).await {
        Ok(r) => r,
        Err(e) => {
            let msg = match &e {
                ApiError::Internal(_) => "Sign-in failed because of a server error.".to_string(),
                other => other.to_string(),
            };
            if let ApiError::Internal(err) = &e {
                tracing::error!(error = ?err, "microsoft callback failed");
            }
            let mut r = login_error(&msg);
            r.headers_mut()
                .insert(header::SET_COOKIE, state_cookie(&st, "", 0));
            r
        }
    }
}

async fn handle_callback(
    st: &AppState,
    client: &ClientInfo,
    headers: &HeaderMap,
    q: CallbackQuery,
) -> ApiResult<Response> {
    if let Some(e) = q.error {
        let d = q.error_description.unwrap_or_default();
        return Err(ApiError::bad(format!(
            "Microsoft: {} {}",
            e,
            d.lines().next().unwrap_or_default()
        )));
    }
    let (code, state) = q
        .code
        .zip(q.state)
        .ok_or_else(|| ApiError::bad("incomplete response from Microsoft"))?;
    // The state must come back to the same browser that started the flow.
    let cookie = auth::read_cookie(headers, STATE_COOKIE).unwrap_or_default();
    if !opnrelay_crypto::ct_eq(cookie.as_bytes(), state.as_bytes()) {
        return Err(ApiError::bad(
            "this sign-in link was started in another browser; please try again",
        ));
    }
    let row: StateRow = sqlx::query_as(
        "DELETE FROM sso_states WHERE state_hash = $1 AND expires_at > now()
         RETURNING purpose, nonce, verifier, return_to, user_id",
    )
    .bind(opnrelay_crypto::hash_token(&state))
    .fetch_optional(&st.db)
    .await?
    .ok_or_else(|| ApiError::bad("the sign-in took too long; please try again"))?;
    let org = org_id(st).await?;
    let provider = microsoft::load(st, &st.db, org)
        .await?
        .ok_or_else(|| ApiError::bad("Microsoft sign-in is not set up"))?;

    if row.purpose == "onedrive" {
        return crate::routes::backups::finish_onedrive(
            st,
            client,
            &provider,
            &row.user_id,
            &code,
            &row.verifier,
        )
        .await;
    }
    if !provider.enabled {
        return Err(ApiError::bad("Microsoft sign-in is turned off"));
    }
    let tokens = microsoft::exchange_code(
        st,
        &provider.app,
        &code,
        &row.verifier,
        microsoft::LOGIN_SCOPE,
    )
    .await?;
    let id_token = tokens
        .id_token
        .ok_or_else(|| ApiError::bad("Microsoft returned no ID token"))?;
    let identity = microsoft::verify_id_token(
        &id_token,
        &st.cfg.ms_login_url,
        &provider.app,
        &row.nonce,
        OffsetDateTime::now_utc().unix_timestamp(),
    )
    .map_err(ApiError::bad)?;

    let user_id = resolve_user(st, org, &provider, &identity, client).await?;
    if provider.group_sync_enabled
        && let Err(e) =
            crate::group_sync::sync_user(st, &provider, user_id, &identity.subject).await
    {
        tracing::warn!(error = ?e, "group sync at sign-in failed");
    }
    let (token, lifetime) =
        auth::create_session(st, user_id, false, client.ip, client.user_agent.as_deref()).await?;
    sqlx::query("UPDATE sessions SET auth_method = 'sso', mfa_at = now() WHERE token_hash = $1")
        .bind(opnrelay_crypto::hash_token(&token))
        .execute(&st.db)
        .await?;
    sqlx::query("UPDATE users SET last_login_at = now(), failed_logins = 0, locked_until = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&st.db)
        .await?;
    audit::record_now(
        &st.db,
        &AuditActor {
            id: Some(user_id),
            label: identity.email.clone(),
            ip: client.ip,
        },
        Event::new("auth.login")
            .target("user", user_id)
            .meta(json!({"method": "microsoft"})),
    )
    .await?;
    let mut h = HeaderMap::new();
    h.append(
        header::SET_COOKIE,
        auth::session_cookie(st, &token, lifetime),
    );
    h.append(header::SET_COOKIE, state_cookie(st, "", 0));
    Ok((h, Redirect::to(&format!("/#{}", row.return_to))).into_response())
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    status: String,
}

async fn resolve_user(
    st: &AppState,
    org: Uuid,
    p: &microsoft::Provider,
    id: &microsoft::Identity,
    client: &ClientInfo,
) -> ApiResult<Uuid> {
    let actor = AuditActor {
        id: None,
        label: id.email.clone(),
        ip: client.ip,
    };
    let mut tx = st.db.begin().await?;
    let linked: Option<UserRow> = sqlx::query_as(
        "SELECT u.id, u.status FROM user_identities i JOIN users u ON u.id = i.user_id
         WHERE i.idp_id = $1 AND i.subject = $2 AND u.deleted_at IS NULL",
    )
    .bind(p.id)
    .bind(&id.subject)
    .fetch_optional(&mut *tx)
    .await?;
    let user = match linked {
        Some(u) => u,
        None => {
            let by_email: Option<UserRow> = sqlx::query_as(
                "SELECT id, status FROM users WHERE email = $1 AND org_id = $2 AND deleted_at IS NULL",
            )
            .bind(&id.email)
            .bind(org)
            .fetch_optional(&mut *tx)
            .await?;
            let u = match by_email {
                Some(u) => u,
                None if p.jit_provisioning => {
                    let uid = Uuid::now_v7();
                    sqlx::query(
                        "INSERT INTO users (id, org_id, email, display_name, org_role, status) VALUES ($1,$2,$3,$4,$5,'active')",
                    )
                    .bind(uid)
                    .bind(org)
                    .bind(&id.email)
                    .bind(id.name.chars().take(100).collect::<String>())
                    .bind(&p.default_role)
                    .execute(&mut *tx)
                    .await?;
                    audit::record(
                        &mut tx,
                        &actor,
                        Event::new("user.created_by_sso")
                            .target("user", uid)
                            .meta(json!({"role": p.default_role})),
                    )
                    .await?;
                    UserRow {
                        id: uid,
                        status: "active".into(),
                    }
                }
                None => {
                    return Err(ApiError::bad(
                        "you don't have an OpnRelay account yet; ask an administrator to invite you",
                    ));
                }
            };
            sqlx::query("INSERT INTO user_identities (user_id, idp_id, subject) VALUES ($1,$2,$3)")
                .bind(u.id)
                .bind(p.id)
                .bind(&id.subject)
                .execute(&mut *tx)
                .await?;
            audit::record(
                &mut tx,
                &actor,
                Event::new("user.sso_linked").target("user", u.id),
            )
            .await?;
            u
        }
    };
    match user.status.as_str() {
        "disabled" => return Err(ApiError::bad("your OpnRelay account is disabled")),
        "invited" => {
            // Signing in with the company account completes the invitation.
            sqlx::query("UPDATE users SET status = 'active' WHERE id = $1")
                .bind(user.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE invitations SET used_at = now() WHERE user_id = $1 AND used_at IS NULL",
            )
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
        }
        _ => {}
    }
    tx.commit().await?;
    Ok(user.id)
}

#[derive(Serialize)]
struct Settings {
    configured: bool,
    enabled: bool,
    tenant_id: String,
    client_id: String,
    has_secret: bool,
    jit_provisioning: bool,
    default_role: String,
    sso_only: bool,
    redirect_uri: String,
    group_sync_enabled: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    group_sync_last_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    group_sync_last_ok_at: Option<OffsetDateTime>,
    group_sync_last_error: Option<String>,
    group_sync_brake_pending: Option<i32>,
}

type SyncStatus = (
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
    Option<String>,
    Option<i32>,
);

async fn get_settings(
    State(st): State<AppState>,
    CurrentUserLax(a): CurrentUserLax,
) -> ApiResult<Json<Settings>> {
    require_admin(&a)?;
    let p = microsoft::load(&st, &st.db, a.org_id).await?;
    let status: Option<SyncStatus> = match &p {
        Some(p) => {
            sqlx::query_as(
                "SELECT group_sync_last_at, group_sync_last_ok_at, group_sync_last_error,
                        group_sync_brake_pending FROM identity_providers WHERE id = $1",
            )
            .bind(p.id)
            .fetch_optional(&st.db)
            .await?
        }
        None => None,
    };
    let (last_at, last_ok_at, last_error, brake) = status.unwrap_or_default();
    Ok(Json(Settings {
        configured: p.is_some(),
        enabled: p.as_ref().is_some_and(|p| p.enabled),
        tenant_id: p
            .as_ref()
            .map(|p| p.app.tenant_id.clone())
            .unwrap_or_default(),
        client_id: p
            .as_ref()
            .map(|p| p.app.client_id.clone())
            .unwrap_or_default(),
        has_secret: p.as_ref().is_some_and(|p| !p.app.client_secret.is_empty()),
        jit_provisioning: p.as_ref().is_none_or(|p| p.jit_provisioning),
        default_role: p
            .as_ref()
            .map(|p| p.default_role.clone())
            .unwrap_or_else(|| "member".into()),
        sso_only: sso_only(&st, a.org_id).await?,
        redirect_uri: microsoft::redirect_uri(&st),
        group_sync_enabled: p.as_ref().is_some_and(|p| p.group_sync_enabled),
        group_sync_last_at: last_at,
        group_sync_last_ok_at: last_ok_at,
        group_sync_last_error: last_error,
        group_sync_brake_pending: brake,
    }))
}

#[derive(Deserialize)]
struct PutSettings {
    enabled: bool,
    tenant_id: String,
    client_id: String,
    /// Empty keeps the stored secret.
    #[serde(default)]
    client_secret: String,
    jit_provisioning: bool,
    default_role: String,
    sso_only: bool,
    #[serde(default)]
    group_sync_enabled: bool,
}

async fn put_settings(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<PutSettings>,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    if req.group_sync_enabled && !graph::tenant_is_specific(&req.tenant_id) {
        return Err(ApiError::bad(
            "Group sync needs your specific tenant ID (not organizations, common or consumers).",
        ));
    }
    if !microsoft::valid_tenant(&req.tenant_id) {
        return Err(ApiError::bad(
            "enter your Directory (tenant) ID or primary domain, not 'common'",
        ));
    }
    if !microsoft::is_guid(&req.client_id) {
        return Err(ApiError::bad(
            "the Application (client) ID must look like 00000000-0000-0000-0000-000000000000",
        ));
    }
    let role =
        OrgRole::parse(&req.default_role).ok_or_else(|| ApiError::bad("invalid default role"))?;
    if role == OrgRole::Owner {
        return Err(ApiError::bad("new accounts cannot be owners"));
    }
    if req.sso_only && !req.enabled {
        return Err(ApiError::bad(
            "turn on Microsoft sign-in before making it the only option",
        ));
    }
    let existing = microsoft::load(&st, &st.db, a.org_id).await?;
    let secret = if req.client_secret.trim().is_empty() {
        existing
            .as_ref()
            .map(|p| p.app.client_secret.clone())
            .unwrap_or_default()
    } else {
        req.client_secret.trim().to_string()
    };
    if req.enabled && secret.is_empty() {
        return Err(ApiError::bad("a client secret is required"));
    }
    if req.group_sync_enabled && secret.is_empty() {
        return Err(ApiError::bad("Group sync needs a client secret"));
    }
    let id = existing.as_ref().map(|p| p.id).unwrap_or_else(Uuid::now_v7);
    let app = AppConfig {
        tenant_id: req.tenant_id.trim().to_string(),
        client_id: req.client_id.trim().to_lowercase(),
        client_secret: secret,
    };
    let app_changed = existing.as_ref().is_none_or(|p| {
        p.app.tenant_id != app.tenant_id
            || p.app.client_id != app.client_id
            || p.app.client_secret != app.client_secret
    });
    let sync_was = existing.as_ref().is_some_and(|p| p.group_sync_enabled);
    let enc = microsoft::seal_config(&st, id, &app)?;
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, kind, name, config_enc, enabled, jit_provisioning, default_role, group_sync_enabled)
         VALUES ($1,$2,'oidc',$3,$4,$5,$6,$7,$8)
         ON CONFLICT (id) DO UPDATE SET config_enc = EXCLUDED.config_enc, enabled = EXCLUDED.enabled,
            jit_provisioning = EXCLUDED.jit_provisioning, default_role = EXCLUDED.default_role,
            group_sync_enabled = EXCLUDED.group_sync_enabled",
    )
    .bind(id)
    .bind(a.org_id)
    .bind(microsoft::PROVIDER_NAME)
    .bind(enc)
    .bind(req.enabled)
    .bind(req.jit_provisioning)
    .bind(role.as_str())
    .bind(req.group_sync_enabled)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE org_policies SET sso_only = $2 WHERE org_id = $1")
        .bind(a.org_id)
        .bind(req.sso_only)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("org.sso.updated").meta(json!({
            "enabled": req.enabled, "tenant_id": app.tenant_id, "client_id": app.client_id,
            "secret_changed": !req.client_secret.trim().is_empty(), "jit": req.jit_provisioning,
            "default_role": role.as_str(), "sso_only": req.sso_only
        })),
    )
    .await?;
    if sync_was != req.group_sync_enabled {
        audit::record(
            &mut tx,
            &a.audit(),
            Event::new("idp.sync.settings_changed")
                .meta(json!({"group_sync_enabled": req.group_sync_enabled})),
        )
        .await?;
    }
    tx.commit().await?;
    if app_changed {
        // The cached Graph token belongs to the old app credentials.
        *st.graph_token.lock().await = None;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The provider, if Microsoft group sync is turned on and usable.
pub async fn sync_provider(st: &AppState, org: Uuid) -> ApiResult<microsoft::Provider> {
    let p = microsoft::load(st, &st.db, org)
        .await?
        .filter(|p| p.group_sync_enabled)
        .ok_or_else(|| ApiError::bad("Group sync is off"))?;
    if !graph::tenant_is_specific(&p.app.tenant_id) {
        return Err(ApiError::bad(
            "Group sync needs your specific tenant ID (not organizations, common or consumers).",
        ));
    }
    Ok(p)
}

#[derive(Deserialize, Default)]
struct SyncRequest {
    #[serde(default)]
    confirm_brake: bool,
}

async fn sync_now(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<SyncRequest>,
) -> ApiResult<Json<group_sync::RunSummary>> {
    require_admin(&a)?;
    sync_provider(&st, a.org_id).await?;
    let sum = group_sync::sync_all(&st, &a.audit(), req.confirm_brake).await?;
    Ok(Json(sum))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search_groups(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<Value>>> {
    require_admin(&a)?;
    let p = sync_provider(&st, a.org_id).await?;
    let term = q.q.trim();
    if term.is_empty() {
        return Ok(Json(vec![]));
    }
    let found = graph::search_groups(&st, &p.app, term)
        .await
        .map_err(group_sync::api_err)?;
    let ids: Vec<String> = found.iter().map(|g| g.id.clone()).collect();
    let linked: Vec<String> = sqlx::query_scalar(
        "SELECT external_id FROM groups WHERE org_id = $1 AND source = 'idp' AND external_id = ANY($2)",
    )
    .bind(a.org_id)
    .bind(&ids)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(
        found
            .into_iter()
            .map(|g| {
                json!({"linked": linked.contains(&g.id), "id": g.id, "display_name": g.display_name,
                    "description": g.description, "kind": g.kind, "on_prem": g.on_prem})
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::safe_return;

    #[test]
    fn return_paths() {
        assert_eq!(safe_return(Some("/connect?x=1")), "/connect?x=1");
        assert_eq!(safe_return(Some("//evil.com")), "/");
        assert_eq!(safe_return(Some("https://evil.com")), "/");
        assert_eq!(safe_return(None), "/");
    }
}
