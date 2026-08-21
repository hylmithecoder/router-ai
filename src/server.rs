//! Server lifecycle management.
//!
//! This module binds the TCP listener, builds the Axum router, and runs until a shutdown
//! signal is received. Keeping this separate from `main.rs` makes it easy to start the
//! server programmatically in integration tests.

use crate::log_info;
use crate::{
    ai::router::AiRouter, config::Settings, database::Db, routes::create_router, state::AppState,
};
use anyhow::Result;
use axum::serve;
use tokio::net::TcpListener;

/// Start the HTTP server and block until shutdown.
pub async fn run(settings: Settings) -> Result<()> {
    // Open the database and seed keys from the environment.
    let db =
        Db::open_with_master_key(&settings.router.db_path, &settings.router.master_key).await?;
    db.seed_api_keys(&settings.router.api_keys).await?;

    // Build the fallback router over persisted Groq keys and discovered local CLIs.
    let router = AiRouter::new(&settings, db.clone()).await;

    let state = AppState::new(settings.clone(), db, router);
    let app = create_router(state);

    let bind_addr = settings.bind_address();
    let listener = TcpListener::bind(&bind_addr).await?;

    log_info!("{} listening on http://{}", settings.app.name, bind_addr);

    serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    log_info!("server stopped");
    Ok(())
}

/// Wait for a shutdown signal.
///
/// On Unix this handles both `Ctrl+C` and `SIGTERM`, which is what Docker/Kubernetes use.
/// On non-Unix platforms only `Ctrl+C` is handled.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => log_info!("received Ctrl+C, shutting down"),
        _ = terminate => log_info!("received SIGTERM, shutting down"),
    }
}
