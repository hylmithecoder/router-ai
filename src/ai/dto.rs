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
    /// Optional router-level provider selector. It is accepted by the router
    /// but never forwarded to an upstream provider.
    #[serde(default, alias = "agent", skip_serializing)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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
