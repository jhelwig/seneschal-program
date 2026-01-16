use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;

mod api;
mod auto_import;
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
mod websocket;

use crate::config::{RuntimeConfig, StaticConfig};
use crate::db::Database;
use crate::service::SeneschalService;

// Re-export config crate types to avoid namespace collision
use ::config::{Config as ConfigBuilder, Environment, File};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    init_logging();

    info!(
        "Starting Seneschal Program service v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Load static configuration (server binding, storage path)
    // We need to load this first to know where the database is
    let static_config: StaticConfig = ConfigBuilder::builder()
        .add_source(File::with_name("config").required(false))
        .add_source(
            Environment::with_prefix("SENESCHAL")
                .separator("__")
                .try_parsing(true),
        )
        .build()?
        .try_deserialize()?;

    info!(
        host = %static_config.server.host,
        port = static_config.server.port,
        "Static configuration loaded"
    );

    // Ensure data directory exists
    std::fs::create_dir_all(&static_config.storage.data_dir)?;

    // Initialize database
    let db_path = static_config.storage.data_dir.join("seneschal.db");
    let db = Arc::new(Database::open(&db_path)?);
    info!(path = %db_path.display(), "Database initialized");

    // Load runtime config (static + dynamic with DB overrides)
    let runtime_config = Arc::new(RuntimeConfig::load(&db)?);
    info!("Runtime configuration loaded with DB settings");

    // Initialize the service
    let service = Arc::new(SeneschalService::new(db, runtime_config.clone()).await?);

    // Backfill document hashes for existing documents (one-time migration)
    match service.backfill_document_hashes().await {
        Ok(count) if count > 0 => info!(count, "Backfilled document hashes"),
        Err(e) => tracing::warn!(error = %e, "Document hash backfill failed"),
        _ => {}
    }

    // Build the router
    let mut app = api::router(service.clone(), &runtime_config);

    // Add MCP endpoint if enabled
    let mcp_config = runtime_config.dynamic();
    if mcp_config.mcp.enabled {
        let mcp_path = mcp_config.mcp.path.clone();
        info!(path = %mcp_path, "MCP server enabled");
        app = app.nest(&mcp_path, mcp::mcp_router(service.clone()));
    }

    // Start document processing worker (resumes any pending documents)
    SeneschalService::start_document_processing_worker(service.clone());

    // Start image captioning worker (runs in parallel, separate from document processing)
    SeneschalService::start_captioning_worker(service.clone());

    // Start auto-import worker if configured
    if let Some(auto_import_dir) = &runtime_config.static_config.storage.auto_import_dir {
        auto_import::start_auto_import_worker(service.clone(), auto_import_dir.clone());
    }

    // Start conversation cleanup background task
    let cleanup_service = service.clone();
    let cleanup_interval = runtime_config.dynamic().conversation.cleanup_interval();
    let max_per_user = runtime_config.dynamic().conversation.max_per_user;
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
    let addr = format!(
        "{}:{}",
        runtime_config.static_config.server.host, runtime_config.static_config.server.port
    );
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

    // Use RUST_LOG if set, otherwise default to info level for our crate
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("seneschal_service=info"));

    tracing_subscriber::registry()
        .with(fmt::layer().event_format(format))
        .with(filter)
        .init();
}
