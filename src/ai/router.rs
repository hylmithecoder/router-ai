//! The provider router.
//!
//! Provider configuration is persisted in SQLite, while cooldown and enabled
//! state are kept in a small async-protected runtime snapshot. Network and CLI
//! calls happen after the snapshot lock is released, so one slow agent cannot
//! serialize every request in the service.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::{
    ai::{
        dto::{
            ChatCompletionRequest, ChatCompletionResponse, CompletionOutcome, RouterError,
            StreamOutcome, StreamSource,
        },
        provider::{Provider, ProviderError, ProviderKind},
    },
    config::Settings,
    database::Db,
};

/// Route completions across configured providers with automatic failover.
#[derive(Debug, Clone)]
pub struct AiRouter {
    client: reqwest::Client,
    providers: Arc<RwLock<Vec<Provider>>>,
    cooldown_secs: u64,
    agent_workdir: String,
    cli_timeout_secs: u64,
    db: Db,
}

use crate::dns::build_dns_hardened_client;

impl AiRouter {
    /// Build the router from persisted providers, environment-seeded Groq keys,
    /// and locally discovered agent CLIs.
    pub async fn new(settings: &Settings, db: Db) -> Self {
        let client = build_dns_hardened_client(&[]).await;

        for (i, spec) in settings.router.groq_api_keys.iter().enumerate() {
            db.ensure_provider(
                &format!("groq-{}", i + 1),
                "groq",
                &format!("Groq #{}", i + 1),
                &spec.base_url,
                Some(&spec.key),
                None,
                &settings.router.default_model,
            )
            .await
            .expect("failed to seed Groq provider");
        }

        for (i, spec) in settings.router.nvidia_api_keys.iter().enumerate() {
            db.ensure_provider(
                &format!("nvidia-{}", i + 1),
                "nvidia",
                &format!("NVIDIA #{}", i + 1),
                &spec.base_url,
                Some(&spec.key),
                None,
                "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
            )
            .await
            .expect("failed to seed NVIDIA provider");
        }

        for spec in discover_local_agents() {
            db.ensure_provider(
                spec.id,
                spec.kind.as_str(),
                spec.name,
                "",
                None,
                Some(spec.command),
                "default",
            )
            .await
            .expect("failed to seed local agent provider");
        }

        let stored = db
            .list_provider_configs()
            .await
            .expect("failed to load providers");
        let mut providers = stored
            .into_iter()
            .filter_map(|stored| {
                Provider::from_stored(
                    stored,
                    &settings.router.agent_workdir,
                    settings.router.cli_timeout_secs,
                )
            })
            .collect::<Vec<_>>();
        // HTTP providers (Groq, Nvidia) are the default pool for `auto`; local agents remain
        // available as explicit selectors and as later fallbacks.
        providers.sort_by_key(|provider| if provider.kind.is_cli() { 1 } else { 0 });

        Self {
            client,
            providers: Arc::new(RwLock::new(providers)),
            cooldown_secs: settings.router.provider_cooldown_secs,
            agent_workdir: settings.router.agent_workdir.clone(),
            cli_timeout_secs: settings.router.cli_timeout_secs,
            db,
        }
    }

    /// Snapshot providers for admin/status views. Secrets remain in memory only
    /// and are redacted by the admin DTO layer.
    pub async fn providers(&self) -> Vec<Provider> {
        self.providers.read().await.clone()
    }

