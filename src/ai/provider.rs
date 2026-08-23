//! Provider abstraction for HTTP APIs and locally installed agent CLIs.
//!
//! Groq entries use an OpenAI-compatible HTTP request. OpenCode, Codex, Claude
//! Code, and Agy are deliberately executed as bounded, non-interactive
//! subprocesses. The router never invokes a shell and uses read-only/sandboxed
//! flags wherever the CLI exposes them.

use std::{fmt, path::Path, process::Stdio};

use chrono::Utc;
use serde_json::Value;
use tokio::{process::Command, time::Duration};
use uuid::Uuid;

use crate::{
    ai::dto::{ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Usage},
    database::StoredProvider,
};

/// The execution backend for a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Groq,
    Nvidia,
    OpenCode,
    Codex,
    Claude,
    Agy,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::Nvidia => "nvidia",
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Agy => "agy",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "groq" => Some(Self::Groq),
            "nvidia" | "nim" | "build-nvidia" => Some(Self::Nvidia),
            "opencode" | "open-code" => Some(Self::OpenCode),
            "codex" | "codex-cli" => Some(Self::Codex),
            "claude" | "claude-code" => Some(Self::Claude),
            "agy" | "agy-cli" => Some(Self::Agy),
            _ => None,
        }
    }

    pub fn is_cli(self) -> bool {
        matches!(
            self,
            Self::OpenCode | Self::Codex | Self::Claude | Self::Agy
        )
    }

    /// Local agents that can be handed image attachments on the command line,
    /// and so may serve a vision request when every cloud model is exhausted.
    pub fn cli_supports_images(self) -> bool {
        matches!(self, Self::OpenCode | Self::Codex)
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single configured upstream or local provider.
#[derive(Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// Present only for HTTP providers. The value is never serialized in the
    /// admin response and is redacted from `Debug` output.
    pub api_key: Option<String>,
    /// Absolute path or executable name for local CLI providers.
    pub command: Option<String>,
    /// HTTP model default or local CLI model alias (`default` when unset).
    pub model: String,
    /// Disabled providers are skipped by the router.
    pub enabled: bool,
    /// Local agent working directory.
    pub agent_workdir: String,
    /// Maximum local process duration.
    pub cli_timeout_secs: u64,
    /// In-memory cooldown; providers in cooldown are skipped.
    cooldown_until: Option<std::time::Instant>,
}

impl fmt::Debug for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key_configured", &self.api_key.is_some())
            .field("command", &self.command)
            .field("model", &self.model)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Why a call to a provider failed. The router decides fallback based on this.
#[derive(Debug)]
pub enum ProviderError {
    /// 429 from upstream (per-key rate limit) — good candidate for fallback.
    RateLimited(String),
    /// 401/403 — the key is invalid.
    Auth(String),
    /// 404 — model unknown to this provider; try the next one.
    NotFound(String),
    /// 400/422 — the request shape or the model id is wrong for this provider.
    /// The key itself is healthy, so the router retries another model on the
    /// same provider instead of putting the key in cooldown.
    BadRequest(String),
    /// 5xx upstream or a non-zero local CLI exit.
    Upstream(String),
    /// Network / timeout / transport / missing executable error.
    Network(String),
}

impl ProviderError {
    /// Is this failure caused by the model/request rather than by the key?
    ///
    /// These are worth retrying against a different model on the *same*
    /// provider; they say nothing about whether the credential still works.
    pub fn is_model_specific(&self) -> bool {
        match self {
            Self::BadRequest(_) | Self::NotFound(_) => true,
            // NVIDIA NIM reports per-model worker saturation as a 503
            // ("ResourceExhausted: Worker local total request limit reached
            // (16/16)"). That one model is full; a different one on the same
            // key usually still answers, so keep working the ladder.
            Self::Upstream(message) => {
                let m = message.to_ascii_lowercase();
                m.contains("resourceexhausted")
                    || m.contains("worker local total request limit")
                    || m.contains("no healthy upstream")
                    || m.contains("model is currently loading")
            }
            _ => false,
        }
    }

