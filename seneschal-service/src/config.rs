//! Configuration management for Seneschal service.
//!
//! This module provides layered configuration with:
//! - Static config: Server, storage, FVTT paths (startup-only, cannot change at runtime)
//! - Dynamic config: Ollama, embeddings, processing settings (hot-reloadable via API)
//!
//! Configuration sources (in order of precedence):
//! 1. Database settings (highest priority, for dynamic config only)
//! 2. Environment variables (SENESCHAL__ prefix)
//! 3. config.toml file
//! 4. Default values

mod dynamic_config;
mod loader;
mod static_config;

use arc_swap::ArcSwap;
use std::sync::Arc;

use crate::db::Database;
use crate::error::ServiceResult;

// Re-export public types from submodules
pub use dynamic_config::{DynamicConfig, EmbeddingsConfig, ImageExtractionConfig, OllamaConfig};
pub use loader::{load_dynamic_config, load_static_config};
pub use static_config::{AssetsAccess, StaticConfig};

// ==================== RuntimeConfig (combines static + dynamic) ====================

/// Runtime configuration manager
/// Combines static config (startup-only) with dynamic config (hot-reloadable via ArcSwap)
pub struct RuntimeConfig {
    /// Static configuration (never changes after startup)
    pub static_config: StaticConfig,
    /// Dynamic configuration (can be hot-reloaded)
    dynamic: ArcSwap<DynamicConfig>,
}

impl RuntimeConfig {
    /// Get current dynamic config snapshot (lock-free read)
    pub fn dynamic(&self) -> arc_swap::Guard<Arc<DynamicConfig>> {
        self.dynamic.load()
    }

    /// Update dynamic config (atomic swap)
    pub fn update_dynamic(&self, new_config: DynamicConfig) {
        self.dynamic.store(Arc::new(new_config));
    }

    /// Load config from all sources with DB overrides
    pub fn load(db: &Database) -> ServiceResult<Self> {
        // Load static config from env/file
        let static_config = load_static_config()?;

        // Load dynamic config defaults from env/file, then apply DB overrides
        let mut dynamic = load_dynamic_config()?;
        let db_settings = db.get_all_settings()?;
        dynamic.merge_from_db(&db_settings);

        Ok(Self {
            static_config,
            dynamic: ArcSwap::from_pointee(dynamic),
        })
    }

    /// Rebuild dynamic config from file/env defaults + DB and swap atomically
    pub fn reload_from_db(&self, db: &Database) -> ServiceResult<()> {
        let mut dynamic = load_dynamic_config()?;
        let db_settings = db.get_all_settings()?;
        dynamic.merge_from_db(&db_settings);
        self.update_dynamic(dynamic);
        Ok(())
    }
}
