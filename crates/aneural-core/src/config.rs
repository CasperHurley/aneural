//! `.aneural/config.json` and node type definitions.

use crate::kinds::NodeKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONFIG_SCHEMA_URL: &str = "https://aneural.dev/schema/config-v1.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub version: u32,
    /// Display name of the workspace (defaults to the directory name).
    pub name: String,
    /// Directories to index, relative to the workspace root.
    pub roots: Vec<String>,
    pub respect_gitignore: bool,
    /// Extra ignore globs (gitignore syntax) applied on top of `.gitignore`.
    pub ignore: Vec<String>,
    /// Languages to analyse for imports.
    pub languages: Vec<String>,
    pub typescript: TypeScriptConfig,
    pub spores: SporesConfig,
    /// Per-kind style overrides and lightweight custom kinds.
    pub node_types: BTreeMap<String, NodeTypeStyle>,
    pub gui: GuiConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema: Some(CONFIG_SCHEMA_URL.to_string()),
            version: crate::SCHEMA_VERSION,
            name: String::new(),
            roots: vec![".".to_string()],
            respect_gitignore: true,
            ignore: vec![
                "**/node_modules/**".into(),
                "**/target/**".into(),
                "**/dist/**".into(),
                "**/build/**".into(),
                "**/.git/**".into(),
                "**/.turbo/**".into(),
                "**/__pycache__/**".into(),
                "**/.venv/**".into(),
                "**/vendor/**".into(),
            ],
            languages: Language::ALL.iter().map(|s| s.to_string()).collect(),
            typescript: TypeScriptConfig::default(),
            spores: SporesConfig::default(),
            node_types: BTreeMap::new(),
            gui: GuiConfig::default(),
        }
    }
}

impl Config {
    pub fn language_enabled(&self, lang: &str) -> bool {
        self.languages.iter().any(|l| l == lang)
    }
}

/// Language identifiers used throughout (file `lang` prop, config, spores).
pub struct Language;

impl Language {
    pub const TYPESCRIPT: &'static str = "typescript";
    pub const JAVASCRIPT: &'static str = "javascript";
    pub const PYTHON: &'static str = "python";
    pub const RUST: &'static str = "rust";
    pub const GO: &'static str = "go";
    pub const JAVA: &'static str = "java";
    pub const PHP: &'static str = "php";
    pub const RUBY: &'static str = "ruby";
    pub const MARKDOWN: &'static str = "markdown";

