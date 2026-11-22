// SPDX-License-Identifier: AGPL-3.0-only
//! Guides, their steps, publishing and version history.

use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use opnrelay_core::{Classification, GuideStatus, Step, StepKind, validate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, QueryBuilder};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::{Actor, CurrentUser};
use crate::authz::{self, Perm};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/guides", get(list).post(create))
        .route("/guides/{id}", get(get_one).patch(update).delete(remove))
        .route("/guides/{id}/steps", put(put_steps))
        .route("/guides/{id}/publish", post(publish))
        .route("/guides/{id}/reviewed", post(mark_reviewed))
        .route("/guides/{id}/duplicate", post(duplicate))
        .route("/guides/{id}/versions", get(list_versions))
        .route("/guides/{id}/versions/{v}", get(get_version))
        .route("/guides/{id}/versions/{v}/restore", post(restore_version))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct GuideSummary {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub classification: String,
    pub source: String,
    pub revision: i32,
    pub current_version: i32,
    pub tags: Vec<String>,
    pub legal_hold: bool,
    pub created_by: Option<Uuid>,
    pub author_name: Option<String>,
    pub step_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub reviewed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub review_due_at: Option<OffsetDateTime>,
    pub needs_review: bool,
}

/// When a published guide is next due for review (NULL if it never is).
/// Needs the `g` (guides) and `w` (workspaces) aliases. A macro so that it can
/// be spliced into `concat!` for the column list as well as the filter.
macro_rules! review_due_sql {
    () => {
        "(CASE WHEN g.status = 'published' AND w.review_period_days IS NOT NULL
        THEN g.reviewed_at + make_interval(days => w.review_period_days) END)"
    };
}

