//! Admin API routes, mounted at `/api/v1/admin`.
//!
//! All endpoints are protected by the master key middleware.

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, patch, post},
};

use crate::{
    handlers::admin::{
        create_key, create_provider, delete_key, delete_provider, dns_lookup, list_keys,
        list_providers, toggle_provider, update_key, update_provider, usage_log, usage_summary,
    },
    middleware::require_master_key,
    state::AppState,
};

/// Router for the `/api/v1/admin` namespace.
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/usage/summary", get(usage_summary))
        .route("/usage/log", get(usage_log))
        .route("/dns", get(dns_lookup))
        .route("/keys", get(list_keys).post(create_key))
        .route("/keys/{id}", patch(update_key).delete(delete_key))
        .route("/providers", get(list_providers).post(create_provider))
        .route(
            "/providers/{id}",
            patch(update_provider).delete(delete_provider),
        )
        .route("/providers/{id}/toggle", post(toggle_provider))
        .layer(from_fn_with_state(state, require_master_key))
}
