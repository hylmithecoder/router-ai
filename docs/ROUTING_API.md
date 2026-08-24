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
| `POST` | `/api/v1/ocr/description` | `Bearer <API_KEY>` | Visual description, OCR extraction, and safety tags (aliased at `/api/v1/vision/description`) |
| `POST` | `/api/v1/ocr/licenseplate` | `Bearer <API_KEY>` | Vehicle License Plate OCR using NVIDIA vision, local agent, and ALPR fallback |
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

### Strategy C: Two-Level Automatic Failover & Cooldown

Failover happens on two levels, because "this model is wrong" and "this key is
dead" are different problems and were previously treated the same.

**Level 1 — model ladder (within one provider).** Each provider is tried against
its configured model first, then the fallback ladder for its kind. A `400` or
`404` means the model id is wrong or the request shape does not suit it; the
router rotates to the next model on the *same* key and **does not** put that key
in cooldown.

**Level 2 — key rotation (across providers).** A `429`, `401/403`, `5xx`,
timeout, or network error is the key's problem. That provider enters cooldown
(`ROUTER_PROVIDER_COOLDOWN_SECS`, default 60s) and the router moves to the next
candidate key.

**Saturation is both.** NVIDIA NIM reports per-model worker saturation as a 503
(`ResourceExhausted: Worker local total request limit reached (16/16)`). One
model being full does not mean the key is dead, so the router works through the
rest of the ladder first — and only then puts the key in cooldown.

**Last resort.** If every candidate is enabled but sitting in cooldown, the
router retries them anyway rather than returning `503`. A burst of failures can
never leave a request with nothing to try.

Providers that cannot possibly work — no API key configured, or a local CLI
whose binary is not installed — are skipped up front instead of consuming a
failover slot.

Each individual provider+model attempt is bounded by
`ROUTER_ATTEMPT_TIMEOUT_SECS` (default 90s) so one wedged upstream cannot hold
the whole request open.

The ladders are configurable:

| Variable | Applies to | Default |
|---|---|---|
| `ROUTER_GROQ_FALLBACK_MODELS` | Groq keys, text requests | `openai/gpt-oss-120b,llama-3.3-70b-versatile,openai/gpt-oss-20b` |
| `ROUTER_NVIDIA_FALLBACK_MODELS` | NVIDIA keys, text requests | `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning,nvidia/nemotron-3-ultra-550b-a55b,google/gemma-4-31b-it` |
| `ROUTER_VISION_FALLBACK_MODELS` | Any request containing an image | `google/diffusiongemma-26b-a4b-it,nvidia/nemotron-3-nano-omni-30b-a3b-reasoning,google/gemma-4-31b-it,minimaxai/minimax-m3` |

A request containing an image is only ever offered to vision-capable models and
providers. The license-plate OCR handler constrains its first pass to NVIDIA,
then uses local agents only after that cloud pass is exhausted.

---

### Strategy D: Local Agent CLIs

When every HTTP key is exhausted, `auto` degrades to the locally installed agent
CLIs. They are discovered from `PATH` at startup and sorted last in the pool.

| Selector | CLI | Invocation |
|---|---|---|
| `agy` | Agy CLI | `--sandbox --disable-slash-commands --output-format text [--add-dir DIR] [--model M] --effort E --print=<prompt>` |
| `claude` | Claude Code | `--print --output-format json --no-session-persistence --system-prompt … --tools Read --add-dir DIR --permission-mode dontAsk [--model M] <prompt>` |
| `codex` | Codex CLI | `exec --ephemeral --sandbox read-only --skip-git-repo-check --ignore-user-config --color never [--model M] --image PATH -` (prompt on stdin) |
| `opencode` | OpenCode | `run <prompt> --format json --pure [--model M] --file PATH` |

Every agent is run non-interactively, sandboxed/read-only where the CLI offers
it, with only the minimum read access needed for image OCR and slash-command
expansion off so message content cannot trigger unrelated actions.

Because agents are the last resort, `model: "auto"` makes them run on their
cheapest model rather than whatever the CLI defaults to:

| Variable | Default |
|---|---|
| `ROUTER_AGY_MODEL` | `gemini-3.5-flash` |
| `ROUTER_AGY_EFFORT` | `low` |
| `ROUTER_CLAUDE_MODEL` | `haiku` |
| `ROUTER_CODEX_MODEL` | `gpt-5.5` |
| `ROUTER_OPENCODE_MODEL` | *(CLI default)* |
| `ROUTER_OPENCODE_VISION_MODEL` | `opencode/x-preview-f-free` |
| `ROUTER_CODEX_VISION_MODEL` | `gpt-5.5` |

**Image attachments.** OpenCode (`--file`) and Codex (`--image`) receive native
attachments. Agy and Claude Code receive a read-only `--add-dir` plus the
materialized image path in the prompt, allowing their agent read tools to
inspect the image. The router downloads or base64-decodes each image into a
temporary directory and deletes it when the process exits.

**License-plate OCR priority.** When `provider` and `model` are omitted (or
`provider: "auto"` / `model: "auto"`), `/api/v1/ocr/licenseplate` first tries
the configured NVIDIA API pool, including its key/model failover on upstream
errors. Only if that cloud pass fails or returns no plausible plate does it try:

1. Agy — `gemini-3.5-flash`
2. Claude Code — `haiku`
3. Codex — `gpt-5.5`
4. OpenCode — `opencode/x-preview-f-free`

If all four fail or return no usable plate, the handler finally tries the local
ALPR engine. An explicit local provider selector bypasses NVIDIA and targets
that one agent directly.

Note that `--file` and `--image` are variadic, so the prompt is passed *before*
them; a prompt placed afterwards is swallowed as another filename.

Agent CLIs receive the chat request flattened into a single prompt. The caller's
`system` messages are passed through the CLI's own system-prompt flag where one
exists (Claude Code), and otherwise framed as the agent's configuration for the
request. They are deliberately **not** rendered as `SYSTEM:` / `USER:` blocks:
coding agents recognize that shape as an injected instruction frame and refuse
the task instead of doing it.

---

## 3b. Vision: `POST /api/v1/ocr/description`

```json
{
  "image": "https://cdn.discordapp.com/attachments/... | data:image/png;base64,...",
  "instruction": "Evaluasi moderasi visual untuk server Discord.",
  "model": "auto",
  "timeout_secs": 200
}
```

Routing notes:

- The request is only ever offered to vision-capable models. A text-only model
  is never handed an image, whatever `model` or `provider` says.
- If the upstream cannot fetch the URL — expiring Discord CDN links, hosts that
  answer `403` to datacenter IPs — the router retries once with the image bytes
  downloaded and inlined as a `data:` URI.
- `image` may be a URL, a `data:` URI, or bare base64.

### Failure semantics for moderation callers

When every vision model fails, the router falls back to the local OCR engine.
That engine reads text; it **cannot judge whether an image is sensitive**. Its
result is therefore returned as:

```json
{
  "is_sensitive": false,
  "safety_reason": "unverified: model visual tidak tersedia, gambar belum dinilai",
  "tags": ["local-ocr", "fallback", "unverified"],
  "provider": "local"
}
```

`is_sensitive: false` here means **"nobody looked"**, not "this is fine". Any
moderation client must check for `provider == "local"` or the `unverified` tag
and treat the attachment as *not checked* — routing it to human review rather
than passing it. Reporting it as a clean verdict would let anything through
during an outage. If the local engine reads no text at all, the endpoint returns
`503` instead of inventing a result.

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
