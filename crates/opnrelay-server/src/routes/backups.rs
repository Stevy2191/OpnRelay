// SPDX-License-Identifier: AGPL-3.0-only
//! Admin backup settings, manual runs, downloads and the OneDrive connection.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::audit::{self, AuditActor, Event};
use crate::auth::{ClientInfo, CurrentUser};
use crate::authz::require_admin;
use crate::backup;
use crate::error::{ApiError, ApiResult};
use crate::microsoft::{self, Provider};
use crate::onedrive;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/backups", get(status))
        .route("/admin/backups/settings", put(put_settings))
        .route("/admin/backups/run", post(run_now))
        .route("/admin/backups/files/{name}", get(download))
        .route("/admin/backups/onedrive/connect", get(connect))
        .route("/admin/backups/onedrive", axum::routing::delete(disconnect))
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
pub struct SettingsRow {
    enabled: bool,
    schedule_hour: i32,
    keep_local: i32,
    onedrive_enabled: bool,
    onedrive_folder: String,
    keep_remote: i32,
    #[serde(default, skip_deserializing)]
    onedrive_account: Option<String>,
    #[serde(default, skip_deserializing, with = "time::serde::rfc3339::option")]
    last_run_at: Option<OffsetDateTime>,
    #[serde(default, skip_deserializing)]
    last_status: Option<String>,
    #[serde(default, skip_deserializing)]
    last_error: Option<String>,
    #[serde(default, skip_deserializing)]
    last_file: Option<String>,
    #[serde(default, skip_deserializing)]
    last_size: Option<i64>,
    #[serde(default, skip_deserializing, with = "time::serde::rfc3339::option")]
    last_remote_at: Option<OffsetDateTime>,
}

async fn load(st: &AppState, org: Uuid) -> ApiResult<SettingsRow> {
    Ok(sqlx::query_as(
        "SELECT enabled, schedule_hour, keep_local, onedrive_enabled, onedrive_folder, keep_remote, onedrive_account,
                last_run_at, last_status, last_error, last_file, last_size, last_remote_at
         FROM backup_settings WHERE org_id = $1",
    )
    .bind(org)
    .fetch_one(&st.db)
    .await?)
}

async fn status(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&a)?;
    let s = load(&st, a.org_id).await?;
    let files: Vec<_> = backup::list_local(&st.cfg.backup_dir)
        .await
        .into_iter()
        .map(|(name, size)| json!({"name": name, "size": size}))
        .collect();
    let ms_ready = microsoft::load(&st, &st.db, a.org_id).await?.is_some();
    Ok(Json(
        json!({"settings": s, "files": files, "microsoft_app_configured": ms_ready}),
    ))
}

async fn put_settings(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(s): Json<SettingsRow>,
) -> ApiResult<Json<SettingsRow>> {
    require_admin(&a)?;
    if !(0..=23).contains(&s.schedule_hour)
        || !(1..=365).contains(&s.keep_local)
        || !(1..=3650).contains(&s.keep_remote)
    {
        return Err(ApiError::bad("values out of range"));
    }
    if !onedrive::valid_folder(&s.onedrive_folder) {
        return Err(ApiError::bad(
            "the OneDrive folder name cannot contain / \\ : * ? \" < > |",
        ));
    }
    let connected: bool = sqlx::query_scalar(
        "SELECT onedrive_token_enc IS NOT NULL FROM backup_settings WHERE org_id = $1",
    )
    .bind(a.org_id)
    .fetch_one(&st.db)
    .await?;
    if s.onedrive_enabled && !connected {
        return Err(ApiError::bad("connect a OneDrive account first"));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "UPDATE backup_settings SET enabled = $2, schedule_hour = $3, keep_local = $4, onedrive_enabled = $5,
            onedrive_folder = $6, keep_remote = $7 WHERE org_id = $1",
    )
    .bind(a.org_id)
    .bind(s.enabled)
    .bind(s.schedule_hour)
    .bind(s.keep_local)
    .bind(s.onedrive_enabled)
    .bind(s.onedrive_folder.trim())
    .bind(s.keep_remote)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("backup.settings.updated").meta(json!({
            "enabled": s.enabled, "hour": s.schedule_hour, "keep_local": s.keep_local,
            "onedrive": s.onedrive_enabled, "folder": s.onedrive_folder, "keep_remote": s.keep_remote
        })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load(&st, a.org_id).await?))
}

