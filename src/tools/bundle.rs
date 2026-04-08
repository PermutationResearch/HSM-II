//! Versioned tool bundles for registry metadata (gap 5).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolBundle {
    pub id: String,
    pub version: String,
    pub tool_names: Vec<String>,
}

impl ToolBundle {
    pub fn new(id: impl Into<String>, version: impl Into<String>, tool_names: Vec<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            tool_names,
        }
    }
}
