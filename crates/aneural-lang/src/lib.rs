//! aneural-lang: tree-sitter based import extraction and module resolution.

pub mod extract;
pub mod parsers;
pub mod resolve;

pub use extract::{Capture, ImportKind, ImportRef, extract_imports, run_query};
pub use parsers::{abi_check, lang_for_path, parser_for};
pub use resolve::{Resolved, Resolver};

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported language `{0}`")]
    UnsupportedLanguage(String),
    #[error("grammar for {lang} failed to load: {message}")]
    Abi { lang: String, message: String },
    #[error("query error for {lang}: {message}")]
    Query { lang: String, message: String },
    #[error("parse failed for {0}")]
    Parse(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of analysing one file: its language and every import with its resolution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileAnalysis {
    pub lang: String,
    pub imports: Vec<(ImportRef, Resolved)>,
}

/// Extract and resolve all imports of `path` (absolute) with contents `source`.
pub fn analyze_file(
    resolver: &Resolver,
    path: &Path,
    source: &[u8],
) -> Result<FileAnalysis, Error> {
    let lang = lang_for_path(path)
        .ok_or_else(|| Error::UnsupportedLanguage(path.display().to_string()))?;
    let imports = extract_imports(lang, path, source)?;
    let imports = imports
        .into_iter()
        .map(|i| {
            let r = resolver.resolve(lang, path, &i);
            (i, r)
        })
        .collect();
    Ok(FileAnalysis {
        lang: lang.to_string(),
        imports,
    })
}