    /// Try each selected provider in order until one succeeds.
    ///
    /// Failover happens on two levels: within a provider we rotate through the
    /// model candidates (a 400/404 means the *model* is wrong, not the key),
    /// and across providers we rotate through the keys. A final pass retries
    /// providers that are in cooldown, so a burst of failures can never leave
    /// the request with nothing to try.
    pub async fn complete(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<CompletionOutcome, RouterError> {
        let has_images = req.has_images();
        let candidates = self
            .candidates(req.provider.as_deref(), Some(&req.model), has_images)
            .await?;
        let mut last_error: Option<String> = None;

        for (id, provider) in self.attempt_order(candidates) {
            match self.try_provider_models(&provider, req, has_images).await {
                Ok((model, response)) => {
                    self.mark_success(&id).await;
                    return Ok(CompletionOutcome {
                        provider_id: id,
                        provider_name: provider.name,
                        model: if response.model.is_empty() {
                            model
                        } else {
                            response.model.clone()
                        },
                        response,
                    });
                }
                Err(err) => {
                    let msg = provider_error_message(&err);
                    tracing::warn!(provider = %id, error = %msg, "provider failed, falling over");
                    last_error = Some(msg.clone());
                    // A bad model id says nothing about the key's health, so it
                    // must not park the key in cooldown.
                    if err.blames_key() {
                        self.mark_failure(&id, &msg).await;
                    }
                }
            }
        }

        Err(RouterError::AllProvidersFailed(
            last_error.unwrap_or_else(|| "no providers available".to_string()),
        ))
    }

    /// Order the candidates into the sequence to actually try: healthy providers
    /// first, then the ones sitting in cooldown as a last resort.
    fn attempt_order(&self, candidates: Vec<(String, Provider)>) -> Vec<(String, Provider)> {
        let (ready, cooling): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .filter(|(_, provider)| provider.enabled)
            .partition(|(_, provider)| !provider.in_cooldown());
        ready.into_iter().chain(cooling).collect()
    }

    /// Try one provider against each of its model candidates in turn.
    async fn try_provider_models(
        &self,
        provider: &Provider,
        req: &ChatCompletionRequest,
        has_images: bool,
    ) -> Result<(String, ChatCompletionResponse), ProviderError> {
        let models = model_candidates(provider, &req.model, has_images);
        let mut last: Option<ProviderError> = None;

        let budget = attempt_timeout(provider);
        for model in models {
            let mut attempt = req.clone();
            attempt.model = model.clone();
            let call = provider.chat_completion(&self.client, &attempt);
            // A wedged upstream must not hold the whole request hostage; a
            // stalled model is just another reason to rotate.
            let result = match tokio::time::timeout(budget, call).await {
                Ok(result) => result,
                Err(_) => Err(ProviderError::Network(format!(
                    "model {model} timed out after {}s",
                    budget.as_secs()
                ))),
            };

            match result {
                Ok(response) => return Ok((model, response)),
                Err(err) => {
                    let retry_next_model = err.is_model_specific();
                    tracing::warn!(
                        provider = %provider.id,
                        model = %model,
                        error = %provider_error_message(&err),
                        retry_next_model,
                        "model attempt failed"
                    );
                    last = Some(err);
                    if !retry_next_model {
                        break;
                    }
                }
            }
        }

        Err(last.unwrap_or_else(|| {
            ProviderError::Network("no model candidates for provider".to_string())
        }))
    }

    /// Route a streaming request. HTTP providers are passed through as SSE;
    /// local CLIs return one synthesized SSE completion after the process exits.
    pub async fn stream(&self, req: &ChatCompletionRequest) -> Result<StreamOutcome, RouterError> {
        let has_images = req.has_images();
        let candidates = self
            .candidates(req.provider.as_deref(), Some(&req.model), has_images)
            .await?;
        let mut last_error: Option<String> = None;

        for (id, provider) in self.attempt_order(candidates) {
            if provider.kind.is_cli() {
                let mut non_stream = req.clone();
                non_stream.stream = Some(false);
                match self
                    .try_provider_models(&provider, &non_stream, has_images)
                    .await
                {
                    Ok((_, response)) => {
                        self.mark_success(&id).await;
                        return Ok(StreamOutcome {
                            provider_id: id,
                            provider_name: provider.name,
                            model: response.model.clone(),
                            source: StreamSource::Completed(response),
                        });
                    }
                    Err(err) => {
                        let msg = provider_error_message(&err);
                        last_error = Some(msg.clone());
                        if err.blames_key() {
                            self.mark_failure(&id, &msg).await;
                        }
                    }
                }
                continue;
            }

            match self.try_stream_models(&provider, req, has_images).await {
                Ok((model, response)) => {
                    self.mark_success(&id).await;
                    return Ok(StreamOutcome {
                        provider_id: id,
                        provider_name: provider.name,
                        model,
                        source: StreamSource::Http(response),
                    });
                }
                Err(err) => {
                    let msg = provider_error_message(&err);
                    last_error = Some(msg.clone());
                    if err.blames_key() {
                        self.mark_failure(&id, &msg).await;
                    }
                }
            }
        }

        Err(RouterError::AllProvidersFailed(
            last_error.unwrap_or_else(|| "no providers available".to_string()),
        ))
    }

    /// Streaming counterpart of [`Self::try_provider_models`].
    async fn try_stream_models(
        &self,
        provider: &Provider,
        req: &ChatCompletionRequest,
        has_images: bool,
    ) -> Result<(String, reqwest::Response), ProviderError> {
        let models = model_candidates(provider, &req.model, has_images);
        let mut last: Option<ProviderError> = None;

        for model in models {
            let mut attempt = req.clone();
            attempt.model = model.clone();
            match provider.stream_chat(&self.client, &attempt).await {
                Ok(response) => return Ok((model, response)),
                Err(err) => {
                    let retry_next_model = err.is_model_specific();
                    last = Some(err);
                    if !retry_next_model {
                        break;
                    }
                }
            }
        }

        Err(last.unwrap_or_else(|| {
            ProviderError::Network("no model candidates for provider".to_string())
        }))
    }

    /// Automatically pick or validate the best available vision model for multimodal tasks.
    pub async fn resolve_vision_model(&self, requested: Option<&str>) -> String {
        if let Some(m) = requested
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("auto"))
        {
            return m.to_string();
        }

        let providers = self.providers.read().await;
        for p in providers.iter() {
            if p.enabled && !p.in_cooldown() && p.kind == ProviderKind::Nvidia {
                if !p.model.is_empty() && p.model != "default" {
                    return p.model.clone();
                }
                return "google/diffusiongemma-26b-a4b-it".to_string();
            }
        }

        "google/diffusiongemma-26b-a4b-it".to_string()
    }

