//! Standardized API Response envelopes.
//!
//! Provides a uniform JSON response structure for successful API endpoints:
//! ```json
//! {
//!   "success": true,
//!   "data": { ... }
//! }
//! ```
//! Or for message-only responses:
//! ```json
//! {
//!   "success": true,
//!   "message": "Operation completed successfully"
//! }
//! ```

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// Generic API response wrapper for success results.
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a success response wrapping response data.
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            message: None,
        }
    }

    /// Create a success response with a descriptive message and data.
    pub fn with_message(data: T, message: impl Into<String>) -> Self {
        Self {
            success: true,
            data: Some(data),
            message: Some(message.into()),
        }
    }

    /// Convert into an Axum `Response` with `StatusCode::OK`.
    pub fn into_ok(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }

    /// Convert into an Axum `Response` with `StatusCode::CREATED`.
    pub fn into_created(self) -> Response {
        (StatusCode::CREATED, Json(self)).into_response()
    }
}

impl ApiResponse<()> {
    /// Create a success response with only a message (no payload data).
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            success: true,
            data: None,
            message: Some(msg.into()),
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}
