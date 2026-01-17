//! Traveller Map API internal tools.

use base64::Engine;

use crate::config::AssetsAccess;
use crate::service::SeneschalService;
use crate::tools::traveller_map::{JumpMapOptions, PosterOptions};
use crate::tools::{ToolCall, ToolResult, TravellerMapTool};

use super::sanitize_map_filename;

impl SeneschalService {
    pub(crate) async fn tool_traveller_map_search(&self, call: &ToolCall) -> ToolResult {
        let query = call
            .args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let milieu = call.args.get("milieu").and_then(|v| v.as_str());
        let tool = TravellerMapTool::Search {
            query: query.to_string(),
            milieu: milieu.map(|s| s.to_string()),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_jump_worlds(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
        let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
        let tool = TravellerMapTool::JumpWorlds {
            sector: sector.to_string(),
            hex: hex.to_string(),
            jump,
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_route(&self, call: &ToolCall) -> ToolResult {
        let start = call
            .args
            .get("start")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let end = call.args.get("end").and_then(|v| v.as_str()).unwrap_or("");
        let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
        let wild = call
            .args
            .get("wild")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let imperium_only = call
            .args
            .get("imperium_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let no_red_zones = call
            .args
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
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_world_data(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
        let tool = TravellerMapTool::WorldData {
            sector: sector.to_string(),
            hex: hex.to_string(),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_sector_data(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subsector = call.args.get("subsector").and_then(|v| v.as_str());
        let tool = TravellerMapTool::SectorData {
            sector: sector.to_string(),
            subsector: subsector.map(|s| s.to_string()),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_coordinates(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str());
        let tool = TravellerMapTool::Coordinates {
            sector: sector.to_string(),
            hex: hex.map(|s| s.to_string()),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_list_sectors(&self, call: &ToolCall) -> ToolResult {
        let milieu = call.args.get("milieu").and_then(|v| v.as_str());
        let tool = TravellerMapTool::ListSectors {
            milieu: milieu.map(|s| s.to_string()),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_poster_url(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subsector = call.args.get("subsector").and_then(|v| v.as_str());
        let style = call.args.get("style").and_then(|v| v.as_str());
        let tool = TravellerMapTool::PosterUrl {
            sector: sector.to_string(),
            subsector: subsector.map(|s| s.to_string()),
            style: style.map(|s| s.to_string()),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_jump_map_url(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
        let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
        let style = call.args.get("style").and_then(|v| v.as_str());
        let tool = TravellerMapTool::JumpMapUrl {
            sector: sector.to_string(),
            hex: hex.to_string(),
            jump,
            style: style.map(|s| s.to_string()),
        };
        match tool.execute(&self.traveller_map_client).await {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) async fn tool_traveller_map_save_poster(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subsector = call.args.get("subsector").and_then(|v| v.as_str());
        let style = call.args.get("style").and_then(|v| v.as_str());
        let scale = call
            .args
            .get("scale")
            .and_then(|v| v.as_u64())
            .map(|s| s as u32);
        let target_folder = call.args.get("target_folder").and_then(|v| v.as_str());

        let options = PosterOptions {
            subsector: subsector.map(|s| s.to_string()),
            style: style.map(|s| s.to_string()),
            scale,
            ..Default::default()
        };

        // Download the image
        let (bytes, extension) = match self
            .traveller_map_client
            .download_poster(sector, &options)
            .await
        {
            Ok(result) => result,
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
        };

        // Generate filename
        let filename = if let Some(ss) = subsector {
            format!(
                "{}-{}.{}",
                sanitize_map_filename(sector),
                sanitize_map_filename(ss),
                extension
            )
        } else {
            format!("{}.{}", sanitize_map_filename(sector), extension)
        };

        // Determine relative path for FVTT assets
        let folder = target_folder.unwrap_or("traveller-maps");
        let relative_path = format!("{}/{}", folder, filename);
        let fvtt_path = format!("assets/{}", relative_path);

        // Check assets access mode
        match self.runtime_config.static_config.fvtt.check_assets_access() {
            AssetsAccess::Direct(assets_dir) => {
                // Create target directory
                let full_path = assets_dir.join(&relative_path);
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to create directory: {}", e),
                    );
                }

                // Write file
                if let Err(e) = std::fs::write(&full_path, &bytes) {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to write image: {}", e),
                    );
                }

                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": true,
                        "mode": "direct",
                        "fvtt_path": fvtt_path,
                        "filename": filename,
                        "size_bytes": bytes.len(),
                        "message": format!("Sector map saved to FVTT assets at {}", fvtt_path)
                    }),
                )
            }
            AssetsAccess::Shuttle => {
                // Return base64-encoded data for client to handle
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": false,
                        "mode": "shuttle",
                        "suggested_path": fvtt_path,
                        "filename": filename,
                        "extension": extension,
                        "size_bytes": bytes.len(),
                        "base64_data": base64_data,
                        "message": "Direct delivery not available. Use the FVTT module to save this image."
                    }),
                )
            }
        }
    }

    pub(crate) async fn tool_traveller_map_save_jump_map(&self, call: &ToolCall) -> ToolResult {
        let sector = call
            .args
            .get("sector")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
        let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
        let style = call.args.get("style").and_then(|v| v.as_str());
        let scale = call
            .args
            .get("scale")
            .and_then(|v| v.as_u64())
            .map(|s| s as u32);
        let target_folder = call.args.get("target_folder").and_then(|v| v.as_str());

        let options = JumpMapOptions {
            style: style.map(|s| s.to_string()),
            scale,
            ..Default::default()
        };

        // Download the image
        let (bytes, extension) = match self
            .traveller_map_client
            .download_jump_map(sector, hex, jump, &options)
            .await
        {
            Ok(result) => result,
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
        };

        // Generate filename
        let filename = format!(
            "{}-{}-jump{}.{}",
            sanitize_map_filename(sector),
            hex,
            jump,
            extension
        );

        // Determine relative path for FVTT assets
        let folder = target_folder.unwrap_or("traveller-maps");
        let relative_path = format!("{}/{}", folder, filename);
        let fvtt_path = format!("assets/{}", relative_path);

        // Check assets access mode
        match self.runtime_config.static_config.fvtt.check_assets_access() {
            AssetsAccess::Direct(assets_dir) => {
                // Create target directory
                let full_path = assets_dir.join(&relative_path);
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to create directory: {}", e),
                    );
                }

                // Write file
                if let Err(e) = std::fs::write(&full_path, &bytes) {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to write image: {}", e),
                    );
                }

                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": true,
                        "mode": "direct",
                        "fvtt_path": fvtt_path,
                        "filename": filename,
                        "size_bytes": bytes.len(),
                        "message": format!("Jump map saved to FVTT assets at {}", fvtt_path)
                    }),
                )
            }
            AssetsAccess::Shuttle => {
                // Return base64-encoded data for client to handle
                let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": false,
                        "mode": "shuttle",
                        "suggested_path": fvtt_path,
                        "filename": filename,
                        "extension": extension,
                        "size_bytes": bytes.len(),
                        "base64_data": base64_data,
                        "message": "Direct delivery not available. Use the FVTT module to save this image."
                    }),
                )
            }
        }
    }
}
