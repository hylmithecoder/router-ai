//! License plate OCR handler using multimodal vision models.

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
};
use serde_json::Value;

use crate::{
    ai::dto::{
        ChatCompletionRequest, ChatContentPart, ChatMessage, ChatMessageContent,
        CompletionOutcome, ImageUrlDetail, LicensePlateData, LicensePlateOcrRequest,
        LicensePlateOcrResponse, RouterError,
    },
    error::AppError,
    middleware::AuthKey,
    state::AppState,
};

/// Supported high-accuracy vision models for license plate OCR.
pub const SUPPORTED_VISION_MODELS: &[&str] = &[
    "google/diffusiongemma-26b-a4b-it",
    "google/gemma-4-31b-it",
    "minimaxai/minimax-m3",
    "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
];

pub const DEFAULT_OCR_MODEL: &str = "google/diffusiongemma-26b-a4b-it";

/// Local agentic OCR order. Each CLI is invoked with a model that is known to
/// be useful for visual OCR, then the handler falls through to cloud vision
/// and the deterministic local ALPR engine if no agent returns a usable plate.
const LOCAL_OCR_AGENT_ORDER: &[(&str, &str)] = &[
    ("agy", "gemini-3.5-flash"),
    ("claude", "haiku"),
    ("codex", "gpt-5.5"),
    ("opencode", "opencode/x-preview-f-free"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalOcrAttempt {
    provider: &'static str,
    model: String,
}

/// Select the local agentic OCR chain for the default route, or one explicit
/// local provider when the caller asks for it. Explicit cloud providers keep
/// their existing behavior and skip this chain.
fn local_ocr_agent_plan(req: &LicensePlateOcrRequest) -> Vec<LocalOcrAttempt> {
    let requested_provider = req
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let provider_is_auto = requested_provider
        .map(|value| value.eq_ignore_ascii_case("auto"))
        .unwrap_or(true);

    let provider = if provider_is_auto {
        req.model
            .as_deref()
            .and_then(canonical_local_provider)
    } else {
        canonical_local_provider(requested_provider.unwrap_or_default())
    };

    if let Some(provider) = provider {
        let model = req
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && !is_agent_model_alias(value))
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| default_local_ocr_model(provider).to_string());
        return vec![LocalOcrAttempt { provider, model }];
    }

    let model_is_auto = req
        .model
        .as_deref()
        .map(|value| {
            let value = value.trim();
            value.is_empty() || value.eq_ignore_ascii_case("auto")
        })
        .unwrap_or(true);
    if provider_is_auto && model_is_auto {
        return LOCAL_OCR_AGENT_ORDER
            .iter()
            .map(|(provider, model)| LocalOcrAttempt {
                provider,
                model: (*model).to_string(),
            })
            .collect();
    }

    Vec::new()
}

fn canonical_local_provider(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "agy" | "agy-cli" => Some("agy"),
        "claude" | "claude-code" => Some("claude"),
        "codex" | "codex-cli" => Some("codex"),
        "opencode" | "open-code" => Some("opencode"),
        _ => None,
    }
}

fn is_agent_model_alias(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("auto")
        || value.eq_ignore_ascii_case("default")
        || value.eq_ignore_ascii_case("router")
        || canonical_local_provider(value).is_some()
}

fn default_local_ocr_model(provider: &str) -> &'static str {
    LOCAL_OCR_AGENT_ORDER
        .iter()
        .find(|(candidate, _)| *candidate == provider)
        .map(|(_, model)| *model)
        .unwrap_or("gpt-5.5")
}

