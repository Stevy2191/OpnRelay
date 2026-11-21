// SPDX-License-Identifier: AGPL-3.0-only
//! Desktop recorder and browser extension packages bundled inside the server
//! image (see deploy/Dockerfile). Only administrators can list or download
//! them; they decide how to distribute the recorder to staff.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio_util::io::ReaderStream;

use crate::auth::CurrentUser;
use crate::authz::require_admin;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads", get(list))
        .route("/downloads/{name}", get(fetch))
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DownloadFile {
    pub name: String,
    pub size: u64,
    /// windows | macos | linux | extension | other
    pub platform: &'static str,
    /// Human description, e.g. "Windows installer".
    pub label: String,
    pub sha256: Option<String>,
}

/// Classify a release asset by its file name.
pub fn classify(name: &str) -> (&'static str, String) {
    let n = name.to_lowercase();
    let kind = if [".msi", ".exe", ".dmg", ".pkg"].iter().any(|e| n.ends_with(e)) {
        "installer"
    } else if n.ends_with(".deb") {
        "Debian/Ubuntu package"
    } else if n.ends_with(".rpm") {
        "Fedora/RHEL package"
    } else if n.ends_with(".appimage") {
        "AppImage"
    } else if n.contains("portable") {
        "portable app"
    } else {
        "archive"
    };
    if n.contains("extension") {
        let browser = if n.contains("firefox") {
            "Firefox"
        } else {
            "Chrome / Edge"
        };
        return (
            "extension",
            format!("Browser extension for {browser} (for IT deployment)"),
        );
    }
    if n.contains("windows") || n.ends_with(".msi") || n.ends_with(".exe") {
        return ("windows", format!("Windows {kind}"));
    }
    if n.contains("macos") || n.contains("mac") || n.ends_with(".dmg") {
        return ("macos", format!("macOS {kind}"));
    }
    if n.contains("linux") || n.ends_with(".deb") || n.ends_with(".rpm") || n.ends_with(".appimage")
    {
        return ("linux", format!("Linux {kind}"));
    }
    ("other", name.to_string())
}

fn dir(st: &AppState) -> ApiResult<PathBuf> {
    st.cfg.downloads_dir.clone().ok_or(ApiError::NotFound)
}

/// Only package types are offered (not READMEs or other files in the folder).
const EXTENSIONS: &[&str] = &[
    ".msi",
    ".exe",
    ".dmg",
    ".pkg",
    ".deb",
    ".rpm",
    ".appimage",
    ".zip",
    ".tar.gz",
    ".xpi",
    ".crx",
];

fn safe_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXTENSIONS.iter().any(|e| lower.ends_with(e))
        && !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
}

/// Read `SHA256SUMS` (sha256sum format) if present.
async fn checksums(dir: &std::path::Path) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    if let Ok(text) = tokio::fs::read_to_string(dir.join("SHA256SUMS")).await {
        for line in text.lines() {
            if let Some((hash, file)) = line.split_once(char::is_whitespace) {
                let file = file.trim().trim_start_matches('*');
                if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    out.insert(file.to_string(), hash.to_lowercase());
                }
            }
        }
    }
    out
}

pub async fn list_dir(dir: &std::path::Path) -> Vec<DownloadFile> {
    let sums = checksums(dir).await;
    let mut out = Vec::new();
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return out;
    };
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        if !safe_name(&name) {
            continue;
        }
        let Ok(meta) = e.metadata().await else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let (platform, label) = classify(&name);
        out.push(DownloadFile {
            sha256: sums.get(&name).cloned(),
            name,
            size: meta.len(),
            platform,
            label,
        });
    }
    out.sort_by(|a, b| a.platform.cmp(b.platform).then(a.name.cmp(&b.name)));
    out
}

#[derive(Serialize)]
struct Listing {
    server_url: String,
    files: Vec<DownloadFile>,
}

async fn list(State(st): State<AppState>, CurrentUser(a): CurrentUser) -> ApiResult<Json<Listing>> {
    require_admin(&a)?;
    let files = match &st.cfg.downloads_dir {
        Some(d) => list_dir(d).await,
        None => vec![],
    };
    Ok(Json(Listing {
        server_url: st.cfg.public_url.clone(),
        files,
    }))
}

async fn fetch(
    State(st): State<AppState>,
    CurrentUser(a): CurrentUser,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    require_admin(&a)?;
    if !safe_name(&name) {
        return Err(ApiError::NotFound);
    }
    let path = dir(&st)?.join(&name);
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let len = file.metadata().await.map_err(anyhow::Error::from)?.len();
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
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok((h, Body::from_stream(ReaderStream::new(file))).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(classify("OpnRelay-Recorder-Windows-x64.msi").0, "windows");
        assert_eq!(classify("OpnRelay-Recorder-macOS-universal.dmg").0, "macos");
        assert_eq!(
            classify("opnrelay-recorder_0.1.0_amd64.deb"),
            ("linux", "Linux Debian/Ubuntu package".into())
        );
        assert_eq!(classify("OpnRelay-Extension-Firefox.zip").0, "extension");
        assert!(!safe_name("../etc/passwd"));
        assert!(!safe_name(".env"));
        assert!(safe_name("OpnRelay-Recorder-Linux-x64.tar.gz"));
    }
}
