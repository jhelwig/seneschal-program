//! Traveller Map API integration.
//!
//! This module provides tools for querying the Traveller Map web service
//! (https://travellermap.com) to retrieve sector data, world information,
//! jump routes, and more.

use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default base URL for the Traveller Map API
const DEFAULT_BASE_URL: &str = "https://travellermap.com";

/// Default timeout for API requests
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Traveller Map API client
#[derive(Clone)]
pub struct TravellerMapClient {
    client: Client,
    base_url: String,
}

impl Default for TravellerMapClient {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL, DEFAULT_TIMEOUT_SECS)
    }
}

impl TravellerMapClient {
    /// Create a new Traveller Map client
    pub fn new(base_url: &str, timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("Seneschal-Program/1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Search for worlds, sectors, and subsectors by name or criteria
    pub async fn search(
        &self,
        query: &str,
        milieu: Option<&str>,
    ) -> Result<SearchResults, TravellerMapError> {
        let mut url = format!(
            "{}/api/search?q={}",
            self.base_url,
            urlencoding::encode(query)
        );
        if let Some(m) = milieu {
            url.push_str(&format!("&milieu={}", urlencoding::encode(m)));
        }

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: SearchResults = response.json().await?;
        Ok(results)
    }

    /// Get worlds within jump range of a location
    pub async fn jump_worlds(
        &self,
        sector: &str,
        hex: &str,
        jump: u8,
    ) -> Result<JumpWorldsResult, TravellerMapError> {
        let url = format!(
            "{}/api/jumpworlds?sector={}&hex={}&jump={}",
            self.base_url,
            urlencoding::encode(sector),
            urlencoding::encode(hex),
            jump
        );

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: JumpWorldsResult = response.json().await?;
        Ok(results)
    }

    /// Calculate a jump route between two locations
    pub async fn route(
        &self,
        start: &str,
        end: &str,
        jump: u8,
        options: RouteOptions,
    ) -> Result<RouteResult, TravellerMapError> {
        let mut url = format!(
            "{}/api/route?start={}&end={}&jump={}",
            self.base_url,
            urlencoding::encode(start),
            urlencoding::encode(end),
            jump
        );

        if options.wild {
            url.push_str("&wild=1");
        }
        if options.imperium_only {
            url.push_str("&im=1");
        }
        if options.no_red_zones {
            url.push_str("&nored=1");
        }
        if options.allow_anomalies {
            url.push_str("&aok=1");
        }

        let response = self.client.get(&url).send().await?;
        if response.status() == 404 {
            return Err(TravellerMapError::NoRouteFound {
                start: start.to_string(),
                end: end.to_string(),
            });
        }
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: RouteResult = response.json().await?;
        Ok(results)
    }

    /// Get world/credit data for a specific location
    pub async fn credits(&self, sector: &str, hex: &str) -> Result<WorldData, TravellerMapError> {
        let url = format!(
            "{}/api/credits?sector={}&hex={}",
            self.base_url,
            urlencoding::encode(sector),
            urlencoding::encode(hex)
        );

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let data: WorldData = response.json().await?;
        Ok(data)
    }

    /// Get coordinates for a location (sector/hex to world-space)
    pub async fn coordinates(
        &self,
        sector: &str,
        hex: Option<&str>,
    ) -> Result<Coordinates, TravellerMapError> {
        let mut url = format!(
            "{}/api/coordinates?sector={}",
            self.base_url,
            urlencoding::encode(sector)
        );
        if let Some(h) = hex {
            url.push_str(&format!("&hex={}", urlencoding::encode(h)));
        }

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let coords: Coordinates = response.json().await?;
        Ok(coords)
    }

    /// Get sector metadata
    pub async fn sector_metadata(&self, sector: &str) -> Result<SectorMetadata, TravellerMapError> {
        let url = format!(
            "{}/api/metadata?sector={}",
            self.base_url,
            urlencoding::encode(sector)
        );

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let metadata: SectorMetadata = response.json().await?;
        Ok(metadata)
    }

    /// Get sector data in SEC/T5 format
    pub async fn sector_data(
        &self,
        sector: &str,
        subsector: Option<&str>,
    ) -> Result<String, TravellerMapError> {
        let mut url = format!(
            "{}/api/sec?sector={}&type=TabDelimited",
            self.base_url,
            urlencoding::encode(sector)
        );
        if let Some(ss) = subsector {
            url.push_str(&format!("&subsector={}", urlencoding::encode(ss)));
        }

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let data = response.text().await?;
        Ok(data)
    }

    /// List all sectors in the universe
    pub async fn universe(
        &self,
        milieu: Option<&str>,
        require_data: bool,
    ) -> Result<UniverseResult, TravellerMapError> {
        let mut url = format!("{}/api/universe", self.base_url);
        let mut params = vec![];

        if let Some(m) = milieu {
            params.push(format!("milieu={}", urlencoding::encode(m)));
        }
        if require_data {
            params.push("requireData=1".to_string());
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: UniverseResult = response.json().await?;
        Ok(results)
    }

    /// Get available milieux (time periods)
    pub async fn milieux(&self) -> Result<MilieuxResult, TravellerMapError> {
        let url = format!("{}/api/milieux", self.base_url);

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: MilieuxResult = response.json().await?;
        Ok(results)
    }

    /// Get allegiance reference data
    #[allow(dead_code)]
    pub async fn allegiances(&self) -> Result<Vec<AllegianceCode>, TravellerMapError> {
        let url = format!("{}/t5ss/allegiances", self.base_url);

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: Vec<AllegianceCode> = response.json().await?;
        Ok(results)
    }

    /// Get sophont reference data
    #[allow(dead_code)]
    pub async fn sophonts(&self) -> Result<Vec<SophontCode>, TravellerMapError> {
        let url = format!("{}/t5ss/sophonts", self.base_url);

        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        let results: Vec<SophontCode> = response.json().await?;
        Ok(results)
    }

    /// Generate a URL for the poster/map image API
    pub fn poster_url(&self, sector: &str, options: &PosterOptions) -> String {
        let mut url = format!(
            "{}/api/poster?sector={}",
            self.base_url,
            urlencoding::encode(sector)
        );

        if let Some(ss) = &options.subsector {
            url.push_str(&format!("&subsector={}", urlencoding::encode(ss)));
        }
        if let Some(scale) = options.scale {
            url.push_str(&format!("&scale={}", scale));
        }
        if let Some(style) = &options.style {
            url.push_str(&format!("&style={}", style));
        }
        if options.thumbnail {
            url.push_str("&thumb=1");
        }

        url
    }

    /// Generate a URL for jump map image
    pub fn jump_map_url(
        &self,
        sector: &str,
        hex: &str,
        jump: u8,
        options: &JumpMapOptions,
    ) -> String {
        let mut url = format!(
            "{}/api/jumpmap?sector={}&hex={}&jump={}",
            self.base_url,
            urlencoding::encode(sector),
            urlencoding::encode(hex),
            jump
        );

        if let Some(scale) = options.scale {
            url.push_str(&format!("&scale={}", scale));
        }
        if let Some(style) = &options.style {
            url.push_str(&format!("&style={}", style));
        }
        if !options.clip {
            url.push_str("&clip=0");
        }
        if !options.border {
            url.push_str("&border=0");
        }

        url
    }

    /// Download a poster/sector map image
    pub async fn download_poster(
        &self,
        sector: &str,
        options: &PosterOptions,
    ) -> Result<(Vec<u8>, String), TravellerMapError> {
        let url = self.poster_url(sector, options);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        // Determine file extension from content-type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/png");

        let extension = match content_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "application/pdf" => "pdf",
            "image/svg+xml" => "svg",
            _ => "png",
        };

        let bytes = response.bytes().await?.to_vec();
        Ok((bytes, extension.to_string()))
    }

    /// Download a jump map image
    pub async fn download_jump_map(
        &self,
        sector: &str,
        hex: &str,
        jump: u8,
        options: &JumpMapOptions,
    ) -> Result<(Vec<u8>, String), TravellerMapError> {
        let url = self.jump_map_url(sector, hex, jump, options);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(TravellerMapError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        // Determine file extension from content-type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/png");

        let extension = match content_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "application/pdf" => "pdf",
            "image/svg+xml" => "svg",
            _ => "png",
        };

        let bytes = response.bytes().await?.to_vec();
        Ok((bytes, extension.to_string()))
    }
}

// Error types

#[derive(Debug, thiserror::Error)]
pub enum TravellerMapError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error (status {status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("No route found between {start} and {end}")]
    NoRouteFound { start: String, end: String },

    #[error("Invalid location: {0}")]
    #[allow(dead_code)]
    InvalidLocation(String),
}

// Response types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SearchResults {
    #[serde(default)]
    pub worlds: Option<WorldSearchResults>,
    #[serde(default)]
    pub labels: Option<LabelSearchResults>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WorldSearchResults {
    #[serde(default)]
    pub world: Vec<WorldSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WorldSearchResult {
    pub sector: String,
    pub hex: String,
    pub name: String,
    pub uwp: Option<String>,
    pub sector_x: Option<i32>,
    pub sector_y: Option<i32>,
    pub hex_x: Option<u32>,
    pub hex_y: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LabelSearchResults {
    #[serde(default)]
    pub label: Vec<LabelSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LabelSearchResult {
    pub name: String,
    #[serde(rename = "Type")]
    pub label_type: String,
    pub sector: Option<String>,
    pub hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JumpWorldsResult {
    #[serde(default)]
    pub worlds: Vec<JumpWorld>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JumpWorld {
    pub sector: String,
    pub hex: String,
    pub name: String,
    pub uwp: String,
    #[serde(default)]
    pub allegiance: String,
    #[serde(default)]
    pub remarks: String,
    pub distance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RouteResult {
    #[serde(default)]
    pub route: Vec<RouteWorld>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RouteWorld {
    pub sector: String,
    pub hex: String,
    pub name: String,
    pub uwp: Option<String>,
    #[serde(default)]
    pub distance: f64,
}

#[derive(Debug, Clone, Default)]
pub struct RouteOptions {
    /// Require wilderness refueling capability
    pub wild: bool,
    /// Restrict to Third Imperium member worlds
    pub imperium_only: bool,
    /// Exclude TAS Red Zones
    pub no_red_zones: bool,
    /// Allow anomalies (unusual star systems)
    pub allow_anomalies: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WorldData {
    pub name: Option<String>,
    pub sector: Option<String>,
    pub hex: Option<String>,
    pub uwp: Option<String>,
    pub allegiance: Option<String>,
    pub remarks: Option<String>,
    #[serde(rename = "PBG")]
    pub pbg: Option<String>,
    pub zone: Option<String>,
    pub bases: Option<String>,
    pub stellar: Option<String>,
    pub importance: Option<String>,
    pub economic: Option<String>,
    pub cultural: Option<String>,
    pub nobility: Option<String>,
    pub worlds: Option<u32>,
    #[serde(rename = "ResourceUnits")]
    pub resource_units: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Coordinates {
    pub sector: Option<String>,
    pub hex: Option<String>,
    #[serde(rename = "SectorX")]
    pub sector_x: Option<i32>,
    #[serde(rename = "SectorY")]
    pub sector_y: Option<i32>,
    #[serde(rename = "HexX")]
    pub hex_x: Option<u32>,
    #[serde(rename = "HexY")]
    pub hex_y: Option<u32>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SectorMetadata {
    pub names: Option<Vec<SectorName>>,
    pub abbreviation: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub tags: Option<String>,
    pub credits: Option<String>,
    pub subsectors: Option<Vec<SubsectorMetadata>>,
    pub allegiances: Option<Vec<AllegianceMetadata>>,
    pub routes: Option<Vec<RouteMetadata>>,
    pub borders: Option<Vec<BorderMetadata>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SectorName {
    pub text: String,
    pub lang: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubsectorMetadata {
    pub index: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AllegianceMetadata {
    pub code: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RouteMetadata {
    pub start: Option<String>,
    pub end: Option<String>,
    pub start_offset_x: Option<i32>,
    pub start_offset_y: Option<i32>,
    pub end_offset_x: Option<i32>,
    pub end_offset_y: Option<i32>,
    #[serde(rename = "Type")]
    pub route_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BorderMetadata {
    pub allegiance: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UniverseResult {
    #[serde(default)]
    pub sectors: Vec<UniverseSector>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UniverseSector {
    pub abbreviation: Option<String>,
    pub names: Option<Vec<SectorName>>,
    pub x: i32,
    pub y: i32,
    pub tags: Option<String>,
    pub milieu: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MilieuxResult {
    #[serde(default)]
    pub milieux: Vec<MilieuInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MilieuInfo {
    pub code: String,
    pub name: String,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct AllegianceCode {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct SophontCode {
    pub code: String,
    pub name: String,
}

// Options for generating image URLs

#[derive(Debug, Clone, Default)]
pub struct PosterOptions {
    /// Subsector (A-P or name)
    pub subsector: Option<String>,
    /// Scale in pixels per parsec
    pub scale: Option<u32>,
    /// Visual style (poster, print, atlas, candy, draft, fasa, terminal, mongoose)
    pub style: Option<String>,
    /// Generate thumbnail
    pub thumbnail: bool,
}

#[derive(Debug, Clone)]
pub struct JumpMapOptions {
    /// Scale in pixels per parsec
    pub scale: Option<u32>,
    /// Visual style
    pub style: Option<String>,
    /// Clip to hex edges (default true)
    pub clip: bool,
    /// Include border (default true)
    pub border: bool,
}

impl Default for JumpMapOptions {
    fn default() -> Self {
        Self {
            scale: None,
            style: None,
            clip: true,
            border: true,
        }
    }
}

// Tool enum for integration with the service

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
                    .credits(sector, hex)
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

/// Sanitize a string for use in a filename
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poster_url_generation() {
        let client = TravellerMapClient::default();
        let options = PosterOptions {
            subsector: Some("Regina".to_string()),
            style: Some("poster".to_string()),
            ..Default::default()
        };
        let url = client.poster_url("Spinward Marches", &options);
        assert!(url.contains("sector=Spinward%20Marches"));
        assert!(url.contains("subsector=Regina"));
        assert!(url.contains("style=poster"));
    }

    #[test]
    fn test_jump_map_url_generation() {
        let client = TravellerMapClient::default();
        let options = JumpMapOptions::default();
        let url = client.jump_map_url("Spinward Marches", "1910", 2, &options);
        assert!(url.contains("sector=Spinward%20Marches"));
        assert!(url.contains("hex=1910"));
        assert!(url.contains("jump=2"));
    }
}
