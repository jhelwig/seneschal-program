use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;

mod api;
mod config;
mod db;
mod error;
mod i18n;
mod ingestion;
mod mcp;
mod ollama;
mod search;
mod service;
mod tools;

use crate::config::AppConfig;
use crate::service::SeneschalService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    info!(
        "Starting Seneschal Program service v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Load configuration
    let config = AppConfig::load()?;
    info!(
        host = %config.server.host,
        port = config.server.port,
        "Configuration loaded"
    );

    // Initialize the service
    let service = Arc::new(SeneschalService::new(config.clone()).await?);

    // Build the router
    let mut app = api::router(service.clone(), &config);

    // Add MCP endpoint if enabled
    if config.mcp.enabled {
        let mcp_path = config.mcp.path.clone();
        info!(path = %mcp_path, "MCP server enabled");
        app = app.nest(&mcp_path, mcp::mcp_router(service.clone()));
    }

    // Start document processing worker (resumes any pending documents)
    SeneschalService::start_document_processing_worker(service.clone());

    // Start conversation cleanup background task
    let cleanup_service = service.clone();
    let cleanup_interval = config.conversation.cleanup_interval();
    let max_per_user = config.conversation.max_per_user;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(cleanup_interval);
        loop {
            interval.tick().await;
            // Clean up old conversations
            match cleanup_service.cleanup_conversations() {
                Ok(count) if count > 0 => {
                    info!(removed = count, "Cleaned up old conversations");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Conversation cleanup failed");
                }
                _ => {}
            }
            // Clean up excess conversations per user
            if max_per_user > 0 {
                match cleanup_service.cleanup_excess_conversations(max_per_user) {
                    Ok(count) if count > 0 => {
                        info!(removed = count, "Cleaned up excess conversations per user");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Excess conversation cleanup failed");
                    }
                    _ => {}
                }
            }
        }
    });

    // Start the server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let format = fmt::format()
        .with_target(true)
        .with_thread_ids(true)
        .compact();

    tracing_subscriber::registry()
        .with(fmt::layer().event_format(format))
        .with(
            EnvFilter::from_default_env().add_directive("seneschal_service=info".parse().unwrap()),
        )
        .init();
}
