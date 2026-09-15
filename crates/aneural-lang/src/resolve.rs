//! Module resolution. TypeScript/JavaScript goes through `oxc_resolver`
//! (tsconfig paths, package exports, symlinks); the other languages use
//! filesystem heuristics that are right for conventional project layouts and
//! fall back to an `External` package or an `Unresolved` reason otherwise.

use crate::{ImportKind, ImportRef};
use aneural_core::config::{Language, TypeScriptConfig};
use oxc_resolver::{
    ResolveOptions, Resolver as OxcResolver, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Resolved {
    /// A file inside the workspace (absolute path).
    File { path: PathBuf },
    /// A directory-level import (Go packages, Java wildcards); the engine fans out.
    Directory { path: PathBuf },
    /// A third-party package.
    External { ecosystem: String, name: String },
    /// Could not be resolved; `reason` is `stdlib` for standard-library modules.
    Unresolved { reason: String },
}

impl Resolved {
    fn external(eco: &str, name: impl Into<String>) -> Self {
        Resolved::External {
            ecosystem: eco.into(),
            name: name.into(),
        }
    }
    fn unresolved(reason: impl Into<String>) -> Self {
        Resolved::Unresolved {
            reason: reason.into(),
        }
    }
    fn stdlib() -> Self {
        Resolved::unresolved("stdlib")
    }
}

pub struct Resolver {
    root: PathBuf,
    ts: TypeScriptConfig,
    /// oxc resolvers keyed by the tsconfig.json they were built for (or the root when none).
    ts_cache: Mutex<HashMap<PathBuf, Arc<OxcResolver>>>,
    /// Nearest-ancestor lookups keyed by directory.
    nearest_cache: Mutex<HashMap<(String, PathBuf), Option<PathBuf>>>,
}

impl Resolver {
    pub fn new(root: &Path, ts: &TypeScriptConfig) -> Self {
        Resolver {
            root: root.to_path_buf(),
            ts: ts.clone(),
            ts_cache: Mutex::new(HashMap::new()),
            nearest_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve `import` found in `from_file` (absolute).
    pub fn resolve(&self, lang: &str, from_file: &Path, import: &ImportRef) -> Resolved {
        match lang {
            Language::TYPESCRIPT | Language::JAVASCRIPT => self.resolve_js(from_file, import),
            Language::PYTHON => self.resolve_py(from_file, import),
            Language::RUST => self.resolve_rs(from_file, import),
            Language::GO => self.resolve_go(from_file, import),
            Language::JAVA => self.resolve_java(from_file, import),
            Language::PHP => self.resolve_php(from_file, import),
            Language::RUBY => self.resolve_rb(from_file, import),
            other => Resolved::unresolved(format!("unsupported language {other}")),
        }
    }

    /// Walk up from `dir` (inclusive) to the workspace root looking for `name`.
    fn nearest(&self, dir: &Path, name: &str) -> Option<PathBuf> {
        let key = (name.to_string(), dir.to_path_buf());
        if let Some(hit) = self.nearest_cache.lock().unwrap().get(&key) {
            return hit.clone();
        }
        let mut cur = Some(dir);
        let mut found = None;
        while let Some(d) = cur {
            let candidate = d.join(name);
            if candidate.exists() {
                found = Some(candidate);
                break;
            }
            if d == self.root {
                break;
            }
            cur = d.parent();
        }
        self.nearest_cache
            .lock()
            .unwrap()
            .insert(key, found.clone());
        found
    }

    // ------------------------------------------------------------ TypeScript / JavaScript

    fn js_resolver(&self, from_dir: &Path) -> Arc<OxcResolver> {
        let tsconfig = if self.ts.tsconfig == "auto" {
            self.nearest(from_dir, "tsconfig.json")
        } else {
            let p = self.root.join(&self.ts.tsconfig);
            p.exists().then_some(p)
        };
        let key = tsconfig.clone().unwrap_or_else(|| self.root.clone());
        if let Some(r) = self.ts_cache.lock().unwrap().get(&key) {
            return r.clone();
        }
        let options = ResolveOptions {
            tsconfig: tsconfig.map(|config_file| {
                TsconfigDiscovery::Manual(TsconfigOptions {
                    config_file,
                    references: TsconfigReferences::Auto,
                })
            }),
            extensions: [
                ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json", ".d.ts",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            extension_alias: vec![
                (
                    ".js".into(),
                    vec![".ts".into(), ".tsx".into(), ".js".into()],
                ),
                (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
                (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
            ],
            condition_names: self.ts.condition_names.clone(),
            main_fields: vec!["module".into(), "main".into()],
            ..ResolveOptions::default()
        };
        let r = Arc::new(OxcResolver::new(options));
        self.ts_cache.lock().unwrap().insert(key, r.clone());
        r
    }

    fn resolve_js(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let spec = import.specifier.as_str();
        if spec.is_empty() {
            return Resolved::unresolved("empty specifier");
        }
        if spec.starts_with("node:") || NODE_BUILTINS.contains(&spec) {
            return Resolved::stdlib();
        }
        let Some(from_dir) = from_file.parent() else {
            return Resolved::unresolved("no parent dir");
        };
        let resolver = self.js_resolver(from_dir);
        match resolver.resolve(from_dir, spec) {
            Ok(res) => {
                let path = res.path().to_path_buf();
                if let Some(pkg) = node_modules_package(&path) {
                    return Resolved::external("npm", pkg);
                }
                if path.is_dir() {
                    Resolved::Directory { path }
                } else {
                    Resolved::File { path }
                }
            }
            Err(err) => {
                if let Some(pkg) = npm_package_name(spec) {
                    return Resolved::external("npm", pkg);
                }
                Resolved::unresolved(err.to_string())
            }
        }
    }

    // ------------------------------------------------------------ Python

    fn resolve_py(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let spec = import.specifier.as_str();
        let from_dir = from_file.parent().unwrap_or(&self.root);
        let dots = spec.chars().take_while(|c| *c == '.').count();
        if dots > 0 {
            let mut base = from_dir.to_path_buf();
            for _ in 1..dots {
                base = base.parent().map(Path::to_path_buf).unwrap_or(base);
            }
            let rest = &spec[dots..];
            if rest.is_empty() {
                // `from . import x` — x may be a module or a name in __init__.
                for sym in &import.symbols {
                    if let Some(hit) = py_module_file(&base, sym) {
                        return Resolved::File { path: hit };
                    }
                }
                return py_module_file(&base, "__init__")
                    .map(|p| Resolved::File { path: p })
                    .unwrap_or_else(|| Resolved::Directory { path: base });
            }
            return py_resolve_in(&base, rest, &import.symbols).unwrap_or_else(|| {
                Resolved::unresolved(format!("relative module {spec} not found"))
            });
        }
        let top = spec.split('.').next().unwrap_or(spec);
        // Walk up looking for a directory that contains the top-level package/module.
        let mut cur = Some(from_dir);
        while let Some(dir) = cur {
            if let Some(hit) = py_resolve_in(dir, spec, &import.symbols) {
                return hit;
            }
            if dir == self.root
                || dir.join("pyproject.toml").exists()
                || dir.join("setup.py").exists()
            {
                // also try a conventional src/ layout at the project root
                if let Some(hit) = py_resolve_in(&dir.join("src"), spec, &import.symbols) {
                    return hit;
                }
                break;
            }
            cur = dir.parent();
        }
        if PY_STDLIB.contains(&top) {
            return Resolved::stdlib();
        }
        Resolved::external("pypi", top)
    }

    // ------------------------------------------------------------ Rust

    fn resolve_rs(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let from_dir = from_file.parent().unwrap_or(&self.root);
        let file_stem = from_file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // Directory that holds this module's children.
        let mod_dir: PathBuf = if matches!(file_stem, "mod" | "lib" | "main") {
            from_dir.to_path_buf()
        } else {
            from_dir.join(file_stem)
        };
        if import.kind == ImportKind::Mod {
            return rs_module_file(&mod_dir, &import.specifier)
                .map(|p| Resolved::File { path: p })
                .unwrap_or_else(|| {
                    Resolved::unresolved(format!("mod {} not found", import.specifier))
                });
        }
        let segs: Vec<&str> = import.specifier.split("::").collect();
        let Some(first) = segs.first().copied() else {
            return Resolved::unresolved("empty path");
        };
        let (base, rest): (PathBuf, &[&str]) = match first {
            "crate" => {
                let Some(cargo) = self.nearest(from_dir, "Cargo.toml") else {
                    return Resolved::unresolved("no Cargo.toml above file");
                };
                let src = cargo.parent().unwrap_or(&self.root).join("src");
                (src, &segs[1..])
            }
            "self" => (mod_dir.clone(), &segs[1..]),
            "super" => {
                let mut base = mod_dir
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or(mod_dir.clone());
                let mut i = 1;
                while segs.get(i) == Some(&"super") {
                    base = base.parent().map(Path::to_path_buf).unwrap_or(base);
                    i += 1;
                }
                (base, &segs[i..])
            }
            "std" | "core" | "alloc" => return Resolved::stdlib(),
            _ => {
                // Could be a sibling module referenced without `self::` (2018 edition allows `use foo::bar`
                // when `foo` is a module of the current crate root only from the root). Try the crate src first.
                if let Some(cargo) = self.nearest(from_dir, "Cargo.toml") {
                    let src = cargo.parent().unwrap_or(&self.root).join("src");
                    if let Some(hit) = rs_resolve_path(&src, &segs, &import.symbols) {
                        return hit;
                    }
                }
                if let Some(hit) = rs_resolve_path(&mod_dir, &segs, &import.symbols) {
                    return hit;
                }
                return Resolved::external("cargo", first.replace('_', "-"));
            }
        };
        if rest.is_empty() {
            // `use crate;`/`use self;`/`use super::{...}` handled via symbols
            for sym in &import.symbols {
                if let Some(p) = rs_module_file(&base, sym) {
                    return Resolved::File { path: p };
                }
            }
            return rs_module_file(
                base.parent().unwrap_or(&base),
                base.file_name().and_then(|s| s.to_str()).unwrap_or("mod"),
            )
            .or_else(|| rs_root_file(&base))
            .map(|p| Resolved::File { path: p })
            .unwrap_or_else(|| Resolved::Directory { path: base });
        }
        rs_resolve_path(&base, rest, &import.symbols)
            .or_else(|| {
                // `crate::Item` / `self::Item` / `super::Item`: an item defined or
                // re-exported by the module at `base` itself.
                if rest.len() != 1 {
                    return None;
                }
                rs_module_file(
                    base.parent().unwrap_or(&base),
                    base.file_name().and_then(|s| s.to_str()).unwrap_or("mod"),
                )
                .or_else(|| rs_root_file(&base))
                .map(|p| Resolved::File { path: p })
            })
            .unwrap_or_else(|| {
                Resolved::unresolved(format!("module path {} not found", import.specifier))
            })
    }

    // ------------------------------------------------------------ Go

    fn resolve_go(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let spec = import.specifier.as_str();
        let from_dir = from_file.parent().unwrap_or(&self.root);
        if let Some(gomod) = self.nearest(from_dir, "go.mod")
            && let Some(module) = go_module_path(&gomod)
            && (spec == module || spec.starts_with(&format!("{module}/")))
        {
            let rel = spec
                .strip_prefix(&module)
                .unwrap_or("")
                .trim_start_matches('/');
            let dir = gomod.parent().unwrap_or(&self.root).join(rel);
            return if dir.is_dir() {
                Resolved::Directory { path: dir }
            } else {
                Resolved::unresolved(format!("package dir {} not found", dir.display()))
            };
        }
        let first = spec.split('/').next().unwrap_or(spec);
        if !first.contains('.') {
            return Resolved::stdlib();
        }
        Resolved::external("go", spec)
    }

    // ------------------------------------------------------------ Java

    fn resolve_java(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let spec = import.specifier.as_str();
        if spec.starts_with("java.") || spec.starts_with("javax.") || spec.starts_with("jdk.") {
            return Resolved::stdlib();
        }
        let wildcard = import.symbols.iter().any(|s| s == "*");
        let from_dir = from_file.parent().unwrap_or(&self.root);
        let rel: PathBuf = spec.split('.').collect();
        for root in java_source_roots(from_dir, &self.root) {
            let candidate = root.join(&rel);
            if wildcard && candidate.is_dir() {
                return Resolved::Directory { path: candidate };
            }
            let file = candidate.with_extension("java");
            if file.is_file() {
                return Resolved::File { path: file };
            }
            // `import a.b.C.Inner` or static member imports: try the parent class file.
            if let Some(parent) = candidate.parent() {
                let pf = parent.with_extension("java");
                if pf.is_file() && candidate.parent() != Some(&root) {
                    return Resolved::File { path: pf };
                }
            }
        }
        let name = spec.split('.').take(2).collect::<Vec<_>>().join(".");
        Resolved::external("maven", name)
    }

    // ------------------------------------------------------------ PHP

    fn resolve_php(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let spec = import.specifier.as_str();
        let from_dir = from_file.parent().unwrap_or(&self.root);
        if import.kind == ImportKind::Include {
            let rel = spec.trim_start_matches('/');
            let candidate = from_dir.join(rel);
            return if candidate.is_file() {
                Resolved::File { path: candidate }
            } else {
                Resolved::unresolved(format!("include {spec} not found"))
            };
        }
        if let Some(composer) = self.nearest(from_dir, "composer.json") {
            let base = composer.parent().unwrap_or(&self.root);
            for (prefix, dirs) in composer_psr4(&composer) {
                let prefix = prefix.trim_end_matches('\\');
                let Some(rest) = spec.strip_prefix(prefix) else {
                    continue;
                };
                let rest = rest.trim_start_matches('\\');
                for dir in &dirs {
                    let rel: PathBuf = rest.split('\\').collect();
                    let file = base.join(dir).join(rel).with_extension("php");
                    if file.is_file() {
                        return Resolved::File { path: file };
                    }
                }
            }
        }
        let name = spec
            .split('\\')
            .take(2)
            .map(|s| s.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("/");
        Resolved::external("packagist", name)
    }

    // ------------------------------------------------------------ Ruby

    fn resolve_rb(&self, from_file: &Path, import: &ImportRef) -> Resolved {
        let spec = import.specifier.as_str();
        let from_dir = from_file.parent().unwrap_or(&self.root);
        let with_rb = |p: PathBuf| {
            if p.extension().is_some() {
                p
            } else {
                p.with_extension("rb")
            }
        };
        if import.kind == ImportKind::Include {
            let candidate = with_rb(from_dir.join(spec));
            return if candidate.is_file() {
                Resolved::File { path: candidate }
            } else {
                Resolved::unresolved(format!(
                    "{spec} not found relative to {}",
                    from_dir.display()
                ))
            };
        }
        if let Some(gemfile) = self.nearest(from_dir, "Gemfile") {
            let base = gemfile.parent().unwrap_or(&self.root);
            let lib = with_rb(base.join("lib").join(spec));
            if lib.is_file() {
                return Resolved::File { path: lib };
            }
            if let Some(hit) = find_under(&base.join("app"), &with_rb(PathBuf::from(spec)), 6) {
                return Resolved::File { path: hit };
            }
        }
        if RUBY_STDLIB.contains(&spec) {
            return Resolved::stdlib();
        }
        Resolved::external("rubygems", spec.split('/').next().unwrap_or(spec))
    }
}

// ---------------------------------------------------------------- helpers

/// If `path` lies inside a `node_modules` directory, the package name (`@scope/name` aware).
fn node_modules_package(path: &Path) -> Option<String> {
    let comps: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let idx = comps.iter().rposition(|c| c == "node_modules")?;
    let first = comps.get(idx + 1)?;
    if first.starts_with('@') {
        Some(format!("{first}/{}", comps.get(idx + 2)?))
    } else {
        Some(first.clone())
    }
}

/// Package name for a bare specifier (`react`, `@scope/pkg/sub` → `@scope/pkg`); None for relative/absolute/alias.
fn npm_package_name(spec: &str) -> Option<String> {
    if spec.starts_with('.') || spec.starts_with('/') || spec.starts_with('#') {
        return None;
    }
    let mut parts = spec.split('/');
    let first = parts.next()?;
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
    };
    if let Some(scope) = first.strip_prefix('@') {
        let name = parts.next()?;
        (valid(scope) && valid(name)).then(|| format!("@{scope}/{name}"))
    } else {
        valid(first).then(|| first.to_string())
    }
}

fn py_module_file(base: &Path, name: &str) -> Option<PathBuf> {
    let f = base.join(format!("{name}.py"));
    if f.is_file() {
        return Some(f);
    }
    let init = base.join(name).join("__init__.py");
    if init.is_file() {
        return Some(init);
    }
    None
}

/// Resolve `a.b.c` under `base`, allowing the last segment to be a symbol in `a/b.py`.
fn py_resolve_in(base: &Path, dotted: &str, symbols: &[String]) -> Option<Resolved> {
    let segs: Vec<&str> = dotted.split('.').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return None;
    }
    let dir = segs[..segs.len() - 1]
        .iter()
        .fold(base.to_path_buf(), |p, s| p.join(s));
    if !dir.exists() && segs.len() > 1 {
        return None;
    }
    let last = segs[segs.len() - 1];
    if let Some(p) = py_module_file(&dir, last) {
        // `from pkg import submodule` prefers the submodule file when it exists.
        if segs.len() == 1
            && symbols.len() == 1
            && p.file_name().and_then(|s| s.to_str()) == Some("__init__.py")
            && let Some(sub) = py_module_file(&dir.join(last), &symbols[0])
        {
            return Some(Resolved::File { path: sub });
        }
        return Some(Resolved::File { path: p });
    }
    // `a.b.c` where `c` is a symbol inside `a/b.py`
    if segs.len() > 1
        && let Some(parent_dir) = dir.parent()
        && let Some(p) = py_module_file(parent_dir, dir.file_name()?.to_str()?)
        && p.file_name().and_then(|s| s.to_str()) != Some("__init__.py")
    {
        return Some(Resolved::File { path: p });
    }
    None
}

fn rs_module_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let f = dir.join(format!("{name}.rs"));
    if f.is_file() {
        return Some(f);
    }
    let m = dir.join(name).join("mod.rs");
    if m.is_file() {
        return Some(m);
    }
    None
}

fn rs_root_file(dir: &Path) -> Option<PathBuf> {
    ["lib.rs", "main.rs", "mod.rs"]
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.is_file())
}

/// Resolve a `::`-path under `base`; trailing segments may be items rather than modules.
fn rs_resolve_path(base: &Path, segs: &[&str], symbols: &[String]) -> Option<Resolved> {
    if segs.is_empty() {
        return None;
    }
    // Longest module path first: a::b::c → src/a/b/c.rs, then src/a/b.rs (c is an item), ...
    for n in (1..=segs.len()).rev() {
        let dir = segs[..n - 1]
            .iter()
            .fold(base.to_path_buf(), |p, s| p.join(s));
        if let Some(p) = rs_module_file(&dir, segs[n - 1]) {
            // If the import lists symbols that are themselves child modules, prefer the deepest module.
            if n == segs.len() && symbols.len() == 1 {
                let child_dir = dir.join(segs[n - 1]);
                if let Some(c) = rs_module_file(&child_dir, &symbols[0]) {
                    return Some(Resolved::File { path: c });
                }
            }
            return Some(Resolved::File { path: p });
        }
    }
    None
}

fn go_module_path(gomod: &Path) -> Option<String> {
    let text = std::fs::read_to_string(gomod).ok()?;
    text.lines().map(str::trim).find_map(|l| {
        l.strip_prefix("module ")
            .map(|m| m.trim().trim_matches('"').to_string())
    })
}

fn java_source_roots(from_dir: &Path, root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut cur = Some(from_dir);
    while let Some(d) = cur {
        for sub in ["src/main/java", "src/test/java", "src"] {
            let p = d.join(sub);
            if p.is_dir() && !roots.contains(&p) {
                roots.push(p);
            }
        }
        if d == root {
            break;
        }
        cur = d.parent();
    }
    // Also allow the file's own ancestors named like source roots.
    let mut cur = Some(from_dir);
    while let Some(d) = cur {
        if (d.ends_with("java") || d.ends_with("src")) && !roots.contains(&d.to_path_buf()) {
            roots.push(d.to_path_buf());
        }
        if d == root {
            break;
        }
        cur = d.parent();
    }
    roots
}

fn composer_psr4(composer: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(text) = std::fs::read_to_string(composer) else {
        return vec![];
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return vec![];
    };
    let mut out = Vec::new();
    for section in ["autoload", "autoload-dev"] {
        if let Some(map) = json
            .get(section)
            .and_then(|a| a.get("psr-4"))
            .and_then(|m| m.as_object())
        {
            for (prefix, dirs) in map {
                let dirs: Vec<String> = match dirs {
                    serde_json::Value::String(s) => vec![s.clone()],
                    serde_json::Value::Array(a) => a
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                    _ => vec![],
                };
                out.push((prefix.clone(), dirs));
            }
        }
    }
    // Longest prefix first so `App\Models\` beats `App\`.
    out.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    out
}

/// Breadth-limited search for a relative file path below `base`.
fn find_under(base: &Path, rel: &Path, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 || !base.is_dir() {
        return None;
    }
    let direct = base.join(rel);
    if direct.is_file() {
        return Some(direct);
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    entries
        .into_iter()
        .find_map(|d| find_under(&d, rel, max_depth - 1))
}

const NODE_BUILTINS: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "constants",
    "crypto",
    "dgram",
    "dns",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "worker_threads",
    "zlib",
];

const PY_STDLIB: &[&str] = &[
    "os",
    "sys",
    "re",
    "json",
    "typing",
    "pathlib",
    "collections",
    "itertools",
    "functools",
    "datetime",
    "math",
    "time",
    "subprocess",
    "logging",
    "unittest",
    "dataclasses",
    "enum",
    "abc",
    "io",
    "asyncio",
    "http",
    "urllib",
    "random",
    "string",
    "shutil",
    "tempfile",
    "glob",
    "csv",
    "hashlib",
    "uuid",
    "copy",
    "argparse",
    "contextlib",
    "threading",
    "multiprocessing",
    "queue",
    "socket",
    "struct",
    "textwrap",
    "traceback",
    "types",
    "warnings",
    "weakref",
    "base64",
    "pickle",
    "decimal",
    "fractions",
    "statistics",
    "secrets",
    "inspect",
    "importlib",
    "operator",
    "pprint",
    "signal",
    "sqlite3",
    "zlib",
    "gzip",
    "tarfile",
    "zipfile",
    "xml",
    "html",
    "email",
    "unicodedata",
    "bisect",
    "heapq",
    "array",
    "numbers",
    "cmath",
    "ast",
    "dis",
    "gc",
    "platform",
    "sysconfig",
    "site",
    "builtins",
    "__future__",
];

const RUBY_STDLIB: &[&str] = &[
    "json",
    "set",
    "time",
    "date",
    "fileutils",
    "pathname",
    "securerandom",
    "digest",
    "net/http",
    "uri",
    "yaml",
    "erb",
    "logger",
    "optparse",
    "open3",
    "tempfile",
    "tmpdir",
    "csv",
    "base64",
    "openssl",
    "socket",
    "pp",
    "stringio",
    "forwardable",
    "singleton",
    "observer",
    "ostruct",
    "benchmark",
    "English",
    "shellwords",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_names() {
        assert_eq!(npm_package_name("react").as_deref(), Some("react"));
        assert_eq!(
            npm_package_name("@scope/pkg/sub").as_deref(),
            Some("@scope/pkg")
        );
        assert_eq!(npm_package_name("@/x"), None);
        assert_eq!(npm_package_name("./x"), None);
        assert_eq!(
            node_modules_package(Path::new("/a/node_modules/@s/p/index.js")).as_deref(),
            Some("@s/p")
        );
        assert_eq!(node_modules_package(Path::new("/a/src/x.ts")), None);
    }
}