fn build_ocr_chat_request(
    image_url: String,
    prompt: String,
    model: String,
    provider: Option<String>,
) -> ChatCompletionRequest {
    let enable_gemma_thinking = model.to_ascii_lowercase().contains("gemma");
    ChatCompletionRequest {
        model,
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: ChatMessageContent::Text(
                    "You are an expert vehicle license plate OCR and computer vision analyst. Always output pure JSON without markdown formatting.".to_string(),
                ),
            },
            ChatMessage {
                role: "user".to_string(),
                content: ChatMessageContent::Parts(vec![
                    ChatContentPart::Text { text: prompt },
                    ChatContentPart::ImageUrl {
                        image_url: ImageUrlDetail {
                            url: image_url,
                            detail: Some("high".to_string()),
                        },
                    },
                ]),
            },
        ],
        temperature: Some(0.1),
        max_tokens: Some(1024),
        stream: Some(false),
        top_p: None,
        frequency_penalty: None,
        presence_penalty: None,
        stop: None,
        chat_template_kwargs: enable_gemma_thinking
            .then(|| serde_json::json!({ "enable_thinking": true })),
        provider,
    }
}

/// Base URL of the persistent local ALPR server (`python-alpr-local/server.py`).
fn local_alpr_url() -> String {
    std::env::var("ALPR_LOCAL_URL").unwrap_or_else(|_| "http://127.0.0.1:8791".to_string())
}

/// Wall-clock budget for a local ALPR call. Without a bound, a wedged engine
/// holds the HTTP request open indefinitely.
fn local_alpr_timeout() -> std::time::Duration {
    let ms = std::env::var("ALPR_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15_000);
    std::time::Duration::from_millis(ms)
}

/// Absolute path to the CLI entrypoint used when the server is unreachable.
///
/// Resolved from `ALPR_SCRIPT_PATH`, falling back to a path derived from this
/// source file's crate root. The previous relative path only worked when the
/// server happened to be started from the repository root.
fn local_alpr_script() -> std::path::PathBuf {
    std::env::var("ALPR_SCRIPT_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("python-alpr-local")
                .join("main.py")
        })
}

/// Python interpreter for the fallback subprocess.
///
/// Defaults to the `run-python` wrapper that `setup.sh` generates, not to a
/// bare `python3`: the venv's binary wheels need native library paths that only
/// that wrapper sets, and without them importing OpenCV fails outright.
fn local_alpr_python() -> std::path::PathBuf {
    std::env::var("ALPR_PYTHON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("python-alpr-local")
                .join("run-python")
        })
}

/// Path to the training datasets directory.
fn datasets_dir() -> std::path::PathBuf {
    std::env::var("DATASETS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("datasets")
        })
}