    async fn candidates(
        &self,
        selector: Option<&str>,
        requested_model: Option<&str>,
        has_images: bool,
    ) -> Result<Vec<(String, Provider)>, RouterError> {
        let providers = self.providers.read().await;
        let selector = selector.map(str::trim).filter(|s| !s.is_empty());
        let requested_model = requested_model.map(str::trim).filter(|s| !s.is_empty());

        let mut out = Vec::new();
        for provider in providers.iter() {
            // A provider with no key, or a CLI whose binary was uninstalled,
            // can only ever fail. Skipping it here keeps it from burning a
            // failover slot and from polluting the reported error.
            if !provider.is_available() {
                continue;
            }

            // When images are present, only providers that can actually see may
            // serve the request: vision-capable HTTP endpoints, plus the agent
            // CLIs that accept image attachments as a last resort. A text-only
            // endpoint would either 400 or answer about nothing.
            let explicitly_named = selector.is_some_and(|value| {
                provider.id.eq_ignore_ascii_case(value) || provider.name.eq_ignore_ascii_case(value)
            });
            let image_capable_cli = provider.kind.is_cli() && provider.kind.cli_supports_images();
            if has_images && !explicitly_named {
                let can_see = if provider.kind.is_cli() {
                    image_capable_cli
                } else {
                    provider.kind == ProviderKind::Nvidia
                        || is_vision_model(&provider.model)
                        || requested_model.map(is_vision_model).unwrap_or(false)
                };
                if !can_see {
                    continue;
                }
            }

            let matches = match selector {
                Some(value)
                    if value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("all") =>
                {
                    true
                }
                Some(value) if value.eq_ignore_ascii_case("groq") => {
                    provider.kind == ProviderKind::Groq
                }
                Some(value) if value.eq_ignore_ascii_case("nvidia") => {
                    provider.kind == ProviderKind::Nvidia
                }
                Some(value) => {
                    ProviderKind::parse(value).is_some_and(|kind| provider.kind == kind)
                        || provider.id.eq_ignore_ascii_case(value)
                        || provider.name.eq_ignore_ascii_case(value)
                        || provider.kind.as_str().eq_ignore_ascii_case(value)
                }
                None => {
                    if has_images {
                        // When images are present, prioritize vision-capable providers (Nvidia)
                        provider.kind == ProviderKind::Nvidia
                            || is_vision_model(&provider.model)
                            || requested_model.map(is_vision_model).unwrap_or(false)
                            || image_capable_cli
                    } else if let Some(model) = requested_model {
                        if is_nvidia_model(model)
                            || (provider.kind == ProviderKind::Nvidia
                                && provider.model.eq_ignore_ascii_case(model))
                        {
                            provider.kind == ProviderKind::Nvidia
                        } else if is_groq_model(model)
                            || (provider.kind == ProviderKind::Groq
                                && provider.model.eq_ignore_ascii_case(model))
                        {
                            provider.kind == ProviderKind::Groq
                        } else if let Some(cli_kind) =
                            ProviderKind::parse(model).filter(|k| k.is_cli())
                        {
                            provider.kind == cli_kind || provider.id.eq_ignore_ascii_case(model)
                        } else {
                            !provider.kind.is_cli()
                        }
                    } else {
                        !provider.kind.is_cli()
                    }
                }
            };
            if matches {
                out.push((provider.id.clone(), provider.clone()));
            }
        }

        if selector.is_some() && out.is_empty() {
            return Err(RouterError::ClientError(format!(
                "unknown provider selector: {}",
                selector.unwrap_or_default()
            )));
        }
        if out.is_empty() {
            return Err(RouterError::NoProviders);
        }
        out.sort_by_key(|(_, provider)| provider_relevance(provider, requested_model, has_images));
        Ok(out)
    }