    /// Should this failure count against the *key* (cooldown), as opposed to
    /// just the model that was tried?
    ///
    /// A wrong or retired model id says nothing about the credential. Worker
    /// saturation does — once every model on that key is full, it deserves a
    /// rest — so it advances the ladder *and* still blames the key at the end.
    pub fn blames_key(&self) -> bool {
        !matches!(self, Self::BadRequest(_) | Self::NotFound(_))
    }
}

impl Provider {
    pub fn new_http(
        id: &str,
        name: &str,
        kind: ProviderKind,
        base_url: &str,
        api_key: Option<&str>,
        model: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            base_url: base_url.to_string(),
            api_key: api_key.map(ToOwned::to_owned),
            command: None,
            model: model.to_string(),
            enabled: true,
            agent_workdir: ".".to_string(),
            cli_timeout_secs: 120,
            cooldown_until: None,
        }
    }

    pub fn new_cli(
        id: &str,
        name: &str,
        kind: ProviderKind,
        command: &str,
        model: &str,
        agent_workdir: &str,
        cli_timeout_secs: u64,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            base_url: String::new(),
            api_key: None,
            command: Some(command.to_string()),
            model: model.to_string(),
            enabled: true,
            agent_workdir: agent_workdir.to_string(),
            cli_timeout_secs,
            cooldown_until: None,
        }
    }

    /// Rehydrate a provider loaded from SQLite.
    pub fn from_stored(
        stored: StoredProvider,
        agent_workdir: &str,
        cli_timeout_secs: u64,
    ) -> Option<Self> {
        let kind = ProviderKind::parse(&stored.kind)?;
        Some(Self {
            id: stored.id,
            name: stored.name,
            kind,
            base_url: stored.base_url,
            api_key: stored.api_key,
            command: stored.command,
            model: stored.model,
            enabled: stored.enabled,
            agent_workdir: agent_workdir.to_string(),
            cli_timeout_secs,
            cooldown_until: None,
        })
    }

    /// Is this provider currently skipping requests due to a recent failure?
    pub fn in_cooldown(&self) -> bool {
        self.cooldown_until
            .map(|t| t > std::time::Instant::now())
            .unwrap_or(false)
    }

    pub fn enter_cooldown(&mut self, secs: u64) {
        self.cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(secs));
    }

    pub fn clear_cooldown(&mut self) {
        self.cooldown_until = None;
    }

    pub fn is_available(&self) -> bool {
        if self.kind.is_cli() {
            self.command.as_deref().is_some_and(command_is_available)
        } else {
            self.api_key.as_ref().is_some_and(|key| !key.is_empty())
                && !self.base_url.trim().is_empty()
        }
    }

    /// POST `/chat/completions` for HTTP providers, or execute a local CLI.
    pub async fn chat_completion(
        &self,
        client: &reqwest::Client,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        if self.kind.is_cli() {
            return self.run_cli(client, req).await;
        }

        let forwarded = self.normalized_request(req);
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let is_stream = req.stream.unwrap_or(false);
        let accept_header = if is_stream {
            "text/event-stream"
        } else {
            "application/json"
        };

        let mut request = client
            .post(&url)
            .header(reqwest::header::ACCEPT, accept_header)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&forwarded);
        if let Some(key) = self.api_key.as_deref() {
            request = request.bearer_auth(key);
        }
        let resp = request
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = resp.status();
        if status.is_success() {
            return resp
                .json::<ChatCompletionResponse>()
                .await
                .map_err(|e| ProviderError::Upstream(format!("bad body: {e}")));
        }

        let body = resp.text().await.unwrap_or_default();
        Err(classify_error(status.as_u16(), body))
    }

    /// POST `/chat/completions` with `stream: true` for HTTP providers.
    pub async fn stream_chat(
        &self,
        client: &reqwest::Client,
        req: &ChatCompletionRequest,
    ) -> Result<reqwest::Response, ProviderError> {
        if self.kind.is_cli() {
            return Err(ProviderError::Network(
                "local CLI providers use synthesized streaming".to_string(),
            ));
        }

        let forwarded = self.normalized_request(req);
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut request = client
            .post(&url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&forwarded);
        if let Some(key) = self.api_key.as_deref() {
            request = request.bearer_auth(key);
        }
        let resp = request
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let body = resp.text().await.unwrap_or_default();
        Err(classify_error(status.as_u16(), body))
    }

    fn normalized_request(&self, req: &ChatCompletionRequest) -> ChatCompletionRequest {
        let mut forwarded = req.clone();
        forwarded.provider = None;
        if forwarded.model.trim().is_empty()
            || is_provider_alias(&self.id, self.kind, &forwarded.model)
        {
            forwarded.model = self.model.clone();
        }
        forwarded
    }

    async fn run_cli(
        &self,
        client: &reqwest::Client,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        let command = self.command.as_deref().ok_or_else(|| {
            ProviderError::Network("local CLI command is not configured".to_string())
        })?;
        if !command_is_available(command) {
            return Err(ProviderError::Network(format!(
                "CLI binary is not available: {command}"
            )));
        }

        // Agent CLIs take images as files on disk, so anything the request
        // carries inline or by URL has to be materialized first. The guard
        // lives here (rather than at the call site) so a text-only agent never
        // silently answers a vision request about an image it cannot see.
        let image_urls = collect_image_urls(req);
        let _images = if image_urls.is_empty() {
            MaterializedImages::default()
        } else if self.kind.cli_supports_images() {
            MaterializedImages::fetch(client, &image_urls).await?
        } else {
            return Err(ProviderError::BadRequest(format!(
                "{} cannot accept image attachments",
                self.kind.as_str()
            )));
        };

        // Claude takes the caller's system messages through its own
        // `--system-prompt`; the others only have the prompt itself.
        let native_system_prompt = matches!(self.kind, ProviderKind::Claude);
        let prompt = render_prompt(req, !native_system_prompt);
        let model = cli_model_argument(self, req);
        let (args, prompt_on_stdin) = self.cli_args(
            model.as_deref(),
            &prompt,
            &cli_system_prompt(req),
            &_images.paths,
        );

        let mut child_command = Command::new(command);
        child_command
            .args(args)
            .current_dir(&self.agent_workdir)
            .stdin(if prompt_on_stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = child_command
            .spawn()
            .map_err(|e| ProviderError::Network(format!("failed to start {command}: {e}")))?;

        if prompt_on_stdin && let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
                ProviderError::Network(format!("failed to send prompt to {command}: {e}"))
            })?;
            stdin
                .shutdown()
                .await
                .map_err(|e| ProviderError::Network(format!("failed to close prompt pipe: {e}")))?;
        }

        let timeout_secs = self.cli_timeout_secs.max(1);
        let output =
            tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
                .await
                .map_err(|_| {
                    ProviderError::Network(format!("{command} timed out after {timeout_secs}s"))
                })?
                .map_err(|e| {
                    ProviderError::Network(format!("failed waiting for {command}: {e}"))
                })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            let detail = truncate_message(if stderr.trim().is_empty() {
                &stdout
            } else {
                &stderr
            });
            return Err(ProviderError::Upstream(format!(
                "{command} exited with {}: {detail}",
                output.status
            )));
        }

        let text = extract_cli_text(self.kind, &stdout).ok_or_else(|| {
            ProviderError::Upstream(format!("{command} returned an empty response"))
        })?;

        let prompt_tokens = estimate_tokens(&prompt);
        let completion_tokens = estimate_tokens(&text);
        let response_model =
            if req.model.trim().is_empty() || is_provider_alias(&self.id, self.kind, &req.model) {
                if self.model.trim().is_empty() || self.model == "default" {
                    self.id.clone()
                } else {
                    self.model.clone()
                }
            } else {
                req.model.clone()
            };

        Ok(ChatCompletionResponse {
            id: format!("chatcmpl-router-{}", Uuid::new_v4().simple()),
            object: "chat.completion".to_string(),
            created: Utc::now().timestamp().max(0) as u64,
            model: response_model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage::assistant(text),
                finish_reason: Some("stop".to_string()),
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    fn cli_args(
        &self,
        model: Option<&str>,
        prompt: &str,
        system_prompt: &str,
        image_paths: &[String],
    ) -> (Vec<String>, bool) {
        match self.kind {
            ProviderKind::OpenCode => {
                // The prompt goes first: `--file` is a variadic array flag, so
                // a prompt placed after it is swallowed as another filename
                // ("Error: File not found: Describe this image...").
                let mut args = vec![
                    "run".to_string(),
                    prompt.to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--pure".to_string(),
                ];
                if let Some(model) = model {
                    args.extend(["--model".to_string(), model.to_string()]);
                }
                for path in image_paths {
                    args.extend(["--file".to_string(), path.to_string()]);
                }
                (args, false)
            }
            ProviderKind::Codex => {
                // `codex exec` is already non-interactive and never prompts for
                // approval, so there is no `--ask-for-approval` here; passing it
                // made codex exit 2 before it ever read the prompt.
                let mut args = vec![
                    "exec".to_string(),
                    "--ephemeral".to_string(),
                    "--sandbox".to_string(),
                    "read-only".to_string(),
                    "--skip-git-repo-check".to_string(),
                    "--ignore-user-config".to_string(),
                    "--color".to_string(),
                    "never".to_string(),
                ];
                if let Some(model) = model {
                    args.extend(["--model".to_string(), model.to_string()]);
                }
                for path in image_paths {
                    args.extend(["--image".to_string(), path.to_string()]);
                }
                args.push("-".to_string());
                (args, true)
            }
            ProviderKind::Claude => {
                // Without replacing the system prompt, Claude Code answers as a
                // repo coding agent ("I can't just return bare JSON…") instead
                // of serving the request. `--bare` would also be desirable here
                // but it forces ANTHROPIC_API_KEY auth and breaks OAuth logins.
                let mut args = vec![
                    "--print".to_string(),
                    "--output-format".to_string(),
                    "json".to_string(),
                    "--no-session-persistence".to_string(),
                    "--system-prompt".to_string(),
                    system_prompt.to_string(),
                    "--exclude-dynamic-system-prompt-sections".to_string(),
                    "--tools".to_string(),
                    "".to_string(),
                    "--permission-mode".to_string(),
                    "dontAsk".to_string(),
                ];
                if let Some(model) = model {
                    args.extend(["--model".to_string(), model.to_string()]);
                }
                args.push(prompt.to_string());
                (args, false)
            }
            ProviderKind::Agy => {
                // `--print` consumes the next token as its prompt, so every
                // boolean flag must come first and the prompt must be attached
                // with `=`. Passing `--print --sandbox <prompt>` makes agy read
                // "--sandbox" as the prompt and exit 2.
                let mut args = vec![
                    "--sandbox".to_string(),
                    "--disable-slash-commands".to_string(),
                    "--output-format".to_string(),
                    "text".to_string(),
                ];
                if let Some(model) = model {
                    args.extend(["--model".to_string(), model.to_string()]);
                }
                args.push(format!("--print={prompt}"));
                (args, false)
            }
            ProviderKind::Groq | ProviderKind::Nvidia => (vec![prompt.to_string()], false),
        }
    }
}

fn cli_model_argument(provider: &Provider, req: &ChatCompletionRequest) -> Option<String> {
    let model = req.model.trim();
    if model.is_empty() || is_provider_alias(&provider.id, provider.kind, model) {
        // `auto` on a local agent means "whatever is cheapest here", not
        // "whatever the CLI defaults to" — local agents are the last-resort
        // fallback and should not burn a premium model on a moderation call.
        return cheap_cli_model(provider, req.has_images());
    }
    Some(model.to_string())
}

/// The cheapest model to run a local agent CLI with. A vision request needs a
/// model that can actually see, so it gets its own (still free) default.
///
/// Overridable per kind via `ROUTER_AGY_MODEL`, `ROUTER_CLAUDE_MODEL`,
/// `ROUTER_CODEX_MODEL`, `ROUTER_OPENCODE_MODEL`, and for images
/// `ROUTER_OPENCODE_VISION_MODEL` / `ROUTER_CODEX_VISION_MODEL`.
fn cheap_cli_model(provider: &Provider, has_images: bool) -> Option<String> {
    if !provider.model.trim().is_empty() && provider.model != "default" {
        return Some(provider.model.clone());
    }
    let (var, default) = match (provider.kind, has_images) {
        (ProviderKind::OpenCode, true) => (
            "ROUTER_OPENCODE_VISION_MODEL",
            Some("opencode/x-preview-f-free"),
        ),
        (ProviderKind::Codex, true) => ("ROUTER_CODEX_VISION_MODEL", None),
        (ProviderKind::Agy, _) => ("ROUTER_AGY_MODEL", Some("gemini-3.5-flash-low")),
        (ProviderKind::Claude, _) => ("ROUTER_CLAUDE_MODEL", Some("haiku")),
        (ProviderKind::Codex, false) => ("ROUTER_CODEX_MODEL", None),
        (ProviderKind::OpenCode, false) => ("ROUTER_OPENCODE_MODEL", None),
        _ => return None,
    };
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| default.map(ToString::to_string))
}

