// SPDX-License-Identifier: AGPL-3.0-only
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;

use crate::state::AppState;

pub mod admin;
pub mod audit_log;
pub mod auth;
pub mod guides;
pub mod me;
pub mod media;
pub mod workspaces;

/// All JSON API routes, mounted under `/api/v1`.
pub fn api() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::router())
        .nest("/me", me::router())
        .nest("/admin", admin::router())
        .merge(workspaces::router())
        .merge(guides::router())
        .merge(media::router())
        .merge(audit_log::router())
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn readyz(State(st): State<AppState>) -> (StatusCode, &'static str) {
    let db = sqlx::query("SELECT 1").execute(&st.db).await.is_ok();
    let storage = st.storage.health().await;
    if db && storage {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}

pub fn health() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
}
