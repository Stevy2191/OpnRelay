// SPDX-License-Identifier: AGPL-3.0-only
//! API errors rendered as RFC 9457 problem details.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("the resource was modified by someone else; reload and try again")]
    PreconditionFailed,
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("unsupported media type")]
    UnsupportedMediaType,
    #[error("too many requests; try again later")]
    TooManyRequests,
    #[error("internal error")]
    Internal(#[from] anyhow::Error),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn bad(msg: impl Into<String>) -> Self {
        ApiError::BadRequest(msg.into())
    }

    fn status(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            ApiError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ApiError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Unauthorized => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::NotFound => "not_found",
            ApiError::Conflict(_) => "conflict",
            ApiError::PreconditionFailed => "precondition_failed",
            ApiError::PayloadTooLarge => "payload_too_large",
            ApiError::UnsupportedMediaType => "unsupported_media_type",
            ApiError::TooManyRequests => "too_many_requests",
            ApiError::Internal(_) => "internal",
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::RowNotFound = e {
            return ApiError::NotFound;
        }
        if let Some(db) = e.as_database_error()
            && db.code().as_deref() == Some("23505")
        {
            return ApiError::Conflict("already exists".into());
        }
        ApiError::Internal(e.into())
    }
}

impl From<opnrelay_core::validate::ValidationError> for ApiError {
    fn from(e: opnrelay_core::validate::ValidationError) -> Self {
        ApiError::BadRequest(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if let ApiError::Internal(err) = &self {
            tracing::error!(error = ?err, "internal error");
        }
        let body = json!({
            "type": format!("https://opnrelay.dev/problems/{}", self.code()),
            "title": status.canonical_reason().unwrap_or("error"),
            "status": status.as_u16(),
            "code": self.code(),
            "detail": self.to_string(),
        });
        let mut res = (status, Json(body)).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        res
    }
}