/// `POST /api/v1/ocr/licenseplate`
///
/// Recognizes and extracts vehicle license plate details from an image.
/// By default it tries local agentic OCR in priority order, then cloud vision
/// and the deterministic ALPR fallback; explicit provider/model overrides are
/// respected.
pub async fn license_plate_ocr(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthKey>,
    Json(req): Json<LicensePlateOcrRequest>,
) -> Result<Json<LicensePlateOcrResponse>, AppError> {
    let image_input = req.image.trim();
    if image_input.is_empty() {
        return Err(AppError::Validation("image field is required".to_string()));
    }

    let started = std::time::Instant::now();
    let is_fast_local = matches!(
        req.model.as_deref().map(|s| s.to_lowercase()).as_deref(),
        Some("local") | Some("fast") | Some("fast-alpr") | Some("local-alpr") | Some("onnx")
    );
    let for_train = req
        .for_train
        .unwrap_or_else(|| std::env::var("OCR_ALWAYS_HARVEST").map(|v| v == "1" || v == "true").unwrap_or(false));

    if is_fast_local {
        match try_local_alpr_fallback(&state, image_input, &auth.id, started, true, for_train).await {
            Ok(resp) => return Ok(Json(resp)),
            Err(err) => {
                tracing::warn!("explicit fast local alpr execution failed: {err}, continuing to cloud vision");
            }
        }
    }

    let image_url = prepare_image_for_upstream(&state.http, image_input).await;
    let prompt = build_ocr_prompt(req.instruction.as_deref());

    // OCR defaults to the local agentic chain requested by the operator. A
    // model/provider override can still target one local agent or the cloud
    // path directly. A successful process is not enough: only a plausible
    // plate read wins, so a text-only apology or malformed JSON falls through
    // to the next agent.
    let local_ocr_plan = local_ocr_agent_plan(&req);
    let local_agent_was_attempted = !local_ocr_plan.is_empty();
    for attempt in &local_ocr_plan {
        let local_req = build_ocr_chat_request(
            image_url.clone(),
            prompt.clone(),
            attempt.model.clone(),
            Some(attempt.provider.to_string()),
        );
        match state.router.complete(&local_req).await {
            Ok(outcome) => {
                let content = outcome
                    .response
                    .choices
                    .first()
                    .map(|choice| choice.message.content.as_text())
                    .unwrap_or_default();
                let data = parse_license_plate_output(
                    &content,
                    &outcome.model,
                    &outcome.provider_name,
                );

                if !is_plausible_plate(&data.plate_number) {
                    tracing::warn!(
                        provider = attempt.provider,
                        model = %attempt.model,
                        "local agent returned no plausible license plate; trying next OCR provider"
                    );
                    continue;
                }

                if for_train {
                    record_ocr_sample_background(
                        state.db.clone(),
                        state.http.clone(),
                        image_input.to_string(),
                        data.clone(),
                        "agentic",
                    );
                }
                record_ocr_usage(
                    &state,
                    &auth.id,
                    &outcome,
                    started,
                    StatusCode::OK.as_u16() as i64,
                )
                .await;

                return Ok(Json(LicensePlateOcrResponse {
                    success: true,
                    data,
                    usage: outcome.response.usage,
                }));
            }
            Err(err) => {
                tracing::warn!(
                    provider = attempt.provider,
                    model = %attempt.model,
                    error = ?err,
                    "local agent OCR attempt failed; trying next OCR provider"
                );
            }
        }
    }

    // Auto-pick active vision model or use requested model
    let target_model = state
        .router
        .resolve_vision_model(req.model.as_deref())
        .await;

    // Do not repeat the local chain after it has already been exhausted. The
    // cloud pass is intentionally constrained to NVIDIA's vision pool.
    let cloud_provider = if local_agent_was_attempted {
        Some("nvidia".to_string())
    } else {
        req.provider.clone()
    };
    let chat_req = build_ocr_chat_request(image_url, prompt, target_model.clone(), cloud_provider);

    match state.router.complete(&chat_req).await {
        Ok(outcome) => {
            let content = outcome
                .response
                .choices
                .first()
                .map(|c| c.message.content.as_text())
                .unwrap_or_default();

            let data = parse_license_plate_output(
                &content,
                &outcome.model,
                &outcome.provider_name,
            );

            // A 200 from the vision model is not the same as a usable read: it
            // regularly returns an empty or malformed plate. Retry locally so
            // the local engine acts as an accuracy safety net, not just an
            // outage fallback, and keep whichever answer actually parses.
            if !is_plausible_plate(&data.plate_number)
                && let Ok(local) =
                    try_local_alpr_fallback(&state, image_input, &auth.id, started, false, for_train).await
            {
                return Ok(Json(local));
            }

            // Asynchronously harvest dataset sample for training & continuous evaluation if requested
            if for_train {
                record_ocr_sample_background(
                    state.db.clone(),
                    state.http.clone(),
                    image_input.to_string(),
                    data.clone(),
                    "cloud",
                );
            }

            record_ocr_usage(
                &state,
                &auth.id,
                &outcome,
                started,
                StatusCode::OK.as_u16() as i64,
            )
            .await;

            Ok(Json(LicensePlateOcrResponse {
                success: true,
                data,
                usage: outcome.response.usage,
            }))
        }
        Err(err) => {
            let (status, message) = match &err {
                RouterError::NoProviders => (503, "no providers configured".to_string()),
                RouterError::AllProvidersFailed(msg) => {
                    (503, format!("all providers failed: {msg}"))
                }
                RouterError::ClientError(msg)
                    if local_agent_was_attempted
                        && msg.eq_ignore_ascii_case("unknown provider selector: nvidia") =>
                {
                    (
                        503,
                        "all local OCR agents failed and no cloud vision provider is configured"
                            .to_string(),
                    )
                }
                RouterError::ClientError(msg) => (400, msg.clone()),
            };

            // Attempt seamless fallback to Local ALPR Engine when upstream providers are down
            if status == 503
                && let Ok(fallback_resp) =
                    try_local_alpr_fallback(&state, image_input, &auth.id, started, false, for_train).await
            {
                return Ok(Json(fallback_resp));
            }

            let _ = state
                .db
                .insert_usage(
                    &auth.id,
                    &target_model,
                    "none",
                    0,
                    0,
                    started.elapsed().as_millis() as i64,
                    status as i64,
                )
                .await;

            if status == 503 {
                Err(AppError::UpstreamUnavailable(message))
            } else {
                Err(AppError::BadRequest(message))
            }
        }
    }
}

