//! `.bases` record storage (`records.json`).

use serde::{Deserialize, Serialize};

/// A single data record in a base.
///
/// Fields are stored as a flat JSON map keyed by field name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseRecord {
    /// All field values (including `id`).
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl BaseRecord {
    /// Create a new record from a JSON map.
    #[must_use]
    pub fn new(fields: serde_json::Map<String, serde_json::Value>) -> Self {
        Self { fields }
    }

    /// Get the value of field `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&serde_json::Value> {
        self.fields.get(name)
    }

    /// Get the `id` field as a string slice.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.fields.get("id")?.as_str()
    }
}
