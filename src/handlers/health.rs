//! Health check handlers.

use axum::{Json, response::IntoResponse};
use serde_json::json;

/// Liveness probe.
///
/// Returns a plain text `OK` so that simple HTTP health checks pass without parsing JSON.
pub async fn health_check() -> &'static str {
    "OK"
}

/// Readiness probe.
///
/// In a real service this would verify dependencies (database, caches, external APIs)
/// before reporting `ready`. For the template it simply returns a JSON status object.
pub async fn readiness() -> impl IntoResponse {
    Json(json!({
        "status": "ready",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}
