//! TravellerMapTool enum and execution logic.

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::client::TravellerMapClient;
use super::options::{JumpMapOptions, PosterOptions, RouteOptions};
use super::sanitize_filename;

/// Tool enum for integration with the service
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TravellerMapTool {
    /// Search for worlds, sectors, or subsectors
    Search {
        query: String,
        milieu: Option<String>,
    },

    /// Get worlds within jump range
    JumpWorlds {
        sector: String,
        hex: String,
        jump: u8,
    },

    /// Calculate a jump route
    Route {
        start: String,
        end: String,
        jump: u8,
        #[serde(default)]
        wild: bool,
        #[serde(default)]
        imperium_only: bool,
        #[serde(default)]
        no_red_zones: bool,
    },

    /// Get world data
    WorldData { sector: String, hex: String },

    /// Get sector metadata
    SectorMetadata { sector: String },

    /// Get sector data (UWP listing)
    SectorData {
        sector: String,
        subsector: Option<String>,
    },

    /// Get coordinates for a location
    Coordinates { sector: String, hex: Option<String> },

    /// List all sectors
    ListSectors { milieu: Option<String> },

    /// List available time periods
    ListMilieux,

    /// Get a poster/map URL for a sector
    PosterUrl {
        sector: String,
        subsector: Option<String>,
        style: Option<String>,
    },

    /// Get a jump map URL
    JumpMapUrl {
        sector: String,
        hex: String,
        jump: u8,
        style: Option<String>,
    },

    /// Download a sector/subsector map (returns bytes for saving)
    DownloadPoster {
        sector: String,
        subsector: Option<String>,
        style: Option<String>,
        scale: Option<u32>,
    },

    /// Download a jump map (returns bytes for saving)
    DownloadJumpMap {
        sector: String,
        hex: String,
        jump: u8,
        style: Option<String>,
        scale: Option<u32>,
    },
}

impl TravellerMapTool {
    /// Execute a Traveller Map tool
    pub async fn execute(&self, client: &TravellerMapClient) -> Result<serde_json::Value, String> {
        match self {
            TravellerMapTool::Search { query, milieu } => {
                let results = client
                    .search(query, milieu.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(results).map_err(|e| e.to_string())
            }

            TravellerMapTool::JumpWorlds { sector, hex, jump } => {
                let results = client
                    .jump_worlds(sector, hex, *jump)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(results).map_err(|e| e.to_string())
            }

            TravellerMapTool::Route {
                start,
                end,
                jump,
                wild,
                imperium_only,
                no_red_zones,
            } => {
                let options = RouteOptions {
                    wild: *wild,
                    imperium_only: *imperium_only,
                    no_red_zones: *no_red_zones,
                    allow_anomalies: false,
                };
                let results = client
                    .route(start, end, *jump, options)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(results).map_err(|e| e.to_string())
            }

            TravellerMapTool::WorldData { sector, hex } => {
                let data = client
                    .world_data(sector, hex)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(data).map_err(|e| e.to_string())
            }

            TravellerMapTool::SectorMetadata { sector } => {
                let metadata = client
                    .sector_metadata(sector)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(metadata).map_err(|e| e.to_string())
            }

            TravellerMapTool::SectorData { sector, subsector } => {
                let data = client
                    .sector_data(sector, subsector.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                // Parse the tab-delimited data and return as structured JSON
                let lines: Vec<&str> = data.lines().collect();
                if lines.is_empty() {
                    return Ok(serde_json::json!({ "worlds": [] }));
                }

                // First line is usually headers
                let worlds: Vec<serde_json::Value> = lines
                    .iter()
                    .skip(1)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(|line| {
                        let fields: Vec<&str> = line.split('\t').collect();
                        serde_json::json!({
                            "raw": line,
                            "fields": fields,
                        })
                    })
                    .collect();

                Ok(serde_json::json!({
                    "sector": sector,
                    "subsector": subsector,
                    "world_count": worlds.len(),
                    "raw_data": data,
                }))
            }

            TravellerMapTool::Coordinates { sector, hex } => {
                let coords = client
                    .coordinates(sector, hex.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(coords).map_err(|e| e.to_string())
            }

            TravellerMapTool::ListSectors { milieu } => {
                let results = client
                    .universe(milieu.as_deref(), true)
                    .await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(results).map_err(|e| e.to_string())
            }

            TravellerMapTool::ListMilieux => {
                let results = client.milieux().await.map_err(|e| e.to_string())?;
                serde_json::to_value(results).map_err(|e| e.to_string())
            }

            TravellerMapTool::PosterUrl {
                sector,
                subsector,
                style,
            } => {
                let options = PosterOptions {
                    subsector: subsector.clone(),
                    style: style.clone(),
                    ..Default::default()
                };
                let url = client.poster_url(sector, &options);
                Ok(serde_json::json!({
                    "url": url,
                    "description": format!("Sector map for {}", sector),
                }))
            }

            TravellerMapTool::JumpMapUrl {
                sector,
                hex,
                jump,
                style,
            } => {
                let options = JumpMapOptions {
                    style: style.clone(),
                    ..Default::default()
                };
                let url = client.jump_map_url(sector, hex, *jump, &options);
                Ok(serde_json::json!({
                    "url": url,
                    "description": format!("Jump-{} map from {} {}", jump, sector, hex),
                }))
            }

            TravellerMapTool::DownloadPoster {
                sector,
                subsector,
                style,
                scale,
            } => {
                let options = PosterOptions {
                    subsector: subsector.clone(),
                    style: style.clone(),
                    scale: *scale,
                    ..Default::default()
                };
                let (bytes, extension) = client
                    .download_poster(sector, &options)
                    .await
                    .map_err(|e| e.to_string())?;

                // Return base64-encoded image data with metadata
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);

                // Generate a suggested filename
                let filename = if let Some(ss) = subsector {
                    format!(
                        "{}-{}.{}",
                        sanitize_filename(sector),
                        sanitize_filename(ss),
                        extension
                    )
                } else {
                    format!("{}.{}", sanitize_filename(sector), extension)
                };

                Ok(serde_json::json!({
                    "filename": filename,
                    "extension": extension,
                    "size_bytes": bytes.len(),
                    "base64_data": base64_data,
                    "description": format!("Sector map for {}{}", sector, subsector.as_ref().map(|s| format!(" subsector {}", s)).unwrap_or_default()),
                }))
            }

            TravellerMapTool::DownloadJumpMap {
                sector,
                hex,
                jump,
                style,
                scale,
            } => {
                let options = JumpMapOptions {
                    style: style.clone(),
                    scale: *scale,
                    ..Default::default()
                };
                let (bytes, extension) = client
                    .download_jump_map(sector, hex, *jump, &options)
                    .await
                    .map_err(|e| e.to_string())?;

                // Return base64-encoded image data with metadata
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);

                // Generate a suggested filename
                let filename = format!(
                    "{}-{}-jump{}.{}",
                    sanitize_filename(sector),
                    hex,
                    jump,
                    extension
                );

                Ok(serde_json::json!({
                    "filename": filename,
                    "extension": extension,
                    "size_bytes": bytes.len(),
                    "base64_data": base64_data,
                    "description": format!("Jump-{} map from {} {}", jump, sector, hex),
                }))
            }
        }
    }
}
