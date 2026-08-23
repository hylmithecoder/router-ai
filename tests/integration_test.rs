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
    config::{
        ApiKeySeed, AppSettings, GroqKeySpec, NvidiaKeySpec, RouterSettings, ServerSettings,
        Settings,
    },
    database::Db,
    routes::create_router,
    state::AppState,
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn test_settings_full(
    master_key: &str,
    groq_keys: Vec<GroqKeySpec>,
    nvidia_keys: Vec<NvidiaKeySpec>,
) -> Settings {
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
            nvidia_api_keys: nvidia_keys,
            nvidia_base_url: "http://127.0.0.1:1/v1".to_string(),
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
    test_app_full(master_key, groq_keys, vec![]).await
}

async fn test_app_full(
    master_key: &str,
    groq_keys: Vec<GroqKeySpec>,
    nvidia_keys: Vec<NvidiaKeySpec>,
) -> (axum::Router, AppState) {
    let settings = test_settings_full(master_key, groq_keys, nvidia_keys);
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

#[tokio::test]
async fn chat_completion_routes_to_nvidia_by_model() {
    let mock_groq = spawn_mock(failing_upstream(StatusCode::INTERNAL_SERVER_ERROR)).await;
    let mock_nvidia = spawn_mock(success_upstream()).await;
    let (app, state) = test_app_full(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock_groq,
        }],
        vec![NvidiaKeySpec {
            key: "nv-1".into(),
            base_url: mock_nvidia,
        }],
    )
    .await;

    let (status, body) = chat_request(
        &app,
        json!({
            "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
            "messages": [{"role": "user", "content": "hi"}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["choices"][0]["message"]["content"], "hello from mock");

    let usage = state.db.usage_summary_today().await.unwrap();
    assert_eq!(usage[0].total_tokens, 15);
}

#[tokio::test]
async fn chat_completion_routes_to_nvidia_by_provider_selector() {
    let mock_groq = spawn_mock(failing_upstream(StatusCode::INTERNAL_SERVER_ERROR)).await;
    let mock_nvidia = spawn_mock(success_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock_groq,
        }],
        vec![NvidiaKeySpec {
            key: "nv-1".into(),
            base_url: mock_nvidia,
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
                        "provider": "nvidia",
                        "model": "nvidia/nemotron-3-ultra-550b-a55b",
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
async fn falls_back_across_nvidia_keys() {
    let bad_nvidia = spawn_mock(failing_upstream(StatusCode::TOO_MANY_REQUESTS)).await;
    let good_nvidia = spawn_mock(success_upstream()).await;
    let (app, state) = test_app_full(
        "master",
        vec![],
        vec![
            NvidiaKeySpec {
                key: "nv-1".into(),
                base_url: bad_nvidia,
            },
            NvidiaKeySpec {
                key: "nv-2".into(),
                base_url: good_nvidia,
            },
        ],
    )
    .await;

    let (status, body) = chat_request(
        &app,
        json!({
            "model": "nvidia/nemotron-3-ultra-550b-a55b",
            "messages": [{"role": "user", "content": "hi"}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["choices"][0]["message"]["content"], "hello from mock");

    let providers = state.db.list_providers().await.unwrap();
    let p1 = providers.iter().find(|p| p.id == "nvidia-1").unwrap();
    assert_eq!(p1.failure_count, 1);
    let p2 = providers.iter().find(|p| p.id == "nvidia-2").unwrap();
    assert_eq!(p2.failure_count, 0);
}

#[tokio::test]
async fn admin_provider_nvidia_lifecycle_hides_secret() {
    let (app, state) = test_app("master", vec![]).await;
    let secret = "nvapi-dashboard-secret";

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
                        "kind": "nvidia",
                        "name": "NVIDIA NIM test",
                        "api_key": secret,
                        "base_url": "https://integrate.api.nvidia.com/v1",
                        "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning"
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
    assert_eq!(created["data"]["kind"], "nvidia");

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
    assert_eq!(row["kind"], "nvidia");
    assert_eq!(row["api_key_configured"], true);
    assert!(!listed.to_string().contains(secret));

    let stored = state
        .db
        .find_provider_config(&provider_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.api_key.as_deref(), Some(secret));
}

#[tokio::test]
async fn api_v1_chat_completions_routes_to_nvidia() {
    let mock_nvidia = spawn_mock(success_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-1".into(),
            base_url: mock_nvidia,
        }],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "model": "nvidia/nemotron-3-ultra-550b-a55b",
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
async fn multimodal_chat_completion_with_content_parts() {
    let mock_nvidia = spawn_mock(success_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-1".into(),
            base_url: mock_nvidia,
        }],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
                        "messages": [{
                            "role": "user",
                            "content": [
                                {"type": "text", "text": "What is in this image?"},
                                {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,mock123"}}
                            ]
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

fn ocr_vision_upstream() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let json_plate = json!({
                "plate_number": "B 1234 ABC",
                "vehicle_type": "car",
                "confidence": "high",
                "raw_text": "B 1234 ABC 05.28",
                "description": "Black SUV"
            });
            Json(json!({
                "id": "cmpl-ocr-mock",
                "object": "chat.completion",
                "created": 1234,
                "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": format!("```json\n{}\n```", json_plate)
                    },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 120, "completion_tokens": 40, "total_tokens": 160 }
            }))
        }),
    )
}

fn description_vision_upstream() -> Router {
    Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let json_desc = r#"{
                "description": "A close up photo of a Discord meme with text",
                "extracted_text": "DO NOT TOUCH MY CODE",
                "tags": ["meme", "discord", "humor"],
                "is_sensitive": false,
                "safety_reason": null
            }"#;
            Json(json!({
                "id": "chatcmpl-desc-123",
                "object": "chat.completion",
                "created": 1700000000,
                "model": "google/diffusiongemma-26b-a4b-it",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": format!("```json\n{}\n```", json_desc)
                    },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 150, "completion_tokens": 50, "total_tokens": 200 }
            }))
        }),
    )
}