async fn record_ocr_usage(
    state: &crate::state::AppState,
    auth_id: &str,
    outcome: &CompletionOutcome,
    started: std::time::Instant,
    status: i64,
) {
    let _ = state
        .db
        .insert_usage(
            auth_id,
            &outcome.model,
            &outcome.provider_id,
            outcome.response.usage.prompt_tokens,
            outcome.response.usage.completion_tokens,
            started.elapsed().as_millis() as i64,
            status,
        )
        .await;
}

fn record_ocr_sample_background(
    db: crate::database::Db,
    client: reqwest::Client,
    image_input: String,
    data: LicensePlateData,
    source: &'static str,
) {
    // Nothing to learn from a sample with no plate string.
    if data.plate_number.trim().is_empty() {
        return;
    }
    tokio::spawn(async move {
        let sample_id = uuid::Uuid::new_v4().to_string();
        let (filename, image_bytes) =
            extract_image_bytes(&client, &image_input, &sample_id).await;

        let base_dir = datasets_dir();
        let images_dir = base_dir.join("plates").join("images");
        let labels_dir = base_dir.join("plates").join("labels");

        if let Some(bytes) = image_bytes {
            if let Err(e) = tokio::fs::create_dir_all(&images_dir).await {
                tracing::warn!("failed to create datasets images dir: {e}");
            } else {
                let file_path = images_dir.join(&filename);
                if let Err(e) = tokio::fs::write(&file_path, bytes).await {
                    tracing::warn!("failed to save dataset sample image: {e}");
                }
            }

            if let Err(e) = tokio::fs::create_dir_all(&labels_dir).await {
                tracing::warn!("failed to create datasets labels dir: {e}");
            } else {
                let meta_json = serde_json::json!({
                    "id": sample_id,
                    "image_filename": filename,
                    "plate_number": data.plate_number,
                    "vehicle_type": data.vehicle_type,
                    "confidence": data.confidence,
                    "confidence_score": data.confidence_score,
                    "bbox": data.bbox,
                    "raw_text": data.raw_text,
                    "description": data.description,
                    "model": data.model,
                    "provider": data.provider,
                    "source": source,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                });
                let label_file = labels_dir.join(format!("{sample_id}.json"));
                if let Err(e) = tokio::fs::write(
                    &label_file,
                    serde_json::to_vec_pretty(&meta_json).unwrap_or_default(),
                )
                .await
                {
                    tracing::warn!("failed to save dataset sample metadata json: {e}");
                }
            }
        }

        let _ = db
            .insert_ocr_sample(crate::database::NewOcrSample {
                id: &sample_id,
                image_filename: &filename,
                plate_number: &data.plate_number,
                vehicle_type: data.vehicle_type.as_deref(),
                confidence: data.confidence.as_deref(),
                raw_text: data.raw_text.as_deref(),
                description: data.description.as_deref(),
                model: &data.model,
                provider: &data.provider,
                source,
                confidence_score: data.confidence_score,
                bbox: data.bbox,
            })
            .await;
    });
}