fn is_provider_alias(id: &str, kind: ProviderKind, model: &str) -> bool {
    model.eq_ignore_ascii_case(id)
        || model.eq_ignore_ascii_case(kind.as_str())
        || model.eq_ignore_ascii_case("auto")
        || model.eq_ignore_ascii_case("default")
        || model.eq_ignore_ascii_case("router")
}

/// Every image referenced by a chat request, in order.
fn collect_image_urls(req: &ChatCompletionRequest) -> Vec<String> {
    req.messages
        .iter()
        .filter_map(|message| match &message.content {
            crate::ai::dto::ChatMessageContent::Parts(parts) => Some(parts),
            crate::ai::dto::ChatMessageContent::Text(_) => None,
        })
        .flatten()
        .filter_map(|part| match part {
            crate::ai::dto::ChatContentPart::ImageUrl { image_url } => Some(image_url.url.clone()),
            crate::ai::dto::ChatContentPart::Text { .. } => None,
        })
        .collect()
}

/// Request images written to a temporary directory so an agent CLI can attach
/// them. The directory is removed when this value is dropped.
#[derive(Debug, Default)]
struct MaterializedImages {
    paths: Vec<String>,
    dir: Option<std::path::PathBuf>,
}

impl MaterializedImages {
    async fn fetch(
        client: &reqwest::Client,
        urls: &[String],
    ) -> Result<Self, ProviderError> {
        let dir = std::env::temp_dir().join(format!("router-cli-images-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.map_err(|e| {
            ProviderError::Network(format!("failed to create image scratch dir: {e}"))
        })?;

        let mut out = Self {
            paths: Vec::new(),
            dir: Some(dir.clone()),
        };

        for (index, url) in urls.iter().enumerate() {
            let (bytes, extension) = decode_image_source(client, url).await?;
            let path = dir.join(format!("image-{index}.{extension}"));
            tokio::fs::write(&path, &bytes)
                .await
                .map_err(|e| ProviderError::Network(format!("failed to write image file: {e}")))?;
            out.paths.push(path.to_string_lossy().to_string());
        }

        Ok(out)
    }
}

