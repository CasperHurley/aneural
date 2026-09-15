//! Import extraction: a small tree-sitter query per language locates the
//! import-like statements; Rust code then reads specifiers and symbols off
//! the matched nodes. Everything works on error-recovered trees, so a file
//! with syntax errors still yields the imports around them.

use crate::parsers::{parser_for, ts_language};
use crate::Error;
use aneural_core::config::Language;
use serde::{Deserialize, Serialize};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportKind {
    /// Ordinary static import (`import`, `use`, Go/Java/PHP `use`, Ruby `require`).
    Static,
    /// `import('x')`.
    Dynamic,
    /// `import type` / `import { type X }` only.
    TypeOnly,
    /// `export ... from 'x'`.
    ReExport,
    /// `require('x')` / `import x = require('x')`.
    Require,
    /// PHP `include`/`require`, Ruby `require_relative`/`load`.
    Include,
    /// Rust `mod foo;`.
    Mod,
    /// Reserved for symbol-level uses.
    Use,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportRef {
    /// The module specifier exactly as written (quotes stripped).
    pub specifier: String,
    /// Imported names (`default`, `*`, or identifiers). Empty for whole-module imports.
    pub symbols: Vec<String>,
    /// 1-based line of the statement.
    pub line: u32,
    pub kind: ImportKind,
}

impl ImportRef {
    fn new(specifier: impl Into<String>, symbols: Vec<String>, node: Node<'_>, kind: ImportKind) -> Self {
        ImportRef { specifier: specifier.into(), symbols, line: line_of(node), kind }
    }
}

/// One capture from [`run_query`].
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub name: String,
    pub text: String,
    pub line: u32,
}

fn line_of(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

fn text<'a>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

/// Text of a string literal node with its quotes removed.
fn unquote(node: Node<'_>, src: &[u8]) -> String {
    let t = text(node, src).trim();
    let t = t
        .strip_prefix('\'')
        .or_else(|| t.strip_prefix('"'))
        .or_else(|| t.strip_prefix('`'))
        .unwrap_or(t);
    let t = t
        .strip_suffix('\'')
        .or_else(|| t.strip_suffix('"'))
        .or_else(|| t.strip_suffix('`'))
        .unwrap_or(t);
    t.to_string()
}

fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn all_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Whether the node has an anonymous (keyword) child with this text.
fn has_keyword(node: Node<'_>, kw: &str) -> bool {
    all_children(node).iter().any(|c| !c.is_named() && c.kind() == kw)
}

fn first_descendant_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    if node.kind() == kind {
        return Some(node);
    }
    for c in named_children(node) {
        if let Some(n) = first_descendant_of_kind(c, kind) {
            return Some(n);
        }
    }
    None
}

fn query_source(lang: &str) -> Result<&'static str, Error> {
    Ok(match lang {
        Language::TYPESCRIPT => include_str!("../queries/typescript/imports.scm"),
        Language::JAVASCRIPT => include_str!("../queries/javascript/imports.scm"),
        Language::PYTHON => include_str!("../queries/python/imports.scm"),
        Language::RUST => include_str!("../queries/rust/imports.scm"),
        Language::GO => include_str!("../queries/go/imports.scm"),
        Language::JAVA => include_str!("../queries/java/imports.scm"),
        Language::PHP => include_str!("../queries/php/imports.scm"),
        Language::RUBY => include_str!("../queries/ruby/imports.scm"),
        other => return Err(Error::UnsupportedLanguage(other.to_string())),
    })
}

fn parse(lang: &str, path: Option<&Path>, source: &[u8]) -> Result<Tree, Error> {
    let mut parser = parser_for(lang, path)?;
    parser
        .parse(source, None)
        .ok_or_else(|| Error::Parse(path.map(|p| p.display().to_string()).unwrap_or_else(|| lang.to_string())))
}

fn compile_query(lang: &str, path: Option<&Path>, query_src: &str) -> Result<Query, Error> {
    let language = ts_language(lang, path)?;
    Query::new(&language, query_src).map_err(|e| Error::Query { lang: lang.to_string(), message: e.to_string() })
}

