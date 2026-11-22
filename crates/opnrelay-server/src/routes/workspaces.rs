// SPDX-License-Identifier: AGPL-3.0-only
//! Workspaces, their members, and folders.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::CurrentUser;
use crate::authz::{self, Perm, WsRole, require_admin};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces", get(list).post(create))
        .route(
            "/workspaces/{id}",
            get(get_one).patch(update).delete(remove),
        )
        .route(
            "/workspaces/{id}/members",
            get(get_members).put(set_members),
        )
        .route(
            "/workspaces/{id}/folders",
            get(list_folders).post(create_folder),
        )
        .route("/folders/{id}", patch(update_folder).delete(delete_folder))
}

#[derive(Serialize, sqlx::FromRow)]
struct WorkspaceRow {
    id: Uuid,
    name: String,
    description: String,
    legal_hold: bool,
    review_period_days: Option<i32>,
    guide_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[sqlx(skip)]
    my_permission: Option<Perm>,
}

const WS_COLS: &str = "w.id, w.name, w.description, w.legal_hold, w.review_period_days, w.created_at,
    (SELECT count(*) FROM guides g WHERE g.workspace_id = w.id AND g.deleted_at IS NULL) AS guide_count";

async fn list(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<Vec<WorkspaceRow>>> {
    let mut rows: Vec<WorkspaceRow> = sqlx::query_as(&format!(
        "SELECT {WS_COLS} FROM workspaces w WHERE w.org_id = $3 AND {} ORDER BY w.name",
        authz::VISIBLE_WORKSPACES_SQL
    ))
    .bind(a.user_id)
    .bind(authz::sees_all(&a))
    .bind(a.org_id)
    .fetch_all(&st.db)
    .await?;
    for r in &mut rows {
        r.my_permission = Some(authz::workspace_perm(&st.db, &a, r.id).await?);
    }
    Ok(Json(rows))
}

async fn get_one(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<WorkspaceRow>> {
    let perm = authz::workspace_perm(&st.db, &a, id).await?;
    perm.require(Perm::Read)?;
    let mut row: WorkspaceRow = sqlx::query_as(&format!(
        "SELECT {WS_COLS} FROM workspaces w WHERE w.id = $1"
    ))
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    row.my_permission = Some(perm);
    Ok(Json(row))
}

#[derive(Deserialize)]
struct NewWorkspace {
    name: String,
    #[serde(default)]
    description: String,
}

async fn create(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<NewWorkspace>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_admin(&a)?;
    let name = util::name("name", &req.name, 100)?;
    let description = opnrelay_core::validate::description(&req.description)?;
    let id = Uuid::now_v7();
    let mut tx = st.db.begin().await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name, description, created_by) VALUES ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(a.org_id)
        .bind(&name)
        .bind(&description)
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workspace_members (workspace_id, principal_type, principal_id, role) VALUES ($1,'user',$2,'manager')")
        .bind(id)
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("workspace.created")
            .target("workspace", id)
            .meta(json!({"name": name})),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "name": name, "description": description})),
    ))
}

