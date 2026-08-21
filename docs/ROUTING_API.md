# AI Router API Specification & Client Integration Guide

This guide documents the routing mechanism, available endpoints, model selection strategies, streaming protocol, and instructions for building a native client library for `router-api-ai`.

---

## 1. Architecture & Routing Engine

The AI Router acts as a high-performance, fault-tolerant gateway that fronts multiple upstream HTTP providers (such as **NVIDIA NIM** and **Groq**) as well as locally installed agent CLIs (**OpenCode**, **Claude Code**, **Codex**, **Agy**).

```
┌────────────────────────────────┐
│  Client / Custom Native Lib    │
└───────────────┬────────────────┘
                │ Authorization: Bearer sk-router-...
                ▼
┌────────────────────────────────┐
│      router-api-ai (Axum)      │
│  - API Key & Daily Token Quota │
│  - Cooldown & Failover Tracker │
│  - Usage Analytics (SQLite)    │
└───────┬──────────────┬─────────┘
        │              │
        │ Match Model  │ Match Provider
        ▼              ▼
┌────────────────┐   ┌────────────────┐   ┌───────────────────────────┐
│ NVIDIA NIM     │   │ Groq API       │   │ Local Sandboxed Agents    │
│ - Nemotron 30B │   │ - Llama 3.3 70B│   │ - OpenCode / Claude Code  │
│ - Nemotron 550B│   │ - Mixtral 8x7B │   │ - Codex CLI / Agy CLI     │
└────────────────┘   └────────────────┘   └───────────────────────────┘
```

---

## 2. Endpoints Overview

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `POST` | `/v1/chat/completions` | `Bearer <API_KEY>` | Standard OpenAI-compatible chat completion (JSON + SSE Stream) |
| `POST` | `/api/v1/chat/completions` | `Bearer <API_KEY>` | Unified OpenAI-compatible chat completion with multi-provider scaling |
| `POST` | `/api/v1/chat` | `Bearer <API_KEY>` | Unified router endpoint (defaults to `auto` pool including local agents) |
| `POST` | `/api/v1/ocr/licenseplate` | `Bearer <API_KEY>` | Vehicle License Plate OCR using NVIDIA Multimodal Vision Models |
| `GET` | `/v1/models` / `/api/v1/models` | `Bearer <API_KEY>` | List all active models and provider aliases |
| `GET` | `/health` | Public | Liveness probe (`200 OK`) |
| `GET` | `/health/ready` | Public | Readiness probe (database connectivity) |
| `GET/POST` | `/api/v1/admin/*` | `Bearer <MASTER_KEY>` | Key management, provider credentials, and usage analytics |

---

## 3. Model & Provider Routing Strategies

### Strategy A: Routing by Model Name (Recommended)
You can directly specify the model in the `model` field. The router analyzes the model identifier and routes the request to the upstream provider that natively serves it:

- **NVIDIA Models**:
  - `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning`
  - `nvidia/nemotron-3-ultra-550b-a55b`
  - Any model starting with `nvidia/` or `nemotron`
- **Groq Models**:
  - `llama-3.3-70b-versatile`
  - `llama-3.1-8b-instant`
  - `mixtral-8x7b-32768`
  - Any model starting with `llama`, `groq/`, `mixtral`, `gemma`
- **Local CLI Agents**:
  - `opencode`, `claude`, `codex`, `agy`

#### Example Request (NVIDIA Nemotron 30B):
```bash
curl -X POST http://127.0.0.1:5790/v1/chat/completions \
  -H "Authorization: Bearer sk-router-your-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
    "messages": [
      {"role": "system", "content": "You are an expert AI assistant."},
      {"role": "user", "content": "Explain hybrid Mamba-Transformer architecture."}
    ],
    "temperature": 0.7,
    "max_tokens": 2048
  }'
```

---

### Strategy B: Explicit Provider Selector (`provider`)
You can pass the optional `provider` (or `agent`) parameter in the request payload to explicitly constrain which upstream backend handles the request.

Supported values:
- `"nvidia"`: Routes through configured NVIDIA NIM API keys (with multi-key failover).
- `"groq"`: Routes through configured Groq API keys.
- `"opencode"`: Routes to local OpenCode CLI subprocess.
- `"claude"`: Routes to local Claude Code CLI subprocess.
- `"codex"`: Routes to local Codex CLI subprocess.
- `"agy"`: Routes to local Agy CLI subprocess.
- `"auto"`: Pools all enabled providers, trying HTTP keys first then agent fallbacks.
- `<provider_id>`: Concrete provider ID (e.g. `nvidia-1`, `groq-2`, `nvidia-dashboard-xxx`).

