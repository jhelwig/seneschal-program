//! Traveller Worlds integration.
//!
//! This module provides tools for generating world maps using travellerworlds.com.
//! It uses chromiumoxide to automate a headless Chrome/Chromium browser
//! for extracting the generated SVG map.

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::target::CreateTargetParams;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::traveller_map::WorldData;

/// Default base URL for Traveller Worlds
const DEFAULT_BASE_URL: &str = "http://www.travellerworlds.com/";

/// Traveller Worlds client for generating world maps
#[derive(Clone)]
pub struct TravellerWorldsClient {
    base_url: String,
    chrome_path: Option<String>,
}

impl Default for TravellerWorldsClient {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL, None)
    }
}

impl TravellerWorldsClient {
    /// Create a new Traveller Worlds client
    pub fn new(base_url: &str, chrome_path: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            chrome_path,
        }
    }

    /// Build URL from WorldData (for canon worlds fetched from Traveller Map API)
    pub fn build_url_from_world_data(&self, world_data: &WorldData) -> String {
        let mut params = Vec::new();

        // Required parameters
        if let Some(name) = &world_data.name {
            params.push(format!("name={}", urlencoding::encode(name)));
        }
        if let Some(uwp) = &world_data.uwp {
            params.push(format!("uwp={}", urlencoding::encode(uwp)));
        }

        // Location info
        if let Some(hex) = &world_data.hex {
            params.push(format!("hex={}", urlencoding::encode(hex)));
            // Seed is hex+hex for reproducibility
            let seed = format!("{}{}", hex, hex);
            params.push(format!("seed={}", seed));
        }
        if let Some(sector) = &world_data.sector {
            params.push(format!("sector={}", urlencoding::encode(sector)));
        }

        // Extended world data
        if let Some(stellar) = &world_data.stellar {
            params.push(format!("stellar={}", urlencoding::encode(stellar)));
        }
        if let Some(bases) = &world_data.bases {
            params.push(format!("bases={}", urlencoding::encode(bases)));
        }
        if let Some(allegiance) = &world_data.allegiance {
            params.push(format!("allegiance={}", urlencoding::encode(allegiance)));
        }
        if let Some(zone) = &world_data.zone
            && !zone.is_empty()
        {
            params.push(format!("travelZone={}", urlencoding::encode(zone)));
        }
        if let Some(pbg) = &world_data.pbg {
            params.push(format!("pbg={}", urlencoding::encode(pbg)));
        }
        if let Some(worlds) = world_data.worlds {
            params.push(format!("worlds={}", worlds));
        }

        // T5 extended data
        if let Some(importance) = &world_data.importance {
            params.push(format!("iX={}", urlencoding::encode(importance)));
        }
        if let Some(economic) = &world_data.economic {
            params.push(format!("eX={}", urlencoding::encode(economic)));
        }
        if let Some(cultural) = &world_data.cultural {
            params.push(format!("cX={}", urlencoding::encode(cultural)));
        }
        if let Some(nobility) = &world_data.nobility {
            params.push(format!("nobz={}", urlencoding::encode(nobility)));
        }

        // Trade codes from remarks field
        if let Some(remarks) = &world_data.remarks {
            for tc in Self::parse_trade_codes(remarks) {
                params.push(format!("tc={}", urlencoding::encode(&tc)));
            }
        }

        format!("{}/?{}", self.base_url, params.join("&"))
    }

    /// Build URL from custom parameters (for homebrew worlds)
    pub fn build_url_from_params(&self, params: &CustomWorldParams) -> String {
        let mut url_params = Vec::new();

        // Required parameters
        url_params.push(format!("name={}", urlencoding::encode(&params.name)));
        url_params.push(format!("uwp={}", urlencoding::encode(&params.uwp)));

        // Seed handling: hex+hex if hex provided, else explicit seed, else omit (random)
        if let Some(hex) = &params.hex {
            url_params.push(format!("hex={}", urlencoding::encode(hex)));
            let seed = params
                .seed
                .clone()
                .unwrap_or_else(|| format!("{}{}", hex, hex));
            url_params.push(format!("seed={}", seed));
        } else if let Some(seed) = &params.seed {
            url_params.push(format!("seed={}", seed));
        }

        if let Some(sector) = &params.sector {
            url_params.push(format!("sector={}", urlencoding::encode(sector)));
        }
        if let Some(stellar) = &params.stellar {
            url_params.push(format!("stellar={}", urlencoding::encode(stellar)));
        }
        if let Some(bases) = &params.bases {
            url_params.push(format!("bases={}", urlencoding::encode(bases)));
        }
        if let Some(tc) = &params.tc {
            for code in tc {
                url_params.push(format!("tc={}", urlencoding::encode(code)));
            }
        }
        if let Some(zone) = &params.travel_zone
            && !zone.is_empty()
        {
            url_params.push(format!("travelZone={}", urlencoding::encode(zone)));
        }
        if let Some(pbg) = &params.pbg {
            url_params.push(format!("pbg={}", urlencoding::encode(pbg)));
        }

        format!("{}/?{}", self.base_url, url_params.join("&"))
    }

    /// Extract SVG from travellerworlds.com using headless browser
    pub async fn extract_svg(&self, url: &str) -> Result<String, TravellerWorldsError> {
        // Build browser config
        let mut builder = BrowserConfig::builder()
            .window_size(1920, 1080)
            .arg("--headless")
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage");

        if let Some(path) = &self.chrome_path {
            builder = builder.chrome_executable(path);
        }

        let config = builder
            .build()
            .map_err(|e| TravellerWorldsError::BrowserLaunch(e.to_string()))?;

        // Launch browser
        let (mut browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| TravellerWorldsError::BrowserLaunch(e.to_string()))?;

        // Spawn handler task to process browser events
        let handle = tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if h.is_err() {
                    break;
                }
            }
        });

        // Navigate and extract SVG
        let result = self.navigate_and_extract(&browser, url).await;

        // Cleanup: close browser and wait for handler
        let _ = browser.close().await;
        handle.abort();

        result
    }

    /// Navigate to URL and extract SVG
    async fn navigate_and_extract(
        &self,
        browser: &Browser,
        url: &str,
    ) -> Result<String, TravellerWorldsError> {
        // Create new page/tab
        let page = browser
            .new_page(CreateTargetParams::new(url))
            .await
            .map_err(|e| TravellerWorldsError::Navigation(e.to_string()))?;

        // Wait for SVG with timeout (polling loop)
        self.wait_for_svg(&page).await
    }

    /// Wait for SVG element to be ready and extract it
    async fn wait_for_svg(
        &self,
        page: &chromiumoxide::Page,
    ) -> Result<String, TravellerWorldsError> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(30);

        loop {
            if start.elapsed() > timeout {
                return Err(TravellerWorldsError::Timeout(
                    "SVG element not populated within timeout".to_string(),
                ));
            }

            // Check if SVG is populated using JavaScript evaluation
            let result = page
                .evaluate(
                    r#"
                    (() => {
                        const elem = document.querySelector('#worldMapSVG');
                        if (elem && (elem.innerHTML.includes('<path') || elem.innerHTML.includes('<circle'))) {
                            return elem.outerHTML;
                        }
                        return null;
                    })()
                "#,
                )
                .await;

            if let Ok(val) = result
                && let Ok(Some(svg)) = val.into_value::<Option<String>>()
            {
                return Ok(svg);
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Parse trade codes from the remarks field
    fn parse_trade_codes(remarks: &str) -> Vec<String> {
        // Trade codes in remarks are space-separated, each is typically 2-4 chars
        // Examples: "Hi In Ht", "Ag Ri", "Po Ba"
        remarks
            .split_whitespace()
            .filter(|s| {
                // Filter to likely trade codes (2-4 uppercase letters)
                s.len() >= 2
                    && s.len() <= 4
                    && s.chars().all(|c| c.is_ascii_alphabetic())
                    && s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            })
            .map(|s| s.to_string())
            .collect()
    }
}

/// Parameters for custom world generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomWorldParams {
    /// World name (required)
    pub name: String,
    /// Universal World Profile (required)
    pub uwp: String,
    /// Hex coordinate (used for seed if provided)
    pub hex: Option<String>,
    /// Sector name
    pub sector: Option<String>,
    /// Random seed (default: hex+hex if hex provided, else random)
    pub seed: Option<String>,
    /// Stellar data
    pub stellar: Option<String>,
    /// Base codes
    pub bases: Option<String>,
    /// Trade classifications
    pub tc: Option<Vec<String>>,
    /// Travel zone (A, R, or blank)
    pub travel_zone: Option<String>,
    /// PBG code
    pub pbg: Option<String>,
}

