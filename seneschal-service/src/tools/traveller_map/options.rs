//! Options types for Traveller Map API requests.

/// Options for route calculation
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

/// Options for generating poster/map image URLs
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

/// Options for generating jump map image URLs
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
