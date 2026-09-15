//! placeholder
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Resolved { File { path: PathBuf }, Directory { path: PathBuf }, External { ecosystem: String, name: String }, Unresolved { reason: String } }
pub struct Resolver {}
impl Resolver {
    pub fn new(_root: &Path, _ts: &aneural_core::config::TypeScriptConfig) -> Self { Resolver {} }
    pub fn resolve(&self, _lang: &str, _from: &Path, _i: &crate::ImportRef) -> Resolved { Resolved::Unresolved { reason: "todo".into() } }
}
