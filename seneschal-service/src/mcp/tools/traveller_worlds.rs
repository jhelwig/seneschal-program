//! Traveller Worlds MCP tool implementations.

use crate::config::AssetsAccess;
use crate::tools::traveller_map::WorldData;
use crate::tools::{CustomWorldParams, TravellerMapTool};

use super::super::{McpError, McpState};
use super::sanitize_filename;

pub(super) async fn execute_traveller_worlds_canon_url(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");

    // Fetch world data from Traveller Map API
    let tool = TravellerMapTool::WorldData {
        sector: sector.to_string(),
        hex: hex.to_string(),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => {
            // Parse world data and build URL
            match serde_json::from_value::<WorldData>(result) {
                Ok(world_data) => {
                    let url = state
                        .service
                        .traveller_worlds_client
                        .build_url_from_world_data(&world_data);
                    let text = serde_json::to_string_pretty(&serde_json::json!({
                        "url": url,
                        "world_name": world_data.name.unwrap_or_default(),
                        "sector": world_data.sector.unwrap_or_default(),
                        "hex": world_data.hex.unwrap_or_default()
                    }))
                    .unwrap_or_default();
                    Ok(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": text
                        }]
                    }))
                }
                Err(e) => Err(McpError {
                    code: -32000,
                    message: format!("Failed to parse world data: {}", e),
                }),
            }
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_worlds_canon_save(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let target_folder = arguments.get("target_folder").and_then(|v| v.as_str());

    // Fetch world data from Traveller Map API
    let tool = TravellerMapTool::WorldData {
        sector: sector.to_string(),
        hex: hex.to_string(),
    };

    let world_data = match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => match serde_json::from_value::<WorldData>(result) {
            Ok(wd) => wd,
            Err(e) => {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to parse world data: {}", e),
                });
            }
        },
        Err(e) => {
            return Err(McpError {
                code: -32000,
                message: e,
            });
        }
    };

    // Build URL and extract SVG
    let url = state
        .service
        .traveller_worlds_client
        .build_url_from_world_data(&world_data);
    let svg = match state
        .service
        .traveller_worlds_client
        .extract_svg(&url)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return Err(McpError {
                code: -32000,
                message: e.to_string(),
            });
        }
    };

    // Generate filename
    let world_name = world_data
        .name
        .clone()
        .unwrap_or_else(|| "world".to_string());
    let filename = format!("{}.svg", sanitize_filename(&world_name));
    let folder = target_folder.unwrap_or("traveller-worlds");
    let relative_path = format!("{}/{}", folder, filename);
    let fvtt_path = format!("assets/{}", relative_path);

    // Save to FVTT assets
    match state
        .service
        .runtime_config
        .static_config
        .fvtt
        .check_assets_access()
    {
        AssetsAccess::Direct(assets_dir) => {
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to create directory: {}", e),
                });
            }

            if let Err(e) = std::fs::write(&full_path, &svg) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to write SVG: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "filename": filename,
                "size_bytes": svg.len(),
                "world_name": world_name,
                "message": format!("World map saved to FVTT assets at {}", fvtt_path)
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        AssetsAccess::Shuttle => {
            let result = serde_json::json!({
                "success": false,
                "mode": "shuttle",
                "suggested_path": fvtt_path,
                "world_name": world_name,
                "message": "Direct asset writing not available. FVTT assets directory not configured or not writable."
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
    }
}

pub(super) fn execute_traveller_worlds_custom_url(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let params = CustomWorldParams {
        name: arguments
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        uwp: arguments
            .get("uwp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        hex: arguments
            .get("hex")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        sector: arguments
            .get("sector")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        seed: arguments
            .get("seed")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        stellar: arguments
            .get("stellar")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        bases: arguments
            .get("bases")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tc: arguments.get("tc").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        }),
        travel_zone: arguments
            .get("travel_zone")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        pbg: arguments
            .get("pbg")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    let url = state
        .service
        .traveller_worlds_client
        .build_url_from_params(&params);
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "url": url,
        "world_name": params.name
    }))
    .unwrap_or_default();

    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    }))
}

pub(super) async fn execute_traveller_worlds_custom_save(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let params = CustomWorldParams {
        name: arguments
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        uwp: arguments
            .get("uwp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        hex: arguments
            .get("hex")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        sector: arguments
            .get("sector")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        seed: arguments
            .get("seed")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        stellar: arguments
            .get("stellar")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        bases: arguments
            .get("bases")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tc: arguments.get("tc").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        }),
        travel_zone: arguments
            .get("travel_zone")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        pbg: arguments
            .get("pbg")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };
    let target_folder = arguments.get("target_folder").and_then(|v| v.as_str());

    let url = state
        .service
        .traveller_worlds_client
        .build_url_from_params(&params);
    let svg = match state
        .service
        .traveller_worlds_client
        .extract_svg(&url)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return Err(McpError {
                code: -32000,
                message: e.to_string(),
            });
        }
    };

    // Generate filename
    let filename = format!("{}.svg", sanitize_filename(&params.name));
    let folder = target_folder.unwrap_or("traveller-worlds");
    let relative_path = format!("{}/{}", folder, filename);
    let fvtt_path = format!("assets/{}", relative_path);

    // Save to FVTT assets
    match state
        .service
        .runtime_config
        .static_config
        .fvtt
        .check_assets_access()
    {
        AssetsAccess::Direct(assets_dir) => {
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to create directory: {}", e),
                });
            }

            if let Err(e) = std::fs::write(&full_path, &svg) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to write SVG: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "filename": filename,
                "size_bytes": svg.len(),
                "world_name": params.name,
                "message": format!("World map saved to FVTT assets at {}", fvtt_path)
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        AssetsAccess::Shuttle => {
            let result = serde_json::json!({
                "success": false,
                "mode": "shuttle",
                "suggested_path": fvtt_path,
                "world_name": params.name,
                "message": "Direct asset writing not available. FVTT assets directory not configured or not writable."
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
    }
}
