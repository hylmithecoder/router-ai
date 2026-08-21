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
    OpenCode,
    Codex,
    Claude,
    Agy,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Agy => "agy",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "groq" => Some(Self::Groq),
            "opencode" | "open-code" => Some(Self::OpenCode),
            "codex" | "codex-cli" => Some(Self::Codex),
            "claude" | "claude-code" => Some(Self::Claude),
            "agy" | "agy-cli" => Some(Self::Agy),
            _ => None,
        }
    }

    pub fn is_cli(self) -> bool {
        !matches!(self, Self::Groq)
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
    /// 5xx upstream or a non-zero local CLI exit.
    Upstream(String),
    /// Network / timeout / transport / missing executable error.
    Network(String),
}

impl Provider {
    pub fn new_http(
        id: &str,
        name: &str,
        base_url: &str,
        api_key: Option<&str>,
        model: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            kind: ProviderKind::Groq,
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
        match self.kind {
            ProviderKind::Groq => {
                self.api_key.as_ref().is_some_and(|key| !key.is_empty())
                    && !self.base_url.trim().is_empty()
            }
            _ => self.command.as_deref().is_some_and(command_is_available),
        }
    }

    /// POST `/chat/completions` for HTTP providers, or execute a local CLI.
    pub async fn chat_completion(
        &self,
        client: &reqwest::Client,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        if self.kind.is_cli() {
            return self.run_cli(req).await;
        }

        let forwarded = self.normalized_request(req);
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut request = client.post(&url).json(&forwarded);
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
        let mut request = client.post(&url).json(&forwarded);
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
            || forwarded.model.eq_ignore_ascii_case("auto")
            || forwarded.model.eq_ignore_ascii_case("default")
        {
            forwarded.model = self.model.clone();
        }
        forwarded
    }

    async fn run_cli(
        &self,
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

        let prompt = render_prompt(req);
        let model = cli_model_argument(self, req);
        let (args, prompt_on_stdin) = self.cli_args(model.as_deref(), &prompt);

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
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: text,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    fn cli_args(&self, model: Option<&str>, prompt: &str) -> (Vec<String>, bool) {
        match self.kind {
            ProviderKind::OpenCode => {
                let mut args = vec![
                    "run".to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--pure".to_string(),
                ];
                if let Some(model) = model {
                    args.extend(["--model".to_string(), model.to_string()]);
                }
                args.push(prompt.to_string());
                (args, false)
            }
            ProviderKind::Codex => {
                let mut args = vec![
                    "exec".to_string(),
                    "--ephemeral".to_string(),
                    "--sandbox".to_string(),
                    "read-only".to_string(),
                    "--ask-for-approval".to_string(),
                    "never".to_string(),
                    "--skip-git-repo-check".to_string(),
                    "--color".to_string(),
                    "never".to_string(),
                ];
                if let Some(model) = model {
                    args.extend(["--model".to_string(), model.to_string()]);
                }
                args.push("-".to_string());
                (args, true)
            }
            ProviderKind::Claude => {
                let mut args = vec![
                    "--print".to_string(),
                    "--output-format".to_string(),
                    "json".to_string(),
                    "--no-session-persistence".to_string(),
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
                let mut args = vec!["--print".to_string(), "--sandbox".to_string()];
                args.push(prompt.to_string());
                (args, false)
            }
            ProviderKind::Groq => (vec![prompt.to_string()], false),
        }
    }
}

fn cli_model_argument(provider: &Provider, req: &ChatCompletionRequest) -> Option<String> {
    let model = req.model.trim();
    if model.is_empty() || is_provider_alias(&provider.id, provider.kind, model) {
        None
    } else {
        Some(model.to_string())
    }
}

fn is_provider_alias(id: &str, kind: ProviderKind, model: &str) -> bool {
    model.eq_ignore_ascii_case(id)
        || model.eq_ignore_ascii_case(kind.as_str())
        || model.eq_ignore_ascii_case("auto")
        || model.eq_ignore_ascii_case("default")
        || model.eq_ignore_ascii_case("router")
}

fn render_prompt(req: &ChatCompletionRequest) -> String {
    let mut out = String::from(
        "You are responding through an AI router API. Return only the assistant answer. Do not edit files, run commands, or ask for interactive permission.\n\n",
    );
    for message in &req.messages {
        out.push_str(&message.role.to_ascii_uppercase());
        out.push_str(":\n");
        out.push_str(&message.content);
        out.push_str("\n\n");
    }
    out.push_str("ASSISTANT:\n");
    out
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
        ProviderKind::Agy | ProviderKind::Groq => None,
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
        500..=599 => ProviderError::Upstream(msg),
        _ => ProviderError::Network(format!("unexpected status {status}: {msg}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "/bin/echo",
            "default",
            ".",
            2,
        );
        let request = ChatCompletionRequest {
            model: "agy".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            temperature: None,
            max_tokens: None,
            stream: Some(false),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            provider: Some("agy".to_string()),
        };
        let response = provider
            .chat_completion(&reqwest::Client::new(), &request)
            .await
            .unwrap();
        assert!(response.choices[0].message.content.contains("ASSISTANT:"));
        assert!(response.usage.total_tokens > 0);
    }
}
