//! General vision description & OCR analysis handler with auto failover rotation.

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
};
use serde_json::Value;

use crate::{
    ai::dto::{
        ChatCompletionRequest, ChatContentPart, ChatMessage, ChatMessageContent,
        ImageDescriptionData, ImageDescriptionRequest, ImageDescriptionResponse, ImageUrlDetail,
    },
    error::AppError,
    middleware::AuthKey,
    state::AppState,
};

/// Candidate high-performance vision models for image description.
pub const VISION_CANDIDATE_MODELS: &[&str] = &[
    "google/diffusiongemma-26b-a4b-it",
    "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
    "google/gemma-4-31b-it",
    "minimaxai/minimax-m3",
];

pub const DEFAULT_VISION_TIMEOUT_SECS: u64 = 60;

/// `POST /api/v1/ocr/description` & `POST /api/v1/vision/description`
///
/// Generates a comprehensive visual description, OCR text extraction, tags, and
/// safety/moderation flags from an image.
/// Automatically rotates across vision models if a model times out (10-20s) or fails.
pub async fn image_description(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthKey>,
    Json(req): Json<ImageDescriptionRequest>,
) -> Result<Json<ImageDescriptionResponse>, AppError> {
    let image_input = req.image.trim();
    if image_input.is_empty() {
        return Err(AppError::Validation("image field is required".to_string()));
    }

    let image_url = crate::handlers::ocr::prepare_image_for_upstream(&state.http, image_input).await;
    let prompt = build_description_prompt(req.instruction.as_deref());
    let per_model_timeout = req.timeout_secs.unwrap_or(DEFAULT_VISION_TIMEOUT_SECS);

    // The router now rotates vision models and keys internally (and refuses to
    // route an image to a text-only model), so this handler asks once and lets
    // the router work the ladder instead of multiplying attempts N x M.
    let target_model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_string();

    // Some upstreams refuse or fail to fetch remote URLs (Discord CDN links are
    // signed and short-lived, and NVIDIA NIM rejects many hosts outright). When
    // that happens, retry once with the bytes inlined as a data URI.
    let mut image_sources = vec![image_url.clone()];
    let mut inline_retry_available = image_url.starts_with("http");

    let started = std::time::Instant::now();
    let mut last_error_msg = String::from("no vision models available");

    let mut source_index = 0;
    while source_index < image_sources.len() {
        let image_url = image_sources[source_index].clone();
        source_index += 1;
        let target_model = &target_model;
        let chat_req = ChatCompletionRequest {
            model: target_model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: ChatMessageContent::Text(
                        "You are an expert visual analyst and OCR parser. Always respond with pure JSON without markdown formatting.".to_string(),
                    ),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: ChatMessageContent::Parts(vec![
                        ChatContentPart::Text { text: prompt.clone() },
                        ChatContentPart::ImageUrl {
                            image_url: ImageUrlDetail {
                                url: image_url.clone(),
                                detail: Some("high".to_string()),
                            },
                        },
                    ]),
                },
            ],
            temperature: Some(0.2),
            max_tokens: Some(1024),
            stream: Some(false),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            chat_template_kwargs: if target_model.contains("gemma") {
                Some(serde_json::json!({ "enable_thinking": true }))
            } else {
                None
            },
            provider: req.provider.clone(),
        };

        // Enforce per-model timeout (e.g. 15s) for automatic rotation
        let attempt_future = state.router.complete(&chat_req);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(per_model_timeout),
            attempt_future,
        )
        .await;

        match result {
            Ok(Ok(outcome)) => {
                let content = outcome
                    .response
                    .choices
                    .first()
                    .map(|c| c.message.content.as_text())
                    .unwrap_or_default();

                let data = parse_description_output(
                    &content,
                    &outcome.model,
                    &outcome.provider_name,
                );

                let latency = started.elapsed().as_millis() as i64;
                let _ = state
                    .db
                    .insert_usage(
                        &auth.id,
                        &outcome.model,
                        &outcome.provider_id,
                        outcome.response.usage.prompt_tokens,
                        outcome.response.usage.completion_tokens,
                        latency,
                        StatusCode::OK.as_u16() as i64,
                    )
                    .await;

                return Ok(Json(ImageDescriptionResponse {
                    success: true,
                    data,
                    usage: outcome.response.usage,
                }));
            }
            Ok(Err(err)) => {
                let msg = format!("{err:?}");
                tracing::warn!(
                    model = %target_model,
                    error = %msg,
                    "vision attempt failed"
                );
                last_error_msg = format!("model {target_model} failed: {msg}");

                if inline_retry_available && upstream_could_not_load_image(&msg) {
                    inline_retry_available = false;
                    match inline_image_as_data_uri(&state.http, &image_url).await {
                        Some(data_uri) => {
                            tracing::info!(
                                "upstream could not fetch the image URL, retrying with inlined bytes"
                            );
                            image_sources.push(data_uri);
                        }
                        None => tracing::warn!("failed to inline image for upstream retry"),
                    }
                }
            }
            Err(_elapsed) => {
                tracing::warn!(
                    model = %target_model,
                    timeout_secs = per_model_timeout,
                    "vision model timed out (>{}s), rotating to next model in queue",
                    per_model_timeout
                );
                last_error_msg = format!("model {target_model} timed out after {per_model_timeout}s");
            }
        }
    }

    // Attempt local OCR before giving up. This engine only reads text — it
    // cannot judge whether an image is sensitive — so the result is explicitly
    // marked unverified rather than reported as "safe". Callers doing
    // moderation must treat this as "not checked", never as a clean verdict.
    if let Ok(local_text) = run_local_vision_fallback(image_input).await
        && !local_text.trim().is_empty()
    {
        let data = ImageDescriptionData {
            description: format!(
                "Pemindaian visual gagal; hanya teks yang berhasil dibaca secara lokal: {local_text}"
            ),
            extracted_text: Some(local_text),
            tags: vec![
                "local-ocr".to_string(),
                "fallback".to_string(),
                "unverified".to_string(),
            ],
            is_sensitive: false,
            safety_reason: Some(
                "unverified: model visual tidak tersedia, gambar belum dinilai".to_string(),
            ),
            model: "local-vision-v1".to_string(),
            provider: "local".to_string(),
        };
        let latency = started.elapsed().as_millis() as i64;
        let _ = state
            .db
            .insert_usage(
                &auth.id,
                "local-vision-v1",
                "local",
                0,
                0,
                latency,
                StatusCode::OK.as_u16() as i64,
            )
            .await;

        return Ok(Json(ImageDescriptionResponse {
            success: true,
            data,
            usage: crate::ai::dto::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        }));
    }

    // All candidate models failed or timed out
    let latency = started.elapsed().as_millis() as i64;
    let _ = state
        .db
        .insert_usage(
            &auth.id,
            "vision-auto",
            "none",
            0,
            0,
            latency,
            StatusCode::SERVICE_UNAVAILABLE.as_u16() as i64,
        )
        .await;

    Err(AppError::UpstreamUnavailable(format!(
        "all vision models failed or timed out: {last_error_msg}"
    )))
}

