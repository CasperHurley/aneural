//! Stable node identifiers: `<prefix>:<workspace-relative path>[#<fragment>]`.
//!
//! Ids are content-independent so that layout positions and focus selections
//! survive edits. Paths always use forward slashes and never start with `./`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(raw: impl Into<String>) -> Self {
        NodeId(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `dir:apps/web/src` (the workspace root itself is `dir:.`).
    pub fn dir(rel: impl AsRef<Path>) -> Self {
        NodeId(format!("dir:{}", canonical_rel(rel.as_ref())))
    }

    pub fn file(rel: impl AsRef<Path>) -> Self {
        NodeId(format!("file:{}", canonical_rel(rel.as_ref())))
    }

    pub fn repo(rel: impl AsRef<Path>) -> Self {
        NodeId(format!("repo:{}", canonical_rel(rel.as_ref())))
    }

    pub fn manifest(rel: impl AsRef<Path>) -> Self {
        NodeId(format!("manifest:{}", canonical_rel(rel.as_ref())))
    }

    /// `pkg:npm/react`, `pkg:cargo/serde`, `pkg:pypi/requests`, `pkg:go/github.com/x/y`.
    pub fn package(ecosystem: &str, name: &str) -> Self {
        NodeId(format!("pkg:{ecosystem}/{name}"))
    }

    /// `comment:<file>#<hash(text)>` — survives line shifts, changes with text.
    pub fn comment(file_rel: impl AsRef<Path>, text: &str) -> Self {
        NodeId(format!(
            "comment:{}#{}",
            canonical_rel(file_rel.as_ref()),
            crate::short_hash(text.trim())
        ))
    }

    pub fn plan(rel: impl AsRef<Path>) -> Self {
        NodeId(format!("plan:{}", canonical_rel(rel.as_ref())))
    }

    pub fn note(rel: impl AsRef<Path>) -> Self {
        NodeId(format!("note:{}", canonical_rel(rel.as_ref())))
    }

    pub fn idea(rel: impl AsRef<Path>, heading: &str) -> Self {
        NodeId(format!("idea:{}#{}", canonical_rel(rel.as_ref()), crate::slug(heading)))
    }

    /// Generic constructor for spore-defined kinds: `<prefix>:<path>[#fragment]`.
    pub fn custom(prefix: &str, rel: impl AsRef<Path>, fragment: Option<&str>) -> Self {
        match fragment {
            Some(f) => NodeId(format!("{prefix}:{}#{f}", canonical_rel(rel.as_ref()))),
            None => NodeId(format!("{prefix}:{}", canonical_rel(rel.as_ref()))),
        }
    }

    /// The `<prefix>` part, e.g. `file`.
    pub fn prefix(&self) -> &str {
        self.0.split_once(':').map(|(p, _)| p).unwrap_or("")
    }

    /// The path-ish part after the prefix and before any `#fragment`.
    pub fn path_part(&self) -> &str {
        let rest = self.0.split_once(':').map(|(_, r)| r).unwrap_or("");
        rest.split_once('#').map(|(p, _)| p).unwrap_or(rest)
    }

    pub fn fragment(&self) -> Option<&str> {
        self.0.split_once('#').map(|(_, f)| f)
    }

    pub fn is_file(&self) -> bool {
        self.prefix() == "file"
    }

    pub fn is_dir(&self) -> bool {
        self.prefix() == "dir"
    }

    /// Validate the `<prefix>:<rest>` shape.
    pub fn parse(raw: &str) -> crate::Result<Self> {
        match raw.split_once(':') {
            Some((p, r)) if !p.is_empty() && !r.is_empty() => Ok(NodeId(raw.to_string())),
            _ => Err(crate::Error::InvalidId(raw.to_string())),
        }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        NodeId(s.to_string())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        NodeId(s)
    }
}

/// Normalise a workspace-relative path: forward slashes, no `./`, no trailing
/// slash, `.` for the root.
pub fn canonical_rel(p: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for comp in p.components() {
        use std::path::Component::*;
        match comp {
            CurDir | RootDir | Prefix(_) => {}
            ParentDir => {
                parts.pop();
            }
            Normal(s) => parts.push(s.to_string_lossy().into_owned()),
        }
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_paths() {
        assert_eq!(canonical_rel(Path::new("./apps/web/")), "apps/web");
        assert_eq!(canonical_rel(Path::new("apps/../src/./a.ts")), "src/a.ts");
        assert_eq!(canonical_rel(Path::new("")), ".");
        assert_eq!(canonical_rel(Path::new(".")), ".");
    }

    #[test]
    fn constructors_and_parts() {
        let id = NodeId::file("./apps/web/src/index.ts");
        assert_eq!(id.as_str(), "file:apps/web/src/index.ts");
        assert_eq!(id.prefix(), "file");
        assert_eq!(id.path_part(), "apps/web/src/index.ts");
        assert_eq!(id.fragment(), None);

        let c = NodeId::comment("a.ts", "TODO: x");
        assert_eq!(c.prefix(), "comment");
        assert_eq!(c.path_part(), "a.ts");
        assert_eq!(c.fragment().unwrap().len(), 12);
        assert_eq!(c, NodeId::comment("a.ts", "  TODO: x \n"));

        assert_eq!(NodeId::package("npm", "react").as_str(), "pkg:npm/react");
        assert_eq!(NodeId::idea("x.md", "Big Idea!").as_str(), "idea:x.md#big-idea");
        assert!(NodeId::parse("nope").is_err());
        assert!(NodeId::parse("file:a").is_ok());
    }
}
