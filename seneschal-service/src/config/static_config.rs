//! Static configuration that cannot be changed at runtime.
//! These settings affect server binding or require restart to change.

use serde::Deserialize;
use std::path::PathBuf;

/// Static configuration that cannot be changed at runtime
/// These settings affect server binding or require restart to change
#[derive(Debug, Clone, Deserialize)]
pub struct StaticConfig {
    #[serde(default = "default_server")]
    pub server: ServerConfig,

    #[serde(default = "default_storage")]
    pub storage: StorageConfig,

    #[serde(default)]
    pub fvtt: FvttConfig,
}

/// HTTP server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,
}

/// Storage configuration
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// Optional auto-import directory. When set, files placed here are automatically
    /// imported. Files are moved to processed/ or failed/ subdirectories after import.
    #[serde(default)]
    pub auto_import_dir: Option<PathBuf>,
}

/// FVTT integration configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FvttConfig {
    /// Path to FVTT assets directory (Data/assets). If provided and writable,
    /// images are copied directly. Otherwise, shuttled via API.
    #[serde(default)]
    pub assets_path: Option<PathBuf>,
}

/// Determines how to deliver images to FVTT
#[derive(Debug, Clone)]
pub enum AssetsAccess {
    /// Backend can write directly to FVTT assets directory
    Direct(PathBuf),
    /// Images must be shuttled via API to the module
    Shuttle,
}

impl FvttConfig {
    /// Check if we can write directly to FVTT assets
    pub fn check_assets_access(&self) -> AssetsAccess {
        match &self.assets_path {
            None => AssetsAccess::Shuttle,
            Some(path) => {
                // Test write access by creating the seneschal directory
                let seneschal_dir = path.join("seneschal");
                match std::fs::create_dir_all(&seneschal_dir) {
                    // Return the base assets path, not the seneschal subdir
                    // The fvtt_image_path function includes seneschal/ in its output
                    Ok(_) => AssetsAccess::Direct(path.clone()),
                    Err(e) => {
                        tracing::warn!(
                            path = %seneschal_dir.display(),
                            error = %e,
                            "FVTT assets path not writable, falling back to API shuttle"
                        );
                        AssetsAccess::Shuttle
                    }
                }
            }
        }
    }
}

// ==================== Default Value Functions ====================

pub(crate) fn default_server() -> ServerConfig {
    ServerConfig {
        host: default_host(),
        port: default_port(),
    }
}

pub(crate) fn default_host() -> String {
    "0.0.0.0".to_string()
}

pub(crate) fn default_port() -> u16 {
    8080
}

pub(crate) fn default_storage() -> StorageConfig {
    StorageConfig {
        data_dir: default_data_dir(),
        auto_import_dir: None,
    }
}

pub(crate) fn default_data_dir() -> PathBuf {
    PathBuf::from("./data")
}