/// Run an arbitrary tree-sitter query; returns the captures of each match.
pub fn run_query(lang: &str, path: Option<&Path>, query_src: &str, source: &[u8]) -> Result<Vec<Vec<Capture>>, Error> {
    let tree = parse(lang, path, source)?;
    let query = compile_query(lang, path, query_src)?;
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source);
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let caps = m
            .captures()
            .iter()
            .map(|c| Capture { name: names[c.index as usize].to_string(), text: text(c.node, source).to_string(), line: line_of(c.node) })
            .collect();
        out.push(caps);
    }
    Ok(out)
}

/// Extract every import-like statement from `source`.
pub fn extract_imports(lang: &str, path: &Path, source: &[u8]) -> Result<Vec<ImportRef>, Error> {
    let tree = parse(lang, Some(path), source)?;
    let query = compile_query(lang, Some(path), query_source(lang)?)?;
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source);
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        // The capture whose name is the pattern's "statement" capture comes first
        // in every query above; the others are helpers.
        let mut by_name: Vec<(&str, Node<'_>)> = m.captures().iter().map(|c| (names[c.index as usize], c.node)).collect();
        by_name.sort_by_key(|(_, n)| n.start_byte());
        let refs = match lang {
            Language::TYPESCRIPT | Language::JAVASCRIPT => js_imports(&by_name, source),
            Language::PYTHON => py_imports(&by_name, source),
            Language::RUST => rs_imports(&by_name, source),
            Language::GO => go_imports(&by_name, source),
            Language::JAVA => java_imports(&by_name, source),
            Language::PHP => php_imports(&by_name, source),
            Language::RUBY => rb_imports(&by_name, source),
            _ => Vec::new(),
        };
        out.extend(refs);
    }
    // Dynamic/require captures can duplicate when nested; keep first occurrence per (line, specifier, kind).
    out.sort_by_key(|r| (r.line, r.specifier.clone()));
    out.dedup_by(|a, b| a.line == b.line && a.specifier == b.specifier && a.kind == b.kind);
    Ok(out)
}

fn capture<'t>(caps: &[(&str, Node<'t>)], name: &str) -> Option<Node<'t>> {
    caps.iter().find(|(n, _)| *n == name).map(|(_, n)| *n)
}

// ---------------------------------------------------------------- TypeScript / JavaScript

