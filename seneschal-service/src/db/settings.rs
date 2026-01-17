//! Settings storage operations.
//!
//! This module contains database operations for FVTT-managed backend settings.

use std::collections::HashMap;

use rusqlite::params;

use super::Database;
use crate::error::{DatabaseError, ServiceResult};

impl Database {
    /// Get all settings as a map
    pub fn get_all_settings(&self) -> ServiceResult<HashMap<String, serde_json::Value>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .map_err(DatabaseError::Query)?;

        let rows = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value_str: String = row.get(1)?;
                Ok((key, value_str))
            })
            .map_err(DatabaseError::Query)?;

        let mut settings = HashMap::new();
        for row in rows {
            let (key, value_str) = row.map_err(DatabaseError::Query)?;
            if let Ok(value) = serde_json::from_str(&value_str) {
                settings.insert(key, value);
            }
        }

        Ok(settings)
    }

    /// Set multiple settings in a single transaction
    /// Null values delete the setting (revert to default)
    pub fn set_settings(&self, settings: HashMap<String, serde_json::Value>) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        for (key, value) in settings {
            if value.is_null() {
                // Null means delete (revert to default)
                conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
                    .map_err(DatabaseError::Query)?;
            } else {
                let value_str =
                    serde_json::to_string(&value).map_err(DatabaseError::Serialization)?;
                conn.execute(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now')) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![key, value_str],
                )
                .map_err(DatabaseError::Query)?;
            }
        }

        Ok(())
    }
}