#### Example Request with Provider Selector:
```bash
curl -X POST http://127.0.0.1:5790/api/v1/chat \
  -H "Authorization: Bearer sk-router-your-key" \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "nvidia",
    "model": "nvidia/nemotron-3-ultra-550b-a55b",
    "messages": [
      {"role": "user", "content": "Write a concise summary of quantum computing."}
    ]
  }'
```

---

### Strategy C: Multi-Key Automatic Failover & Cooldown
If you configure multiple keys for a provider (e.g. `NVIDIA_API_KEYS=key1,key2` or `GROQ_API_KEYS=key1,key2`):
1. The router sends the request to the first active key.
2. If the provider returns `429 Too Many Requests`, `5xx Server Error`, or times out:
   - The provider enters a temporary cooldown (configured by `ROUTER_PROVIDER_COOLDOWN_SECS`, default 60s).
   - The router immediately fails over to the next candidate key.
3. The client receives a transparent, uninterrupted response with zero downtime.

---

## 4. Streaming (Server-Sent Events)

When `"stream": true` is set, the router streams chunks using the standard SSE format:

```text
data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1700000000,"model":"nvidia/nemotron-3-nano-omni-30b-a3b-reasoning","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1700000000,"model":"nvidia/nemotron-3-nano-omni-30b-a3b-reasoning","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":25,"total_tokens":37}}

data: [DONE]
```

#### Streaming Request Example:
```bash
curl -N -X POST http://127.0.0.1:5790/v1/chat/completions \
  -H "Authorization: Bearer sk-router-your-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
    "stream": true,
    "messages": [{"role": "user", "content": "Count from 1 to 5."}]
  }'
```

---

## 5. Building a Native Library for this API

When creating a native library (e.g. in Rust, C++, Go, Python, or TypeScript), implement the following client contract:

### 1. Request Data Structure (OpenAI-compatible)
```json
{
  "model": "string (e.g. nvidia/nemotron-3-nano-omni-30b-a3b-reasoning or llama-3.3-70b-versatile)",
  "provider": "optional string (e.g. nvidia, groq, opencode, auto)",
  "messages": [
    {
      "role": "system | user | assistant",
      "content": "string"
    }
  ],
  "temperature": 0.7,
  "max_tokens": 4096,
  "stream": false
}
```

### 2. Response Data Structure (JSON)
```json
{
  "id": "chatcmpl-router-xxx",
  "object": "chat.completion",
  "created": 1700000000,
  "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "The generated answer..."
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 15,
    "completion_tokens": 45,
    "total_tokens": 60
  }
}
```

### 3. Rust Native Client Example Blueprint
```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub messages: Vec<Message>,
    pub stream: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
}

#[derive(Deserialize)]
pub struct Choice {
    pub message: Message,
}

pub struct RouterClient {
    base_url: String,
    api_key: String,
    http: Client,
}

impl RouterClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: Client::new(),
        }
    }

    pub async fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, reqwest::Error> {
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        self.http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await?
            .json::<ChatResponse>()
            .await
    }
}
```

---

## 6. Supported NVIDIA Models Reference

| Model Identifier | Context Length | Architecture / Notes |
|---|---|---|
| `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning` | 262K | Omni-modal reasoning model understanding text, speech, audio, image, and video |
| `nvidia/nemotron-3-ultra-550b-a55b` | 1M | 550B MoE Hybrid Mamba-Transformer frontier reasoning model |

---

## 7. Vehicle License Plate OCR Endpoint (`/api/v1/ocr/licenseplate`)

The dedicated License Plate OCR endpoint automatically utilizes multimodal vision models (e.g. `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning`) to detect, recognize, and structure vehicle license plate details.

### Request Payload:
```json
{
  "image": "data:image/jpeg;base64,... (or raw base64 string or image URL)",
  "instruction": "Optional custom instruction (e.g. Focus on the front motorcycle plate)",
  "model": "optional model override (defaults to nvidia/nemotron-3-nano-omni-30b-a3b-reasoning)",
  "provider": "optional provider override (defaults to nvidia)"
}
```

### Curl Example:
```bash
curl -X POST http://127.0.0.1:5790/api/v1/ocr/licenseplate \
  -H "Authorization: Bearer sk-router-your-key" \
  -H "Content-Type: application/json" \
  -d '{
    "image": "https://example.com/vehicle-front.jpg",
    "instruction": "Baca plat nomor kendaraan ini dengan teliti"
  }'
```

### Successful Response:
```json
{
  "success": true,
  "data": {
    "plate_number": "B 1234 ABC",
    "vehicle_type": "car",
    "confidence": "high",
    "raw_text": "B 1234 ABC 05.28",
    "description": "Black Toyota Avanza front view",
    "model": "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
    "provider": "NVIDIA #1"
  },
  "usage": {
    "prompt_tokens": 125,
    "completion_tokens": 42,
    "total_tokens": 167
  }
}
```

