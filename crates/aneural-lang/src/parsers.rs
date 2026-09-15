//! Grammar loading. Every grammar is pinned to an exact version in the
//! workspace manifest; `abi_check` proves they all load under this tree-sitter.

use crate::Error;
use aneural_core::config::Language;
use std::path::Path;
use tree_sitter::{Language as TsLanguage, Parser};

/// Language for a path via its extension. `None` for markdown and unknown files.
pub fn lang_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match Language::from_extension(&ext) {
        Some(Language::MARKDOWN) | None => None,
        Some(l) => Some(l),
    }
}

/// All languages we can parse.
pub const SUPPORTED: &[&str] = Language::ALL;

/// Whether a path is a TSX/JSX flavour of TypeScript/JavaScript.
fn is_tsx(path: Option<&Path>) -> bool {
    path.and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("tsx"))
        .unwrap_or(false)
}

/// The tree-sitter language for `lang`, picking a grammar variant from `path`.
pub fn ts_language(lang: &str, path: Option<&Path>) -> Result<TsLanguage, Error> {
    let f = match lang {
        Language::TYPESCRIPT => {
            if is_tsx(path) {
                tree_sitter_typescript::LANGUAGE_TSX
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT
            }
        }
        Language::JAVASCRIPT => tree_sitter_javascript::LANGUAGE,
        Language::PYTHON => tree_sitter_python::LANGUAGE,
        Language::RUST => tree_sitter_rust::LANGUAGE,
        Language::GO => tree_sitter_go::LANGUAGE,
        Language::JAVA => tree_sitter_java::LANGUAGE,
        Language::PHP => tree_sitter_php::LANGUAGE_PHP,
        Language::RUBY => tree_sitter_ruby::LANGUAGE,
        other => return Err(Error::UnsupportedLanguage(other.to_string())),
    };
    Ok(TsLanguage::new(f))
}

/// A parser configured for `lang`.
pub fn parser_for(lang: &str, path: Option<&Path>) -> Result<Parser, Error> {
    let language = ts_language(lang, path)?;
    let mut parser = Parser::new();
    parser.set_language(&language).map_err(|e| Error::Abi {
        lang: lang.to_string(),
        message: e.to_string(),
    })?;
    Ok(parser)
}

/// Load every grammar (both TypeScript variants included). Fails on ABI mismatch.
pub fn abi_check() -> Result<(), Error> {
    for lang in SUPPORTED {
        parser_for(lang, None)?;
    }
    parser_for(Language::TYPESCRIPT, Some(Path::new("x.tsx")))?;
    Ok(())
}
