//! Configuration loading from files and environment variables.

use config::{Config, Environment, File};
use serde::Deserialize;

use crate::error::ServiceResult;

use super::dynamic_config::DynamicConfig;
use super::static_config::{
    FvttConfig, ServerConfig, StaticConfig, StorageConfig, default_server, default_storage,
};

/// Internal struct for loading static fields from config sources
#[derive(Debug, Clone, Deserialize)]
struct StaticConfigLoader {
    #[serde(default = "default_server")]
    pub server: ServerConfig,

    #[serde(default = "default_storage")]
    pub storage: StorageConfig,

    #[serde(default)]
    pub fvtt: FvttConfig,
}

/// Load static configuration from file and env vars
pub fn load_static_config() -> ServiceResult<StaticConfig> {
    let loader: StaticConfigLoader = Config::builder()
        .add_source(File::with_name("config").required(false))
        .add_source(
            Environment::with_prefix("SENESCHAL")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to build config: {}", e),
        })?
        .try_deserialize()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to deserialize static config: {}", e),
        })?;

    Ok(StaticConfig {
        server: loader.server,
        storage: loader.storage,
        fvtt: loader.fvtt,
    })
}

/// Load dynamic configuration from file and env vars (without DB overrides)
pub fn load_dynamic_config() -> ServiceResult<DynamicConfig> {
    Config::builder()
        .add_source(File::with_name("config").required(false))
        .add_source(
            Environment::with_prefix("SENESCHAL")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to build config: {}", e),
        })?
        .try_deserialize()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to deserialize dynamic config: {}", e),
        })
}