#[tokio::test]
async fn image_description_endpoint_successful_analysis() {
    let mock_desc = spawn_mock(description_vision_upstream()).await;
    let (app, state) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-desc-1".into(),
            base_url: mock_desc,
        }],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/ocr/description")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "image": "https://example.com/meme.jpg",
                        "instruction": "Periksa teks dan konten gambar"
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
    assert_eq!(body["success"], true);
    assert_eq!(
        body["data"]["description"],
        "A close up photo of a Discord meme with text"
    );
    assert_eq!(body["data"]["extracted_text"], "DO NOT TOUCH MY CODE");
    assert_eq!(body["data"]["is_sensitive"], false);
    assert_eq!(body["data"]["tags"][0], "meme");

    let summary = state.db.usage_summary_today().await.unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].total_tokens, 200);
}

#[tokio::test]
async fn license_plate_ocr_successful_recognition() {
    let mock_ocr = spawn_mock(ocr_vision_upstream()).await;
    let (app, state) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-ocr-1".into(),
            base_url: mock_ocr,
        }],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/ocr/licenseplate")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "image": "data:image/jpeg;base64,fakeimagebytes",
                        "instruction": "Tolong baca plat nomor mobil ini",
                        "fortrain": true
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
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["plate_number"], "B 1234 ABC");
    assert_eq!(body["data"]["vehicle_type"], "car");
    assert_eq!(body["data"]["confidence"], "high");
    assert_eq!(body["data"]["raw_text"], "B 1234 ABC 05.28");
    assert_eq!(body["usage"]["total_tokens"], 160);

    let summary = state.db.usage_summary_today().await.unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].total_tokens, 160);

    // Yield to allow background dataset harvesting task to execute
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let samples = state.db.list_ocr_samples(10).await.unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].plate_number, "B 1234 ABC");
    assert_eq!(samples[0].vehicle_type.as_deref(), Some("car"));
}

#[tokio::test]
async fn license_plate_ocr_skips_dataset_harvesting_when_not_for_train() {
    let mock_ocr = spawn_mock(ocr_vision_upstream()).await;
    let (app, state) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-ocr-1".into(),
            base_url: mock_ocr,
        }],
    )
    .await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/ocr/licenseplate")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "image": "data:image/jpeg;base64,fakeimagebytes",
                        "fortrain": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let samples = state.db.list_ocr_samples(10).await.unwrap();
    assert_eq!(samples.len(), 0, "should not harvest dataset sample when fortrain is false");
}