    pub const ALL: &'static [&'static str] = &[
        Self::TYPESCRIPT,
        Self::JAVASCRIPT,
        Self::PYTHON,
        Self::RUST,
        Self::GO,
        Self::JAVA,
        Self::PHP,
        Self::RUBY,
    ];

    /// Language for a file extension (lowercase, without the dot).
    pub fn from_extension(ext: &str) -> Option<&'static str> {
        Some(match ext {
            "ts" | "tsx" | "mts" | "cts" => Self::TYPESCRIPT,
            "js" | "jsx" | "mjs" | "cjs" => Self::JAVASCRIPT,
            "py" | "pyi" => Self::PYTHON,
            "rs" => Self::RUST,
            "go" => Self::GO,
            "java" => Self::JAVA,
            "php" => Self::PHP,
            "rb" | "rake" | "gemspec" => Self::RUBY,
            "md" | "mdx" | "markdown" => Self::MARKDOWN,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TypeScriptConfig {
    /// `"auto"` (nearest tsconfig.json per file) or a workspace-relative path.
    pub tsconfig: String,
    /// `exports`/`imports` condition names used during resolution.
    pub condition_names: Vec<String>,
}

impl Default for TypeScriptConfig {
    fn default() -> Self {
        TypeScriptConfig {
            tsconfig: "auto".into(),
            condition_names: ["import", "require", "node", "types", "default"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SporesConfig {
    /// Names of enabled spores (first-party or installed under `.aneural/spores`).
    pub enabled: Vec<String>,
    /// Marketplace registry URL.
    pub registry: String,
}

impl Default for SporesConfig {
    fn default() -> Self {
        SporesConfig {
            enabled: ["comments", "plans", "icebox", "wiki-links"].into_iter().map(String::from).collect(),
            registry: "https://raw.githubusercontent.com/aneural/spores/main/registry.json".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GuiConfig {
    pub theme: String,
    /// Camera zoom above which node labels are drawn.
    pub label_zoom_threshold: f32,
    /// Maximum nodes applied per frame during growth (keeps the animation legible).
    pub growth_budget_per_frame: u32,
}

impl Default for GuiConfig {
    fn default() -> Self {
        GuiConfig { theme: "mycelium-dark".into(), label_zoom_threshold: 0.8, growth_budget_per_frame: 200 }
    }
}

/// Visual style overrides for a kind, as used in `config.json#nodeTypes`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeTypeStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A full node type definition (`.aneural/nodes/<kind>.json`, spore manifests, builtins).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeTypeDef {
    pub kind: String,
    #[serde(default)]
    pub label: String,
    /// icondata static name, e.g. `LuFolder`, `SiTypescript`.
    #[serde(default = "NodeTypeDef::default_icon")]
    pub icon: String,
    /// Hex colour.
    #[serde(default = "NodeTypeDef::default_color")]
    pub color: String,
    /// `circle` | `hexagon` | `pill` | `square` | `diamond`.
    #[serde(default = "NodeTypeDef::default_shape")]
    pub shape: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Who provides this kind: `builtin`, `workspace`, or `spore:<name>`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
}

impl NodeTypeDef {
    pub const FALLBACK_ICON: &'static str = "LuCircleDot";

    fn default_icon() -> String {
        Self::FALLBACK_ICON.into()
    }
    fn default_color() -> String {
        "#9aa0a6".into()
    }
    fn default_shape() -> String {
        "circle".into()
    }

    pub fn new(kind: &str, label: &str, icon: &str, color: &str, shape: &str, description: &str) -> Self {
        NodeTypeDef {
            kind: kind.into(),
            label: label.into(),
            icon: icon.into(),
            color: color.into(),
            shape: shape.into(),
            description: description.into(),
            provider: "builtin".into(),
        }
    }

    pub fn apply(&mut self, style: &NodeTypeStyle) {
        if let Some(i) = &style.icon {
            self.icon = i.clone();
        }
        if let Some(c) = &style.color {
            self.color = c.clone();
        }
        if let Some(s) = &style.shape {
            self.shape = s.clone();
        }
        if let Some(l) = &style.label {
            self.label = l.clone();
        }
    }
}

/// Node types the engine produces itself, with the default mycelium palette.
pub fn builtin_node_types() -> Vec<NodeTypeDef> {
    vec![
        NodeTypeDef::new(NodeKind::DIRECTORY, "Directory", "LuFolder", "#8fae6b", "circle", "A folder in the workspace"),
        NodeTypeDef::new(NodeKind::FILE, "File", "LuFile", "#d9d2c5", "circle", "A source or asset file"),
        NodeTypeDef::new(NodeKind::REPO, "Repo", "VsRepo", "#e0a458", "hexagon", "A git repository root"),
        NodeTypeDef::new(NodeKind::MANIFEST, "Manifest", "VsPackage", "#c78b5e", "square", "A package manifest (package.json, Cargo.toml, ...)"),
        NodeTypeDef::new(NodeKind::PACKAGE, "Package", "LuPackage", "#7d8aa5", "diamond", "An external dependency"),
        NodeTypeDef::new(NodeKind::SYMBOL, "Symbol", "VsSymbolMethod", "#b0b0b0", "circle", "An exported symbol (reserved)"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let c = Config::default();
        let json = serde_json::to_string_pretty(&c).unwrap();
        assert!(json.contains("\"$schema\""));
        assert!(json.contains("respectGitignore"));
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn partial_config_fills_defaults() {
        let c: Config = serde_json::from_str(r#"{"name":"x","languages":["rust"]}"#).unwrap();
        assert_eq!(c.name, "x");
        assert_eq!(c.languages, vec!["rust"]);
        assert!(c.respect_gitignore);
        assert!(!c.ignore.is_empty());
    }

    #[test]
    fn language_from_extension() {
        assert_eq!(Language::from_extension("tsx"), Some("typescript"));
        assert_eq!(Language::from_extension("rb"), Some("ruby"));
        assert_eq!(Language::from_extension("png"), None);
    }

    #[test]
    fn schema_exports() {
        let schema = schemars::schema_for!(Config);
        let v = serde_json::to_value(&schema).unwrap();
        assert!(v["properties"]["languages"].is_object());
    }
}