impl Drop for MaterializedImages {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Resolve a `data:` URI, bare base64, or HTTP URL into raw image bytes.
async fn decode_image_source(
    client: &reqwest::Client,
    source: &str,
) -> Result<(Vec<u8>, &'static str), ProviderError> {
    use base64::prelude::*;

    let source = source.trim();
    if let Some(rest) = source.strip_prefix("data:") {
        let (meta, payload) = rest.split_once(",").ok_or_else(|| {
            ProviderError::BadRequest("malformed data URI for image".to_string())
        })?;
        let bytes = BASE64_STANDARD
            .decode(payload.trim())
            .map_err(|e| ProviderError::BadRequest(format!("invalid base64 image: {e}")))?;
        return Ok((bytes, extension_for_mime(meta)));
    }

    if !source.starts_with("http://") && !source.starts_with("https://") {
        let bytes = BASE64_STANDARD
            .decode(source)
            .map_err(|e| ProviderError::BadRequest(format!("invalid base64 image: {e}")))?;
        return Ok((bytes, "jpg"));
    }

    let resp = client
        .get(source)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| ProviderError::Network(format!("failed to download image: {e}")))?;
    if !resp.status().is_success() {
        return Err(ProviderError::Network(format!(
            "failed to download image: status {}",
            resp.status()
        )));
    }
    let extension = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(extension_for_mime)
        .unwrap_or("jpg");
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ProviderError::Network(format!("failed to read image body: {e}")))?;

    Ok((bytes.to_vec(), extension))
}