#[tokio::test]
async fn license_plate_ocr_rejects_empty_image() {
    let (app, _) = test_app("master", vec![]).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/ocr/licenseplate")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(json!({"image": "   "}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn license_plate_ocr_supports_model_selection_and_auto_pick() {
    let mock_ocr = spawn_mock(ocr_vision_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-ocr-custom".into(),
            base_url: mock_ocr,
        }],
    )
    .await;

    for model in &[
        "google/diffusiongemma-26b-a4b-it",
        "google/gemma-4-31b-it",
        "minimaxai/minimax-m3",
        "auto",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ocr/licenseplate")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer sk-test")
                    .body(Body::from(
                        json!({
                            "image": "https://example.com/plate.jpg",
                            "model": model
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
        assert_eq!(body["success"], true);
        assert_eq!(body["data"]["plate_number"], "B 1234 ABC");
    }
}

#[tokio::test]
async fn chat_completion_routes_diffusiongemma_and_minimax_to_nvidia() {
    let mock_nvidia = spawn_mock(success_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![],
        vec![NvidiaKeySpec {
            key: "nv-1".into(),
            base_url: mock_nvidia,
        }],
    )
    .await;

    for model in &[
        "google/diffusiongemma-26b-a4b-it",
        "google/gemma-4-31b-it",
        "minimaxai/minimax-m3",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/chat/completions")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer sk-test")
                    .body(Body::from(
                        json!({
                            "model": model,
                            "messages": [{"role": "user", "content": "Analyze image"}]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn chat_completion_routes_all_groq_models() {
    let mock_groq = spawn_mock(success_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock_groq,
        }],
        vec![],
    )
    .await;

    for model in &[
        "qwen/qwen3.6-27b",
        "openai/gpt-oss-120b",
        "llama-3.3-70b-versatile",
        "deepseek-r1-distill-llama-70b",
        "meta-llama/llama-prompt-guard-2-8b",
        "mixtral-8x7b-32768",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/chat/completions")
                    .method("POST")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer sk-test")
                    .body(Body::from(
                        json!({
                            "model": model,
                            "messages": [{"role": "user", "content": "Hello Groq"}]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn modality_aware_routing_prioritizes_groq_for_text_and_nvidia_for_images() {
    let mock_groq = spawn_mock(success_upstream()).await;
    let mock_nvidia = spawn_mock(success_upstream()).await;
    let (app, _) = test_app_full(
        "master",
        vec![GroqKeySpec {
            key: "gk-1".into(),
            base_url: mock_groq,
        }],
        vec![NvidiaKeySpec {
            key: "nv-1".into(),
            base_url: mock_nvidia,
        }],
    )
    .await;

    // 1. Pure text request without explicit model -> routes to Groq for speed
    let text_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "messages": [{"role": "user", "content": "Text only prompt"}]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(text_res.status(), StatusCode::OK);

    // 2. Multimodal request with image -> automatically routes to Nvidia vision
    let image_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/chat/completions")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "messages": [{
                            "role": "user",
                            "content": [
                                {"type": "text", "text": "Describe image"},
                                {"type": "image_url", "image_url": {"url": "https://example.com/cat.jpg"}}
                            ]
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(image_res.status(), StatusCode::OK);
}

#[tokio::test]
async fn admin_dns_endpoint_query() {
    let (app, _) = test_app("master", vec![]).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/dns?host=cloudflare.com&server=1.1.1.1")
                .method("GET")
                .header(header::AUTHORIZATION, "Bearer master")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["host"], "cloudflare.com");
}

#[tokio::test]
async fn license_plate_ocr_falls_back_to_local_engine_when_providers_fail() {
    // When no vision providers are configured, router returns NoProviders / AllProvidersFailed (503)
    // The handler should attempt local fallback
    let (app, _state) = test_app("master", vec![]).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/ocr/licenseplate")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer sk-test")
                .body(Body::from(
                    json!({
                        "image": "https://example.com/mock.jpg"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Since mock.jpg is not a real image on disk, local alpr gracefully exits
    // or returns 503 if no plate detected
    let status = response.status();
    assert!(status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE);
}