    async fn mark_success(&self, id: &str) {
        if let Some(provider) = self.providers.write().await.iter_mut().find(|p| p.id == id) {
            provider.clear_cooldown();
        }
        self.db.provider_success(id).await.ok();
    }

    async fn mark_failure(&self, id: &str, message: &str) {
        if let Some(provider) = self.providers.write().await.iter_mut().find(|p| p.id == id) {
            provider.enter_cooldown(self.cooldown_secs);
        }
        self.db
            .provider_failure(id, message, self.cooldown_secs)
            .await
            .ok();
    }

    /// Enable/disable a provider from the dashboard.
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> bool {
        let found = {
            let mut providers = self.providers.write().await;
            let Some(provider) = providers.iter_mut().find(|p| p.id == id) else {
                return false;
            };
            provider.enabled = enabled;
            if !enabled {
                provider.clear_cooldown();
            }
            true
        };
        if !found {
            return false;
        }
        self.db.set_provider_enabled(id, enabled).await.ok();
        true
    }

    /// Reload one provider after an admin create/update operation.
    pub async fn reload_provider(&self, id: &str) -> anyhow::Result<bool> {
        let Some(stored) = self.db.find_provider_config(id).await? else {
            return Ok(false);
        };
        let Some(provider) =
            Provider::from_stored(stored, &self.agent_workdir, self.cli_timeout_secs)
        else {
            return Ok(false);
        };

        let mut providers = self.providers.write().await;
        if let Some(existing) = providers.iter_mut().find(|p| p.id == id) {
            let enabled = existing.enabled;
            *existing = provider;
            existing.enabled = enabled;
        } else {
            providers.push(provider);
        }
        Ok(true)
    }

    pub async fn remove_provider(&self, id: &str) -> bool {
        let mut providers = self.providers.write().await;
        let before = providers.len();
        providers.retain(|provider| provider.id != id);
        before != providers.len()
    }

    /// Unique models and provider aliases for `GET /v1/models`.
    pub async fn models(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        for provider in self.providers.read().await.iter() {
            if !provider.enabled {
                continue;
            }
            if !provider.model.is_empty() && provider.model != "default" {
                seen.insert(provider.model.clone());
            }
            if provider.kind == ProviderKind::Nvidia {
                seen.insert("nvidia/nemotron-3-nano-omni-30b-a3b-reasoning".to_string());
                seen.insert("nvidia/nemotron-3-ultra-550b-a55b".to_string());
            }
            if provider.kind.is_cli() {
                seen.insert(provider.id.clone());
            }
        }
        seen.into_iter().collect()
    }
}

#[derive(Debug)]
struct LocalAgentSpec {
    id: &'static str,
    name: &'static str,
    kind: ProviderKind,
    command: String,
}

fn discover_local_agents() -> Vec<LocalAgentSpec> {
    [
        ("opencode", "OpenCode", ProviderKind::OpenCode),
        ("codex", "Codex CLI", ProviderKind::Codex),
        ("claude", "Claude Code", ProviderKind::Claude),
        ("agy", "Agy CLI", ProviderKind::Agy),
    ]
    .into_iter()
    .filter_map(|(id, name, kind)| {
        find_command(id).map(|command| LocalAgentSpec {
            id,
            name,
            kind,
            command,
        })
    })
    .collect()
}

