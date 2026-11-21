// SPDX-License-Identifier: AGPL-3.0-only
//! Organisation administration: users, invitations, policies, groups.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::{Actor, CurrentUser};
use crate::authz::{OrgRole, require_admin};
use crate::error::{ApiError, ApiResult};
use crate::graph;
use crate::group_sync;
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(invite_user))
        .route("/users/{id}", axum::routing::patch(update_user))
        .route("/users/{id}/reset-link", post(reset_link))
        .route("/users/{id}/reset-mfa", post(reset_mfa))
        .route("/users/{id}/revoke-sessions", post(revoke_sessions))
        .route("/policies", get(get_policies).put(put_policies))
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/link", post(link_group))
        .route("/groups/{id}", delete(delete_group))
        .route("/groups/{id}/members", put(set_group_members))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    org_role: String,
    status: String,
    mfa_enabled: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    last_login_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

const USER_COLS: &str = "u.id, u.email::text AS email, u.display_name, u.org_role, u.status,
    EXISTS (SELECT 1 FROM mfa_factors f WHERE f.user_id = u.id AND f.confirmed_at IS NOT NULL) AS mfa_enabled,
    u.last_login_at, u.created_at";

/// Users list is visible to every signed-in member so they can be added to
/// workspaces; only admins can change them.
async fn list_users(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Vec<UserRow>>> {
    let rows = sqlx::query_as(&format!(
        "SELECT {USER_COLS} FROM users u WHERE u.org_id = $1 AND u.deleted_at IS NULL ORDER BY u.display_name"
    ))
    .bind(a.org_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

async fn load_user(st: &AppState, a: &Actor, id: Uuid) -> ApiResult<UserRow> {
    sqlx::query_as(&format!(
        "SELECT {USER_COLS} FROM users u WHERE u.id = $1 AND u.org_id = $2 AND u.deleted_at IS NULL"
    ))
    .bind(id)
    .bind(a.org_id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(ApiError::NotFound)
}

/// Create a one-time link (invite or password reset) and return its URL.
pub async fn create_link(
    conn: &mut sqlx::PgConnection,
    public_url: &str,
    org_id: Uuid,
    user_id: Uuid,
    created_by: Option<Uuid>,
    purpose: &str,
) -> ApiResult<String> {
    let token = opnrelay_crypto::random_token();
    let hours = if purpose == "invite" { 24 * 7 } else { 24 };
    sqlx::query("UPDATE invitations SET used_at = now() WHERE user_id = $1 AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO invitations (id, org_id, user_id, token_hash, purpose, created_by, expires_at)
         VALUES ($1,$2,$3,$4,$5,$6, now() + make_interval(hours => $7))",
    )
    .bind(Uuid::now_v7())
    .bind(org_id)
    .bind(user_id)
    .bind(opnrelay_crypto::hash_token(&token))
    .bind(purpose)
    .bind(created_by)
    .bind(hours)
    .execute(&mut *conn)
    .await?;
    // Token goes in the URL fragment so it never reaches server logs.
    Ok(format!("{public_url}/#/invite/{token}"))
}

#[derive(Deserialize)]
struct InviteReq {
    email: String,
    display_name: String,
    #[serde(default = "default_role")]
    org_role: String,
}

fn default_role() -> String {
    "member".into()
}

fn check_role_change(a: &Actor, new_role: OrgRole) -> ApiResult<()> {
    if new_role == OrgRole::Owner && a.role != OrgRole::Owner {
        return Err(ApiError::Forbidden(
            "only an owner can grant the owner role",
        ));
    }
    Ok(())
}

async fn invite_user(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<InviteReq>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_admin(&a)?;
    let email = req.email.trim().to_lowercase();
    if !util::valid_email(&email) {
        return Err(ApiError::bad("enter a valid email address"));
    }
    let name = util::name("display_name", &req.display_name, 100)?;
    let role = OrgRole::parse(&req.org_role).ok_or_else(|| ApiError::bad("invalid org_role"))?;
    check_role_change(&a, role)?;
    let id = Uuid::now_v7();
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "INSERT INTO users (id, org_id, email, display_name, org_role, status) VALUES ($1,$2,$3,$4,$5,'invited')",
    )
    .bind(id)
    .bind(a.org_id)
    .bind(&email)
    .bind(&name)
    .bind(role.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| match ApiError::from(e) {
        ApiError::Conflict(_) => ApiError::Conflict("a user with this email already exists".into()),
        other => other,
    })?;
    let url = create_link(
        &mut tx,
        &st.cfg.public_url,
        a.org_id,
        id,
        Some(a.user_id),
        "invite",
    )
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.invited")
            .target("user", id)
            .meta(json!({"email": email, "role": role.as_str()})),
    )
    .await?;
    tx.commit().await?;
    let user = load_user(&st, &a, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "user": user, "invite_url": url })),
    ))
}

