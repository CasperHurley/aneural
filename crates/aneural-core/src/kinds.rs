//! Built-in node and edge kinds. Kinds are open strings (spores and
//! `.aneural/nodes/*.json` can add more); these constants are the ones the
//! engine itself produces.

/// Node kind names. Stored as plain strings in the graph.
pub struct NodeKind;

impl NodeKind {
    pub const DIRECTORY: &'static str = "Directory";
    pub const FILE: &'static str = "File";
    pub const REPO: &'static str = "Repo";
    pub const MANIFEST: &'static str = "Manifest";
    pub const PACKAGE: &'static str = "Package";
    pub const SYMBOL: &'static str = "Symbol";
    // spore-provided, but well known to the GUI defaults
    pub const COMMENT: &'static str = "Comment";
    pub const PLAN: &'static str = "Plan";
    pub const IDEA: &'static str = "Idea";
    pub const NOTE: &'static str = "Note";

    /// Kinds produced by the core engine (not by spores).
    pub const BUILTIN: &'static [&'static str] = &[
        Self::DIRECTORY,
        Self::FILE,
        Self::REPO,
        Self::MANIFEST,
        Self::PACKAGE,
    ];

    /// The id prefix used for a kind (see [`crate::id`]).
    pub fn id_prefix(kind: &str) -> &str {
        match kind {
            Self::DIRECTORY => "dir",
            Self::FILE => "file",
            Self::REPO => "repo",
            Self::MANIFEST => "manifest",
            Self::PACKAGE => "pkg",
            Self::SYMBOL => "sym",
            Self::COMMENT => "comment",
            Self::PLAN => "plan",
            Self::IDEA => "idea",
            Self::NOTE => "note",
            other => other,
        }
    }
}

/// Edge kind names (directed, Neo4j-style SCREAMING_CASE).
pub struct EdgeKind;

impl EdgeKind {
    /// Directory → Directory/File, Repo → root Directory.
    pub const CONTAINS: &'static str = "CONTAINS";
    /// File/Directory/Manifest → Repo.
    pub const BELONGS_TO: &'static str = "BELONGS_TO";
    /// File → File (resolved import).
    pub const IMPORTS: &'static str = "IMPORTS";
    /// File → File (`export ... from`).
    pub const RE_EXPORTS: &'static str = "RE_EXPORTS";
    /// File → File, weak reference (require(), `mod`, asset url).
    pub const REFERENCES: &'static str = "REFERENCES";
    /// Manifest/File → Package (external dependency or unresolved import).
    pub const DEPENDS_ON: &'static str = "DEPENDS_ON";
    /// Comment/Plan → File/Directory.
    pub const ANNOTATES: &'static str = "ANNOTATES";
    /// Note/Idea/Plan → anything (wiki-link, frontmatter).
    pub const RELATES_TO: &'static str = "RELATES_TO";

    pub const ALL: &'static [&'static str] = &[
        Self::CONTAINS,
        Self::BELONGS_TO,
        Self::IMPORTS,
        Self::RE_EXPORTS,
        Self::REFERENCES,
        Self::DEPENDS_ON,
        Self::ANNOTATES,
        Self::RELATES_TO,
    ];

    /// Structural edges describe the tree; the rest are "semantic".
    pub fn is_structural(kind: &str) -> bool {
        matches!(kind, Self::CONTAINS | Self::BELONGS_TO)
    }
}

/// Which producer created a node/edge. Stored in the `source` column so a
/// re-run of one producer can replace exactly its own output.
pub struct Source;

impl Source {
    pub const WALKER: &'static str = "walker";
    pub const MANIFEST: &'static str = "manifest";
    pub const LANG: &'static str = "lang";
    pub fn spore(name: &str) -> String {
        format!("spore:{name}")
    }
}
