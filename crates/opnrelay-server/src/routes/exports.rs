// SPDX-License-Identifier: AGPL-3.0-only
//! Company branding and guide exports.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use opnrelay_core::{Classification, Step};
use opnrelay_export::{Branding, Format, GuideMeta, RenderModel};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use time::macros::format_description;
use uuid::Uuid;

use crate::audit::{self, Event};
use crate::auth::CurrentUser;
use crate::authz::{self, Perm, require_admin};
use crate::error::{ApiError, ApiResult};
use crate::routes::guides::{load_steps, load_summary};
use crate::routes::media::store_image;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/branding", get(get_branding))
        .route("/admin/branding", put(put_branding))
        .route("/admin/branding/logo", put(put_logo).delete(delete_logo))
        .route("/guides/{id}/export", get(export))
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct BrandingRow {
    #[serde(default, skip_deserializing)]
    pub logo_media_id: Option<Uuid>,
    pub primary_color: String,
    pub accent_color: String,
    pub header_text: String,
    pub footer_text: String,
    pub cover_page: bool,
    pub show_step_numbers: bool,
    pub watermark_exports: bool,
}

async fn load_branding(st: &AppState, org: Uuid) -> ApiResult<BrandingRow> {
    Ok(sqlx::query_as(
        "SELECT logo_media_id, primary_color, accent_color, header_text, footer_text, cover_page,
                show_step_numbers, watermark_exports
         FROM branding WHERE org_id = $1",
    )
    .bind(org)
    .fetch_one(&st.db)
    .await?)
}

