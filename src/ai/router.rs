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
        dto::{ChatCompletionRequest, CompletionOutcome, RouterError, StreamOutcome, StreamSource},
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

impl AiRouter {
    /// Build the router from persisted providers, environment-seeded Groq keys,
    /// and locally discovered agent CLIs.
    pub async fn new(settings: &Settings, db: Db) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build http client");

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
        // HTTP Groq keys are the default pool for `auto`; local agents remain
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
    pub async fn complete(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<CompletionOutcome, RouterError> {
        let candidates = self.candidates(req.provider.as_deref()).await?;
        let mut last_error: Option<String> = None;

        for (id, provider) in candidates {
            if !provider.enabled || provider.in_cooldown() {
                continue;
            }

            match provider.chat_completion(&self.client, req).await {
                Ok(response) => {
                    self.mark_success(&id).await;
                    return Ok(CompletionOutcome {
                        provider_id: id,
                        provider_name: provider.name,
                        model: if response.model.is_empty() {
                            provider.model
                        } else {
                            response.model.clone()
                        },
                        response,
                    });
                }
                Err(err) => {
                    let msg = provider_error_message(&err);
                    last_error = Some(msg.clone());
                    self.mark_failure(&id, &msg).await;
                    if is_client_error(&err) {
                        return Err(RouterError::ClientError(msg));
                    }
                }
            }
        }

        Err(RouterError::AllProvidersFailed(
            last_error.unwrap_or_else(|| "no providers available".to_string()),
        ))
    }

    /// Route a streaming request. HTTP providers are passed through as SSE;
    /// local CLIs return one synthesized SSE completion after the process exits.
    pub async fn stream(&self, req: &ChatCompletionRequest) -> Result<StreamOutcome, RouterError> {
        let candidates = self.candidates(req.provider.as_deref()).await?;
        let mut last_error: Option<String> = None;

        for (id, provider) in candidates {
            if !provider.enabled || provider.in_cooldown() {
                continue;
            }

            if provider.kind.is_cli() {
                let mut non_stream = req.clone();
                non_stream.stream = Some(false);
                match provider.chat_completion(&self.client, &non_stream).await {
                    Ok(response) => {
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
                        self.mark_failure(&id, &msg).await;
                        if is_client_error(&err) {
                            return Err(RouterError::ClientError(msg));
                        }
                    }
                }
                continue;
            }

            match provider.stream_chat(&self.client, req).await {
                Ok(response) => {
                    self.mark_success(&id).await;
                    return Ok(StreamOutcome {
                        provider_id: id,
                        provider_name: provider.name,
                        model: req.model.clone().if_empty_then(provider.model),
                        source: StreamSource::Http(response),
                    });
                }
                Err(err) => {
                    let msg = provider_error_message(&err);
                    last_error = Some(msg.clone());
                    self.mark_failure(&id, &msg).await;
                    if is_client_error(&err) {
                        return Err(RouterError::ClientError(msg));
                    }
                }
            }
        }

        Err(RouterError::AllProvidersFailed(
            last_error.unwrap_or_else(|| "no providers available".to_string()),
        ))
    }

    async fn candidates(
        &self,
        selector: Option<&str>,
    ) -> Result<Vec<(String, Provider)>, RouterError> {
        let providers = self.providers.read().await;
        let selector = selector.map(str::trim).filter(|s| !s.is_empty());

        let mut out = Vec::new();
        for provider in providers.iter() {
            let matches = match selector {
                // Existing OpenAI-compatible `/v1` clients stay on HTTP Groq
                // providers unless they explicitly opt into local agents.
                None => provider.kind == ProviderKind::Groq,
                Some(value)
                    if value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("all") =>
                {
                    true
                }
                Some(value) if value.eq_ignore_ascii_case("groq") => {
                    provider.kind == ProviderKind::Groq
                }
                Some(value) => {
                    ProviderKind::parse(value).is_some_and(|kind| provider.kind == kind)
                        || provider.id.eq_ignore_ascii_case(value)
                        || provider.name.eq_ignore_ascii_case(value)
                        || provider.kind.as_str().eq_ignore_ascii_case(value)
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
        ProviderError::Upstream(m) => format!("upstream error: {m}"),
        ProviderError::Network(m) => format!("network error: {m}"),
    }
}

/// Client errors that should not be retried against another provider.
fn is_client_error(err: &ProviderError) -> bool {
    matches!(err, ProviderError::Auth(_))
}

trait EmptyStringFallback {
    fn if_empty_then(self, fallback: String) -> String;
}

impl EmptyStringFallback for String {
    fn if_empty_then(self, fallback: String) -> String {
        if self.is_empty() { fallback } else { self }
    }
}
