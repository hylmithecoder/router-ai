//! Application configuration.
//!
//! All environment-specific values are centralized here. The rest of the app receives
//! a fully validated `Settings` object and never reads `std::env` directly.
//!
//! Variables are read from the process environment with the prefix `APP_`:
//!
//! - `APP_NAME=my-api`
//! - `APP_SERVER_HOST=0.0.0.0`
//! - `APP_SERVER_PORT=8080`
//!
//! A `.env` file is loaded in `main.rs` before this module is called, so local development
//! can keep values in a file while production uses real environment variables.

use std::env;

/// Default values used when the environment does not provide them.
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 5790;
const DEFAULT_APP_NAME: &str = "router-api-ai";

/// Default Groq endpoint.
const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Root configuration object.
#[derive(Debug, Clone)]
pub struct Settings {
    pub app: AppSettings,
    pub server: ServerSettings,
    pub router: RouterSettings,
}

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
}

/// AI router specific settings.
#[derive(Debug, Clone)]
pub struct RouterSettings {
    /// Admin key that unlocks the admin API and the dashboard.
    pub master_key: String,
    /// Comma-separated personal API keys (optional; keys can also be created via the admin API).
    pub api_keys: Vec<ApiKeySeed>,
    /// Comma-separated upstream keys, ordered by fallback priority.
    /// Each entry may be `key` (uses `groq_base_url`) or `key@https://base.url`.
    pub groq_api_keys: Vec<GroqKeySpec>,
    /// Base URL for the Groq OpenAI-compatible API.
    pub groq_base_url: String,
    /// Default model served by every provider.
    pub default_model: String,
    /// SQLite database file path.
    pub db_path: String,
    /// Directory of the statically exported dashboard (served on the same port).
    pub static_dir: String,
    /// How long (seconds) a provider stays in cooldown after a failure.
    pub provider_cooldown_secs: u64,
    /// Default daily token quota per key (0 = unlimited).
    pub daily_quota_tokens: i64,
    /// Maximum time a local agent CLI may run for one request.
    pub cli_timeout_secs: u64,
    /// Working directory exposed to local agent CLIs. Local providers are run in
    /// read-only/sandboxed modes by the router.
    pub agent_workdir: String,
}

/// A key seeded from the environment at startup.
#[derive(Debug, Clone)]
pub struct ApiKeySeed {
    pub name: String,
    pub key: String,
}

/// One upstream provider entry: an API key plus (optional) custom base URL.
#[derive(Debug, Clone)]
pub struct GroqKeySpec {
    pub key: String,
    pub base_url: String,
}

impl Settings {
    /// Build configuration from defaults and environment variables.
    pub fn new() -> Self {
        let master_key = env::var("ROUTER_MASTER_KEY")
            .unwrap_or_else(|_| panic!("ROUTER_MASTER_KEY must be set (use `.env` in dev)"));

        let groq_base_url =
            env::var("ROUTER_GROQ_BASE_URL").unwrap_or_else(|_| DEFAULT_GROQ_BASE_URL.to_string());

        let api_keys = env::var("ROUTER_API_KEYS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter(|k| !k.trim().is_empty())
                    .enumerate()
                    .map(|(i, k)| ApiKeySeed {
                        name: format!("seed-{}", i + 1),
                        key: k.trim().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let groq_api_keys = env::var("GROQ_API_KEYS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter(|k| !k.trim().is_empty())
                    .map(|entry| {
                        let entry = entry.trim();
                        match entry.split_once('@') {
                            Some((key, url)) => GroqKeySpec {
                                key: key.trim().to_string(),
                                base_url: url.trim().to_string(),
                            },
                            None => GroqKeySpec {
                                key: entry.to_string(),
                                base_url: groq_base_url.clone(),
                            },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let default_model = env::var("ROUTER_DEFAULT_MODEL")
            .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());

        let db_path = env::var("ROUTER_DB_PATH").unwrap_or_else(|_| "router.db".to_string());

        let static_dir = env::var("ROUTER_STATIC_DIR").unwrap_or_else(|_| "webui/out".to_string());

        let provider_cooldown_secs = env::var("ROUTER_PROVIDER_COOLDOWN_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);

        let daily_quota_tokens = env::var("ROUTER_DAILY_QUOTA_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1_000_000);

        let cli_timeout_secs = env::var("ROUTER_CLI_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);

        let agent_workdir = env::var("ROUTER_AGENT_WORKDIR").unwrap_or_else(|_| ".".to_string());

        Self {
            app: AppSettings {
                name: env::var("APP_NAME").unwrap_or_else(|_| DEFAULT_APP_NAME.to_string()),
            },
            server: ServerSettings {
                host: env::var("APP_SERVER_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string()),
                port: env::var("APP_SERVER_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_PORT),
            },
            router: RouterSettings {
                master_key,
                api_keys,
                groq_api_keys,
                groq_base_url,
                default_model,
                db_path,
                static_dir,
                provider_cooldown_secs,
                daily_quota_tokens,
                cli_timeout_secs,
                agent_workdir,
            },
        }
    }

    /// Convenience helper for binding the TCP listener.
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_equals_one() {
        assert_eq!(1, 1);
    }

    #[test]
    fn default_settings_are_valid() {
        unsafe {
            std::env::set_var("ROUTER_MASTER_KEY", "test-master");
        }
        let settings = Settings::default();
        assert_eq!(settings.server.port, DEFAULT_PORT);
        assert_eq!(settings.bind_address(), "127.0.0.1:5790");
        assert_eq!(settings.router.master_key, "test-master");
    }
}
