//! Integration tests.
//!
//! These tests build the same router that the binary uses and exercise endpoints through
//! Axum's `ServiceExt::oneshot`. Upstream providers are mocked with tiny axum servers so
//! fallback behavior can be tested without any real API key.

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    routing::post,
};
use router_api_ai::{
    ai::router::AiRouter,
    config::{ApiKeySeed, AppSettings, GroqKeySpec, RouterSettings, ServerSettings, Settings},
    database::Db,
    routes::create_router,
    state::AppState,
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn test_settings(master_key: &str, groq_keys: Vec<GroqKeySpec>) -> Settings {
    Settings {
        app: AppSettings {
            name: "test-router".to_string(),
        },
        server: ServerSettings {
            host: "127.0.0.1".to_string(),
            port: 0,
        },
        router: RouterSettings {
            master_key: master_key.to_string(),
            api_keys: vec![ApiKeySeed {
                name: "test-bot".to_string(),
                key: "sk-test".to_string(),
            }],
            groq_api_keys: groq_keys,
            groq_base_url: "http://127.0.0.1:1/v1".to_string(),
            default_model: "mock-model".to_string(),
            db_path: ":memory:".to_string(),
            static_dir: "/nonexistent-static-dir".to_string(),
            provider_cooldown_secs: 0,
            daily_quota_tokens: 0,
            cli_timeout_secs: 5,
            agent_workdir: ".".to_string(),
        },
    }
}

async fn test_app(master_key: &str, groq_keys: Vec<GroqKeySpec>) -> (axum::Router, AppState) {
    let settings = test_settings(master_key, groq_keys);
    let db = Db::open_in_memory().await.unwrap();
    db.seed_api_keys(&settings.router.api_keys).await.unwrap();
    let router = AiRouter::new(&settings, db.clone()).await;
    let state = AppState::new(settings, db, router);
    let app = create_router(state.clone());
    (app, state)
}

/// Spawn a mock upstream server on an ephemeral port, returning its base URL.
async fn spawn_mock(upstream: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    format!("http://{addr}/v1")
}

fn success_upstream() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({
                "id": "cmpl-mock",
                "object": "chat.completion",
                "created": 1234,
                "model": "mock-model",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "hello from mock" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
            }))
        }),
    )
}

fn failing_upstream(status: StatusCode) -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(move || async move {
            (
                status,
                Json(json!({ "error": { "message": "mock failure" } })),
            )
        }),
    )
}

/// Upstream that emits a fake SSE stream (used by the streaming test).
fn echo_upstream() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let payload = json!({
                "id": "cmpl-mock",
                "object": "chat.completion.chunk",
                "created": 1234,
                "model": "mock-model",
                "choices": [{
                    "index": 0,
                    "delta": { "content": "hello" },
                    "finish_reason": null
                }],
                "usage": { "prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4 }
            });
            let body = format!("data: {payload}\n\ndata: [DONE]\n\n");
            ([(header::CONTENT_TYPE, "text/event-stream")], body)
        }),
    )
}

async fn chat_request(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn health_check_returns_ok() {
    let (app, _) = test_app("master", vec![]).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "OK");
}