/// Needs the `g` (guides) and `w` (workspaces) aliases; also join `users u`.
const SUMMARY_COLS: &str = concat!(
    "g.id, g.workspace_id, g.folder_id, g.title, g.description, g.status, g.classification,
    g.source, g.revision, g.current_version, g.tags, g.legal_hold, g.created_by, u.display_name AS author_name,
    (SELECT count(*) FROM steps s WHERE s.guide_id = g.id AND s.kind = 'action') AS step_count,
    g.created_at, g.updated_at, g.published_at, g.reviewed_at,
    ",
    review_due_sql!(),
    " AS review_due_at,
    COALESCE(",
    review_due_sql!(),
    " < now(), false) AS needs_review"
);

#[derive(Deserialize)]
struct ListQuery {
    workspace_id: Option<Uuid>,
    folder_id: Option<Uuid>,
    q: Option<String>,
    status: Option<String>,
    needs_review: Option<bool>,
    limit: Option<i64>,
}

async fn list(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<GuideSummary>>> {
    let mut qb = QueryBuilder::new(format!(
        "SELECT {SUMMARY_COLS} FROM guides g JOIN workspaces w ON w.id = g.workspace_id
         LEFT JOIN users u ON u.id = g.created_by
         WHERE g.deleted_at IS NULL AND w.deleted_at IS NULL AND w.org_id = "
    ));
    qb.push_bind(a.org_id);
    if !authz::sees_all(&a) {
        qb.push(
            " AND EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = w.id AND (
                (wm.principal_type = 'user' AND wm.principal_id = ",
        );
        qb.push_bind(a.user_id);
        qb.push(") OR (wm.principal_type = 'group' AND wm.principal_id IN (SELECT group_id FROM group_members WHERE user_id = ");
        qb.push_bind(a.user_id);
        qb.push("))))");
    }
    if let Some(ws) = q.workspace_id {
        qb.push(" AND g.workspace_id = ").push_bind(ws);
    }
    if let Some(f) = q.folder_id {
        qb.push(" AND g.folder_id = ").push_bind(f);
    }
    if let Some(s) = q.status.as_deref().filter(|s| !s.is_empty()) {
        GuideStatus::parse(s).ok_or_else(|| ApiError::bad("invalid status"))?;
        qb.push(" AND g.status = ").push_bind(s.to_string());
    } else {
        qb.push(" AND g.status <> 'archived'");
    }
    if q.needs_review == Some(true) {
        // Only workspaces where the caller can act on the flag (edit or
        // manage): admins and owners everywhere, members as editor/manager.
        qb.push(concat!(" AND ", review_due_sql!(), " < now()"));
        if a.role == authz::OrgRole::Member {
            qb.push(
                " AND EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = w.id
                  AND wm.role IN ('editor', 'manager') AND (
                    (wm.principal_type = 'user' AND wm.principal_id = ",
            );
            qb.push_bind(a.user_id);
            qb.push(") OR (wm.principal_type = 'group' AND wm.principal_id IN (SELECT group_id FROM group_members WHERE user_id = ");
            qb.push_bind(a.user_id);
            qb.push("))))");
        } else if !a.role.is_admin() {
            qb.push(" AND false");
        }
    }
    let search = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if let Some(term) = search {
        if term.len() > 200 {
            return Err(ApiError::bad("search is too long"));
        }
        let like = format!(
            "%{}%",
            term.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        qb.push(" AND (g.search @@ websearch_to_tsquery('english', ")
            .push_bind(term.to_string())
            .push(") OR g.title ILIKE ")
            .push_bind(like)
            .push(")");
        qb.push(" ORDER BY ts_rank(g.search, websearch_to_tsquery('english', ")
            .push_bind(term.to_string())
            .push(")) DESC, g.updated_at DESC");
    } else if q.needs_review == Some(true) {
        qb.push(concat!(" ORDER BY ", review_due_sql!(), " ASC, g.id"));
    } else {
        qb.push(" ORDER BY g.updated_at DESC");
    }
    qb.push(" LIMIT ")
        .push_bind(q.limit.unwrap_or(200).clamp(1, 500));
    let rows = qb
        .build_query_as::<GuideSummary>()
        .fetch_all(&st.db)
        .await?;
    Ok(Json(rows))
}

#[derive(Serialize)]
pub struct GuideFull {
    #[serde(flatten)]
    pub guide: GuideSummary,
    pub steps: Vec<Step>,
    pub my_permission: Perm,
}

#[derive(sqlx::FromRow)]
struct StepRow {
    id: Uuid,
    kind: String,
    title: String,
    body_md: String,
    action: Option<Value>,
    media_id: Option<Uuid>,
    annotations: Value,
}

impl StepRow {
    fn into_step(self) -> ApiResult<Step> {
        Ok(Step {
            id: self.id,
            kind: StepKind::parse(&self.kind).ok_or_else(|| anyhow::anyhow!("bad step kind"))?,
            title: self.title,
            body_md: self.body_md,
            action: self
                .action
                .map(serde_json::from_value)
                .transpose()
                .map_err(anyhow::Error::from)?,
            media_id: self.media_id,
            annotations: serde_json::from_value(self.annotations).unwrap_or_default(),
        })
    }
}

pub async fn load_summary(conn: &mut PgConnection, id: Uuid) -> ApiResult<GuideSummary> {
    Ok(sqlx::query_as(&format!(
        "SELECT {SUMMARY_COLS} FROM guides g JOIN workspaces w ON w.id = g.workspace_id
         LEFT JOIN users u ON u.id = g.created_by WHERE g.id = $1"
    ))
    .bind(id)
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn load_steps(conn: &mut PgConnection, guide_id: Uuid) -> ApiResult<Vec<Step>> {
    let rows: Vec<StepRow> = sqlx::query_as(
        "SELECT id, kind, title, body_md, action, media_id, annotations FROM steps WHERE guide_id = $1 ORDER BY position",
    )
    .bind(guide_id)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter().map(StepRow::into_step).collect()
}

async fn load_full(st: &AppState, id: Uuid, perm: Perm) -> ApiResult<GuideFull> {
    let mut conn = st.db.acquire().await?;
    Ok(GuideFull {
        guide: load_summary(&mut conn, id).await?,
        steps: load_steps(&mut conn, id).await?,
        my_permission: perm,
    })
}

async fn get_one(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<GuideFull>> {
    let (perm, _) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Read)?;
    let full = load_full(&st, id, perm).await?;
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("guide.viewed").target("guide", id),
    )
    .await?;
    Ok(Json(full))
}

#[derive(Deserialize)]
struct NewGuide {
    workspace_id: Uuid,
    folder_id: Option<Uuid>,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(default)]
    source: Option<String>,
}

async fn check_folder(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    folder: Option<Uuid>,
) -> ApiResult<()> {
    if let Some(f) = folder {
        let ok: Option<bool> =
            sqlx::query_scalar("SELECT true FROM folders WHERE id = $1 AND workspace_id = $2")
                .bind(f)
                .bind(workspace_id)
                .fetch_optional(&mut *conn)
                .await?;
        if ok.is_none() {
            return Err(ApiError::bad("folder not found in this workspace"));
        }
    }
    Ok(())
}

async fn create(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(req): Json<NewGuide>,
) -> ApiResult<(StatusCode, Json<GuideFull>)> {
    let perm = authz::workspace_perm(&st.db, &a, req.workspace_id).await?;
    perm.require(Perm::Edit)?;
    let title = validate::guide_title(&req.title)?;
    let description = validate::description(&req.description)?;
    let source = req.source.as_deref().unwrap_or("manual");
    if !["manual", "desktop", "extension"].contains(&source) {
        return Err(ApiError::bad("invalid source"));
    }
    let id = Uuid::now_v7();
    let mut tx = st.db.begin().await?;
    check_folder(&mut tx, req.workspace_id, req.folder_id).await?;
    sqlx::query(
        "INSERT INTO guides (id, workspace_id, folder_id, title, description, source, created_by, updated_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$7)",
    )
    .bind(id)
    .bind(req.workspace_id)
    .bind(req.folder_id)
    .bind(&title)
    .bind(&description)
    .bind(source)
    .bind(a.user_id)
    .execute(&mut *tx)
    .await?;
    write_steps(&mut tx, &a, id, &req.steps).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.created")
            .target("guide", id)
            .meta(json!({"workspace_id": req.workspace_id, "title": title, "source": source})),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(load_full(&st, id, perm).await?)))
}

/// Replace all steps of a guide (inside a transaction) and refresh search.
async fn write_steps(
    conn: &mut PgConnection,
    a: &Actor,
    guide_id: Uuid,
    steps: &[Step],
) -> ApiResult<()> {
    validate::steps(steps)?;
    let media: Vec<Uuid> = steps
        .iter()
        .filter_map(|s| s.media_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if !media.is_empty() {
        // Media must be this guide's, or freshly uploaded by this user.
        let ok: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM media WHERE id = ANY($1) AND org_id = $2 AND kind = 'image'
               AND (guide_id = $3 OR (guide_id IS NULL AND created_by = $4))",
        )
        .bind(&media)
        .bind(a.org_id)
        .bind(guide_id)
        .bind(a.user_id)
        .fetch_one(&mut *conn)
        .await?;
        if ok != media.len() as i64 {
            return Err(ApiError::bad(
                "a step references an image that does not exist or belongs to another guide",
            ));
        }
        sqlx::query("UPDATE media SET guide_id = $1 WHERE id = ANY($2)")
            .bind(guide_id)
            .bind(&media)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("DELETE FROM steps WHERE guide_id = $1")
        .bind(guide_id)
        .execute(&mut *conn)
        .await?;
    let mut text = String::new();
    for (pos, s) in steps.iter().enumerate() {
        let action = s
            .action
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(anyhow::Error::from)?;
        sqlx::query(
            "INSERT INTO steps (id, guide_id, position, kind, title, body_md, action, media_id, annotations)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(s.id)
        .bind(guide_id)
        .bind(pos as i32)
        .bind(s.kind.as_str())
        .bind(&s.title)
        .bind(&s.body_md)
        .bind(action)
        .bind(s.media_id)
        .bind(serde_json::to_value(&s.annotations).map_err(anyhow::Error::from)?)
        .execute(&mut *conn)
        .await
        .map_err(|e| match ApiError::from(e) {
            ApiError::Conflict(_) => ApiError::Conflict("duplicate step id; step ids must be unique".into()),
            other => other,
        })?;
        text.push_str(&s.title);
        text.push(' ');
        text.push_str(&s.body_md);
        text.push(' ');
        if let Some(l) = s.action.as_ref().and_then(|a| a.target_label.as_deref()) {
            text.push_str(l);
            text.push(' ');
        }
    }
    sqlx::query(
        "UPDATE guides SET search =
            setweight(to_tsvector('english', title), 'A') ||
            setweight(to_tsvector('english', description), 'B') ||
            setweight(to_tsvector('english', $2), 'C')
         WHERE id = $1",
    )
    .bind(guide_id)
    .bind(text)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Lock the guide row and check the optional `If-Match` revision.
async fn lock_revision(conn: &mut PgConnection, id: Uuid, expected: Option<i32>) -> ApiResult<i32> {
    let rev: i32 = sqlx::query_scalar("SELECT revision FROM guides WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if expected.is_some_and(|e| e != rev) {
        return Err(ApiError::PreconditionFailed);
    }
    Ok(rev)
}

async fn bump(conn: &mut PgConnection, id: Uuid, a: &Actor) -> ApiResult<()> {
    sqlx::query("UPDATE guides SET revision = revision + 1, updated_at = now(), updated_by = $2 WHERE id = $1")
        .bind(id)
        .bind(a.user_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct PutSteps {
    steps: Vec<Step>,
}

async fn put_steps(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<PutSteps>,
) -> ApiResult<Json<GuideFull>> {
    let (perm, _) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Edit)?;
    let mut tx = st.db.begin().await?;
    lock_revision(&mut tx, id, util::if_match(&headers)?).await?;
    write_steps(&mut tx, &a, id, &req.steps).await?;
    bump(&mut tx, id, &a).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.steps.updated")
            .target("guide", id)
            .meta(json!({"steps": req.steps.len()})),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_full(&st, id, perm).await?))
}

#[derive(Deserialize)]
struct UpdateGuide {
    title: Option<String>,
    description: Option<String>,
    classification: Option<String>,
    tags: Option<Vec<String>>,
    status: Option<String>,
    /// Present with `null` = move to workspace root.
    #[serde(default, deserialize_with = "double_option")]
    folder_id: Option<Option<Uuid>>,
    legal_hold: Option<bool>,
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<Uuid>>, D::Error> {
    Ok(Some(Option::<Uuid>::deserialize(d)?))
}

async fn update(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<UpdateGuide>,
) -> ApiResult<Json<GuideFull>> {
    let (perm, ws) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Edit)?;
    if req.legal_hold.is_some() {
        authz::require_admin(&a)?;
    }
    let mut tx = st.db.begin().await?;
    lock_revision(&mut tx, id, util::if_match(&headers)?).await?;
    let mut changes = serde_json::Map::new();
    if let Some(t) = &req.title {
        let t = validate::guide_title(t)?;
        sqlx::query("UPDATE guides SET title = $2 WHERE id = $1")
            .bind(id)
            .bind(&t)
            .execute(&mut *tx)
            .await?;
        changes.insert("title".into(), json!(t));
    }
    if let Some(d) = &req.description {
        let d = validate::description(d)?;
        sqlx::query("UPDATE guides SET description = $2 WHERE id = $1")
            .bind(id)
            .bind(d)
            .execute(&mut *tx)
            .await?;
        changes.insert("description".into(), json!(true));
    }
    if let Some(c) = &req.classification {
        let c = Classification::parse(c).ok_or_else(|| ApiError::bad("invalid classification"))?;
        sqlx::query("UPDATE guides SET classification = $2 WHERE id = $1")
            .bind(id)
            .bind(c.as_str())
            .execute(&mut *tx)
            .await?;
        changes.insert("classification".into(), json!(c.as_str()));
    }
    if let Some(tags) = &req.tags {
        let mut clean: Vec<String> = tags
            .iter()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty() && t.chars().count() <= 40)
            .collect();
        clean.sort();
        clean.dedup();
        if clean.len() > 30 {
            return Err(ApiError::bad("too many tags (max 30)"));
        }
        sqlx::query("UPDATE guides SET tags = $2 WHERE id = $1")
            .bind(id)
            .bind(&clean)
            .execute(&mut *tx)
            .await?;
        changes.insert("tags".into(), json!(clean));
    }
    if let Some(s) = &req.status {
        let s = GuideStatus::parse(s).ok_or_else(|| ApiError::bad("invalid status"))?;
        if s == GuideStatus::Published {
            return Err(ApiError::bad("use the publish endpoint to publish"));
        }
        sqlx::query("UPDATE guides SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(s.as_str())
            .execute(&mut *tx)
            .await?;
        changes.insert("status".into(), json!(s.as_str()));
    }
    if let Some(f) = req.folder_id {
        check_folder(&mut tx, ws, f).await?;
        sqlx::query("UPDATE guides SET folder_id = $2 WHERE id = $1")
            .bind(id)
            .bind(f)
            .execute(&mut *tx)
            .await?;
        changes.insert("folder_id".into(), json!(f));
    }
    if let Some(h) = req.legal_hold {
        sqlx::query("UPDATE guides SET legal_hold = $2 WHERE id = $1")
            .bind(id)
            .bind(h)
            .execute(&mut *tx)
            .await?;
        changes.insert("legal_hold".into(), json!(h));
    }
    if req.title.is_some() || req.description.is_some() {
        // Refresh the title/description part of the search vector.
        let steps = load_steps(&mut tx, id).await?;
        write_search_only(&mut tx, id, &steps).await?;
    }
    bump(&mut tx, id, &a).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.updated")
            .target("guide", id)
            .meta(Value::Object(changes)),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_full(&st, id, perm).await?))
}

async fn write_search_only(conn: &mut PgConnection, id: Uuid, steps: &[Step]) -> ApiResult<()> {
    let text: String = steps
        .iter()
        .map(|s| {
            format!(
                "{} {} {}",
                s.title,
                s.body_md,
                s.action
                    .as_ref()
                    .and_then(|a| a.target_label.clone())
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    sqlx::query(
        "UPDATE guides SET search = setweight(to_tsvector('english', title), 'A') ||
            setweight(to_tsvector('english', description), 'B') || setweight(to_tsvector('english', $2), 'C') WHERE id = $1",
    )
    .bind(id)
    .bind(text)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn remove(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let (perm, ws) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Edit)?;
    let mut tx = st.db.begin().await?;
    let held: bool = sqlx::query_scalar(
        "SELECT g.legal_hold OR w.legal_hold FROM guides g JOIN workspaces w ON w.id = g.workspace_id WHERE g.id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if held {
        return Err(ApiError::Conflict(
            "this guide is under legal hold and cannot be deleted".into(),
        ));
    }
    sqlx::query("UPDATE guides SET deleted_at = now(), updated_by = $2 WHERE id = $1")
        .bind(id)
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE share_links SET revoked_at = now() WHERE guide_id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.deleted")
            .target("guide", id)
            .meta(json!({"workspace_id": ws})),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
struct PublishReq {
    #[serde(default)]
    note: String,
}

async fn publish(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Option<Json<PublishReq>>,
) -> ApiResult<Json<GuideFull>> {
    let (perm, _) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Edit)?;
    let note: String = body
        .map(|b| b.0.note)
        .unwrap_or_default()
        .chars()
        .take(500)
        .collect();
    let mut tx = st.db.begin().await?;
    lock_revision(&mut tx, id, util::if_match(&headers)?).await?;
    let g = load_summary(&mut tx, id).await?;
    let steps = load_steps(&mut tx, id).await?;
    let version = g.current_version + 1;
    let snapshot = json!({
        "title": g.title,
        "description": g.description,
        "classification": g.classification,
        "tags": g.tags,
        "steps": steps,
    });
    sqlx::query("INSERT INTO guide_versions (guide_id, version, snapshot, note, created_by) VALUES ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(version)
        .bind(&snapshot)
        .bind(&note)
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE guides SET status = 'published', current_version = $2, published_at = now(), reviewed_at = now() WHERE id = $1")
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
    bump(&mut tx, id, &a).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.published")
            .target("guide", id)
            .meta(json!({"version": version})),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_full(&st, id, perm).await?))
}

/// "Still accurate": restart the review clock without a new version. Does not
/// touch `revision`, so concurrent editors are not disturbed.
async fn mark_reviewed(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<GuideFull>> {
    let (perm, _) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Edit)?;
    let mut tx = st.db.begin().await?;
    // Row lock first (via the UPDATE), then the audit lock inside
    // `audit::record`: the same order as the rest of this file.
    let updated: Option<Uuid> = sqlx::query_scalar(
        "UPDATE guides SET reviewed_at = now()
         WHERE id = $1 AND status = 'published' AND deleted_at IS NULL RETURNING id",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if updated.is_none() {
        return Err(ApiError::bad(
            "Only published guides can be marked as reviewed",
        ));
    }
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.reviewed").target("guide", id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_full(&st, id, perm).await?))
}

#[derive(Serialize, sqlx::FromRow)]
struct VersionRow {
    version: i32,
    note: String,
    created_by: Option<Uuid>,
    author_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

async fn list_versions(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<VersionRow>>> {
    authz::guide_perm(&st.db, &a, id)
        .await?
        .0
        .require(Perm::Read)?;
    let rows = sqlx::query_as(
        "SELECT v.version, v.note, v.created_by, u.display_name AS author_name, v.created_at
         FROM guide_versions v LEFT JOIN users u ON u.id = v.created_by WHERE v.guide_id = $1 ORDER BY v.version DESC",
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(rows))
}

async fn get_version(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path((id, v)): Path<(Uuid, i32)>,
) -> ApiResult<Json<Value>> {
    authz::guide_perm(&st.db, &a, id)
        .await?
        .0
        .require(Perm::Read)?;
    let snap: Value = sqlx::query_scalar(
        "SELECT snapshot FROM guide_versions WHERE guide_id = $1 AND version = $2",
    )
    .bind(id)
    .bind(v)
    .fetch_optional(&st.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("guide.version.viewed")
            .target("guide", id)
            .meta(json!({"version": v})),
    )
    .await?;
    Ok(Json(json!({"version": v, "snapshot": snap})))
}

async fn restore_version(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path((id, v)): Path<(Uuid, i32)>,
) -> ApiResult<Json<GuideFull>> {
    let (perm, _) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Edit)?;
    let mut tx = st.db.begin().await?;
    lock_revision(&mut tx, id, None).await?;
    let snap: Value = sqlx::query_scalar(
        "SELECT snapshot FROM guide_versions WHERE guide_id = $1 AND version = $2",
    )
    .bind(id)
    .bind(v)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;
    let steps: Vec<Step> =
        serde_json::from_value(snap["steps"].clone()).map_err(anyhow::Error::from)?;
    let title = snap["title"].as_str().unwrap_or_default().to_string();
    let description = snap["description"].as_str().unwrap_or_default().to_string();
    sqlx::query("UPDATE guides SET title = $2, description = $3, status = 'draft' WHERE id = $1")
        .bind(id)
        .bind(&title)
        .bind(&description)
        .execute(&mut *tx)
        .await?;
    write_steps(&mut tx, &a, id, &steps).await?;
    bump(&mut tx, id, &a).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.version.restored")
            .target("guide", id)
            .meta(json!({"version": v})),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_full(&st, id, perm).await?))
}

#[derive(Deserialize, Default)]
struct DuplicateReq {
    workspace_id: Option<Uuid>,
    title: Option<String>,
}

#[derive(sqlx::FromRow)]
struct MediaCopy {
    id: Uuid,
    mime: String,
    storage_key: String,
    dek_wrapped: Vec<u8>,
    width: Option<i32>,
    height: Option<i32>,
}

async fn duplicate(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    body: Option<Json<DuplicateReq>>,
) -> ApiResult<(StatusCode, Json<GuideFull>)> {
    let req = body.map(|b| b.0).unwrap_or_default();
    let (perm, ws) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Read)?;
    let target_ws = req.workspace_id.unwrap_or(ws);
    let target_perm = authz::workspace_perm(&st.db, &a, target_ws).await?;
    target_perm.require(Perm::Edit)?;

    let mut conn = st.db.acquire().await?;
    let g = load_summary(&mut conn, id).await?;
    let mut steps = load_steps(&mut conn, id).await?;
    drop(conn);
    let new_id = Uuid::now_v7();
    let title =
        validate::guide_title(&req.title.unwrap_or_else(|| format!("Copy of {}", g.title)))?;

    // Copy screenshots so each guide owns its media (re-encrypted under new ids).
    let mut copies = Vec::new();
    for s in &mut steps {
        s.id = Uuid::now_v7();
        if let Some(mid) = s.media_id {
            let m: MediaCopy = sqlx::query_as(
                "SELECT id, mime, storage_key, dek_wrapped, width, height FROM media WHERE id = $1",
            )
            .bind(mid)
            .fetch_one(&st.db)
            .await?;
            let bytes = st.storage.get(m.id, &m.storage_key, &m.dek_wrapped).await?;
            let copy_id = Uuid::now_v7();
            let digest = opnrelay_crypto::sha256(&bytes).to_vec();
            let len = bytes.len() as i64;
            let obj = st.storage.put("media", copy_id, bytes).await?;
            copies.push((copy_id, m, obj, digest, len));
            s.media_id = Some(copy_id);
        }
    }
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "INSERT INTO guides (id, workspace_id, title, description, classification, tags, source, created_by, updated_by)
         VALUES ($1,$2,$3,$4,$5,$6,'manual',$7,$7)",
    )
    .bind(new_id)
    .bind(target_ws)
    .bind(&title)
    .bind(&g.description)
    .bind(&g.classification)
    .bind(&g.tags)
    .bind(a.user_id)
    .execute(&mut *tx)
    .await?;
    for (copy_id, m, obj, digest, len) in copies {
        sqlx::query(
            "INSERT INTO media (id, org_id, kind, mime, storage_key, sha256, bytes, width, height, dek_wrapped, kek_id, guide_id, created_by)
             VALUES ($1,$2,'image',$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(copy_id)
        .bind(a.org_id)
        .bind(&m.mime)
        .bind(&obj.storage_key)
        .bind(digest)
        .bind(len)
        .bind(m.width)
        .bind(m.height)
        .bind(&obj.dek_wrapped)
        .bind(&obj.kek_id)
        .bind(new_id)
        .bind(a.user_id)
        .execute(&mut *tx)
        .await?;
    }
    write_steps(&mut tx, &a, new_id, &steps).await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("guide.duplicated")
            .target("guide", new_id)
            .meta(json!({"source_guide": id})),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_full(&st, new_id, target_perm).await?),
    ))
}