async fn run_now(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    require_admin(&a)?;
    let actor = a.audit();
    let st2 = st.clone();
    tokio::spawn(async move {
        if let Err(e) = backup::run(&st2, actor).await {
            tracing::warn!(error = %e, "manual backup failed");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({"started": true}))))
}

async fn download(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    require_admin(&a)?;
    if !backup::is_backup_name(&name) {
        return Err(ApiError::NotFound);
    }
    let f = tokio::fs::File::open(st.cfg.backup_dir.join(&name))
        .await
        .map_err(|_| ApiError::NotFound)?;
    let len = f.metadata().await.map_err(anyhow::Error::from)?.len();
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("backup.downloaded").meta(json!({"file": name})),
    )
    .await?;
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            .map_err(anyhow::Error::from)?,
    );
    Ok((h, Body::from_stream(ReaderStream::new(f))).into_response())
}

/// Browser navigation: starts the Microsoft consent for OneDrive access.
async fn connect(State(st): State<AppState>, CurrentUser(a): CurrentUser) -> ApiResult<Response> {
    require_admin(&a)?;
    let p = microsoft::load(&st, &st.db, a.org_id)
        .await?
        .ok_or_else(|| {
            ApiError::bad("set up the Microsoft app (Admin → Microsoft sign-in) first")
        })?;
    crate::routes::sso::begin(
        &st,
        &p.app,
        "onedrive",
        microsoft::ONEDRIVE_SCOPE,
        "/admin",
        Some(a.user_id),
        Some(&a.email),
    )
    .await
}

/// Completes the OneDrive connection (called from the shared Microsoft callback).
pub async fn finish_onedrive(
    st: &AppState,
    client: &ClientInfo,
    p: &Provider,
    user: &Option<Uuid>,
    code: &str,
    verifier: &str,
) -> ApiResult<Response> {
    let user = user.ok_or_else(|| ApiError::bad("invalid OneDrive connection request"))?;
    let (org, email, role): (Uuid, String, String) = sqlx::query_as(
        "SELECT org_id, email::text, org_role FROM users WHERE id = $1 AND status = 'active'",
    )
    .bind(user)
    .fetch_optional(&st.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if role != "owner" && role != "admin" {
        return Err(ApiError::Forbidden("administrator role required"));
    }
    let t = microsoft::exchange_code(st, &p.app, code, verifier, microsoft::ONEDRIVE_SCOPE).await?;
    let refresh = t
        .refresh_token
        .ok_or_else(|| ApiError::bad("Microsoft did not grant offline access"))?;
    let access = t
        .access_token
        .ok_or_else(|| ApiError::bad("Microsoft returned no access token"))?;
    let account = onedrive::account_name(st, &access)
        .await
        .map_err(|e| ApiError::bad(format!("could not read the OneDrive account: {e}")))?;
    sqlx::query(
        "UPDATE backup_settings SET onedrive_token_enc = $2, onedrive_account = $3, onedrive_enabled = true WHERE org_id = $1",
    )
    .bind(org)
    .bind(onedrive::seal_token(st, org, &refresh)?)
    .bind(&account)
    .execute(&st.db)
    .await?;
    audit::record_now(
        &st.db,
        &AuditActor {
            id: Some(user),
            label: email,
            ip: client.ip,
        },
        Event::new("backup.onedrive.connected").meta(json!({"account": account})),
    )
    .await?;
    Ok(Redirect::to("/#/admin?tab=backups&onedrive=connected").into_response())
}

async fn disconnect(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    sqlx::query(
        "UPDATE backup_settings SET onedrive_token_enc = NULL, onedrive_account = NULL, onedrive_enabled = false WHERE org_id = $1",
    )
    .bind(a.org_id)
    .execute(&st.db)
    .await?;
    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("backup.onedrive.disconnected"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
