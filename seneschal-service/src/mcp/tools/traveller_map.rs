//! Traveller Map API MCP tool implementations.

use crate::config::AssetsAccess;
use crate::tools::TravellerMapTool;
use crate::tools::traveller_map::{JumpMapOptions, PosterOptions};

use super::super::{McpError, McpState};
use super::sanitize_filename;

pub(super) async fn execute_traveller_map_search(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let milieu = arguments.get("milieu").and_then(|v| v.as_str());

    let tool = TravellerMapTool::Search {
        query: query.to_string(),
        milieu: milieu.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_jump_worlds(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;

    let tool = TravellerMapTool::JumpWorlds {
        sector: sector.to_string(),
        hex: hex.to_string(),
        jump,
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_route(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let start = arguments
        .get("start")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let end = arguments.get("end").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let wild = arguments
        .get("wild")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let imperium_only = arguments
        .get("imperium_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let no_red_zones = arguments
        .get("no_red_zones")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let tool = TravellerMapTool::Route {
        start: start.to_string(),
        end: end.to_string(),
        jump,
        wild,
        imperium_only,
        no_red_zones,
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_world_data(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");

    let tool = TravellerMapTool::WorldData {
        sector: sector.to_string(),
        hex: hex.to_string(),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_sector_data(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subsector = arguments.get("subsector").and_then(|v| v.as_str());

    let tool = TravellerMapTool::SectorData {
        sector: sector.to_string(),
        subsector: subsector.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_coordinates(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str());

    let tool = TravellerMapTool::Coordinates {
        sector: sector.to_string(),
        hex: hex.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_list_sectors(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let milieu = arguments.get("milieu").and_then(|v| v.as_str());

    let tool = TravellerMapTool::ListSectors {
        milieu: milieu.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) fn execute_traveller_map_poster_url(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subsector = arguments.get("subsector").and_then(|v| v.as_str());
    let style = arguments.get("style").and_then(|v| v.as_str());

    let tool = TravellerMapTool::PosterUrl {
        sector: sector.to_string(),
        subsector: subsector.map(|s| s.to_string()),
        style: style.map(|s| s.to_string()),
    };

    // This is a synchronous operation (just URL generation)
    let result = futures::executor::block_on(tool.execute(&state.service.traveller_map_client));

    match result {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) fn execute_traveller_map_jump_map_url(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let style = arguments.get("style").and_then(|v| v.as_str());

    let tool = TravellerMapTool::JumpMapUrl {
        sector: sector.to_string(),
        hex: hex.to_string(),
        jump,
        style: style.map(|s| s.to_string()),
    };

    // This is a synchronous operation (just URL generation)
    let result = futures::executor::block_on(tool.execute(&state.service.traveller_map_client));

    match result {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) async fn execute_traveller_map_save_poster(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subsector = arguments.get("subsector").and_then(|v| v.as_str());
    let style = arguments.get("style").and_then(|v| v.as_str());
    let scale = arguments
        .get("scale")
        .and_then(|v| v.as_u64())
        .map(|s| s as u32);
    let target_path = arguments.get("target_path").and_then(|v| v.as_str());

    // Download the image
    let options = PosterOptions {
        subsector: subsector.map(|s| s.to_string()),
        style: style.map(|s| s.to_string()),
        scale,
        ..Default::default()
    };

    let (bytes, extension) = state
        .service
        .traveller_map_client
        .download_poster(sector, &options)
        .await
        .map_err(|e| McpError {
            code: -32000,
            message: e.to_string(),
        })?;

    // Generate filename
    let filename = if let Some(ss) = subsector {
        format!(
            "traveller-map/{}-{}.{}",
            sanitize_filename(sector),
            sanitize_filename(ss),
            extension
        )
    } else {
        format!("traveller-map/{}.{}", sanitize_filename(sector), extension)
    };
    let relative_path = target_path.map(|s| s.to_string()).unwrap_or(filename);

    // The FVTT path is what FVTT uses to reference the file
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

            if let Err(e) = std::fs::write(&full_path, &bytes) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to write image: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "size_bytes": bytes.len(),
                "message": format!("Poster map saved to {}", fvtt_path)
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
            // Can't directly write - need to shuttle via the FVTT module
            let result = serde_json::json!({
                "success": false,
                "mode": "shuttle",
                "suggested_path": fvtt_path,
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

pub(super) async fn execute_traveller_map_save_jump_map(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let style = arguments.get("style").and_then(|v| v.as_str());
    let scale = arguments
        .get("scale")
        .and_then(|v| v.as_u64())
        .map(|s| s as u32);
    let target_path = arguments.get("target_path").and_then(|v| v.as_str());

    // Download the image
    let options = JumpMapOptions {
        style: style.map(|s| s.to_string()),
        scale,
        ..Default::default()
    };

    let (bytes, extension) = state
        .service
        .traveller_map_client
        .download_jump_map(sector, hex, jump, &options)
        .await
        .map_err(|e| McpError {
            code: -32000,
            message: e.to_string(),
        })?;

    // Generate filename
    let filename = format!(
        "traveller-map/{}-{}-jump{}.{}",
        sanitize_filename(sector),
        hex,
        jump,
        extension
    );
    let relative_path = target_path.map(|s| s.to_string()).unwrap_or(filename);

    // The FVTT path is what FVTT uses to reference the file
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

            if let Err(e) = std::fs::write(&full_path, &bytes) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to write image: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "size_bytes": bytes.len(),
                "message": format!("Jump map saved to {}", fvtt_path)
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
