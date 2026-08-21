//! Unified API routes mounted at `/api/v1`.

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::{
    handlers::{
        chat::{list_models, unified_chat_completion},
        ocr::license_plate_ocr,
    },
    middleware::require_api_key,
    state::AppState,
};

/// Unified API namespace `/api/v1`.
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/chat", post(unified_chat_completion))
        .route("/chat/completions", post(unified_chat_completion))
        .route("/models", get(list_models))
        .route("/ocr/licenseplate", post(license_plate_ocr))
        .layer(from_fn_with_state(state, require_api_key))
}