fn find_command(command: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn provider_error_message(err: &ProviderError) -> String {
    match err {
        ProviderError::RateLimited(m) => format!("rate limited: {m}"),
        ProviderError::Auth(m) => format!("auth failed: {m}"),
        ProviderError::NotFound(m) => format!("not found: {m}"),
        ProviderError::BadRequest(m) => format!("bad request: {m}"),
        ProviderError::Upstream(m) => format!("upstream error: {m}"),
        ProviderError::Network(m) => format!("network error: {m}"),
    }
}

/// Ordered model ids to try on one provider for a single request.
///
/// Local CLIs keep whatever the caller asked for — the CLI resolves its own
/// cheap default for aliases like `auto`. HTTP providers get their configured
/// model first, then the kind's fallback ladder, so a request never dies just
/// because one model id was retired or is the wrong shape for the task.
fn model_candidates(provider: &Provider, requested: &str, has_images: bool) -> Vec<String> {
    if provider.kind.is_cli() {
        return vec![requested.to_string()];
    }

    let requested = requested.trim();
    let explicit = (!requested.is_empty()
        && !is_provider_alias_model(requested)
        && !provider.id.eq_ignore_ascii_case(requested))
    .then(|| requested.to_string());

    let mut out: Vec<String> = Vec::new();
    let mut push = |model: String| {
        if !model.trim().is_empty()
            && model != "default"
            && !out.iter().any(|m| m.eq_ignore_ascii_case(&model))
        {
            out.push(model);
        }
    };

    if let Some(model) = explicit {
        push(model);
    }
    push(provider.model.clone());
    for model in fallback_models(provider.kind, has_images) {
        push(model);
    }

    if has_images {
        // A text-only model given an image part answers with a 400 at best and
        // hallucinates at worst; never let a vision task land on one.
        let vision: Vec<String> = out.iter().filter(|m| is_vision_model(m)).cloned().collect();
        if !vision.is_empty() {
            return vision;
        }
    }

    if out.is_empty() {
        out.push(provider.model.clone());
    }
    out
}

/// Wall-clock budget for a single provider+model attempt, so one stalled
/// upstream cannot consume the caller's entire timeout. Local CLIs keep their
/// own (longer) process budget. Override with `ROUTER_ATTEMPT_TIMEOUT_SECS`.
fn attempt_timeout(provider: &Provider) -> std::time::Duration {
    let secs = std::env::var("ROUTER_ATTEMPT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(90)
        .max(5);
    let secs = if provider.kind.is_cli() {
        secs.max(provider.cli_timeout_secs + 5)
    } else {
        secs
    };
    std::time::Duration::from_secs(secs)
}

fn is_provider_alias_model(model: &str) -> bool {
    ["auto", "default", "router", "groq", "nvidia"]
        .iter()
        .any(|alias| model.eq_ignore_ascii_case(alias))
}

/// Fallback ladder per provider kind, overridable with comma-separated env vars:
/// `ROUTER_GROQ_FALLBACK_MODELS`, `ROUTER_NVIDIA_FALLBACK_MODELS`,
/// `ROUTER_VISION_FALLBACK_MODELS`.
fn fallback_models(kind: ProviderKind, has_images: bool) -> Vec<String> {
    let (var, defaults): (&str, &[&str]) = if has_images {
        (
            "ROUTER_VISION_FALLBACK_MODELS",
            &[
                "google/diffusiongemma-26b-a4b-it",
                "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
                "google/gemma-4-31b-it",
                "minimaxai/minimax-m3",
            ],
        )
    } else {
        match kind {
            ProviderKind::Groq => (
                "ROUTER_GROQ_FALLBACK_MODELS",
                &[
                    "openai/gpt-oss-120b",
                    "llama-3.3-70b-versatile",
                    "openai/gpt-oss-20b",
                ],
            ),
            ProviderKind::Nvidia => (
                "ROUTER_NVIDIA_FALLBACK_MODELS",
                &[
                    "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
                    "nvidia/nemotron-3-ultra-550b-a55b",
                    "google/gemma-4-31b-it",
                ],
            ),
            _ => return Vec::new(),
        }
    };

    match std::env::var(var) {
        Ok(raw) => raw
            .split(',')
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect(),
        Err(_) => defaults.iter().map(ToString::to_string).collect(),
    }
}

pub fn is_groq_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m == "groq"
        || m.starts_with("groq/")
        || m.starts_with("qwen")
        || m.starts_with("openai/gpt-oss")
        || m.starts_with("llama")
        || m.starts_with("meta-llama/")
        || m.starts_with("deepseek")
        || m.starts_with("mixtral")
        || m.starts_with("mistral")
        || m.starts_with("gemma2")
        || m.starts_with("whisper")
}

pub fn is_nvidia_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m == "nvidia"
        || m.starts_with("nvidia/")
        || m.starts_with("nemotron")
        || m.starts_with("google/diffusiongemma")
        || m.starts_with("google/gemma-4")
        || m.starts_with("minimaxai/")
}

