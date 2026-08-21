//! OpenAI-compatible API routes, mounted at `/v1`.
//!
//! Both routes are protected by the router API-key middleware, which also
//! enforces the per-key daily token quota.

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::{
    handlers::{
        chat::{chat_completion, list_models},
        ocr::license_plate_ocr,
    },
    middleware::require_api_key,
    state::AppState,
};

/// Router for the `/v1` namespace.
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/chat/completions", post(chat_completion))
        .route("/models", get(list_models))
        .route("/ocr/licenseplate", post(license_plate_ocr))
        .layer(from_fn_with_state(state, require_api_key))
}
