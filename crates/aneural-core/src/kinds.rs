//! Built-in node and edge kinds. Kinds are open strings (spores and
//! `.aneural/nodes/*.json` can add more); these constants are the ones the
//! engine itself produces.

use std::borrow::Cow;

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
    /// Directory → Directory/File, Repo → root Directory. The folder tree
    /// every other kind hangs off, so it is always drawn.
    pub const CONTAINS: &'static str = "CONTAINS";
    /// File → File/Directory/Package: anything a file pulls in by `import`,
    /// `use` or `export … from`. The edge's `importKind` prop keeps the syntax.
    pub const IMPORTS: &'static str = "IMPORTS";
    /// File → File, looser than an import (require(), `mod`, include), and
    /// between spore nodes such as tables linked by a foreign key.
    pub const REFERENCES: &'static str = "REFERENCES";
    /// Comment/Plan → File/Directory: a note pointed at code.
    pub const ANNOTATES: &'static str = "ANNOTATES";
    /// Note/Idea/Plan → anything (wiki-link, source file). Not drawn as a
    /// strand: the node floats near what it relates to instead.
    pub const RELATES_TO: &'static str = "RELATES_TO";

    pub const ALL: &'static [&'static str] = &[
        Self::CONTAINS,
        Self::IMPORTS,
        Self::REFERENCES,
        Self::ANNOTATES,
        Self::RELATES_TO,
    ];

    /// Kinds the user can switch off. The folder tree is the skeleton and
    /// relations are shown by where a node floats, so neither is a line to hide.
    pub fn is_toggleable(kind: &str) -> bool {
        !matches!(kind, Self::CONTAINS | Self::RELATES_TO)
    }

    /// Human-readable name for a kind, e.g. `RE_EXPORTS` → "Re-exports".
    /// Unknown kinds (spores can add their own) are de-screamed generically.
    pub fn label(kind: &str) -> Cow<'static, str> {
        match kind {
            Self::CONTAINS => "Contains".into(),
            Self::IMPORTS => "Imports".into(),
            Self::REFERENCES => "References".into(),
            Self::ANNOTATES => "Annotates".into(),
            Self::RELATES_TO => "Relates to".into(),
            other => Cow::Owned(humanize(other)),
        }
    }
}

/// `SCREAMING_SNAKE` (or anything else) to sentence case: underscores become
/// spaces and only the first letter is capitalised.
fn humanize(kind: &str) -> String {
    let lower = kind.replace('_', " ").to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => lower,
    }
}

/// Which producer created a node/edge. Stored in the `source` column so a
/// re-run of one producer can replace exactly its own output.
pub struct Source;

impl Source {
    pub const WALKER: &'static str = "walker";
    pub const LANG: &'static str = "lang";
    pub fn spore(name: &str) -> String {
        format!("spore:{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::EdgeKind;

    #[test]
    fn edge_labels_are_human_readable() {
        assert_eq!(EdgeKind::label(EdgeKind::RELATES_TO), "Relates to");
        assert_eq!(
            EdgeKind::label("TASTES_LIKE_MUSHROOM"),
            "Tastes like mushroom"
        );
        assert_eq!(EdgeKind::label(""), "");
        for kind in EdgeKind::ALL {
            let label = EdgeKind::label(kind);
            assert!(!label.contains('_'), "{kind} -> {label}");
        }
    }
}