fn extension_for_mime(mime: &str) -> &'static str {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("png") {
        "png"
    } else if mime.contains("webp") {
        "webp"
    } else if mime.contains("gif") {
        "gif"
    } else {
        "jpg"
    }
}

/// System prompt handed to agent CLIs that let one be set, so they behave as a
/// plain completion backend rather than as a repository coding assistant.
const ROUTER_AGENT_SYSTEM_PROMPT: &str = "You are a completion backend behind an AI router API. Answer the user's request directly and return only the answer itself. If the request asks for a specific output format such as JSON, emit exactly that and nothing else — no preamble, no commentary, no markdown fences. Never edit files, run commands, or ask for permission.";

/// Flatten a chat request into a single prompt for a CLI agent.
///
/// Role headers are deliberately *not* rendered as `SYSTEM:` / `USER:` blocks.
/// Coding agents recognize that shape as an injected instruction frame and
/// refuse the task ("I detected a SYSTEM instruction inside your message…"),
/// which is exactly how the moderation fallback used to fail. Framing the
/// caller's system messages as configuration instead keeps them followed.
///
/// When `include_system` is false the caller passes those messages through the
/// CLI's own system-prompt flag instead.
fn render_prompt(req: &ChatCompletionRequest, include_system: bool) -> String {
    let mut out = String::from(
        "You are a completion backend behind an AI router API. Answer the request directly and return only the answer. If a specific output format is requested (for example a single JSON object), emit exactly that with no preamble, commentary, or markdown fences. Never edit files, run commands, or ask for interactive permission.\n\n",
    );

    if include_system {
        let instructions = collect_system_text(req);
        if !instructions.is_empty() {
            out.push_str("## Your configuration for this request\n");
            out.push_str(&instructions);
            out.push_str("\n\n");
        }
    }

    out.push_str("## Request\n");
    for message in &req.messages {
        if message.role.eq_ignore_ascii_case("system") {
            continue;
        }
        if message.role.eq_ignore_ascii_case("assistant") {
            out.push_str("Previous reply: ");
        }
        out.push_str(&message.content.as_text());
        out.push_str("\n\n");
    }

    out.push_str("## Your reply\n");
    out
}

