//! Embedded static web dashboard assets.
//!
//! Embeds the Next.js static export (`webui/out/`) directly into the compiled
//! Rust binary, enabling single-binary deployments without external HTML/JS assets on disk.

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "webui/out/"]
pub struct EmbeddedAssets;

/// Serve embedded Next.js static files and handle client-side routes.
pub async fn static_handler(uri: Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');
    let path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    // 1. Direct file lookup (e.g. `_next/static/...`, `favicon.ico`, `next.svg`)
    if let Some(file) = EmbeddedAssets::get(path) {
        return serve_asset(path, &file.data);
    }

    // 2. Next.js HTML page route without extension (e.g. `/dashboard` -> `dashboard.html` or `dashboard/index.html`)
    let html_path = format!("{path}.html");
    if let Some(file) = EmbeddedAssets::get(&html_path) {
        return serve_asset(&html_path, &file.data);
    }

    let dir_index = format!("{}/index.html", path.trim_end_matches('/'));
    if let Some(file) = EmbeddedAssets::get(&dir_index) {
        return serve_asset(&dir_index, &file.data);
    }

    // 3. Fallback to 404 page if present, or generic 404
    if let Some(file) = EmbeddedAssets::get("404.html") {
        let mut resp = serve_asset("404.html", &file.data);
        *resp.status_mut() = StatusCode::NOT_FOUND;
        return resp;
    }

    (StatusCode::NOT_FOUND, "404 Not Found").into_response()
}

fn serve_asset(path: &str, data: &[u8]) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = mime.as_ref();

    let mut response = Response::builder().status(StatusCode::OK).header(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );

    // Cache immutable _next static assets aggressively
    if path.starts_with("_next/static/") {
        response = response.header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else if path.ends_with(".html") {
        response = response.header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=0, must-revalidate"),
        );
    }

    response
        .body(Body::from(data.to_vec()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