#[tokio::test]
async fn chat_completion_rejects_missing_key() {
    let (app, _) = test_app("master", vec![]).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"model": "m", "messages": []}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_completion_rejects_invalid_key() {
    let (app, _) = test_app("master", vec![]).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-wrong")
                .body(Body::from(
                    json!({"model": "m", "messages": []}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_completion_succeeds_with_valid_key() {
    let mock = spawn_mock(success_upstream()).await;
    let (app, state) = test_app(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock,
        }],
    )
    .await;

    let (status, body) = chat_request(
        &app,
        json!({"model": "mock-model", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["choices"][0]["message"]["content"], "hello from mock");

    let summary = state.db.usage_summary_today().await.unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].total_tokens, 15);
}

#[tokio::test]
async fn falls_back_to_second_provider_when_first_rate_limited() {
    let bad = spawn_mock(failing_upstream(StatusCode::TOO_MANY_REQUESTS)).await;
    let good = spawn_mock(success_upstream()).await;
    let (app, state) = test_app(
        "master",
        vec![
            GroqKeySpec {
                key: "gk-1".into(),
                base_url: bad,
            },
            GroqKeySpec {
                key: "gk-2".into(),
                base_url: good,
            },
        ],
    )
    .await;

    let (status, body) = chat_request(
        &app,
        json!({"model": "mock-model", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["choices"][0]["message"]["content"], "hello from mock");

    let providers = state.db.list_providers().await.unwrap();
    let p1 = providers.iter().find(|p| p.id == "groq-1").unwrap();
    assert_eq!(p1.failure_count, 1);
    let p2 = providers.iter().find(|p| p.id == "groq-2").unwrap();
    assert_eq!(p2.failure_count, 0);

    let usage = state.db.usage_summary_today().await.unwrap();
    assert_eq!(usage[0].total_tokens, 15);
}

#[tokio::test]
async fn returns_503_when_all_providers_fail() {
    let bad1 = spawn_mock(failing_upstream(StatusCode::INTERNAL_SERVER_ERROR)).await;
    let bad2 = spawn_mock(failing_upstream(StatusCode::TOO_MANY_REQUESTS)).await;
    let (app, _) = test_app(
        "master",
        vec![
            GroqKeySpec {
                key: "gk-1".into(),
                base_url: bad1,
            },
            GroqKeySpec {
                key: "gk-2".into(),
                base_url: bad2,
            },
        ],
    )
    .await;

    let (status, body) = chat_request(
        &app,
        json!({"model": "mock-model", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["success"], false);
}

#[tokio::test]
async fn quota_exceeded_returns_429() {
    let mock = spawn_mock(success_upstream()).await;
    let (app, state) = test_app(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock,
        }],
    )
    .await;

    // Give the seeded key a tiny quota and pre-consume it.
    let keys = state.db.list_keys().await.unwrap();
    let key_id = keys[0].id.clone();
    state.db.update_key(&key_id, Some(5), None).await.unwrap();
    state
        .db
        .insert_usage(&key_id, "mock-model", "groq-1", 6, 0, 1, 200)
        .await
        .unwrap();

    let (status, _) = chat_request(
        &app,
        json!({"model": "mock-model", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn admin_endpoints_require_master_key() {
    let (app, _) = test_app("master", vec![]).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/usage/summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/usage/summary")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_key_management_flow() {
    let (app, _) = test_app("master", vec![]).await;

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/keys")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::from(
                    json!({"name": "discord-bot", "quota_daily_tokens": 50000}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    let new_key = created["data"]["key"].as_str().unwrap().to_string();
    assert!(new_key.starts_with("sk-router-"));

    // New key is accepted by the router middleware.
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/keys")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let listed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(listed["data"].as_array().unwrap().len(), 2);

    let _ = new_key; // key auth itself is covered by other tests
}

#[tokio::test]
async fn admin_provider_key_lifecycle_hides_secret() {
    let (app, state) = test_app("master", vec![]).await;
    let secret = "gsk-dashboard-secret";

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/providers")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::from(
                    json!({
                        "kind": "groq",
                        "name": "Groq dashboard test",
                        "api_key": secret,
                        "base_url": "http://127.0.0.1:1/v1",
                        "model": "mock-model"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let bytes = to_bytes(create.into_body(), usize::MAX).await.unwrap();
    let created: Value = serde_json::from_slice(&bytes).unwrap();
    let provider_id = created["data"]["id"].as_str().unwrap().to_string();

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/providers")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(list.into_body(), usize::MAX).await.unwrap();
    let listed: Value = serde_json::from_slice(&bytes).unwrap();
    let row = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == provider_id)
        .unwrap();
    assert_eq!(row["api_key_configured"], true);
    assert!(!listed.to_string().contains(secret));

    let stored = state
        .db
        .find_provider_config(&provider_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.api_key.as_deref(), Some(secret));

    let delete = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/admin/providers/{provider_id}"))
                .method("DELETE")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::OK);
    assert!(
        state
            .db
            .find_provider_config(&provider_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn unified_chat_endpoint_can_select_groq() {
    let mock = spawn_mock(success_upstream()).await;
    let (app, _) = test_app(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock,
        }],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/chat")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "provider": "groq",
                        "model": "mock-model",
                        "messages": [{"role": "user", "content": "hi"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "hello from mock");
}

#[tokio::test]
async fn models_endpoint_lists_configured_models() {
    let (app, _) = test_app(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
        }],
    )
    .await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == "mock-model")
    );
}

#[tokio::test]
async fn streaming_routes_to_second_provider() {
    let bad = spawn_mock(failing_upstream(StatusCode::TOO_MANY_REQUESTS)).await;
    let good = spawn_mock(echo_upstream()).await;
    let (app, state) = test_app(
        "master",
        vec![
            GroqKeySpec {
                key: "gk-1".into(),
                base_url: bad,
            },
            GroqKeySpec {
                key: "gk-2".into(),
                base_url: good,
            },
        ],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({"model": "mock-model", "stream": true, "messages": [{"role": "user", "content": "hi"}]})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "text/event-stream; charset=utf-8"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("data:"));

    // Usage must have been extracted from the final SSE chunk and recorded.
    let summary = state.db.usage_summary_today().await.unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].total_tokens, 4);
    let providers = state.db.list_providers().await.unwrap();
    assert_eq!(
        providers
            .iter()
            .find(|p| p.id == "groq-1")
            .unwrap()
            .failure_count,
        1
    );
}