async fn extract_image_bytes(
    client: &reqwest::Client,
    image_input: &str,
    sample_id: &str,
) -> (String, Option<Vec<u8>>) {
    use base64::prelude::*;
    let trimmed = image_input.trim();
    if let Some(rest) = trimmed.strip_prefix("data:image/")
        && let Some((mime, b64)) = rest.split_once(";base64,")
    {
        let ext = match mime {
            "png" => "png",
            "webp" => "webp",
            _ => "jpg",
        };
        let filename = format!("{sample_id}.{ext}");
        let bytes = BASE64_STANDARD.decode(b64.trim()).ok();
        return (filename, bytes);
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let filename = format!("{sample_id}.jpg");
        if let Ok(resp) = client
            .get(trimmed)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            && resp.status().is_success()
            && let Ok(bytes) = resp.bytes().await
            && bytes.len() > 100
        {
            return (filename, Some(bytes.to_vec()));
        }
        return (filename, None);
    }
    let filename = format!("{sample_id}.jpg");
    let bytes = BASE64_STANDARD.decode(trimmed).ok();
    (filename, bytes)
}

async fn try_local_alpr_fallback(
    state: &AppState,
    image_input: &str,
    auth_id: &str,
    started: std::time::Instant,
    allow_empty: bool,
    for_train: bool,
) -> Result<LicensePlateOcrResponse, String> {
    // Prefer the persistent server: it holds the ONNX models in memory, so a
    // request costs inference only. Spawning the CLI reloads both models and
    // re-imports OpenCV every time, which dominates the response.
    let raw = match call_local_alpr_server(&state.http, image_input).await {
        Ok(body) => body,
        Err(server_err) => {
            tracing::debug!("local alpr server unavailable ({server_err}), spawning CLI");
            run_local_alpr_cli(image_input).await?
        }
    };

    let data = parse_license_plate_output(&clean_json_codeblock(&raw), "local-alpr-v1", "local");
    if !allow_empty && data.plate_number.is_empty() {
        return Err("local alpr did not detect plate".to_string());
    }

    if for_train {
        record_ocr_sample_background(
            state.db.clone(),
            state.http.clone(),
            image_input.to_string(),
            data.clone(),
            "local",
        );
    }

    let latency = started.elapsed().as_millis() as i64;
    let _ = state
        .db
        .insert_usage(
            auth_id,
            "local-alpr-v1",
            "local",
            0,
            0,
            latency,
            StatusCode::OK.as_u16() as i64,
        )
        .await;

    Ok(LicensePlateOcrResponse {
        success: true,
        data,
        usage: crate::ai::dto::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    })
}

async fn call_local_alpr_server(
    client: &reqwest::Client,
    image_input: &str,
) -> Result<String, String> {
    let response = client
        .post(format!("{}/infer", local_alpr_url()))
        .json(&serde_json::json!({ "image": image_input }))
        .timeout(local_alpr_timeout())
        .send()
        .await
        .map_err(|e| format!("local alpr server request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("local alpr server returned {}", response.status()));
    }

    response
        .text()
        .await
        .map_err(|e| format!("failed to read local alpr server response: {e}"))
}

async fn run_local_alpr_cli(image_input: &str) -> Result<String, String> {
    let script_path = local_alpr_script();
    if !script_path.exists() {
        return Err(format!("local alpr script not found at {}", script_path.display()));
    }

    let mut child = tokio::process::Command::new(local_alpr_python())
        .arg(&script_path)
        .arg("infer")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Without this, dropping the future on timeout leaves the interpreter
        // running and holding its share of the CPU.
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("failed to spawn local alpr process: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(image_input.as_bytes()).await;
    }

    let output = tokio::time::timeout(local_alpr_timeout(), child.wait_with_output())
        .await
        .map_err(|_| "local alpr timed out".to_string())?
        .map_err(|e| format!("local alpr execution failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("local alpr exited with error: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(extract_json_object(&stdout).unwrap_or_else(|| stdout.to_string()))
}

/// Slice out the outermost JSON object from a noisy stream.
///
/// The interpreter can print warnings ahead of the result -- a missing native
/// library, a deprecation notice -- and those lines would otherwise be parsed
/// as if they were the engine's answer.
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| text[start..=end].to_string())
}

#[cfg(test)]
fn normalize_image_url(image_input: &str) -> String {
    if image_input.starts_with("http://")
        || image_input.starts_with("https://")
        || image_input.starts_with("data:")
    {
        image_input.to_string()
    } else {
        // Raw base64 string provided, prepend data URI prefix
        format!("data:image/jpeg;base64,{image_input}")
    }
}

/// Returns true when the URL points to a host that upstream cloud providers
/// (NVIDIA, Groq) cannot reach — localhost, 127.x, 192.168.x, 10.x, etc.
fn is_local_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("://localhost")
        || lower.contains("://127.")
        || lower.contains("://0.0.0.0")
        || lower.contains("://[::1]")
        || lower.contains("://10.")
        || lower.contains("://192.168.")
        || lower.contains("://172.16.")
        || lower.contains("://172.17.")
        || lower.contains("://172.18.")
        || lower.contains("://172.19.")
        || lower.contains("://172.2")
        || lower.contains("://172.3")
}

