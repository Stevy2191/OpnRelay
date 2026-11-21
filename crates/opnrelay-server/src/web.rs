// SPDX-License-Identifier: AGPL-3.0-only
//! Serves the compiled web UI embedded in the binary, plus security headers.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/web/dist"]
#[allow_missing = true]
struct Assets;

const FALLBACK_HTML: &str = "<!doctype html><meta charset=utf-8><title>OpnRelay</title>\
<p>The OpnRelay web UI has not been built. Run <code>pnpm --dir apps/web build</code> and rebuild the server.</p>";

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !path.is_empty()
        && let Some(file) = Assets::get(path)
    {
        let mime = file.metadata.mimetype();
        let cache = if path.starts_with("assets/") {
            // Vite emits content-hashed file names under assets/.
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_str(mime)
                        .unwrap_or(HeaderValue::from_static("application/octet-stream")),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            ],
            Body::from(file.data.into_owned()),
        )
            .into_response();
    }
    // Single page app: everything else gets index.html.
    match Assets::get("index.html") {
        Some(f) => (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            ],
            Body::from(f.data.into_owned()),
        )
            .into_response(),
        None => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )],
            FALLBACK_HTML,
        )
            .into_response(),
    }
}

pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; object-src 'none'; \
base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Adds security headers to every response unless a handler set its own.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let hsts = req.extensions().get::<HstsEnabled>().is_some();
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(CSP));
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.entry("x-frame-options")
        .or_insert(HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        "cross-origin-opener-policy",
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=(), usb=()"),
    );
    if hsts {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    res
}

/// Request extension marking that HSTS should be sent (HTTPS deployments).
#[derive(Clone, Copy)]
pub struct HstsEnabled;