/// Error types for Traveller Worlds operations
#[derive(Debug, thiserror::Error)]
pub enum TravellerWorldsError {
    #[error("Failed to launch browser: {0}")]
    BrowserLaunch(String),

    #[error("Navigation failed: {0}")]
    Navigation(String),

    #[error("Timeout: {0}")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url_from_world_data() {
        let client = TravellerWorldsClient::default();

        let world_data = WorldData {
            name: Some("Walston".to_string()),
            sector: Some("Spinward Marches".to_string()),
            hex: Some("1232".to_string()),
            uwp: Some("C544338-8".to_string()),
            allegiance: Some("Im".to_string()),
            remarks: Some("Lo Ni".to_string()),
            pbg: Some("910".to_string()),
            zone: None,
            bases: Some("S".to_string()),
            stellar: Some("M2 V".to_string()),
            importance: Some("{ -2 }".to_string()),
            economic: Some("(631-3)".to_string()),
            cultural: Some("[1313]".to_string()),
            nobility: Some("B".to_string()),
            worlds: Some(6),
            resource_units: None,
        };

        let url = client.build_url_from_world_data(&world_data);
        assert!(url.contains("name=Walston"));
        assert!(url.contains("uwp=C544338-8"));
        assert!(url.contains("hex=1232"));
        assert!(url.contains("seed=12321232"));
        assert!(url.contains("sector=Spinward%20Marches"));
        assert!(url.contains("tc=Lo"));
        assert!(url.contains("tc=Ni"));
    }

    #[test]
    fn test_build_url_from_params() {
        let client = TravellerWorldsClient::default();

        let params = CustomWorldParams {
            name: "New Terra".to_string(),
            uwp: "A867974-C".to_string(),
            hex: Some("1234".to_string()),
            sector: Some("Custom Sector".to_string()),
            seed: None,
            stellar: Some("G2 V".to_string()),
            bases: Some("N".to_string()),
            tc: Some(vec!["Hi".to_string(), "Ri".to_string()]),
            travel_zone: None,
            pbg: Some("503".to_string()),
        };

        let url = client.build_url_from_params(&params);
        assert!(url.contains("name=New%20Terra"));
        assert!(url.contains("uwp=A867974-C"));
        assert!(url.contains("hex=1234"));
        assert!(url.contains("seed=12341234")); // hex+hex
        assert!(url.contains("tc=Hi"));
        assert!(url.contains("tc=Ri"));
    }

    #[test]
    fn test_parse_trade_codes() {
        let codes = TravellerWorldsClient::parse_trade_codes("Hi In Ht Lo");
        assert_eq!(codes, vec!["Hi", "In", "Ht", "Lo"]);

        let codes = TravellerWorldsClient::parse_trade_codes("Ag Ri { +2 }");
        assert_eq!(codes, vec!["Ag", "Ri"]);
    }
}
