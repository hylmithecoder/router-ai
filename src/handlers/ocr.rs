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
        ImageUrlDetail, LicensePlateData, LicensePlateOcrRequest, LicensePlateOcrResponse,
        RouterError,
    },
    error::AppError,
    middleware::AuthKey,
    state::AppState,
};

use crate::ai::router::is_nvidia_model;

/// Supported high-accuracy vision models for license plate OCR.
pub const SUPPORTED_VISION_MODELS: &[&str] = &[
    "google/diffusiongemma-26b-a4b-it",
    "google/gemma-4-31b-it",
    "minimaxai/minimax-m3",
    "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
];

pub const DEFAULT_OCR_MODEL: &str = "google/diffusiongemma-26b-a4b-it";

/// `POST /api/v1/ocr/licenseplate`
///
/// Recognizes and extracts vehicle license plate details from an image.
/// Automatically selects an active vision model or uses the explicitly requested model.
pub async fn license_plate_ocr(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthKey>,
    Json(req): Json<LicensePlateOcrRequest>,
) -> Result<Json<LicensePlateOcrResponse>, AppError> {
    let image_input = req.image.trim();
    if image_input.is_empty() {
        return Err(AppError::Validation("image field is required".to_string()));
    }

    let image_url = normalize_image_url(image_input);
    let prompt = build_ocr_prompt(req.instruction.as_deref());

    // Auto-pick active vision model or use requested model
    let target_model = state
        .router
        .resolve_vision_model(req.model.as_deref())
        .await;

    let chat_req = ChatCompletionRequest {
        model: target_model.clone(),
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
        chat_template_kwargs: if target_model.contains("gemma") {
            Some(serde_json::json!({ "enable_thinking": true }))
        } else {
            None
        },
        provider: req.provider.clone().or_else(|| {
            if is_nvidia_model(&target_model) {
                Some("nvidia".to_string())
            } else {
                Some("auto".to_string())
            }
        }),
    };

    let started = std::time::Instant::now();
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
                RouterError::ClientError(msg) => (400, msg.clone()),
            };
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

        if !plate_number.is_empty() {
            return LicensePlateData {
                plate_number,
                vehicle_type,
                confidence,
                raw_text,
                description,
                model: model.to_string(),
                provider: provider.to_string(),
            };
        }
    }

    // Fallback: extract plate-like pattern or first line if JSON parsing failed
    let fallback_plate = extract_plate_fallback(raw_content);
    LicensePlateData {
        plate_number: fallback_plate,
        vehicle_type: None,
        confidence: Some("medium".to_string()),
        raw_text: Some(raw_content.trim().to_string()),
        description: None,
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

fn extract_plate_fallback(text: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('{') && !trimmed.starts_with('`') {
            return trimmed.to_string();
        }
    }
    text.trim().chars().take(20).collect()
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
