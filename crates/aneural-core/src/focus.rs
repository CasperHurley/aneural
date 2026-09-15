//! `.aneural/state/focus.json`: the GUI → MCP handoff describing what the user
//! is looking at right now.

use crate::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Focus {
    pub version: u32,
    /// RFC 3339.
    pub updated_at: String,
    /// Absolute workspace root.
    pub workspace: String,
    pub filters: Filters,
    pub selection: Selection,
    pub neighborhood: Neighborhood,
    /// Ids currently visible in the GUI after filters + selection.
    pub visible_node_ids: Vec<NodeId>,
    /// Free text typed in the focus panel — a message to the assistant.
    pub notes: String,
}

impl Default for Focus {
    fn default() -> Self {
        Focus {
            version: crate::SCHEMA_VERSION,
            updated_at: String::new(),
            workspace: String::new(),
            filters: Filters::default(),
            selection: Selection::default(),
            neighborhood: Neighborhood::default(),
            visible_node_ids: Vec::new(),
            notes: String::new(),
        }
    }
}

impl Focus {
    pub fn new(workspace: impl Into<String>) -> Self {
        Focus {
            workspace: workspace.into(),
            updated_at: crate::now_rfc3339(),
            ..Default::default()
        }
    }

    /// Everything the user has explicitly pointed at (primary + pinned).
    pub fn anchor_ids(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = Vec::new();
        if let Some(p) = &self.selection.primary {
            ids.push(p.clone());
        }
        for id in &self.selection.pinned {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    pub fn is_empty(&self) -> bool {
        self.anchor_ids().is_empty() && self.visible_node_ids.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Filters {
    /// Node kinds shown. Empty = all.
    pub kinds: Vec<String>,
    /// Edge kinds shown. Empty = all.
    pub edge_kinds: Vec<String>,
    /// Repo node ids shown. Empty = all.
    pub repos: Vec<NodeId>,
    /// Free-text search over labels/paths.
    pub query: String,
}

impl Filters {
    pub fn allows_kind(&self, kind: &str) -> bool {
        self.kinds.is_empty() || self.kinds.iter().any(|k| k == kind)
    }
    pub fn allows_edge_kind(&self, kind: &str) -> bool {
        self.edge_kinds.is_empty() || self.edge_kinds.iter().any(|k| k == kind)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Selection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<NodeId>,
    pub pinned: Vec<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Neighborhood {
    pub depth: u32,
    pub direction: Direction,
}

impl Default for Neighborhood {
    fn default() -> Self {
        Neighborhood {
            depth: 1,
            direction: Direction::Both,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_json_shape() {
        let mut f = Focus::new("/tmp/ws");
        f.selection.primary = Some(NodeId::file("a.ts"));
        f.selection.pinned.push(NodeId::file("b.ts"));
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["selection"]["primary"], "file:a.ts");
        assert_eq!(v["neighborhood"]["direction"], "both");
        assert_eq!(v["visibleNodeIds"], serde_json::json!([]));
        assert_eq!(f.anchor_ids().len(), 2);
        let back: Focus = serde_json::from_value(v).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn empty_filters_allow_everything() {
        let f = Filters::default();
        assert!(f.allows_kind("File"));
        let g = Filters {
            kinds: vec!["File".into()],
            ..Default::default()
        };
        assert!(!g.allows_kind("Directory"));
    }
}
