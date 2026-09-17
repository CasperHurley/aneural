//! Aneural core: the graph model shared by the engine, the store, the Node addon
//! and the Bevy GUI. Pure data types plus the `.aneural/` workspace conventions.
//!
//! Nothing here touches tree-sitter, SQLite or Bevy; the only IO is reading and
//! writing the small JSON files that live under `.aneural/`.

pub mod config;
pub mod focus;
pub mod graph;
pub mod id;
pub mod kinds;
pub mod net;
pub mod spore;
pub mod workspace;

pub use config::{Config, NodeTypeDef};
pub use focus::{Filters, Focus, Neighborhood, Selection};
pub use graph::{Edge, GraphDelta, Node, Subgraph};
pub use id::NodeId;
pub use kinds::{EdgeKind, NodeKind};
pub use spore::SporeManifest;
pub use workspace::Workspace;

/// Crate/format version. Bumped alongside `.aneural/` schema changes.
pub const SCHEMA_VERSION: u32 = 1;

/// Errors shared across the core types.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid node id `{0}`")]
    InvalidId(String),
    #[error("no .aneural workspace found above {0}")]
    NoWorkspace(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Current time as an RFC 3339 string (used in focus.json and friends).
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Current time as unix milliseconds (used in the SQLite store).
pub fn now_millis() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Short, stable content hash used inside derived node ids.
pub fn short_hash(input: &str) -> String {
    blake3::hash(input.as_bytes()).to_hex()[..12].to_string()
}

/// URL/ID safe slug for headings and titles.
pub fn slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = true;
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_stable() {
        assert_eq!(slug("Hello, World!  again"), "hello-world-again");
        assert_eq!(slug("  ## Ideas: Spores? "), "ideas-spores");
    }

    #[test]
    fn short_hash_is_12_hex() {
        let h = short_hash("TODO: fix me");
        assert_eq!(h.len(), 12);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
