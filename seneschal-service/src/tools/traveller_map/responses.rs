//! Response types for Traveller Map API.

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WorldData {
    pub name: Option<String>,
    pub sector: Option<String>,
    pub hex: Option<String>,
    #[serde(rename = "UWP")]
    pub uwp: Option<String>,
    pub allegiance: Option<String>,
    pub remarks: Option<String>,
    #[serde(rename = "PBG")]
    pub pbg: Option<String>,
    pub zone: Option<String>,
    pub bases: Option<String>,
    pub stellar: Option<String>,
    #[serde(rename = "Ix")]
    pub importance: Option<String>,
    #[serde(rename = "Ex")]
    pub economic: Option<String>,
    #[serde(rename = "Cx")]
    pub cultural: Option<String>,
    pub nobility: Option<String>,
    pub worlds: Option<u32>,
    #[serde(rename = "ResourceUnits")]
    pub resource_units: Option<i64>,
}

/// Wrapper for the jumpworlds API response when fetching complete world data
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct JumpWorldsWorldDataResponse {
    pub worlds: Vec<WorldData>,
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
