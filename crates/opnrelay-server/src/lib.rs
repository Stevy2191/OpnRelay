// SPDX-License-Identifier: AGPL-3.0-only
//! OpnRelay server: HTTP API, web UI hosting and background work.

pub mod audit;
pub mod auth;
pub mod authz;
pub mod backup;
pub mod bootstrap;
pub mod config;
pub mod error;
pub mod graph;
pub mod mfa;
pub mod microsoft;
pub mod onedrive;
pub mod ratelimit;
pub mod routes;
pub mod state;
pub mod storage;
pub mod util;
pub mod web;

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Request};
use axum::middleware;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await?)
}

/// Build the complete HTTP application.
pub fn app(state: AppState) -> Router {
    let request_id = HeaderName::from_static("x-request-id");
    let hsts = state.cfg.cookie_secure;
    let max_upload = state.cfg.max_upload_bytes;
    let api = routes::api().layer(DefaultBodyLimit::max(max_upload));
    Router::new()
        .nest("/api/v1", api)
        .merge(routes::health())
        .fallback(web::static_handler)
        .layer(middleware::from_fn(web::security_headers))
        .layer(middleware::from_fn(move |mut req: axum::extract::Request, next: middleware::Next| async move {
            if hsts {
                req.extensions_mut().insert(web::HstsEnabled);
            }
            next.run(req).await
        }))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
                let id = req
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");
                // Never log query strings: they can contain tokens.
                tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id = %id)
            }),
        )
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .with_state(state)
}
