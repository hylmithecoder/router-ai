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

    let image_url = normalize_image_url(image_input);
    let prompt = build_description_prompt(req.instruction.as_deref());
    let per_model_timeout = req.timeout_secs.unwrap_or(DEFAULT_VISION_TIMEOUT_SECS);

    // Build the ordered model candidates to attempt
    let mut model_queue: Vec<String> = Vec::new();
    if let Some(m) = req.model.as_deref().map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto")) {
        model_queue.push(m.to_string());
    }
    for &candidate in VISION_CANDIDATE_MODELS {
        if !model_queue.iter().any(|m| m.eq_ignore_ascii_case(candidate)) {
            model_queue.push(candidate.to_string());
        }
    }

    let started = std::time::Instant::now();
    let mut last_error_msg = String::from("no vision models available");

    for target_model in &model_queue {
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
                    "vision model attempt failed, failing over to next model in queue"
                );
                last_error_msg = format!("model {target_model} failed: {msg}");
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

fn normalize_image_url(image_input: &str) -> String {
    if image_input.starts_with("http://")
        || image_input.starts_with("https://")
        || image_input.starts_with("data:")
    {
        image_input.to_string()
    } else {
        format!("data:image/jpeg;base64,{image_input}")
    }
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