/// Prepare an image for upstream cloud providers.
///
/// If the image is a local/internal HTTP URL that cloud APIs cannot fetch,
/// download it via the router's own HTTP client and inline it as a base64
/// data URI. Public URLs and existing data URIs are passed through unchanged.
pub async fn prepare_image_for_upstream(client: &reqwest::Client, image_input: &str) -> String {
    let trimmed = image_input.trim();

    // Already a data URI or raw base64 — pass through
    if trimmed.starts_with("data:") {
        return trimmed.to_string();
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return format!("data:image/jpeg;base64,{trimmed}");
    }

    // Public URL that cloud providers can fetch directly
    if !is_local_url(trimmed) {
        return trimmed.to_string();
    }

    // Local/internal URL — download and convert to base64 data URI
    tracing::info!(url = %trimmed, "downloading local image for upstream cloud provider");
    match client
        .get(trimmed)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("image/jpeg")
                .to_string();
            let mime = if content_type.contains("png") {
                "image/png"
            } else if content_type.contains("webp") {
                "image/webp"
            } else {
                "image/jpeg"
            };
            match resp.bytes().await {
                Ok(bytes) if bytes.len() > 100 => {
                    use base64::prelude::*;
                    let b64 = BASE64_STANDARD.encode(&bytes);
                    format!("data:{mime};base64,{b64}")
                }
                _ => {
                    tracing::warn!("local image download returned too few bytes, falling back to raw URL");
                    trimmed.to_string()
                }
            }
        }
        Ok(resp) => {
            tracing::warn!(status = %resp.status(), "local image download failed, falling back to raw URL");
            trimmed.to_string()
        }
        Err(e) => {
            tracing::warn!(error = %e, "local image download error, falling back to raw URL");
            trimmed.to_string()
        }
    }
}

fn build_ocr_prompt(custom_instruction: Option<&str>) -> String {
    let base = r#"Extract and read the vehicle license plate (plat nomor kendaraan) from this image.
Analyze the license plate characters, vehicle type, and any other text visible.

Return ONLY a JSON object with this exact schema:
{
  "plate_number": "string (the primary license plate characters with standard spacing, e.g. 'B 1234 ABC')",
  "vehicle_type": "string (e.g. 'car', 'motorcycle', 'truck', 'bus', 'van', 'unknown')",
  "confidence": "string ('high', 'medium', 'low')",
  "raw_text": "string (all text visible on the plate, including expiration date like '05.28')",
  "description": "string (brief description of the vehicle color and model if visible)"
}"#;

    if let Some(instruction) = custom_instruction.map(str::trim).filter(|s| !s.is_empty()) {
        format!("{base}\n\nAdditional user instruction: {instruction}")
    } else {
        base.to_string()
    }
}