fn js_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    if let Some(node) = capture(caps, "import") {
        let Some(source) = node.child_by_field_name("source") else { return vec![] };
        let specifier = unquote(source, src);
        let mut symbols = Vec::new();
        let mut kind = if has_keyword(node, "type") { ImportKind::TypeOnly } else { ImportKind::Static };
        for child in named_children(node) {
            match child.kind() {
                "import_clause" => {
                    let mut all_type = true;
                    let mut any = false;
                    for c in named_children(child) {
                        match c.kind() {
                            "identifier" => {
                                symbols.push("default".into());
                                all_type = false;
                                any = true;
                            }
                            "namespace_import" => {
                                symbols.push("*".into());
                                all_type = false;
                                any = true;
                            }
                            "named_imports" => {
                                for spec in named_children(c).into_iter().filter(|n| n.kind() == "import_specifier") {
                                    any = true;
                                    if !has_keyword(spec, "type") {
                                        all_type = false;
                                    }
                                    if let Some(name) = spec.child_by_field_name("name") {
                                        symbols.push(text(name, src).to_string());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if any && all_type && kind == ImportKind::Static {
                        kind = ImportKind::TypeOnly;
                    }
                }
                "import_require_clause" => {
                    kind = ImportKind::Require;
                    symbols.push("default".into());
                }
                _ => {}
            }
        }
        return vec![ImportRef::new(specifier, symbols, node, kind)];
    }
    if let Some(node) = capture(caps, "reexport") {
        let Some(source) = node.child_by_field_name("source") else { return vec![] };
        let specifier = unquote(source, src);
        let mut symbols = Vec::new();
        let mut saw_clause = false;
        for child in named_children(node) {
            match child.kind() {
                "export_clause" => {
                    saw_clause = true;
                    for spec in named_children(child).into_iter().filter(|n| n.kind() == "export_specifier") {
                        if let Some(name) = spec.child_by_field_name("name") {
                            symbols.push(text(name, src).to_string());
                        }
                    }
                }
                "namespace_export" => {
                    saw_clause = true;
                    symbols.push("*".into());
                }
                _ => {}
            }
        }
        if !saw_clause {
            symbols.push("*".into());
        }
        return vec![ImportRef::new(specifier, symbols, node, ImportKind::ReExport)];
    }
    if let (Some(node), Some(s)) = (capture(caps, "dynamic"), capture(caps, "dynamic_source")) {
        return vec![ImportRef::new(unquote(s, src), vec![], node, ImportKind::Dynamic)];
    }
    if let (Some(node), Some(s)) = (capture(caps, "require"), capture(caps, "require_source")) {
        return vec![ImportRef::new(unquote(s, src), vec![], node, ImportKind::Require)];
    }
    vec![]
}

// ---------------------------------------------------------------- Python

fn py_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    if let Some(node) = capture(caps, "import") {
        let mut out = Vec::new();
        for child in named_children(node) {
            match child.kind() {
                "dotted_name" => out.push(ImportRef::new(text(child, src), vec![], node, ImportKind::Static)),
                "aliased_import" => {
                    if let Some(name) = child.child_by_field_name("name") {
                        out.push(ImportRef::new(text(name, src), vec![], node, ImportKind::Static));
                    }
                }
                _ => {}
            }
        }
        return out;
    }
    if let Some(node) = capture(caps, "from") {
        let Some(module) = node.child_by_field_name("module_name") else { return vec![] };
        let specifier = text(module, src).trim().to_string();
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        for child in node.children_by_field_name("name", &mut cursor) {
            match child.kind() {
                "dotted_name" => symbols.push(text(child, src).to_string()),
                "aliased_import" => {
                    if let Some(name) = child.child_by_field_name("name") {
                        symbols.push(text(name, src).to_string());
                    }
                }
                _ => {}
            }
        }
        if named_children(node).iter().any(|c| c.kind() == "wildcard_import") {
            symbols.push("*".into());
        }
        return vec![ImportRef::new(specifier, symbols, node, ImportKind::Static)];
    }
    vec![]
}

// ---------------------------------------------------------------- Rust

fn rs_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    if let (Some(node), Some(arg)) = (capture(caps, "use"), capture(caps, "argument")) {
        let mut out = Vec::new();
        rs_flatten(arg, "", node, src, &mut out);
        return out;
    }
    if let (Some(node), Some(name)) = (capture(caps, "mod"), capture(caps, "name")) {
        return vec![ImportRef::new(text(name, src), vec![], node, ImportKind::Mod)];
    }
    vec![]
}

fn rs_join(prefix: &str, seg: &str) -> String {
    if prefix.is_empty() { seg.to_string() } else { format!("{prefix}::{seg}") }
}

/// Flatten a `use` tree into one ImportRef per path; braces become symbols.
fn rs_flatten(node: Node<'_>, prefix: &str, stmt: Node<'_>, src: &[u8], out: &mut Vec<ImportRef>) {
    match node.kind() {
        "identifier" | "crate" | "super" | "self" | "metavariable" | "scoped_identifier" => {
            out.push(ImportRef::new(rs_join(prefix, text(node, src)), vec![], stmt, ImportKind::Static));
        }
        "use_as_clause" => {
            if let Some(p) = node.child_by_field_name("path") {
                rs_flatten(p, prefix, stmt, src, out);
            }
        }
        "use_wildcard" => {
            let inner = named_children(node).into_iter().next().map(|n| text(n, src).to_string()).unwrap_or_default();
            out.push(ImportRef::new(rs_join(prefix, &inner), vec!["*".into()], stmt, ImportKind::Static));
        }
        "scoped_use_list" => {
            let path = node.child_by_field_name("path").map(|p| text(p, src).to_string()).unwrap_or_default();
            let full = rs_join(prefix, &path);
            if let Some(list) = node.child_by_field_name("list") {
                rs_list(list, &full, stmt, src, out);
            }
        }
        "use_list" => rs_list(node, prefix, stmt, src, out),
        _ => {}
    }
}

fn rs_list(list: Node<'_>, prefix: &str, stmt: Node<'_>, src: &[u8], out: &mut Vec<ImportRef>) {
    let mut symbols = Vec::new();
    for item in named_children(list) {
        match item.kind() {
            "identifier" | "self" | "crate" | "super" => symbols.push(text(item, src).to_string()),
            "use_as_clause" => {
                if let Some(p) = item.child_by_field_name("path") {
                    if p.kind() == "identifier" || p.kind() == "self" {
                        symbols.push(text(p, src).to_string());
                    } else {
                        rs_flatten(p, prefix, stmt, src, out);
                    }
                }
            }
            "use_wildcard" if named_children(item).is_empty() => symbols.push("*".into()),
            _ => rs_flatten(item, prefix, stmt, src, out),
        }
    }
    if !symbols.is_empty() || out.is_empty() {
        if prefix.is_empty() {
            // `use {a, b};` with bare identifiers — each is its own path.
            for s in symbols {
                out.push(ImportRef::new(s, vec![], stmt, ImportKind::Static));
            }
        } else {
            out.push(ImportRef::new(prefix, symbols, stmt, ImportKind::Static));
        }
    }
}

// ---------------------------------------------------------------- Go

fn go_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    match (capture(caps, "spec"), capture(caps, "path")) {
        (Some(node), Some(path)) => vec![ImportRef::new(unquote(path, src), vec![], node, ImportKind::Static)],
        _ => vec![],
    }
}

// ---------------------------------------------------------------- Java

fn java_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    let Some(node) = capture(caps, "import") else { return vec![] };
    let path = named_children(node)
        .into_iter()
        .find(|c| matches!(c.kind(), "scoped_identifier" | "identifier"))
        .map(|c| text(c, src).to_string())
        .unwrap_or_default();
    if path.is_empty() {
        return vec![];
    }
    let wildcard = named_children(node).iter().any(|c| c.kind() == "asterisk");
    if has_keyword(node, "static") {
        let (module, member) = path.rsplit_once('.').unwrap_or((&path, ""));
        let symbols = if wildcard { vec!["*".into()] } else { vec![member.to_string()] };
        return vec![ImportRef::new(module, symbols, node, ImportKind::Static)];
    }
    let symbols = if wildcard { vec!["*".into()] } else { vec![] };
    vec![ImportRef::new(path, symbols, node, ImportKind::Static)]
}

