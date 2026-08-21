//! Centralized error handling.
//!
//! Every handler can return `Result<T, AppError>`. Because `AppError` implements Axum's
//! `IntoResponse`, Axum automatically converts errors into a consistent JSON body.
//!
//! Response shape:
//! ```json
//! {
//!   "success": false,
//!   "error": {
//!     "message": "Bad request: name is required",
//!     "status": 400
//!   }
//! }
//! ```
//!
//! When adding new error variants:
//! 1. Add a variant to `AppError`.
//! 2. Map it to a status code and message in `IntoResponse`.
//! 3. Optionally add a `#[from]` conversion for an external error type.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

/// Application-wide error type.
#[derive(Debug, Error)]
pub enum AppError {
    /// Catch-all for unexpected internal failures. The original error is logged but not exposed.
    #[error("Internal server error")]
    Internal(#[from] anyhow::Error),

    /// Validation failures that originate from application logic (not just JSON schema).
    #[error("Validation error: {0}")]
    Validation(String),

    /// Resource not found.
    #[error("Not found")]
    NotFound,

    /// Generic bad request with a human-readable message.
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Missing or invalid credentials.
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Daily token quota exceeded.
    #[error("Daily quota exceeded")]
    QuotaExceeded,

    /// Every provider failed; the request could not be routed.
    #[error("Upstream unavailable: {0}")]
    UpstreamUnavailable(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Internal(err) => {
                // Log the real cause internally; return a generic message to callers.
                tracing::error!(error = ?err, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found".to_string()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::QuotaExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                "Daily quota exceeded".to_string(),
            ),
            AppError::UpstreamUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
        };

        let body = Json(json!({
            "success": false,
            "error": {
                "message": message,
                "status": status.as_u16(),
            }
        }));

        (status, body).into_response()
    }
}
