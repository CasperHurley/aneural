//! `spore.json` manifests: declarative node/edge types plus harvesters that
//! infer nodes from files. Spores never execute third-party code; every
//! harvester kind is a built-in runner in `aneural-engine`.

use crate::config::NodeTypeDef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SPORE_SCHEMA_URL: &str = "https://aneural.dev/schema/spore-v1.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SporeManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Lowercase kebab-case identifier, unique in the marketplace.
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Required Aneural version range (semver).
    #[serde(default = "SporeManifest::default_range")]
    pub aneural: String,
    #[serde(default)]
    pub node_types: Vec<NodeTypeDef>,
    #[serde(default)]
    pub edge_types: Vec<EdgeTypeDef>,
    #[serde(default)]
    pub harvesters: Vec<Harvester>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<Panel>,
}

impl SporeManifest {
    fn default_range() -> String {
        ">=0.1".into()
    }

    /// Structural validation (regexes and queries are checked by the engine).
    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        if self.name.is_empty() || crate::slug(&self.name) != self.name {
            errs.push(format!("name `{}` must be lowercase kebab-case", self.name));
        }
        if semver::Version::parse(&self.version).is_err() {
            errs.push(format!("version `{}` is not valid semver", self.version));
        }
        if semver::VersionReq::parse(&self.aneural).is_err() {
            errs.push(format!("aneural range `{}` is not a valid semver requirement", self.aneural));
        }
        let mut ids = std::collections::HashSet::new();
        for h in &self.harvesters {
            if !ids.insert(h.id()) {
                errs.push(format!("duplicate harvester id `{}`", h.id()));
            }
            if h.id().is_empty() {
                errs.push("harvester id must not be empty".into());
            }
            if let Harvester::Wasm { .. } = h {
                errs.push(format!("harvester `{}`: wasm harvesters are reserved and not yet supported", h.id()));
            }
            for nt in &self.node_types {
                if nt.kind.is_empty() {
                    errs.push("node type kind must not be empty".into());
                }
            }
        }
        errs
    }

    /// Whether this spore supports the given Aneural version.
    pub fn supports(&self, aneural_version: &str) -> bool {
        match (semver::VersionReq::parse(&self.aneural), semver::Version::parse(aneural_version)) {
            (Ok(req), Ok(v)) => req.matches(&v),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgeTypeDef {
    pub kind: String,
    #[serde(default)]
    pub label: String,
    /// `solid` | `dotted` | `dashed`.
    #[serde(default = "EdgeTypeDef::default_style")]
    pub style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl EdgeTypeDef {
    fn default_style() -> String {
        "solid".into()
    }
}

/// A harvester turns files into nodes and edges. Templates may reference
/// `{file}` (workspace-relative path), `{line}`, named captures like `{text}`,
/// and the functions `{hash(text)}` and `{slug(text)}`. `$node` inside an
/// edge refers to the node emitted by the same harvester.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Harvester {
    /// Line-oriented regex over files matching `include` globs.
    Regex {
        id: String,
        #[serde(default)]
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        /// Rust regex with named captures. Applied per line.
        pattern: String,
        emit: Emit,
    },
    /// tree-sitter query over a supported language; captures become template vars.
    TreeSitter {
        id: String,
        language: String,
        query: String,
        #[serde(default)]
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        emit: Emit,
    },
    /// Markdown documents or headings; also wiki-links and frontmatter targets.
    Markdown {
        id: String,
        #[serde(default)]
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        #[serde(default)]
        granularity: MarkdownGranularity,
        /// Emit for each document/heading. Vars: `{file}`, `{title}`, `{heading}`,
        /// `{line}`, `{body}` plus frontmatter keys as `{fm.key}`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emit: Option<Emit>,
        /// Emit `RELATES_TO`-style edges for `[[Wiki Links]]`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wikilinks: Option<WikiLinks>,
        /// Emit edges from a frontmatter list of paths.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotate: Option<Annotate>,
    },
    /// Reserved for a future WASM component runtime.
    Wasm { id: String, module: String },
}

impl Harvester {
    pub fn id(&self) -> &str {
        match self {
            Harvester::Regex { id, .. }
            | Harvester::TreeSitter { id, .. }
            | Harvester::Markdown { id, .. }
            | Harvester::Wasm { id, .. } => id,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MarkdownGranularity {
    /// One node per document.
    #[default]
    Document,
    /// One node per `## heading` (level 2 by default).
    Heading,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WikiLinks {
    #[serde(default = "WikiLinks::default_kind")]
    pub edge_kind: String,
}

impl WikiLinks {
    fn default_kind() -> String {
        crate::kinds::EdgeKind::RELATES_TO.into()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Annotate {
    /// Frontmatter key holding a list of workspace-relative paths.
    #[serde(default = "Annotate::default_key")]
    pub frontmatter_key: String,
    #[serde(default = "Annotate::default_kind")]
    pub edge_kind: String,
}

impl Annotate {
    fn default_key() -> String {
        "targets".into()
    }
    fn default_kind() -> String {
        crate::kinds::EdgeKind::ANNOTATES.into()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Emit {
    pub node: EmitNode,
    #[serde(default)]
    pub edges: Vec<EmitEdge>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmitNode {
    pub kind: String,
    /// Id template, e.g. `comment:{file}#{hash(text)}`.
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub props: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmitEdge {
    pub kind: String,
    /// `$node` or an id template.
    pub src: String,
    pub dst: String,
    #[serde(default)]
    pub props: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    pub title: String,
    #[serde(default)]
    pub columns: Vec<String>,
}

/// Information about an installed/available spore, for CLI/MCP listings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SporeInfo {
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub enabled: bool,
    /// `builtin` or `workspace`.
    pub location: String,
    pub path: String,
    pub node_kinds: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMENTS: &str = r##"{
      "name": "comments", "version": "0.1.0", "displayName": "Code Comments",
      "nodeTypes": [{ "kind": "Comment", "icon": "LuMessageSquare", "color": "#e0a458", "shape": "pill" }],
      "edgeTypes": [{ "kind": "ANNOTATES", "style": "dotted" }],
      "harvesters": [{
        "id": "todo", "kind": "regex", "include": ["**/*.ts"],
        "pattern": "(?i)\\b(?P<tag>TODO|FIXME)\\b:?\\s*(?P<text>.+)$",
        "emit": { "node": { "kind": "Comment", "id": "comment:{file}#{hash(text)}", "label": "{text}", "props": { "tag": "{tag}" } },
                  "edges": [{ "kind": "ANNOTATES", "src": "$node", "dst": "file:{file}", "props": { "line": "{line}" } }] }
      }]
    }"##;

    #[test]
    fn parses_and_validates() {
        let m: SporeManifest = serde_json::from_str(COMMENTS).unwrap();
        assert_eq!(m.harvesters.len(), 1);
        assert!(matches!(m.harvesters[0], Harvester::Regex { .. }));
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        assert!(m.supports("0.1.0"));
        assert_eq!(m.node_types[0].label, "");
    }

    #[test]
    fn rejects_bad_name_and_wasm() {
        let mut m: SporeManifest = serde_json::from_str(COMMENTS).unwrap();
        m.name = "Bad Name".into();
        m.harvesters.push(Harvester::Wasm { id: "w".into(), module: "x.wasm".into() });
        let errs = m.validate();
        assert_eq!(errs.len(), 2, "{errs:?}");
    }
}