fn parse_license_plate_output(raw_content: &str, model: &str, provider: &str) -> LicensePlateData {
    let clean = clean_json_codeblock(raw_content);
    if let Ok(value) = serde_json::from_str::<Value>(&clean) {
        let plate_number = value
            .get("plate_number")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        let vehicle_type = value
            .get("vehicle_type")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let confidence = value
            .get("confidence")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let raw_text = value
            .get("raw_text")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let description = value
            .get("description")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let confidence_score = value.get("confidence_score").and_then(Value::as_f64);

        let bbox = value.get("bbox").and_then(Value::as_array).and_then(|arr| {
            let coords: Vec<i64> = arr.iter().filter_map(Value::as_i64).collect();
            <[i64; 4]>::try_from(coords.as_slice()).ok()
        });

        return LicensePlateData {
            plate_number,
            vehicle_type,
            confidence,
            raw_text,
            description,
            model: model.to_string(),
            provider: provider.to_string(),
            confidence_score,
            bbox,
        };
    }

    // Fallback: extract plate-like pattern or first clean line if JSON parsing
    // failed. The extracted line is only trusted if it could be a plate at all,
    // otherwise noise on the stream (a Python import error, a model's prose
    // apology) would be reported as the recognized plate number.
    let fallback_plate = extract_plate_fallback(raw_content);
    let fallback_plate = if is_plausible_plate(&fallback_plate) {
        fallback_plate
    } else {
        String::new()
    };
    LicensePlateData {
        plate_number: fallback_plate,
        vehicle_type: None,
        confidence: Some("low".to_string()),
        raw_text: Some(raw_content.trim().to_string()),
        description: None,
        model: model.to_string(),
        provider: provider.to_string(),
        confidence_score: None,
        bbox: None,
    }
}

fn clean_json_codeblock(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("```") {
        let lines: Vec<&str> = s.lines().collect();
        if lines.len() >= 2 {
            let start = 1;
            let end = if lines.last().map(|l| l.trim().starts_with("```")).unwrap_or(false) {
                lines.len() - 1
            } else {
                lines.len()
            };
            return lines[start..end].join("\n").trim().to_string();
        }
    }
    s.to_string()
}

/// Whether a plate string is shaped like a real registration number.
///
/// Used to decide if a cloud read is worth trusting. Deliberately structural
/// rather than strict: it only rejects answers that cannot be a plate at all
/// (empty, no digits, too short), so a valid but unusual plate is never
/// discarded in favour of a local re-read.
fn is_plausible_plate(plate: &str) -> bool {
    let cleaned: String = plate
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();

    cleaned.len() >= 4
        && cleaned.chars().any(|c| c.is_ascii_digit())
        && cleaned.chars().any(|c| c.is_ascii_alphabetic())
}

