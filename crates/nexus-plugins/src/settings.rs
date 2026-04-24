//! Plugin settings management: JSON Schema registration, validation, and
//! persistence to `settings.json` inside each plugin's directory.

use std::collections::HashMap;
use std::path::Path;

use crate::PluginError;

// ─── SettingsManager ─────────────────────────────────────────────────────────

/// Manages per-plugin settings schemas and provides validation and I/O helpers.
///
/// Each plugin may optionally register a JSON Schema. When a schema is present,
/// [`validate`](SettingsManager::validate),
/// [`load_settings`](SettingsManager::load_settings), and
/// [`save_settings`](SettingsManager::save_settings) all enforce it.
/// Without a schema every settings value is accepted without error.
#[derive(Debug, Default)]
pub struct SettingsManager {
    /// Map of plugin ID → parsed JSON Schema value.
    schemas: HashMap<String, serde_json::Value>,
}

impl SettingsManager {
    /// Create a new, empty `SettingsManager` with no schemas registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a JSON Schema for `plugin_id`.
    ///
    /// `schema_json` must be a valid JSON string. The parsed value is stored
    /// and will be used by subsequent calls to
    /// [`validate`](Self::validate),
    /// [`load_settings`](Self::load_settings), and
    /// [`save_settings`](Self::save_settings).
    ///
    /// # Errors
    /// Returns [`PluginError::SettingsInvalid`] if `schema_json` is not valid
    /// JSON.
    pub fn register_schema(
        &mut self,
        plugin_id: &str,
        schema_json: &str,
    ) -> Result<(), PluginError> {
        let schema: serde_json::Value =
            serde_json::from_str(schema_json).map_err(|e| PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("invalid schema JSON: {e}"),
            })?;
        self.schemas.insert(plugin_id.to_string(), schema);
        Ok(())
    }

    /// Validate `settings` against the schema registered for `plugin_id`.
    ///
    /// If no schema has been registered for this plugin the call always
    /// succeeds. Otherwise all JSON Schema violations are collected and
    /// returned as a single `;`-separated string inside
    /// [`PluginError::SettingsInvalid`].
    ///
    /// # Errors
    /// Returns [`PluginError::SettingsInvalid`] if the settings fail schema
    /// validation.
    pub fn validate(
        &self,
        plugin_id: &str,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        let Some(schema) = self.schemas.get(plugin_id) else {
            return Ok(());
        };

        let compiled =
            jsonschema::options().build(schema).map_err(|e| PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("invalid schema: {e}"),
            })?;

        let errors: Vec<String> = compiled
            .iter_errors(settings)
            .map(|e| e.to_string())
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: errors.join("; "),
            })
        }
    }

    /// Load settings from `<plugin_dir>/settings.json`.
    ///
    /// If the file does not exist an empty JSON object (`{}`) is returned.
    /// After reading, the settings are validated against the registered schema
    /// (if any).
    ///
    /// # Errors
    /// Returns [`PluginError::Io`] on filesystem errors other than
    /// "not found", [`PluginError::SettingsInvalid`] if the file content is
    /// not valid JSON or the settings fail schema validation.
    pub fn load_settings(
        &self,
        plugin_id: &str,
        plugin_dir: &Path,
    ) -> Result<serde_json::Value, PluginError> {
        let path = plugin_dir.join("settings.json");

        let settings: serde_json::Value = match std::fs::read_to_string(&path) {
            Ok(contents) => {
                serde_json::from_str(&contents).map_err(|e| PluginError::SettingsInvalid {
                    plugin_id: plugin_id.to_string(),
                    reason: format!("settings.json is not valid JSON: {e}"),
                })?
            }
            // When the file doesn't exist yet, seed the object with the
            // schema's declared defaults. Without this a schema with
            // required fields would fail validation on first load and
            // the plugin would appear broken until the user manually
            // wrote a file.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self
                .schemas
                .get(plugin_id)
                .map_or_else(
                    || serde_json::Value::Object(serde_json::Map::new()),
                    defaults_from_schema,
                ),
            Err(e) => return Err(PluginError::Io(e)),
        };

        self.validate(plugin_id, &settings)?;
        Ok(settings)
    }

    /// Validate `settings` and, if valid, write them as pretty-printed JSON to
    /// `<plugin_dir>/settings.json`.
    ///
    /// If validation fails the file is **not** written and the error is
    /// returned.
    ///
    /// # Errors
    /// Returns [`PluginError::SettingsInvalid`] if the settings fail schema
    /// validation, or [`PluginError::Io`] if the file cannot be written.
    pub fn save_settings(
        &self,
        plugin_id: &str,
        plugin_dir: &Path,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        self.validate(plugin_id, settings)?;

        let pretty = serde_json::to_string_pretty(settings).map_err(|e| {
            PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("could not serialize settings: {e}"),
            }
        })?;

        let path = plugin_dir.join("settings.json");
        std::fs::write(&path, pretty)?;
        Ok(())
    }

    /// Returns `true` if a schema has been registered for `plugin_id`.
    #[must_use]
    pub fn has_schema(&self, plugin_id: &str) -> bool {
        self.schemas.contains_key(plugin_id)
    }

    /// Return the raw JSON Schema registered for `plugin_id`, if any.
    /// Used by the host to send schemas to the frontend for form
    /// rendering.
    #[must_use]
    pub fn schema(&self, plugin_id: &str) -> Option<&serde_json::Value> {
        self.schemas.get(plugin_id)
    }
}

