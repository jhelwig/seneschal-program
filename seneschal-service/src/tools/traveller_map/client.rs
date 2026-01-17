//! Traveller Map API client implementation.

use reqwest::Client;
use std::time::Duration;

use super::error::TravellerMapError;
use super::options::{JumpMapOptions, PosterOptions, RouteOptions};
use super::responses::{
    Coordinates, JumpWorldsResult, JumpWorldsWorldDataResponse, MilieuxResult, RouteResult,
    SearchResults, SectorMetadata, UniverseResult, WorldData,
};

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

    /// Get complete world data for a specific location
    ///
    /// Uses the jumpworlds API with jump=0 to get full world data including
    /// bases, stellar, worlds count, and other fields missing from the credits API.
    pub async fn world_data(
        &self,
        sector: &str,
        hex: &str,
    ) -> Result<WorldData, TravellerMapError> {
        let url = format!(
            "{}/api/jumpworlds?sector={}&hex={}&jump=0",
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

        let wrapper: JumpWorldsWorldDataResponse = response.json().await?;
        wrapper
            .worlds
            .into_iter()
            .next()
            .ok_or_else(|| TravellerMapError::NotFound {
                message: format!("No world found at {} {}", sector, hex),
            })
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
