//! Traveller Worlds internal tools.

use base64::Engine;

use crate::config::AssetsAccess;
use crate::service::SeneschalService;
use crate::tools::{CustomWorldParams, ToolCall, ToolResult, TravellerMapTool};

use super::sanitize_map_filename;

impl SeneschalService {
    pub(crate) async fn tool_traveller_worlds_canon_url(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");

        // Fetch world data from Traveller Map API
        let tool = TravellerMapTool::WorldData {
            sector: sector.to_string(),
            hex: hex.to_string(),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => {
                // Parse world data and build URL
                match serde_json::from_value::<crate::tools::traveller_map::WorldData>(result) {
                    Ok(world_data) => {
                        let url = self
                            .traveller_worlds_client
                            .build_url_from_world_data(&world_data);
                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "url": url,
                                "world_name": world_data.name.unwrap_or_default(),
                                "sector": world_data.sector.unwrap_or_default(),
                                "hex": world_data.hex.unwrap_or_default()
                            }),
                        )
                    }
                    Err(e) => ToolResult::error(
                        call.id.clone(),
                        format!("Failed to parse world data: {}", e),
                    ),
                }
            }
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_worlds_canon_save(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
        let target_folder = call.args.get("target_folder").and_then(|v| v.as_str());

        // Fetch world data from Traveller Map API
        let tool = TravellerMapTool::WorldData {
            sector: sector.to_string(),
            hex: hex.to_string(),
        };
        let world_data = match tool.execute(&self.traveller_map_client).await {
            Ok(result) => {
                match serde_json::from_value::<crate::tools::traveller_map::WorldData>(result) {
                    Ok(wd) => wd,
                    Err(e) => {
                        return ToolResult::error(
                            call.id.clone(),
                            format!("Failed to parse world data: {}", e),
                        );
                    }
                }
            }
            Err(e) => return ToolResult::error(call.id.clone(), e),
        };

        // Build URL and extract SVG
        let url = self
            .traveller_worlds_client
            .build_url_from_world_data(&world_data);
        let svg = match self.traveller_worlds_client.extract_svg(&url).await {
            Ok(s) => s,
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
        };

        // Generate filename
        let world_name = world_data
            .name
            .clone()
            .unwrap_or_else(|| "world".to_string());
        let filename = format!("{}.svg", sanitize_map_filename(&world_name));
        let folder = target_folder.unwrap_or("traveller-worlds");
        let relative_path = format!("{}/{}", folder, filename);
        let fvtt_path = format!("assets/{}", relative_path);

        // Save to FVTT assets
        match self.runtime_config.static_config.fvtt.check_assets_access() {
            AssetsAccess::Direct(assets_dir) => {
                let full_path = assets_dir.join(&relative_path);
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to create directory: {}", e),
                    );
                }

                if let Err(e) = std::fs::write(&full_path, &svg) {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to write SVG: {}", e),
                    );
                }

                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": true,
                        "mode": "direct",
                        "fvtt_path": fvtt_path,
                        "filename": filename,
                        "size_bytes": svg.len(),
                        "world_name": world_name,
                        "message": format!("World map saved to FVTT assets at {}", fvtt_path)
                    }),
                )
            }
            AssetsAccess::Shuttle => {
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&svg);
                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": false,
                        "mode": "shuttle",
                        "suggested_path": fvtt_path,
                        "filename": filename,
                        "extension": "svg",
                        "size_bytes": svg.len(),
                        "world_name": world_name,
                        "base64_data": base64_data,
                        "message": "Direct delivery not available. Use the FVTT module to save this SVG."
                    }),
                )
            }
        }
    }

    pub(crate) fn tool_traveller_worlds_custom_url(&self, call: &ToolCall) -> ToolResult {
        let params = parse_custom_world_params(call);
        let url = self.traveller_worlds_client.build_url_from_params(&params);
        ToolResult::success(
            call.id.clone(),
            serde_json::json!({
                "url": url,
                "world_name": params.name
            }),
        )
    }

    pub(crate) async fn tool_traveller_worlds_custom_save(&self, call: &ToolCall) -> ToolResult {
        let params = parse_custom_world_params(call);
        let target_folder = call.args.get("target_folder").and_then(|v| v.as_str());

        let url = self.traveller_worlds_client.build_url_from_params(&params);
        let svg = match self.traveller_worlds_client.extract_svg(&url).await {
            Ok(s) => s,
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
        };

        // Generate filename
        let filename = format!("{}.svg", sanitize_map_filename(&params.name));
        let folder = target_folder.unwrap_or("traveller-worlds");
        let relative_path = format!("{}/{}", folder, filename);
        let fvtt_path = format!("assets/{}", relative_path);

        // Save to FVTT assets
        match self.runtime_config.static_config.fvtt.check_assets_access() {
            AssetsAccess::Direct(assets_dir) => {
                let full_path = assets_dir.join(&relative_path);
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to create directory: {}", e),
                    );
                }

                if let Err(e) = std::fs::write(&full_path, &svg) {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to write SVG: {}", e),
                    );
                }

                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": true,
                        "mode": "direct",
                        "fvtt_path": fvtt_path,
                        "filename": filename,
                        "size_bytes": svg.len(),
                        "world_name": params.name,
                        "message": format!("World map saved to FVTT assets at {}", fvtt_path)
                    }),
                )
            }
            AssetsAccess::Shuttle => {
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&svg);
                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": false,
                        "mode": "shuttle",
                        "suggested_path": fvtt_path,
                        "filename": filename,
                        "extension": "svg",
                        "size_bytes": svg.len(),
                        "world_name": params.name,
                        "base64_data": base64_data,
                        "message": "Direct delivery not available. Use the FVTT module to save this SVG."
                    }),
                )
            }
        }
    }
}

/// Parse CustomWorldParams from a tool call
fn parse_custom_world_params(call: &ToolCall) -> CustomWorldParams {
    CustomWorldParams {
        name: call
            .args
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        uwp: call
            .args
            .get("uwp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        hex: call
            .args
            .get("hex")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        sector: call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        seed: call
            .args
            .get("seed")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        stellar: call
            .args
            .get("stellar")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        bases: call
            .args
            .get("bases")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tc: call.args.get("tc").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        }),
        travel_zone: call
            .args
            .get("travel_zone")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        pbg: call
            .args
            .get("pbg")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}