/// The request's system messages, joined.
fn collect_system_text(req: &ChatCompletionRequest) -> String {
    req.messages
        .iter()
        .filter(|m| m.role.eq_ignore_ascii_case("system"))
        .map(|m| m.content.as_text())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// System prompt for CLIs that accept one: the router's own framing plus
/// whatever the caller asked for.
fn cli_system_prompt(req: &ChatCompletionRequest) -> String {
    let caller = collect_system_text(req);
    if caller.trim().is_empty() {
        ROUTER_AGENT_SYSTEM_PROMPT.to_string()
    } else {
        format!("{ROUTER_AGENT_SYSTEM_PROMPT}\n\n{caller}")
    }
}

fn command_is_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(command))
        .any(|candidate| candidate.is_file())
}

fn estimate_tokens(text: &str) -> i64 {
    ((text.chars().count() as i64 + 3) / 4).max(1)
}

fn extract_cli_text(kind: ProviderKind, raw: &str) -> Option<String> {
    let clean = strip_ansi(raw);
    let clean = clean.trim();
    if clean.is_empty() {
        return None;
    }

    let structured = match kind {
        ProviderKind::Claude => extract_claude_result(clean),
        ProviderKind::Codex => extract_codex_messages(clean),
        ProviderKind::OpenCode => extract_opencode_text(clean),
        ProviderKind::Agy | ProviderKind::Groq | ProviderKind::Nvidia => None,
    };

    structured
        .or_else(|| Some(clean.to_string()))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn extract_claude_result(raw: &str) -> Option<String> {
    for line in raw.lines().rev() {
        if let Ok(value) = serde_json::from_str::<Value>(line)
            && let Some(result) = value.get("result").and_then(Value::as_str)
        {
            return Some(result.to_string());
        }
    }
    serde_json::from_str::<Value>(raw).ok().and_then(|value| {
        value
            .get("result")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn extract_codex_messages(raw: &str) -> Option<String> {
    let mut messages = Vec::new();
    for line in raw.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(item) = value.get("item")
            && item.get("type").and_then(Value::as_str) == Some("agent_message")
            && let Some(text) = item.get("text").and_then(Value::as_str)
        {
            messages.push(text.to_string());
        } else if value.get("type").and_then(Value::as_str) == Some("agent_message")
            && let Some(text) = value.get("text").and_then(Value::as_str)
        {
            messages.push(text.to_string());
        }
    }
    if messages.is_empty() {
        None
    } else {
        Some(messages.join("\n"))
    }
}

fn extract_opencode_text(raw: &str) -> Option<String> {
    let mut chunks = Vec::new();
    for line in raw.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        collect_opencode_chunk(&value, &mut chunks);
    }
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(""))
    }
}

