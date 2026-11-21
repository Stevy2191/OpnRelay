// SPDX-License-Identifier: AGPL-3.0-only
//! Image upload (validated, re-encoded, encrypted at rest) and serving.

use std::io::Cursor;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::authz::{self, Perm};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const MAX_DIMENSION: u32 = 16_384;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/media", post(upload))
        .route("/media/{id}", get(fetch))
}

/// A validated, re-encoded image.
pub struct CleanImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Decode an untrusted image and re-encode it, dropping all metadata (EXIF,
/// text chunks, ICC) and anything that is not pixels.
pub fn sanitize_image(data: &[u8]) -> ApiResult<CleanImage> {
    let format = image::guess_format(data).map_err(|_| ApiError::UnsupportedMediaType)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif
    ) {
        return Err(ApiError::UnsupportedMediaType);
    }
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|_| ApiError::bad("the image could not be decoded"))?;
    let (width, height) = (img.width(), img.height());
    let mut out = Cursor::new(Vec::new());
    // Photos stay JPEG (smaller); screenshots and everything else become PNG
    // (lossless text).
    let mime = if format == ImageFormat::Jpeg {
        img.to_rgb8()
            .write_to(&mut out, ImageFormat::Jpeg)
            .map_err(anyhow::Error::from)?;
        "image/jpeg"
    } else {
        img.write_to(&mut out, ImageFormat::Png)
            .map_err(anyhow::Error::from)?;
        "image/png"
    };
    Ok(CleanImage {
        bytes: out.into_inner(),
        mime,
        width,
        height,
    })
}

#[derive(Deserialize)]
struct UploadQuery {
    guide_id: Option<Uuid>,
}

/// Store a sanitised image; returns the new media id.
pub async fn store_image(
    st: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    guide_id: Option<Uuid>,
    kind: &str,
    data: Bytes,
) -> ApiResult<(Uuid, CleanImage)> {
    let clean = tokio::task::spawn_blocking(move || sanitize_image(&data))
        .await
        .map_err(anyhow::Error::from)??;
    let id = Uuid::now_v7();
    let digest = opnrelay_crypto::sha256(&clean.bytes).to_vec();
    let len = clean.bytes.len() as i64;
    let obj = st.storage.put("media", id, clean.bytes.clone()).await?;
    sqlx::query(
        "INSERT INTO media (id, org_id, kind, mime, storage_key, sha256, bytes, width, height, dek_wrapped, kek_id, guide_id, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(id)
    .bind(org_id)
    .bind(kind)
    .bind(clean.mime)
    .bind(&obj.storage_key)
    .bind(digest)
    .bind(len)
    .bind(clean.width as i32)
    .bind(clean.height as i32)
    .bind(&obj.dek_wrapped)
    .bind(&obj.kek_id)
    .bind(guide_id)
    .bind(user_id)
    .execute(&st.db)
    .await?;
    Ok((id, clean))
}

async fn upload(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if let Some(g) = q.guide_id {
        authz::guide_perm(&st.db, &a, g)
            .await?
            .0
            .require(Perm::Edit)?;
    } else if a.role == authz::OrgRole::Auditor {
        return Err(ApiError::Forbidden("auditors cannot upload content"));
    }
    if body.is_empty() {
        return Err(ApiError::bad("empty upload"));
    }
    if !st.limiter.check(
        &format!("upload:{}", a.user_id),
        600,
        std::time::Duration::from_secs(600),
    ) {
        return Err(ApiError::TooManyRequests);
    }
    let (id, clean) = store_image(&st, a.org_id, a.user_id, q.guide_id, "image", body).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "mime": clean.mime, "width": clean.width, "height": clean.height})),
    ))
}

#[derive(sqlx::FromRow)]
struct MediaRow {
    kind: String,
    mime: String,
    storage_key: String,
    dek_wrapped: Vec<u8>,
    guide_id: Option<Uuid>,
    created_by: Option<Uuid>,
    org_id: Uuid,
}

pub fn private_media_headers(mime: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    h
}

async fn fetch(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let m: MediaRow = sqlx::query_as(
        "SELECT kind, mime, storage_key, dek_wrapped, guide_id, created_by, org_id FROM media WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    if m.org_id != a.org_id {
        return Err(ApiError::NotFound);
    }
    match (m.kind.as_str(), m.guide_id) {
        // The company logo is visible to every signed-in user.
        ("logo", _) => {}
        (_, Some(g)) => authz::guide_perm(&st.db, &a, g)
            .await?
            .0
            .require(Perm::Read)?,
        (_, None) if m.created_by == Some(a.user_id) => {}
        _ => return Err(ApiError::NotFound),
    }
    let bytes = st.storage.get(id, &m.storage_key, &m.dek_wrapped).await?;
    Ok((private_media_headers(&m.mime), bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn sanitizes_png() {
        let clean = sanitize_image(&png(4, 3)).unwrap();
        assert_eq!((clean.width, clean.height, clean.mime), (4, 3, "image/png"));
    }

    #[test]
    fn rejects_non_images() {
        assert!(matches!(
            sanitize_image(b"<svg onload=alert(1)>"),
            Err(ApiError::UnsupportedMediaType)
        ));
        assert!(sanitize_image(b"\x89PNG\r\n\x1a\nbroken").is_err());
    }
}
