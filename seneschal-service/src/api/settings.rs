//! Settings API endpoints for managing backend configuration via FVTT module.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::AppState;
use crate::config::DynamicConfig;
use crate::error::{I18nError, ServiceError};

/// Response for GET /api/settings
#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    /// All current settings (merged: defaults + DB overrides)
    pub settings: HashMap<String, serde_json::Value>,
    /// Which keys have DB overrides (vs using defaults)
    pub overridden: Vec<String>,
}

/// Request body for PUT /api/settings
#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    /// Settings to update (key -> value). Use null to delete/revert to default.
    pub settings: HashMap<String, serde_json::Value>,
}

/// GET /api/settings - retrieve all settings with their current values
pub async fn get_settings_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SettingsResponse>, I18nError> {
    // Get DB overrides to know which keys are explicitly set
    let db_settings = state
        .service
        .db
        .get_all_settings()
        .map_err(|e| state.i18n_error(e))?;

    // Get current config values (merged)
    let config = state.service.runtime_config.dynamic();
    let all_settings = config.to_key_value_map();
    let overridden: Vec<String> = db_settings.keys().cloned().collect();

    Ok(Json(SettingsResponse {
        settings: all_settings,
        overridden,
    }))
}

/// PUT /api/settings - update settings (triggers hot reload)
pub async fn update_settings_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, I18nError> {
    // Validate setting keys
    let valid_keys = DynamicConfig::valid_keys();
    for key in request.settings.keys() {
        if !valid_keys.contains(key.as_str()) {
            return Err(state.i18n_error(ServiceError::InvalidRequest {
                message: format!("Unknown setting key: {}", key),
            }));
        }
    }

    // Update settings and trigger hot reload
    state
        .service
        .update_settings(request.settings)
        .await
        .map_err(|e| state.i18n_error(e))?;

    // Return updated settings
    get_settings_handler(State(state)).await
}
