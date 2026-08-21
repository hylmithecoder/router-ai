//! Unified API routes mounted at `/api/v1`.

use axum::{Router, middleware::from_fn_with_state, routing::post};

use crate::{
    handlers::chat::unified_chat_completion, middleware::require_api_key, state::AppState,
};

/// The single endpoint intended for applications that want to select Groq or
/// one of the locally installed agent CLIs per request.
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/chat", post(unified_chat_completion))
        .layer(from_fn_with_state(state, require_api_key))
}