pub fn is_vision_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("google/diffusiongemma")
        || m.starts_with("google/gemma-4")
        || m.starts_with("minimaxai/")
        || m.starts_with("nvidia/nemotron-3-nano-omni")
        || m.contains("vision")
        || m.contains("omni")
}

fn provider_relevance(provider: &Provider, requested_model: Option<&str>, has_images: bool) -> u32 {
    if has_images {
        // When images are present, prioritize vision-capable providers (Nvidia)
        if provider.kind == ProviderKind::Nvidia || is_vision_model(&provider.model) {
            0
        } else if !provider.kind.is_cli() {
            5
        } else {
            20
        }
    } else {
        let Some(model) = requested_model else {
            // Pure text default: Groq has top priority (0) for ultra-fast throughput
            return match provider.kind {
                ProviderKind::Groq => 0,
                ProviderKind::Nvidia => 1,
                _ if !provider.kind.is_cli() => 2,
                _ => 10,
            };
        };
        let model_lower = model.to_ascii_lowercase();
        let is_nvidia_match =
            is_nvidia_model(&model_lower) && provider.kind == ProviderKind::Nvidia;
        let is_groq_match = is_groq_model(&model_lower) && provider.kind == ProviderKind::Groq;
        let is_direct_match =
            provider.model.eq_ignore_ascii_case(model) || provider.id.eq_ignore_ascii_case(model);

        if is_nvidia_match || is_groq_match || is_direct_match {
            0
        } else if !provider.kind.is_cli() {
            1
        } else {
            10
        }
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    fn http_provider(kind: ProviderKind, model: &str) -> Provider {
        Provider::new_http("p1", "P1", kind, "https://example.com", Some("k"), model)
    }

    #[test]
    fn auto_expands_to_the_provider_model_then_the_kind_ladder() {
        let provider = http_provider(ProviderKind::Groq, "openai/gpt-oss-120b");
        let models = model_candidates(&provider, "auto", false);
        assert_eq!(models.first().unwrap(), "openai/gpt-oss-120b");
        assert!(models.contains(&"llama-3.3-70b-versatile".to_string()));
        assert!(models.len() > 1, "auto must have somewhere to fall back to");
    }

    #[test]
    fn explicit_model_is_tried_first_but_still_has_fallbacks() {
        let provider = http_provider(ProviderKind::Nvidia, "nvidia/nemotron-3-ultra-550b-a55b");
        let models = model_candidates(&provider, "google/gemma-4-31b-it", false);
        assert_eq!(models.first().unwrap(), "google/gemma-4-31b-it");
        assert!(models.contains(&"nvidia/nemotron-3-ultra-550b-a55b".to_string()));
    }

    #[test]
    fn image_requests_never_fall_back_to_a_text_only_model() {
        let provider = http_provider(ProviderKind::Nvidia, "nvidia/nemotron-3-ultra-550b-a55b");
        let models = model_candidates(&provider, "auto", true);
        assert!(!models.is_empty());
        assert!(
            models.iter().all(|m| is_vision_model(m)),
            "text-only models leaked into a vision request: {models:?}"
        );
    }

    #[test]
    fn local_agents_keep_the_requested_alias() {
        let provider = Provider::new_cli("agy", "Agy", ProviderKind::Agy, "agy", "default", ".", 60);
        assert_eq!(model_candidates(&provider, "auto", false), vec!["auto"]);
    }
}
