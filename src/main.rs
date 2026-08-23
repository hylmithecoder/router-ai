//! Binary entry point.
//!
//! Keep `main.rs` tiny: it only wires together tracing, configuration, and the server.
//! All business logic lives in `lib.rs` modules so they can be unit and integration tested
//! without running a full binary.

use anyhow::Result;
use router_api_ai::config::Settings;
use router_api_ai::server;
use router_api_ai::ログ_インフォ;

#[tokio::main]
async fn main() -> Result<()> {
    // Load `.env` into the process environment. If the file is missing, this is a no-op,
    // which makes the binary work both locally (with a file) and in containers (with env vars).
    dotenvy::dotenv().ok();

    // Install a global tracing subscriber with a default max log level of INFO.
    // To enable debug logs, set the `RUST_LOG` environment variable before starting.
    tracing_subscriber::fmt()
        .with_max_level(tracing::level_filters::LevelFilter::INFO)
        .with_target(true)
        .init();

    // Load configuration. Missing variables fall back to sensible defaults.
    let settings = Settings::new();
    ログ_インフォ!("スタート {}", settings.app.name);

    // Hand control to the server module. It blocks until a shutdown signal is received.
    server::run(settings).await
}