async fn run_local_vision_fallback(image_input: &str) -> Result<String, String> {
    let script_path = std::path::Path::new("python-alpr-local/main.py");
    if !script_path.exists() {
        return Err("local alpr script not found".to_string());
    }

    let mut child = tokio::process::Command::new("python3")
        .arg(script_path)
        .arg("infer")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn local alpr process: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(image_input.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| "local vision fallback timed out".to_string())?
    .map_err(|e| format!("local vision process failed: {e}"))?;

    if !output.status.success() {
        return Err("local vision process exited with non-zero status".to_string());
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if let Ok(val) = serde_json::from_str::<Value>(&raw) {
        let text = val
            .get("raw_text")
            .or_else(|| val.get("plate_number"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        // An engine payload with no text read is a failure. Returning the raw
        // JSON here is how a "Failed to decode input image" diagnostic used to
        // reach callers dressed up as extracted image text.
        return if text.is_empty() {
            Err("local vision engine extracted no text".to_string())
        } else {
            Ok(text)
        };
    }
    Ok(raw)
}

/// Does this upstream error mean the provider could not fetch the image URL?
fn upstream_could_not_load_image(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    (m.contains("image") || m.contains("url"))
        && (m.contains("failed to load")
            || m.contains("failed to fetch")
            || m.contains("could not download")
            || m.contains("unable to download")
            || m.contains("invalid image"))
}

/// Download an image and re-encode it as a data URI for upstreams that will not
/// (or cannot) fetch the URL themselves.
async fn inline_image_as_data_uri(client: &reqwest::Client, url: &str) -> Option<String> {
    let resp = client
        .get(url)
        // Plenty of image hosts answer 403 to a bare/default agent.
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .header(reqwest::header::ACCEPT, "image/*,*/*;q=0.8")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("image/"))
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() < 100 {
        return None;
    }

    use base64::prelude::*;
    Some(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(&bytes)
    ))
}




fn build_description_prompt(custom_instruction: Option<&str>) -> String {
    let base = r#"Analyze and describe this image thoroughly.
Extract any visible text (OCR), identify key objects/subjects, context, and evaluate safety/moderation status.

Return ONLY a valid JSON object with this exact schema:
{
  "description": "string (comprehensive, accurate visual description of what is happening in the image)",
  "extracted_text": "string or null (any readable text, typography, subtitles, watermarks, or license plates detected)",
  "tags": ["array", "of", "relevant", "semantic", "tags"],
  "is_sensitive": false,
  "safety_reason": "string or null (detail reason if sensitive, e.g. nsfw, hate, violence)"
}"#;

    if let Some(instruction) = custom_instruction.map(str::trim).filter(|s| !s.is_empty()) {
        format!("{base}\n\nAdditional focus/instruction: {instruction}")
    } else {
        base.to_string()
    }
}

fn parse_description_output(raw_content: &str, model: &str, provider: &str) -> ImageDescriptionData {
    let clean = clean_json_codeblock(raw_content);
    if let Ok(value) = serde_json::from_str::<Value>(&clean) {
        let description = value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();

        let extracted_text = value
            .get("extracted_text")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "null");

        let tags: Vec<String> = value
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let is_sensitive = value
            .get("is_sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let safety_reason = value
            .get("safety_reason")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "null");

        if !description.is_empty() {
            return ImageDescriptionData {
                description,
                extracted_text,
                tags,
                is_sensitive,
                safety_reason,
                model: model.to_string(),
                provider: provider.to_string(),
            };
        }
    }

    // Fallback if model output unstructured plain text
    ImageDescriptionData {
        description: raw_content.trim().to_string(),
        extracted_text: None,
        tags: Vec::new(),
        is_sensitive: false,
        safety_reason: None,
        model: model.to_string(),
        provider: provider.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_description_json() {
        let json_str = r#"{
            "description": "A silver sports car parked next to a mountain road",
            "extracted_text": "SPEED LIMIT 60",
            "tags": ["car", "mountain", "road", "nature"],
            "is_sensitive": false,
            "safety_reason": null
        }"#;

        let data = parse_description_output(json_str, "test-model", "test-provider");
        assert_eq!(data.description, "A silver sports car parked next to a mountain road");
        assert_eq!(data.extracted_text.as_deref(), Some("SPEED LIMIT 60"));
        assert_eq!(data.tags, vec!["car", "mountain", "road", "nature"]);
        assert!(!data.is_sensitive);
    }
}
