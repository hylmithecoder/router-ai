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

/// Detailed request and response logger that outputs full API payloads to the terminal.
pub async fn http_logger(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();

    // Skip verbose body logging for static Next.js assets to keep logs clean
    let is_api_route = path.starts_with("/api/")
        || path.starts_with("/v1/")
        || path == "/health"
        || path == "/health/ready";

    if !is_api_route {
        return next.run(req).await;
    }

    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, 5 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = ?e, "failed to buffer request body for logging");
            return error_response(StatusCode::BAD_REQUEST, "invalid request body");
        }
    };

    let req_body_str = if bytes.is_empty() {
        String::new()
    } else {
        String::from_utf8_lossy(&bytes).to_string()
    };

    let req_log = if req_body_str.is_empty() {
        "(empty)".to_string()
    } else {
        truncate_log_string(&req_body_str, 1200)
    };

    println!("\n\x1b[1;36m[API REQUEST]\x1b[0m \x1b[1m{} {}\x1b[0m", method, uri);
    if !req_body_str.is_empty() {
        println!("\x1b[90m└── Body:\x1b[0m {}", req_log);
    }

    let req = Request::from_parts(parts, Body::from(bytes));
    let started = std::time::Instant::now();
    let res = next.run(req).await;
    let elapsed = started.elapsed().as_millis();
    let status = res.status();

    let is_sse = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("text/event-stream"))
        .unwrap_or(false);

    if is_sse {
        println!(
            "\x1b[1;32m[API RESPONSE]\x1b[0m \x1b[1m{} {}\x1b[0m -> \x1b[1;32m{}\x1b[0m (streaming SSE) \x1b[90m[{}ms]\x1b[0m",
            method, path, status, elapsed
        );
        return res;
    }

    let (res_parts, res_body) = res.into_parts();
    let res_bytes = match axum::body::to_bytes(res_body, 5 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = ?e, "failed to buffer response body for logging");
            return Response::from_parts(res_parts, Body::empty());
        }
    };

    let res_body_str = String::from_utf8_lossy(&res_bytes).to_string();
    let res_log = if res_body_str.is_empty() {
        "(empty)".to_string()
    } else {
        truncate_log_string(&res_body_str, 1200)
    };

    let status_color = if status.is_success() {
        "\x1b[1;32m"
    } else if status.is_client_error() {
        "\x1b[1;33m"
    } else {
        "\x1b[1;31m"
    };

    println!(
        "\x1b[1;35m[API RESPONSE]\x1b[0m \x1b[1m{} {}\x1b[0m -> {}{}\x1b[0m \x1b[90m[{}ms]\x1b[0m",
        method, path, status_color, status, elapsed
    );
    if !res_body_str.is_empty() {
        println!("\x1b[90m└── Body:\x1b[0m {}\n", res_log);
    }

    Response::from_parts(res_parts, Body::from(res_bytes))
}

fn truncate_log_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... [truncated {} bytes]", &s[..max_len], s.len() - max_len)
    }
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

    // If the provided token is the master key, grant immediate admin access with unlimited quota
    if token == state.config.router.master_key {
        let mut req = req;
        req.extensions_mut().insert(AuthKey {
            id: "master".to_string(),
            name: "master".to_string(),
            quota_daily_tokens: 0,
        });
        return next.run(req).await;
    }

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