#[derive(Deserialize)]
struct UpdateWorkspace {
    name: Option<String>,
    description: Option<String>,
    legal_hold: Option<bool>,
    /// Absent = unchanged, `null` = review reminders off, number = days.
    #[serde(default, deserialize_with = "double_option")]
    review_period_days: Option<Option<i32>>,
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<i32>>, D::Error> {
    Ok(Some(Option::<i32>::deserialize(d)?))
}

async fn update(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateWorkspace>,
) -> ApiResult<Json<WorkspaceRow>> {
    authz::workspace_perm(&st.db, &a, id)
        .await?
        .require(Perm::Manage)?;
    if req.legal_hold.is_some() {
        require_admin(&a)?;
    }
    if let Some(Some(d)) = req.review_period_days
        && !(1..=3650).contains(&d)
    {
        return Err(ApiError::bad(
            "review_period_days must be between 1 and 3650, or null to turn reminders off",
        ));
    }
    let mut tx = st.db.begin().await?;
    let mut changes = serde_json::Map::new();
    if let Some(n) = &req.name {
        let n = util::name("name", n, 100)?;
        sqlx::query("UPDATE workspaces SET name = $2 WHERE id = $1")
            .bind(id)
            .bind(&n)
            .execute(&mut *tx)
            .await?;
        changes.insert("name".into(), json!(n));
    }
    if let Some(d) = &req.description {
        let d = opnrelay_core::validate::description(d)?;
        sqlx::query("UPDATE workspaces SET description = $2 WHERE id = $1")
            .bind(id)
            .bind(&d)
            .execute(&mut *tx)
            .await?;
        changes.insert("description".into(), json!(true));
    }
    if let Some(h) = req.legal_hold {
        sqlx::query("UPDATE workspaces SET legal_hold = $2 WHERE id = $1")
            .bind(id)
            .bind(h)
            .execute(&mut *tx)
            .await?;
        changes.insert("legal_hold".into(), json!(h));
    }
    if let Some(p) = req.review_period_days {
        sqlx::query("UPDATE workspaces SET review_period_days = $2 WHERE id = $1")
            .bind(id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
        changes.insert("review_period_days".into(), json!(p));
    }
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("workspace.updated")
            .target("workspace", id)
            .meta(Value::Object(changes)),
    )
    .await?;
    tx.commit().await?;
    get_one(State(st), CurrentUser(a), Path(id)).await
}

async fn remove(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    authz::workspace_perm(&st.db, &a, id)
        .await?
        .require(Perm::Manage)?;
    let mut tx = st.db.begin().await?;
    let held: bool = sqlx::query_scalar(
        "SELECT w.legal_hold OR EXISTS (SELECT 1 FROM guides g WHERE g.workspace_id = w.id AND g.legal_hold AND g.deleted_at IS NULL)
         FROM workspaces w WHERE w.id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if held {
        return Err(ApiError::Conflict(
            "this workspace is under legal hold and cannot be deleted".into(),
        ));
    }
    sqlx::query("UPDATE workspaces SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("workspace.deleted").target("workspace", id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone)]
struct Member {
    principal_type: String,
    principal_id: Uuid,
    role: String,
    #[sqlx(default)]
    #[serde(default, skip_deserializing)]
    name: Option<String>,
}

async fn get_members(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Member>>> {
    authz::workspace_perm(&st.db, &a, id)
        .await?
        .require(Perm::Read)?;
    let rows = sqlx::query_as(
        "SELECT wm.principal_type, wm.principal_id, wm.role,
                COALESCE(u.display_name, g.name) AS name
         FROM workspace_members wm
         LEFT JOIN users u ON wm.principal_type = 'user' AND u.id = wm.principal_id
         LEFT JOIN groups g ON wm.principal_type = 'group' AND g.id = wm.principal_id
         WHERE wm.workspace_id = $1 ORDER BY name",
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct SetMembers {
    members: Vec<Member>,
}

async fn set_members(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<SetMembers>,
) -> ApiResult<Json<Vec<Member>>> {
    authz::workspace_perm(&st.db, &a, id)
        .await?
        .require(Perm::Manage)?;
    if req.members.len() > 1000 {
        return Err(ApiError::bad("too many members"));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for m in &req.members {
        WsRole::parse(&m.role)
            .ok_or_else(|| ApiError::bad("role must be manager, editor or viewer"))?;
        let ok: Option<bool> = match m.principal_type.as_str() {
            "user" => sqlx::query_scalar(
                "SELECT true FROM users WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL",
            ),
            "group" => sqlx::query_scalar("SELECT true FROM groups WHERE id = $1 AND org_id = $2"),
            _ => return Err(ApiError::bad("principal_type must be user or group")),
        }
        .bind(m.principal_id)
        .bind(a.org_id)
        .fetch_optional(&mut *tx)
        .await?;
        if ok.is_none() {
            return Err(ApiError::bad("unknown user or group"));
        }
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, principal_type, principal_id, role) VALUES ($1,$2,$3,$4)
             ON CONFLICT (workspace_id, principal_type, principal_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(id)
        .bind(&m.principal_type)
        .bind(m.principal_id)
        .bind(&m.role)
        .execute(&mut *tx)
        .await?;
    }
    // A non-admin manager must not lock themselves out.
    if !a.role.is_admin() {
        let still_manager: Option<bool> = sqlx::query_scalar(
            "SELECT true FROM workspace_members WHERE workspace_id = $1 AND principal_type = 'user' AND principal_id = $2 AND role = 'manager'",
        )
        .bind(id)
        .bind(a.user_id)
        .fetch_optional(&mut *tx)
        .await?;
        if still_manager.is_none() {
            return Err(ApiError::bad("you cannot remove your own manager role"));
        }
    }
    let summary: Vec<Value> = req
        .members
        .iter()
        .map(|m| json!({"type": m.principal_type, "id": m.principal_id, "role": m.role}))
        .collect();
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("workspace.members.set")
            .target("workspace", id)
            .meta(json!({"members": summary})),
    )
    .await?;
    tx.commit().await?;
    get_members(State(st), CurrentUser(a), Path(id)).await
}

#[derive(Serialize, sqlx::FromRow)]
struct FolderRow {
    id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    position: i32,
}

async fn list_folders(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<FolderRow>>> {
    authz::workspace_perm(&st.db, &a, id)
        .await?
        .require(Perm::Read)?;
    let rows = sqlx::query_as(
        "SELECT id, workspace_id, parent_id, name, position FROM folders WHERE workspace_id = $1 ORDER BY position, name",
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct NewFolder {
    name: String,
    parent_id: Option<Uuid>,
}

async fn check_parent(st: &AppState, workspace_id: Uuid, parent: Option<Uuid>) -> ApiResult<()> {
    if let Some(p) = parent {
        let ok: Option<bool> =
            sqlx::query_scalar("SELECT true FROM folders WHERE id = $1 AND workspace_id = $2")
                .bind(p)
                .bind(workspace_id)
                .fetch_optional(&st.db)
                .await?;
        if ok.is_none() {
            return Err(ApiError::bad("parent folder not found in this workspace"));
        }
    }
    Ok(())
}

async fn create_folder(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(ws): Path<Uuid>,
    Json(req): Json<NewFolder>,
) -> ApiResult<(StatusCode, Json<FolderRow>)> {
    authz::workspace_perm(&st.db, &a, ws)
        .await?
        .require(Perm::Edit)?;
    let name = util::name("name", &req.name, 100)?;
    check_parent(&st, ws, req.parent_id).await?;
    let id = Uuid::now_v7();
    let mut tx = st.db.begin().await?;
    let row: FolderRow = sqlx::query_as(
        "INSERT INTO folders (id, workspace_id, parent_id, name, position)
         VALUES ($1,$2,$3,$4, COALESCE((SELECT max(position) + 1 FROM folders WHERE workspace_id = $2), 0))
         RETURNING id, workspace_id, parent_id, name, position",
    )
    .bind(id)
    .bind(ws)
    .bind(req.parent_id)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("folder.created")
            .target("folder", id)
            .meta(json!({"workspace_id": ws, "name": name})),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn folder_workspace(st: &AppState, a: &crate::auth::Actor, id: Uuid) -> ApiResult<Uuid> {
    let ws: Uuid = sqlx::query_scalar(
        "SELECT f.workspace_id FROM folders f JOIN workspaces w ON w.id = f.workspace_id WHERE f.id = $1 AND w.org_id = $2",
    )
    .bind(id)
    .bind(a.org_id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(ws)
}

/// Body: `{name?, position?, parent_id?}`; `"parent_id": null` moves the
/// folder to the top level, an absent key leaves it unchanged.
async fn update_folder(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    Json(req): Json<Value>,
) -> ApiResult<Json<FolderRow>> {
    let ws = folder_workspace(&st, &a, id).await?;
    authz::workspace_perm(&st.db, &a, ws)
        .await?
        .require(Perm::Edit)?;
    let mut tx = st.db.begin().await?;
    if let Some(n) = req.get("name").and_then(Value::as_str) {
        let n = util::name("name", n, 100)?;
        sqlx::query("UPDATE folders SET name = $2 WHERE id = $1")
            .bind(id)
            .bind(n)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(p) = req.get("position").and_then(Value::as_i64) {
        sqlx::query("UPDATE folders SET position = $2 WHERE id = $1")
            .bind(id)
            .bind(p as i32)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(p) = req.get("parent_id") {
        let parent: Option<Uuid> =
            serde_json::from_value(p.clone()).map_err(|_| ApiError::bad("invalid parent_id"))?;
        if parent == Some(id) {
            return Err(ApiError::bad("a folder cannot be its own parent"));
        }
        check_parent(&st, ws, parent).await?;
        if let Some(p) = parent {
            // Reject cycles: the new parent must not be a descendant.
            let cycle: Option<bool> = sqlx::query_scalar(
                "WITH RECURSIVE d AS (SELECT id FROM folders WHERE parent_id = $1
                   UNION SELECT f.id FROM folders f JOIN d ON f.parent_id = d.id)
                 SELECT true FROM d WHERE id = $2",
            )
            .bind(id)
            .bind(p)
            .fetch_optional(&mut *tx)
            .await?;
            if cycle.is_some() {
                return Err(ApiError::bad("cannot move a folder into its own subfolder"));
            }
        }
        sqlx::query("UPDATE folders SET parent_id = $2 WHERE id = $1")
            .bind(id)
            .bind(parent)
            .execute(&mut *tx)
            .await?;
    }
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("folder.updated").target("folder", id),
    )
    .await?;
    let row = sqlx::query_as(
        "SELECT id, workspace_id, parent_id, name, position FROM folders WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(row))
}

async fn delete_folder(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let ws = folder_workspace(&st, &a, id).await?;
    authz::workspace_perm(&st.db, &a, ws)
        .await?
        .require(Perm::Edit)?;
    let mut tx = st.db.begin().await?;
    // Guides inside are kept and moved to the workspace root (FK ON DELETE SET NULL).
    sqlx::query("DELETE FROM folders WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("folder.deleted").target("folder", id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