fn collect_opencode_chunk(value: &Value, chunks: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        return;
    };

    if object.get("type").and_then(Value::as_str) == Some("text")
        && let Some(text) = object.get("text").and_then(Value::as_str)
    {
        chunks.push(text.to_string());
        return;
    }
    if let Some(part) = object.get("part").and_then(Value::as_object)
        && part.get("type").and_then(Value::as_str) == Some("text")
        && let Some(text) = part.get("text").and_then(Value::as_str)
    {
        chunks.push(text.to_string());
        return;
    }
    if let Some(result) = object.get("result").and_then(Value::as_str) {
        chunks.push(result.to_string());
        return;
    }

    for value in object.values() {
        collect_opencode_chunk(value, chunks);
    }
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn truncate_message(value: &str) -> String {
    value.chars().take(500).collect()
}

fn classify_error(status: u16, body: String) -> ProviderError {
    let msg = if body.is_empty() {
        format!("upstream status {status}")
    } else {
        body.chars().take(300).collect()
    };
    match status {
        429 => ProviderError::RateLimited(msg),
        401 | 403 => ProviderError::Auth(msg),
        404 => ProviderError::NotFound(msg),
        400 | 422 => ProviderError::BadRequest(msg),
        500..=599 => ProviderError::Upstream(msg),
        _ => ProviderError::Network(format!("unexpected status {status}: {msg}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_puts_the_prompt_before_the_variadic_file_flag() {
        let provider = Provider::new_cli(
            "opencode",
            "OpenCode",
            ProviderKind::OpenCode,
            "opencode",
            "default",
            ".",
            60,
        );
        let (args, _) = provider.cli_args(
            Some("opencode/x-preview-f-free"),
            "describe this image",
            "",
            &["/tmp/a.png".to_string()],
        );

        let prompt_at = args.iter().position(|a| a == "describe this image");
        let file_at = args.iter().position(|a| a == "--file");
        assert!(prompt_at.is_some() && file_at.is_some());
        // `--file` is variadic: a prompt after it is swallowed as a filename.
        assert!(
            prompt_at < file_at,
            "prompt must precede --file, got {args:?}"
        );
        assert_eq!(args.last().unwrap(), "/tmp/a.png");
    }

    #[test]
    fn text_only_agents_refuse_image_requests_instead_of_guessing() {
        assert!(ProviderKind::OpenCode.cli_supports_images());
        assert!(ProviderKind::Codex.cli_supports_images());
        // Agy and Claude Code have no image attachment flag; answering a vision
        // request from them would be a hallucination about an unseen image.
        assert!(!ProviderKind::Agy.cli_supports_images());
        assert!(!ProviderKind::Claude.cli_supports_images());
    }

    #[test]
    fn nvidia_worker_saturation_advances_the_model_ladder_but_still_blames_the_key() {
        let err = ProviderError::Upstream(
            "{\"error\":{\"message\":\"ResourceExhausted: Worker local total request limit reached (16/16)\"}}"
                .to_string(),
        );
        assert!(err.is_model_specific(), "should try the next model");
        assert!(err.blames_key(), "a saturated key still earns a cooldown");

        let wrong_model = ProviderError::NotFound("no such model".to_string());
        assert!(wrong_model.is_model_specific());
        assert!(!wrong_model.blames_key(), "a bad model id is not the key's fault");
    }

    #[test]
    fn parses_provider_aliases() {
        assert_eq!(
            ProviderKind::parse("claude-code"),
            Some(ProviderKind::Claude)
        );
        assert_eq!(
            ProviderKind::parse("open-code"),
            Some(ProviderKind::OpenCode)
        );
        assert_eq!(ProviderKind::parse("nvidia"), Some(ProviderKind::Nvidia));
        assert_eq!(ProviderKind::parse("nim"), Some(ProviderKind::Nvidia));
        assert_eq!(
            ProviderKind::parse("build-nvidia"),
            Some(ProviderKind::Nvidia)
        );
        assert_eq!(ProviderKind::parse("unknown"), None);
    }

    #[test]
    fn extracts_structured_cli_messages() {
        let codex = r#"{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}"#;
        assert_eq!(
            extract_cli_text(ProviderKind::Codex, codex).as_deref(),
            Some("hello")
        );

        let claude = r#"{"result":"hello from claude"}"#;
        assert_eq!(
            extract_cli_text(ProviderKind::Claude, claude).as_deref(),
            Some("hello from claude")
        );

        let opencode = "{\"part\":{\"type\":\"text\",\"text\":\"hello\"}}\n{\"part\":{\"type\":\"text\",\"text\":\" world\"}}";
        assert_eq!(
            extract_cli_text(ProviderKind::OpenCode, opencode).as_deref(),
            Some("hello world")
        );
    }

    #[tokio::test]
    async fn local_cli_provider_returns_plain_process_output() {
        let provider = Provider::new_cli(
            "agy",
            "Agy CLI",
            ProviderKind::Agy,
            "echo",
            "default",
            ".",
            10,
        );
        let request = ChatCompletionRequest {
            model: "agy".to_string(),
            messages: vec![ChatMessage::user("hello")],
            temperature: None,
            max_tokens: None,
            stream: Some(false),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            chat_template_kwargs: None,
            provider: Some("agy".to_string()),
        };
        let response = provider
            .chat_completion(&reqwest::Client::new(), &request)
            .await
            .unwrap();
        // `echo` replays the rendered prompt, so this asserts the prompt shape
        // the CLI actually receives.
        let echoed = response.choices[0].message.content.as_text();
        assert!(echoed.contains("## Your reply"), "prompt was: {echoed}");
        assert!(echoed.contains("hello"));
        assert!(
            !echoed.contains("USER:"),
            "role headers make coding agents refuse the task: {echoed}"
        );
        assert!(response.usage.total_tokens > 0);
    }
}
