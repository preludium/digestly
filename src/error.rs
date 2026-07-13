//! Unified API error type. Every handler returns `Result<_, AppError>`; errors render as
//! JSON `{ "error": "..." }` with an appropriate status. Secrets are never included.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("registration is disabled")]
    RegistrationDisabled,
    /// 429 - the caller is rate-limited (e.g. the "Ingest now" cooldown).
    #[error("{0}")]
    TooManyRequests(String),
    /// 502 - an upstream dependency (e.g. an AI provider) failed. Message is user-safe (no secrets).
    #[error("{0}")]
    Upstream(String),
    /// 500 - the message is logged but a generic body is returned.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Carries the full error chain across the axum middleware boundary so a
/// request-aware logger can include method + URI alongside the error details.
#[derive(Debug, Clone)]
pub struct InternalError(pub Arc<anyhow::Error>);

impl AppError {
    fn parts(&self) -> (StatusCode, String) {
        match self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::RegistrationDisabled => {
                (StatusCode::FORBIDDEN, "registration is disabled".into())
            }
            AppError::TooManyRequests(m) => (StatusCode::TOO_MANY_REQUESTS, m.clone()),
            AppError::Upstream(m) => (StatusCode::BAD_GATEWAY, m.clone()),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
            ),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = self.parts();
        let mut response = (status, Json(json!({ "error": msg }))).into_response();
        if let AppError::Internal(e) = self {
            response.extensions_mut().insert(InternalError(Arc::new(e)));
        }
        response
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(e.into())
    }
}

/// Convenience alias for handler results.
pub type ApiResult<T> = Result<T, AppError>;