#[derive(Deserialize)]
struct UpdateUser {
    display_name: Option<String>,
    org_role: Option<String>,
    status: Option<String>,
}

async fn count_owners(conn: &mut sqlx::PgConnection, org: Uuid) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE org_id = $1 AND org_role = 'owner' AND status = 'active' AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_one(&mut *conn)
    .await?)
}

async fn update_user(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<Json<UserRow>> {
    require_admin(&a)?;
    let target = load_user(&st, &a, id).await?;
    let target_role =
        OrgRole::parse(&target.org_role).ok_or_else(|| anyhow::anyhow!("bad role"))?;
    if target_role == OrgRole::Owner && a.role != OrgRole::Owner {
        return Err(ApiError::Forbidden("only an owner can change an owner"));
    }
    let mut tx = st.db.begin().await?;
    let mut changes = serde_json::Map::new();
    if let Some(n) = &req.display_name {
        let n = util::name("display_name", n, 100)?;
        sqlx::query("UPDATE users SET display_name = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(&n)
            .execute(&mut *tx)
            .await?;
        changes.insert("display_name".into(), json!(n));
    }
    if let Some(r) = &req.org_role {
        let role = OrgRole::parse(r).ok_or_else(|| ApiError::bad("invalid org_role"))?;
        check_role_change(&a, role)?;
        if id == a.user_id && role != a.role {
            return Err(ApiError::bad("you cannot change your own role"));
        }
        sqlx::query("UPDATE users SET org_role = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(role.as_str())
            .execute(&mut *tx)
            .await?;
        changes.insert("org_role".into(), json!(role.as_str()));
    }
    if let Some(s) = &req.status {
        if !["active", "disabled"].contains(&s.as_str()) {
            return Err(ApiError::bad("status must be active or disabled"));
        }
        if id == a.user_id && s == "disabled" {
            return Err(ApiError::bad("you cannot disable your own account"));
        }
        if s == "active" && target.status == "invited" {
            return Err(ApiError::bad("the user must accept their invitation first"));
        }
        sqlx::query("UPDATE users SET status = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(s)
            .execute(&mut *tx)
            .await?;
        if s == "disabled" {
            sqlx::query("WITH d AS (DELETE FROM sessions WHERE user_id = $1) UPDATE device_grants SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        changes.insert("status".into(), json!(s));
    }
    if count_owners(&mut tx, a.org_id).await? == 0 {
        return Err(ApiError::bad(
            "the organisation must keep at least one active owner",
        ));
    }
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.updated")
            .target("user", id)
            .meta(Value::Object(changes)),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_user(&st, &a, id).await?))
}

async fn reset_link(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    require_admin(&a)?;
    let target = load_user(&st, &a, id).await?;
    if target.org_role == "owner" && a.role != OrgRole::Owner {
        return Err(ApiError::Forbidden("only an owner can reset an owner"));
    }
    let purpose = if target.status == "invited" {
        "invite"
    } else {
        "password_reset"
    };
    let mut tx = st.db.begin().await?;
    let url = create_link(
        &mut tx,
        &st.cfg.public_url,
        a.org_id,
        id,
        Some(a.user_id),
        purpose,
    )
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.reset_link.created").target("user", id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "url": url, "purpose": purpose })))
}

async fn reset_mfa(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    let target = load_user(&st, &a, id).await?;
    if target.org_role == "owner" && a.role != OrgRole::Owner {
        return Err(ApiError::Forbidden("only an owner can reset an owner"));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM mfa_factors WHERE user_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("WITH d AS (DELETE FROM sessions WHERE user_id = $1) UPDATE device_grants SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.mfa.reset").target("user", id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_sessions(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    load_user(&st, &a, id).await?;
    let mut tx = st.db.begin().await?;
    sqlx::query("WITH d AS (DELETE FROM sessions WHERE user_id = $1) UPDATE device_grants SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("user.sessions.revoked").target("user", id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
struct Policies {
    mfa_requirement: String,
    session_idle_minutes: i32,
    session_absolute_hours: i32,
    public_links_enabled: bool,
    default_capture_mode: String,
    auto_redaction_default: bool,
    redaction_patterns: Value,
}

async fn get_policies(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Policies>> {
    require_admin(&a)?;
    let p = sqlx::query_as(
        "SELECT mfa_requirement, session_idle_minutes, session_absolute_hours, public_links_enabled,
                default_capture_mode, auto_redaction_default, redaction_patterns
         FROM org_policies WHERE org_id = $1",
    )
    .bind(a.org_id)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(p))
}

async fn put_policies(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(p): Json<Policies>,
) -> ApiResult<Json<Policies>> {
    require_admin(&a)?;
    if !["off", "admins", "all"].contains(&p.mfa_requirement.as_str()) {
        return Err(ApiError::bad("mfa_requirement must be off, admins or all"));
    }
    if !["full", "window", "region"].contains(&p.default_capture_mode.as_str()) {
        return Err(ApiError::bad(
            "default_capture_mode must be full, window or region",
        ));
    }
    if !(5..=43200).contains(&p.session_idle_minutes)
        || !(1..=8760).contains(&p.session_absolute_hours)
    {
        return Err(ApiError::bad("session timeouts out of range"));
    }
    let patterns = p
        .redaction_patterns
        .as_array()
        .ok_or_else(|| ApiError::bad("redaction_patterns must be a list"))?;
    if patterns.len() > 50
        || patterns
            .iter()
            .any(|v| !v.is_string() || v.as_str().is_some_and(|s| s.len() > 500))
    {
        return Err(ApiError::bad("redaction_patterns must be up to 50 strings"));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "UPDATE org_policies SET mfa_requirement = $2, session_idle_minutes = $3, session_absolute_hours = $4,
            public_links_enabled = $5, default_capture_mode = $6, auto_redaction_default = $7,
            redaction_patterns = $8, updated_at = now()
         WHERE org_id = $1",
    )
    .bind(a.org_id)
    .bind(&p.mfa_requirement)
    .bind(p.session_idle_minutes)
    .bind(p.session_absolute_hours)
    .bind(p.public_links_enabled)
    .bind(&p.default_capture_mode)
    .bind(p.auto_redaction_default)
    .bind(&p.redaction_patterns)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("org.policies.updated")
            .meta(serde_json::to_value(&p).map_err(anyhow::Error::from)?),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(p))
}

#[derive(Serialize, sqlx::FromRow)]
struct GroupRow {
    id: Uuid,
    name: String,
    source: String,
    external_id: Option<String>,
    grants_role: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    missing_since: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    last_synced_at: Option<OffsetDateTime>,
    member_ids: Vec<Uuid>,
}

const GROUP_SELECT: &str = "SELECT g.id, g.name, g.source, g.external_id, g.grants_role, g.missing_since, g.last_synced_at,
                COALESCE(array_agg(gm.user_id) FILTER (WHERE gm.user_id IS NOT NULL), '{}') AS member_ids
         FROM groups g LEFT JOIN group_members gm ON gm.group_id = g.id";

async fn list_groups(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Vec<GroupRow>>> {
    let rows = sqlx::query_as(&format!(
        "{GROUP_SELECT} WHERE g.org_id = $1 GROUP BY g.id ORDER BY g.name"
    ))
    .bind(a.org_id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct NewGroup {
    name: String,
}

async fn create_group(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<NewGroup>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_admin(&a)?;
    let name = util::name("name", &req.name, 100)?;
    let id = Uuid::now_v7();
    let mut tx = st.db.begin().await?;
    sqlx::query("INSERT INTO groups (id, org_id, name) VALUES ($1,$2,$3)")
        .bind(id)
        .bind(a.org_id)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("group.created")
            .target("group", id)
            .meta(json!({"name": name})),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "name": name, "source": "manual", "member_ids": []})),
    ))
}

async fn delete_group(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    let mut tx = st.db.begin().await?;
    let gone: Option<(String, String, Option<String>)> = sqlx::query_as(
        "DELETE FROM groups WHERE id = $1 AND org_id = $2 RETURNING source, name, external_id",
    )
    .bind(id)
    .bind(a.org_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((source, name, external_id)) = gone else {
        return Err(ApiError::NotFound);
    };
    sqlx::query(
        "DELETE FROM workspace_members WHERE principal_type = 'group' AND principal_id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        if source == "idp" {
            Event::new("idp.group.unlinked")
                .target("group", id)
                .meta(json!({"name": name, "external_id": external_id}))
        } else {
            Event::new("group.deleted").target("group", id)
        },
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Members {
    user_ids: Vec<Uuid>,
}

async fn set_group_members(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<Members>,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    let mut tx = st.db.begin().await?;
    let source: Option<String> =
        sqlx::query_scalar("SELECT source FROM groups WHERE id = $1 AND org_id = $2")
            .bind(id)
            .bind(a.org_id)
            .fetch_optional(&mut *tx)
            .await?;
    match source.as_deref() {
        None => return Err(ApiError::NotFound),
        Some("idp") => {
            return Err(ApiError::bad(
                "Members of Microsoft groups come from Microsoft",
            ));
        }
        Some(_) => {}
    }
    let valid: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE id = ANY($1) AND org_id = $2 AND deleted_at IS NULL",
    )
    .bind(&req.user_ids)
    .bind(a.org_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut unique = req.user_ids.clone();
    unique.sort();
    unique.dedup();
    if valid != unique.len() as i64 {
        return Err(ApiError::bad("unknown user in user_ids"));
    }
    sqlx::query("DELETE FROM group_members WHERE group_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO group_members (group_id, user_id) SELECT $1, unnest($2::uuid[])")
        .bind(id)
        .bind(&unique)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("group.members.set")
            .target("group", id)
            .meta(json!({"user_ids": unique})),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LinkGroup {
    external_id: String,
}

/// Start syncing a Microsoft group as an OpnRelay group.
async fn link_group(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<LinkGroup>,
) -> ApiResult<(StatusCode, Json<GroupRow>)> {
    require_admin(&a)?;
    let p = crate::routes::sso::sync_provider(&st, a.org_id).await?;
    let external = Uuid::parse_str(req.external_id.trim())
        .map_err(|_| ApiError::bad("external_id must be a Microsoft group ID"))?
        .hyphenated()
        .to_string();
    let dir = graph::get_group(&st, &p.app, &external)
        .await
        .map_err(group_sync::api_err)?;
    let id = Uuid::now_v7();
    let mut tx = st.db.begin().await?;
    let linked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM groups WHERE org_id = $1 AND source = 'idp' AND external_id = $2)",
    )
    .bind(a.org_id)
    .bind(&external)
    .fetch_one(&mut *tx)
    .await?;
    if linked {
        return Err(ApiError::Conflict("That group is already linked".into()));
    }
    let name = group_sync::free_name(&mut tx, a.org_id, &dir.display_name, None).await?;
    sqlx::query(
        "INSERT INTO groups (id, org_id, name, source, external_id) VALUES ($1,$2,$3,'idp',$4)",
    )
    .bind(id)
    .bind(a.org_id)
    .bind(&name)
    .bind(&external)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if let Err(e) = group_sync::sync_group(&st, &p, id, &a.audit()).await {
        // Do not leave a half-linked group behind.
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(id)
            .execute(&st.db)
            .await?;
        return Err(group_sync::api_err(e));
    }
    // Recorded only once the first sync worked, so a failed link leaves no trace.
    let mut tx = st.db.begin().await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("idp.group.linked")
            .target("group", id)
            .meta(json!({"name": name, "external_id": external})),
    )
    .await?;
    tx.commit().await?;
    let row = sqlx::query_as(&format!("{GROUP_SELECT} WHERE g.id = $1 GROUP BY g.id"))
        .bind(id)
        .fetch_one(&st.db)
        .await?;
    Ok((StatusCode::CREATED, Json(row)))
}
