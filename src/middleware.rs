//! Reusable middleware.
//!
//! Middleware in Axum is implemented as Tower layers or `from_fn` functions. This module
//! contains authentication middleware (API keys + admin master key) and a request-id
//! middleware used across the whole API.

use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::{HeaderName, HeaderValue, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use uuid::Uuid;

use crate::{database::hash_key, state::AppState};

/// Header used to expose the per-request correlation id to clients.
const REQUEST_ID_HEADER: &str = "x-request-id";

/// Authenticated router key attached to the request by `require_api_key`.
#[derive(Debug, Clone)]
pub struct AuthKey {
    pub id: String,
    pub name: String,
    pub quota_daily_tokens: i64,
}

/// Authenticated admin (master key) attached by `require_master_key`.
#[derive(Debug, Clone)]
pub struct AdminAuth;

/// Attach a unique request id to every incoming request.
///
/// The id is stored as a response header so clients can quote it in support tickets.
pub async fn request_id(req: Request<Body>, next: Next) -> Response {
    let request_id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let mut response = next.run(req).await;

    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }

    response
}

fn bearer_token(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = Json(json!({
        "success": false,
        "error": { "message": message, "status": status.as_u16() }
    }));
    (status, body).into_response()
}

/// Authenticate router requests with a stored API key and enforce the daily
/// token quota. On success the key record is inserted into request extensions.
pub async fn require_api_key(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let Some(token) = bearer_token(&req) else {
        return error_response(StatusCode::UNAUTHORIZED, "missing bearer token");
    };

    let record = match state.db.find_key_by_hash(&hash_key(&token)).await {
        Ok(Some(r)) => r,
        Ok(None) => return error_response(StatusCode::UNAUTHORIZED, "invalid API key"),
        Err(e) => {
            tracing::error!(error = ?e, "db lookup failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };

    if !record.enabled {
        return error_response(StatusCode::UNAUTHORIZED, "API key disabled");
    }

    if record.quota_daily_tokens > 0 {
        match state.db.usage_today(&record.id).await {
            Ok(used) if used >= record.quota_daily_tokens => {
                return error_response(StatusCode::TOO_MANY_REQUESTS, "daily token quota exceeded");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!(error = ?e, "quota lookup failed");
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
            }
        }
    }

    let mut req = req;
    req.extensions_mut().insert(AuthKey {
        id: record.id,
        name: record.name,
        quota_daily_tokens: record.quota_daily_tokens,
    });
    next.run(req).await
}

/// Authenticate admin endpoints with the master key from configuration.
pub async fn require_master_key(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let Some(token) = bearer_token(&req) else {
        return error_response(StatusCode::UNAUTHORIZED, "missing bearer token");
    };

    if token != state.config.router.master_key {
        return error_response(StatusCode::UNAUTHORIZED, "invalid master key");
    }

    let mut req = req;
    req.extensions_mut().insert(AdminAuth);
    next.run(req).await
}
