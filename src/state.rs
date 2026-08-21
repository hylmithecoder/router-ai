//! Shared application state.
//!
//! `AppState` is created once at startup and passed to every request handler through
//! Axum's `State` extractor. Cloning is cheap: everything lives behind `Arc`/mutexes.

use std::sync::Arc;

use crate::{ai::router::AiRouter, config::Settings, database::Db};

/// State shared across all requests.
#[derive(Debug, Clone)]
pub struct AppState {
    /// A snapshot of the validated configuration. Handlers can read it but not mutate it.
    pub config: Settings,
    /// SQLite storage (keys, usage, providers).
    pub db: Db,
    /// The fallback router. Provider state is protected internally and upstream
    /// calls do not hold a global request lock.
    pub router: Arc<AiRouter>,
    /// Shared HTTP client used by handlers (upstream proxying happens inside the router).
    pub http: reqwest::Client,
}

impl AppState {
    /// Create the initial state from configuration.
    pub fn new(config: Settings, db: Db, router: AiRouter) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build http client");

        Self {
            config,
            db,
            router: Arc::new(router),
            http,
        }
    }
}