/// Walk a JSON Schema's top-level `properties` and build an object
/// populated with each field's `default` where declared. Used to
/// seed `settings.json` on first load so schemas with `required`
/// fields don't fail validation before the user has saved anything.
fn defaults_from_schema(schema: &serde_json::Value) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        for (key, prop) in props {
            if let Some(default) = prop.get("default") {
                out.insert(key.clone(), default.clone());
            }
        }
    }
    serde_json::Value::Object(out)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    const SCHEMA: &str = r#"{
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "count": { "type": "integer", "minimum": 0 }
        },
        "required": ["name"]
    }"#;

    const PLUGIN_ID: &str = "com.example.test";

    fn manager_with_schema() -> SettingsManager {
        let mut m = SettingsManager::new();
        m.register_schema(PLUGIN_ID, SCHEMA).unwrap();
        m
    }

    // 1. register_valid_schema
    #[test]
    fn register_valid_schema() {
        let mut m = SettingsManager::new();
        assert!(!m.has_schema(PLUGIN_ID));
        m.register_schema(PLUGIN_ID, SCHEMA).unwrap();
        assert!(m.has_schema(PLUGIN_ID));
    }

    // 2. register_invalid_json_returns_error
    #[test]
    fn register_invalid_json_returns_error() {
        let mut m = SettingsManager::new();
        let err = m.register_schema(PLUGIN_ID, "not json {{").unwrap_err();
        assert!(matches!(err, PluginError::SettingsInvalid { .. }));
    }

    // 3. validate_valid_settings
    #[test]
    fn validate_valid_settings() {
        let m = manager_with_schema();
        m.validate(PLUGIN_ID, &json!({ "name": "hello" })).unwrap();
    }

    // 4. validate_missing_required_field
    #[test]
    fn validate_missing_required_field() {
        let m = manager_with_schema();
        let err = m.validate(PLUGIN_ID, &json!({})).unwrap_err();
        assert!(matches!(err, PluginError::SettingsInvalid { .. }));
        if let PluginError::SettingsInvalid { reason, .. } = err {
            assert!(!reason.is_empty(), "reason should not be empty");
        }
    }

    // 5. validate_wrong_type
    #[test]
    fn validate_wrong_type() {
        let m = manager_with_schema();
        // name should be a string, passing a number
        let err = m
            .validate(PLUGIN_ID, &json!({ "name": 42 }))
            .unwrap_err();
        assert!(matches!(err, PluginError::SettingsInvalid { .. }));
    }

    // 6. validate_no_schema_always_passes
    #[test]
    fn validate_no_schema_always_passes() {
        let m = SettingsManager::new();
        // No schema registered — any value passes
        m.validate("unregistered-plugin", &json!({ "anything": true }))
            .unwrap();
        m.validate("unregistered-plugin", &json!(null)).unwrap();
    }

    // 7. load_settings_from_file
    #[test]
    fn load_settings_from_file() {
        let dir = TempDir::new().unwrap();
        let m = manager_with_schema();

        let settings = json!({ "name": "world", "count": 3 });
        std::fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();

        let loaded = m.load_settings(PLUGIN_ID, dir.path()).unwrap();
        assert_eq!(loaded, settings);
    }

    // 8. load_missing_settings_returns_empty_object
    #[test]
    fn load_missing_settings_returns_empty_object() {
        let dir = TempDir::new().unwrap();
        // No schema, so empty object passes
        let m = SettingsManager::new();

        let loaded = m.load_settings(PLUGIN_ID, dir.path()).unwrap();
        assert_eq!(loaded, json!({}));
    }

    // 8b. load_missing_seeds_defaults_from_schema
    #[test]
    fn load_missing_seeds_defaults_from_schema() {
        const SCHEMA_WITH_DEFAULTS: &str = r#"{
            "type": "object",
            "properties": {
                "name": { "type": "string", "default": "world" },
                "count": { "type": "integer", "default": 3 }
            },
            "required": ["name"]
        }"#;
        let dir = TempDir::new().unwrap();
        let mut m = SettingsManager::new();
        m.register_schema(PLUGIN_ID, SCHEMA_WITH_DEFAULTS).unwrap();

        let loaded = m.load_settings(PLUGIN_ID, dir.path()).unwrap();
        assert_eq!(loaded, json!({ "name": "world", "count": 3 }));
    }

    // 9. save_and_load_roundtrip
    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let m = manager_with_schema();

        let settings = json!({ "name": "roundtrip", "count": 7 });
        m.save_settings(PLUGIN_ID, dir.path(), &settings).unwrap();

        let loaded = m.load_settings(PLUGIN_ID, dir.path()).unwrap();
        assert_eq!(loaded, settings);
    }

    // 10. save_invalid_settings_rejected
    #[test]
    fn save_invalid_settings_rejected() {
        let dir = TempDir::new().unwrap();
        let m = manager_with_schema();

        // Missing required "name" field
        let err = m
            .save_settings(PLUGIN_ID, dir.path(), &json!({}))
            .unwrap_err();
        assert!(matches!(err, PluginError::SettingsInvalid { .. }));

        // File must NOT have been written
        assert!(
            !dir.path().join("settings.json").exists(),
            "settings.json should not be written when validation fails"
        );
    }
}