async fn get_branding(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<Json<BrandingRow>> {
    Ok(Json(load_branding(&st, a.org_id).await?))
}

fn valid_hex(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
}

async fn put_branding(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Json(b): Json<BrandingRow>,
) -> ApiResult<Json<BrandingRow>> {
    require_admin(&a)?;
    if !valid_hex(&b.primary_color) || !valid_hex(&b.accent_color) {
        return Err(ApiError::bad("colours must look like #1f4fd1"));
    }
    if b.header_text.chars().count() > 200 || b.footer_text.chars().count() > 200 {
        return Err(ApiError::bad(
            "header and footer text are limited to 200 characters",
        ));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "UPDATE branding SET primary_color = $2, accent_color = $3, header_text = $4, footer_text = $5,
            cover_page = $6, show_step_numbers = $7, watermark_exports = $8, updated_at = now()
         WHERE org_id = $1",
    )
    .bind(a.org_id)
    .bind(b.primary_color.to_lowercase())
    .bind(b.accent_color.to_lowercase())
    .bind(b.header_text.trim())
    .bind(b.footer_text.trim())
    .bind(b.cover_page)
    .bind(b.show_step_numbers)
    .bind(b.watermark_exports)
    .execute(&mut *tx)
    .await?;
    audit::record(&mut tx, &a.audit(), Event::new("org.branding.updated")).await?;
    tx.commit().await?;
    Ok(Json(load_branding(&st, a.org_id).await?))
}

async fn put_logo(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    body: Bytes,
) -> ApiResult<Json<BrandingRow>> {
    require_admin(&a)?;
    if body.len() > 2 * 1024 * 1024 {
        return Err(ApiError::bad("the logo must be smaller than 2 MB"));
    }
    let (id, clean) = store_image(&st, a.org_id, a.user_id, None, "logo", body).await?;
    if clean.width > 4000 || clean.height > 4000 {
        return Err(ApiError::bad("the logo must be at most 4000×4000 pixels"));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE branding SET logo_media_id = $2, updated_at = now() WHERE org_id = $1")
        .bind(a.org_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(
        &mut tx,
        &a.audit(),
        Event::new("org.branding.logo_updated").target("media", id),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(load_branding(&st, a.org_id).await?))
}

async fn delete_logo(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
) -> ApiResult<StatusCode> {
    require_admin(&a)?;
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE branding SET logo_media_id = NULL, updated_at = now() WHERE org_id = $1")
        .bind(a.org_id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, &a.audit(), Event::new("org.branding.logo_removed")).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ExportQuery {
    format: String,
    /// Include screenshots (default true).
    images: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct MediaRef {
    storage_key: String,
    dek_wrapped: Vec<u8>,
}

async fn media_bytes(st: &AppState, id: Uuid) -> ApiResult<Option<Vec<u8>>> {
    let m: Option<MediaRef> =
        sqlx::query_as("SELECT storage_key, dek_wrapped FROM media WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    match m {
        Some(m) => Ok(Some(
            st.storage.get(id, &m.storage_key, &m.dek_wrapped).await?,
        )),
        None => Ok(None),
    }
}

async fn export(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<ExportQuery>,
) -> ApiResult<Response> {
    let format = Format::parse(&q.format)
        .ok_or_else(|| ApiError::bad("format must be pdf, docx, html, md or png"))?;
    let (perm, _) = authz::guide_perm(&st.db, &a, id).await?;
    perm.require(Perm::Read)?;
    if !st.limiter.check(
        &format!("export:{}", a.user_id),
        60,
        std::time::Duration::from_secs(600),
    ) {
        return Err(ApiError::TooManyRequests);
    }
    let with_images = q.images.unwrap_or(true) || format == Format::Images;

    let mut conn = st.db.acquire().await?;
    let g = load_summary(&mut conn, id).await?;
    let steps: Vec<Step> = load_steps(&mut conn, id).await?;
    drop(conn);
    let org_name: String = sqlx::query_scalar("SELECT name FROM organizations WHERE id = $1")
        .bind(a.org_id)
        .fetch_one(&st.db)
        .await?;
    let b = load_branding(&st, a.org_id).await?;
    let logo = match b.logo_media_id {
        Some(lid) => media_bytes(&st, lid).await?,
        None => None,
    };
    let mut with_media = Vec::with_capacity(steps.len());
    for s in steps {
        let img = match (with_images, s.media_id) {
            (true, Some(mid)) => media_bytes(&st, mid).await?,
            _ => None,
        };
        with_media.push((s, img));
    }
    let now = OffsetDateTime::now_utc();
    let date_fmt = format_description!("[day] [month repr:short] [year]");
    let watermark = b.watermark_exports.then(|| {
        format!(
            "Exported by {} · {}",
            a.display_name,
            now.format(format_description!(
                "[year]-[month]-[day] [hour]:[minute] UTC"
            ))
            .unwrap_or_default()
        )
    });
    let meta = GuideMeta {
        title: g.title.clone(),
        description: g.description.clone(),
        classification: Classification::parse(&g.classification).unwrap_or_default(),
        author: g.author_name.clone().unwrap_or_default(),
        date: g.updated_at.format(&date_fmt).unwrap_or_default(),
        version: g.current_version,
    };
    let branding = Branding {
        company: org_name,
        logo,
        primary_color: b.primary_color,
        accent_color: b.accent_color,
        header_text: b.header_text,
        footer_text: b.footer_text,
        cover_page: b.cover_page,
        show_step_numbers: b.show_step_numbers,
        watermark,
    };
    let out = tokio::task::spawn_blocking(move || {
        let model = RenderModel::build(meta, branding, with_media)?;
        opnrelay_export::render(&model, format)
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(|e| match e {
        opnrelay_export::ExportError::TooLarge(_) => ApiError::bad(e.to_string()),
        other => ApiError::Internal(anyhow::anyhow!(other)),
    })?;

    audit::record_now(
        &st.db,
        &a.audit(),
        Event::new("guide.exported").target("guide", id).meta(
            json!({"format": format.as_str(), "images": with_images, "bytes": out.bytes.len()}),
        ),
    )
    .await?;

    let filename = format!("{}.{}", opnrelay_export::slug(&g.title), out.extension);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(out.mime));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(anyhow::Error::from)?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    Ok((headers, out.bytes).into_response())
}
