//! OpenAI-compatible request/response DTOs.
//!
//! These mirror the OpenAI chat completions API so any OpenAI SDK or plain HTTP
//! client can talk to the router without changes.

use serde::{Deserialize, Serialize};

/// `POST /v1/chat/completions` request body.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    #[serde(default)]
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Optional reasoning / thinking configuration used by models like Google DiffusionGemma / Gemma-4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<serde_json::Value>,
    /// Optional router-level provider selector. It is accepted by the router
    /// but never forwarded to an upstream provider.
    #[serde(default, alias = "agent", skip_serializing)]
    pub provider: Option<String>,
}

impl ChatCompletionRequest {
    /// Returns true if any message in the request contains image content.
    pub fn has_images(&self) -> bool {
        self.messages.iter().any(|m| m.content.has_images())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatMessageContent,
}

impl ChatMessage {
    pub fn user(content: impl Into<ChatMessageContent>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<ChatMessageContent>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<ChatMessageContent>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// Message content supporting either a plain text string or multimodal parts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

impl ChatMessageContent {
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(parts) => {
                let mut out = String::new();
                for part in parts {
                    if let ChatContentPart::Text { text } = part {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(text);
                    }
                }
                out
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text.as_str()),
            Self::Parts(_) => None,
        }
    }

    /// Returns true if this message content contains any image parts.
    pub fn has_images(&self) -> bool {
        match self {
            Self::Text(_) => false,
            Self::Parts(parts) => parts
                .iter()
                .any(|p| matches!(p, ChatContentPart::ImageUrl { .. })),
        }
    }
}

impl From<&str> for ChatMessageContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl From<String> for ChatMessageContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<Vec<ChatContentPart>> for ChatMessageContent {
    fn from(parts: Vec<ChatContentPart>) -> Self {
        Self::Parts(parts)
    }
}

/// Multimodal content part (text or image_url).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlDetail },
}

impl ChatContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image_url(url: impl Into<String>) -> Self {
        Self::ImageUrl {
            image_url: ImageUrlDetail {
                url: url.into(),
                detail: None,
            },
        }
    }

    pub fn image_url_with_detail(url: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::ImageUrl {
            image_url: ImageUrlDetail {
                url: url.into(),
                detail: Some(detail.into()),
            },
        }
    }
}

/// Image URL detail payload for multimodal vision models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrlDetail {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `POST /api/v1/ocr/licenseplate` request payload.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LicensePlateOcrRequest {
    /// Base64 string, data URL (e.g. data:image/jpeg;base64,...), or remote HTTP(S) image URL.
    #[serde(alias = "image_url", alias = "image_base64")]
    pub image: String,
    /// Optional model override (defaults to `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning` or configured vision provider).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional provider override (defaults to `nvidia` or `auto`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional additional instruction / prompt override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    /// Optional flag: if true, saves the image & metadata to `datasets/` for training/fine-tuning.
    #[serde(default, alias = "fortrain", alias = "for_train", alias = "is_train", alias = "train", alias = "dataset")]
    pub for_train: Option<bool>,
}

/// Structured vehicle license plate recognition result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicensePlateData {
    /// The recognized primary license plate number (e.g., "B 1234 ABC").
    pub plate_number: String,
    /// Vehicle type if identified (e.g., "car", "motorcycle", "truck", "bus", "unknown").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vehicle_type: Option<String>,
    /// Confidence assessment ("high", "medium", "low").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Raw unformatted plate text or secondary lines (e.g. expiration date "05.28").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_text: Option<String>,
    /// Brief description of vehicle / context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Upstream model used for OCR.
    pub model: String,
    /// Upstream provider used for OCR.
    pub provider: String,
    /// Measured 0..1 recognition confidence. Only the local ALPR engine reports
    /// one; cloud vision models return the coarse `confidence` label instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_score: Option<f64>,
    /// Detected plate region as `[x1, y1, x2, y2]` in source-image pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[i64; 4]>,
}

/// Standard envelope for license plate OCR responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePlateOcrResponse {
    pub success: bool,
    pub data: LicensePlateData,
    #[serde(default)]
    pub usage: Usage,
}

/// Request for general vision description and OCR text analysis.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageDescriptionRequest {
    /// Base64-encoded image string, data URI (`data:image/...`), or public HTTP/HTTPS image URL.
    pub image: String,
    /// Optional instruction to steer what aspects to describe or analyze.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    /// Optional target model. Defaults to "auto" with failover rotation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Explicit provider selector ("nvidia", "auto", etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Optional timeout in seconds before switching to the next vision model (default: 15s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Optional flag: if true, saves the image & metadata to `datasets/` for training/fine-tuning.
    #[serde(default, alias = "fortrain", alias = "for_train", alias = "is_train", alias = "train", alias = "dataset")]
    pub for_train: Option<bool>,
}

/// Structured outcome of general visual description & text extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDescriptionData {
    /// Comprehensive description of the image content and visual elements.
    pub description: String,
    /// Any text, OCR, typography, or captions visible in the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extracted_text: Option<String>,
    /// Semantic keywords / tags describing the image.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether the image contains sensitive/inappropriate content (NSFW, gore, hate symbols).
    pub is_sensitive: bool,
    /// Reason explaining why the image was flagged as sensitive, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_reason: Option<String>,
    /// Upstream vision model that produced this description.
    pub model: String,
    /// Upstream provider name.
    pub provider: String,
}

/// Standard envelope for vision description responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDescriptionResponse {
    pub success: bool,
    pub data: ImageDescriptionData,
    #[serde(default)]
    pub usage: Usage,
}

/// Non-streaming response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

/// Token usage reported by the upstream provider.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: i64,
    #[serde(default)]
    pub completion_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
}

/// Model descriptor for `GET /v1/models`.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

/// Result of a routed non-streaming completion.
#[derive(Debug, Clone)]
pub struct CompletionOutcome {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub response: ChatCompletionResponse,
}

/// Source of a routed streaming completion.
#[derive(Debug)]
pub enum StreamSource {
    /// An upstream OpenAI-compatible SSE response.
    Http(reqwest::Response),
    /// A local CLI completion represented as one synthesized SSE response.
    Completed(ChatCompletionResponse),
}

/// Result of a routed streaming completion.
#[derive(Debug)]
pub struct StreamOutcome {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub source: StreamSource,
}

/// Errors that happen before any provider produced a result.
#[derive(Debug)]
pub enum RouterError {
    /// No providers are configured at all.
    NoProviders,
    /// Every provider failed; the last error is attached.
    AllProvidersFailed(String),
    /// The request itself was rejected by the upstream (client-side 4xx).
    ClientError(String),
}