// ---------------------------------------------------------------- PHP

fn php_name(s: &str) -> String {
    s.trim().trim_start_matches('\\').to_string()
}

fn php_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    if let Some(node) = capture(caps, "use") {
        let mut out = Vec::new();
        let mut prefix: Option<String> = None;
        for child in named_children(node) {
            match child.kind() {
                "namespace_name" => prefix = Some(php_name(text(child, src))),
                "namespace_use_clause" => {
                    let name = named_children(child)
                        .into_iter()
                        .find(|c| matches!(c.kind(), "qualified_name" | "name"))
                        .map(|c| php_name(text(c, src)))
                        .unwrap_or_default();
                    if !name.is_empty() {
                        out.push(ImportRef::new(name, vec![], node, ImportKind::Static));
                    }
                }
                "namespace_use_group" => {
                    for clause in named_children(child).into_iter().filter(|c| c.kind() == "namespace_use_clause") {
                        let name = named_children(clause)
                            .into_iter()
                            .find(|c| matches!(c.kind(), "qualified_name" | "name"))
                            .map(|c| php_name(text(c, src)))
                            .unwrap_or_default();
                        if name.is_empty() {
                            continue;
                        }
                        let full = match &prefix {
                            Some(p) => format!("{p}\\{name}"),
                            None => name,
                        };
                        out.push(ImportRef::new(full, vec![], node, ImportKind::Static));
                    }
                }
                _ => {}
            }
        }
        return out;
    }
    if let Some(node) = capture(caps, "include") {
        if let Some(s) = first_descendant_of_kind(node, "string").or_else(|| first_descendant_of_kind(node, "encapsed_string")) {
            return vec![ImportRef::new(unquote(s, src), vec![], node, ImportKind::Include)];
        }
    }
    vec![]
}

// ---------------------------------------------------------------- Ruby

fn rb_imports(caps: &[(&str, Node<'_>)], src: &[u8]) -> Vec<ImportRef> {
    let (Some(node), Some(method), Some(source)) = (capture(caps, "call"), capture(caps, "method"), capture(caps, "source"))
    else {
        return vec![];
    };
    let kind = match text(method, src) {
        "require" => ImportKind::Static,
        _ => ImportKind::Include,
    };
    vec![ImportRef::new(unquote(source, src), vec![], node, kind)]
}
