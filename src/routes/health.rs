//! Health/readiness routes.
//!
//! These endpoints are mounted at `/health` by the root router. They are intentionally simple
//! and stateless so that load balancers and container orchestrators can use them for probes.

use axum::{Router, routing::get};

use crate::{
    handlers::health::{health_check, readiness},
    state::AppState,
};

/// Router for health endpoints.
///
/// Uses `Router<AppState>` even though the handlers do not need state so it can be merged
/// into the root router, which has the same state type.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(health_check))
        .route("/ready", get(readiness))
}
