//! OpenAI-compatible chat completions handlers.

use axum::{
    Extension, Json,
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;

use crate::{
    ai::{
        dto::{
            ChatCompletionRequest, ChatCompletionResponse, ModelInfo, RouterError, StreamSource,
        },
        usage_stream::UsageTrackingStream,
    },
    error::AppError,
    middleware::AuthKey,
    state::AppState,
};

/// `POST /v1/chat/completions` — OpenAI-compatible, supports both JSON and SSE streaming.
pub async fn chat_completion(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthKey>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    dispatch_chat(state, auth, req).await
}

/// `POST /api/v1/chat` — the unified router endpoint. When no provider is
/// supplied it opts into `auto`, which can use Groq keys and discovered local
/// agent CLIs. The OpenAI-compatible `/v1` endpoint keeps its historical Groq
/// default unless a provider is explicitly selected.
pub async fn unified_chat_completion(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthKey>,
    Json(mut req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    if req.provider.is_none() {
        req.provider = Some("auto".to_string());
    }
    dispatch_chat(state, auth, req).await
}

async fn dispatch_chat(
    state: AppState,
    auth: AuthKey,
    req: ChatCompletionRequest,
) -> Result<Response, AppError> {
    if req.stream == Some(true) {
        stream_completion(state, auth, req).await
    } else {
        json_completion(state, auth, req).await
    }
}

async fn json_completion(
    state: AppState,
    auth: AuthKey,
    req: ChatCompletionRequest,
) -> Result<Response, AppError> {
    let started = std::time::Instant::now();
    match state.router.complete(&req).await {
        Ok(outcome) => {
            record_usage(
                &state,
                &auth.id,
                &outcome.model,
                &outcome.provider_id,
                &outcome.response.usage,
                started,
                StatusCode::OK.as_u16() as i64,
            )
            .await;
            Ok(Json(outcome.response).into_response())
        }
        Err(err) => Err(map_router_error(&state, &auth.id, &req.model, &err, started).await),
    }
}

async fn stream_completion(
    state: AppState,
    auth: AuthKey,
    req: ChatCompletionRequest,
) -> Result<Response, AppError> {
    let started = std::time::Instant::now();
    match state.router.stream(&req).await {
        Ok(outcome) => {
            let body = match outcome.source {
                StreamSource::Http(response) => Body::from_stream(UsageTrackingStream::new(
                    response,
                    state.db.clone(),
                    auth.id.clone(),
                    outcome.model.clone(),
                    outcome.provider_id.clone(),
                )),
                StreamSource::Completed(response) => {
                    record_usage(
                        &state,
                        &auth.id,
                        &response.model,
                        &outcome.provider_id,
                        &response.usage,
                        started,
                        StatusCode::OK.as_u16() as i64,
                    )
                    .await;
                    Body::from(sse_for_completed(&response))
                }
            };

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(
                    "content-type",
                    HeaderValue::from_static("text/event-stream; charset=utf-8"),
                )
                .header("cache-control", HeaderValue::from_static("no-cache"))
                .body(body)
                .expect("response body is valid"))
        }
        Err(err) => Err(map_router_error(
            &state,
            &auth.id,
            &req.model,
            &err,
            std::time::Instant::now(),
        )
        .await),
    }
}

async fn record_usage(
    state: &AppState,
    key_id: &str,
    model: &str,
    provider: &str,
    usage: &crate::ai::dto::Usage,
    started: std::time::Instant,
    status: i64,
) {
    let latency = started.elapsed().as_millis() as i64;
    let _ = state
        .db
        .insert_usage(
            key_id,
            model,
            provider,
            usage.prompt_tokens,
            usage.completion_tokens,
            latency,
            status,
        )
        .await;
}

async fn map_router_error(
    state: &AppState,
    key_id: &str,
    model: &str,
    err: &RouterError,
    started: std::time::Instant,
) -> AppError {
    let (status, message) = match err {
        RouterError::NoProviders => (503, "no providers configured".to_string()),
        RouterError::AllProvidersFailed(msg) => (503, format!("all providers failed: {msg}")),
        RouterError::ClientError(msg) => (400, msg.clone()),
    };
    let _ = state
        .db
        .insert_usage(
            key_id,
            model,
            "none",
            0,
            0,
            started.elapsed().as_millis() as i64,
            status as i64,
        )
        .await;
    if status == 503 {
        AppError::UpstreamUnavailable(message)
    } else {
        AppError::BadRequest(message)
    }
}

/// `GET /v1/models` — list models served by the configured providers.
pub async fn list_models(State(state): State<AppState>) -> Result<Json<Vec<ModelInfo>>, AppError> {
    let models = state
        .router
        .models()
        .await
        .into_iter()
        .map(|id| ModelInfo {
            id: id.clone(),
            object: "model".to_string(),
            created: Utc::now().timestamp() as u64,
            owned_by: "router".to_string(),
        })
        .collect::<Vec<_>>();
    Ok(Json(models))
}

fn sse_for_completed(response: &ChatCompletionResponse) -> String {
    let content = response
        .choices
        .first()
        .map(|choice| choice.message.content.as_text())
        .unwrap_or_default();
    let first = serde_json::json!({
        "id": response.id,
        "object": "chat.completion.chunk",
        "created": response.created,
        "model": response.model,
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": content },
            "finish_reason": null
        }]
    });
    let final_chunk = serde_json::json!({
        "id": response.id,
        "object": "chat.completion.chunk",
        "created": response.created,
        "model": response.model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": response.usage
    });
    format!("data: {first}\n\ndata: {final_chunk}\n\ndata: [DONE]\n\n")
}
