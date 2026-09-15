//! Health checks surfaced by `aneural doctor`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// `error` | `warning` | `info`
    pub level: String,
    /// `config` | `spore` | `icon` | `unresolved` | `cache`
    pub category: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Diagnostic {
    pub fn new(level: &str, category: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            level: level.into(),
            category: category.into(),
            message: message.into(),
            path: None,
        }
    }
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}