fn extract_plate_fallback(text: &str) -> String {
    let plate_regex = regex::Regex::new(r"(?i)\b([A-Z]{1,2})\s*([0-9]{1,4})\s*([A-Z]{1,3})\b").ok();
    if let Some(re) = plate_regex
        && let Some(caps) = re.captures(text)
        && let (Some(c1), Some(c2), Some(c3)) = (caps.get(1), caps.get(2), caps.get(3))
    {
        return format!(
            "{} {} {}",
            c1.as_str().to_uppercase(),
            c2.as_str(),
            c3.as_str().to_uppercase()
        );
    }

    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with('{')
            && !trimmed.starts_with('}')
            && !trimmed.starts_with('`')
            && !trimmed.starts_with('"')
            && !trimmed.contains("plate_number")
            && !trimmed.contains(':')
        {
            return trimmed.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_output() {
        let json_text = r#"{
            "plate_number": "B 1234 ABC",
            "vehicle_type": "car",
            "confidence": "high",
            "raw_text": "B 1234 ABC 05.28",
            "description": "Black SUV"
        }"#;
        let data = parse_license_plate_output(json_text, "test-model", "test-provider");
        assert_eq!(data.plate_number, "B 1234 ABC");
        assert_eq!(data.vehicle_type.as_deref(), Some("car"));
        assert_eq!(data.confidence.as_deref(), Some("high"));
        assert_eq!(data.raw_text.as_deref(), Some("B 1234 ABC 05.28"));
    }

    #[test]
    fn parses_markdown_fenced_json() {
        let md_text = "```json\n{\n  \"plate_number\": \"DK 9999 ZZ\",\n  \"vehicle_type\": \"motorcycle\"\n}\n```";
        let data = parse_license_plate_output(md_text, "test-model", "test-provider");
        assert_eq!(data.plate_number, "DK 9999 ZZ");
        assert_eq!(data.vehicle_type.as_deref(), Some("motorcycle"));
    }

    #[test]
    fn parses_local_engine_confidence_and_bbox() {
        let json_text = r#"{
            "plate_number": "BK 5379 WAJ",
            "vehicle_type": "motorcycle",
            "confidence": "high",
            "confidence_score": 0.9421,
            "bbox": [706, 971, 1028, 1111]
        }"#;
        let data = parse_license_plate_output(json_text, "local-alpr-v1", "local");
        assert_eq!(data.plate_number, "BK 5379 WAJ");
        assert_eq!(data.confidence_score, Some(0.9421));
        assert_eq!(data.bbox, Some([706, 971, 1028, 1111]));
    }

    #[test]
    fn cloud_reads_needing_a_local_retry() {
        // Anything a plate cannot be: the local engine gets a second attempt.
        assert!(!is_plausible_plate(""));
        assert!(!is_plausible_plate("  "));
        assert!(!is_plausible_plate("N/A"));
        assert!(!is_plausible_plate("1234"));
        assert!(!is_plausible_plate("ABC"));

        // Real plates, including unusual ones, are kept as-is.
        assert!(is_plausible_plate("B 1234 ABC"));
        assert!(is_plausible_plate("BK6453AMB"));
        assert!(is_plausible_plate("D 12 A"));
    }

    #[test]
    fn default_local_ocr_priority_uses_requested_agent_models() {
        let req = LicensePlateOcrRequest {
            image: "data:image/jpeg;base64,abc".to_string(),
            model: None,
            provider: None,
            instruction: None,
            for_train: None,
        };
        assert_eq!(
            local_ocr_agent_plan(&req),
            vec![
                LocalOcrAttempt {
                    provider: "agy",
                    model: "gemini-3.5-flash".to_string(),
                },
                LocalOcrAttempt {
                    provider: "claude",
                    model: "haiku".to_string(),
                },
                LocalOcrAttempt {
                    provider: "codex",
                    model: "gpt-5.5".to_string(),
                },
                LocalOcrAttempt {
                    provider: "opencode",
                    model: "opencode/x-preview-f-free".to_string(),
                },
            ]
        );
    }

    #[test]
    fn explicit_cloud_provider_skips_local_ocr_chain() {
        let req = LicensePlateOcrRequest {
            image: "data:image/jpeg;base64,abc".to_string(),
            model: None,
            provider: Some("nvidia".to_string()),
            instruction: None,
            for_train: None,
        };
        assert!(local_ocr_agent_plan(&req).is_empty());
    }

    #[test]
    fn explicit_local_ocr_provider_can_override_its_model() {
        let req = LicensePlateOcrRequest {
            image: "data:image/jpeg;base64,abc".to_string(),
            model: Some("custom-vision-model".to_string()),
            provider: Some("claude-code".to_string()),
            instruction: None,
            for_train: None,
        };
        assert_eq!(
            local_ocr_agent_plan(&req),
            vec![LocalOcrAttempt {
                provider: "claude",
                model: "custom-vision-model".to_string(),
            }]
        );
    }

    #[test]
    fn normalizes_image_urls() {
        assert_eq!(
            normalize_image_url("https://example.com/plate.jpg"),
            "https://example.com/plate.jpg"
        );
        assert_eq!(
            normalize_image_url("data:image/png;base64,abc123"),
            "data:image/png;base64,abc123"
        );
        assert_eq!(
            normalize_image_url("abc123rawbase64"),
            "data:image/jpeg;base64,abc123rawbase64"
        );
    }
}
