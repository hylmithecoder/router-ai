//! Route composition.
//!
//! This module is the single place where all routes are combined and global middleware is
//! applied. Sub-routers only declare their own paths; nesting is handled here.
//!
//! The public `create_router` function is also the main integration point for tests: tests
//! build the same router that production uses, then call routes with `ServiceExt::oneshot`.

use std::time::Duration;

use axum::{Router, http::StatusCode, middleware::from_fn};
use tower::ServiceBuilder;
use tower_http::set_status::SetStatus;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::{
    middleware::{http_logger, request_id},
    state::AppState,
};

mod admin;
mod api;
mod health;
mod v1;

/// Build the full application router with middleware and shared state.
///
/// Returns a `Router<()>` (no missing state) so it can be served directly and used in
/// integration tests with `ServiceExt::oneshot`.
pub fn create_router(state: AppState) -> Router {
    let static_dir = state.config.router.static_dir.clone();

    // Start with `Router<AppState>` because sub-routers contain handlers that extract
    // `State<AppState>`. `merge` and `nest` require both routers to share the same state type.
    Router::<AppState>::new()
        .nest("/health", health::router())
        .nest("/v1", v1::router(state.clone()))
        .nest("/api/v1", api::router(state.clone()))
        .nest("/api/v1/admin", admin::router(state.clone()))
        .layer(
            ServiceBuilder::new()
                // HTTP access logs with method, path, status, and latency. Kept outermost in
                // this builder so that Cors/Timeout response bodies are not wrapped by Trace.
                .layer(TraceLayer::new_for_http())
                // Cross-origin policy. "permissive" is fine for local dev; restrict in production.
                .layer(CorsLayer::permissive())
                // Innermost layer: abort requests that run too long. Must be inside Trace/Cors
                // because both may require the inner response body to implement `Default`.
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    Duration::from_secs(120),
                )),
        )
        // Applied as a separate layer because `from_fn` middleware cannot always be combined
        // into a single `ServiceBuilder` while preserving the required `Service` trait bounds.
        // This layer is the outermost one: request id is attached before CORS, trace, or timeout.
        .layer(from_fn(request_id))
        .layer(from_fn(http_logger))
        // Provide the state. The router is no longer missing state, so the type becomes `Router<()>`.
        .with_state(state)
        // Serve the statically exported Next.js dashboard on the same port.
        // API routes always take precedence; everything else falls through to disk.
        .fallback_service(static_fallback(&static_dir))
}

/// Static file service for the exported dashboard. Falls back to a 404 page
/// when the dashboard has not been built yet (`make webui`).
fn static_fallback(static_dir: &str) -> ServeDir<SetStatus<ServeFile>> {
    let not_found = format!("{static_dir}/404.html");
    ServeDir::new(static_dir).not_found_service(ServeFile::new(not_found))
}
