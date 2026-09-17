//! Spore loading and harvesting. First-party spores are embedded in the binary;
//! workspace spores live in `.aneural/spores/<name>/spore.json`.

pub mod http;
pub mod markdown;
pub mod sqlite;
pub mod template;

use aneural_core::config::{NodeTypeDef, SporesConfig};
use aneural_core::kinds::Source;
use aneural_core::spore::{Emit, Harvester, MarkdownGranularity, SporeInfo, SporeManifest};
use aneural_core::{Edge, Node, NodeId, Workspace};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use template::{Vars, coerce, render};

/// (name, manifest json) for the spores shipped with Aneural.
pub const BUILTIN_SPORES: &[(&str, &str)] = &[
    (
        "comments",
        include_str!("../../../../spores/comments/spore.json"),
    ),
    ("plans", include_str!("../../../../spores/plans/spore.json")),
    (
        "icebox",
        include_str!("../../../../spores/icebox/spore.json"),
    ),
    (
        "wiki-links",
        include_str!("../../../../spores/wiki-links/spore.json"),
    ),
    // Off by default: it walks every `.db` in the workspace, which is worth
    // opting into rather than assuming.
    (
        "database",
        include_str!("../../../../spores/database/spore.json"),
    ),
    // Off by default, and the only shipped spore that needs consent: it makes
    // web requests with the user's own GitHub token.
    (
        "github",
        include_str!("../../../../spores/github/spore.json"),
    ),
];

#[derive(Debug, thiserror::Error)]
pub enum SporeError {
    #[error("{name}: {}", problems.join("; "))]
    Invalid { name: String, problems: Vec<String> },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// A loaded, compiled spore.
pub struct Spore {
    pub manifest: SporeManifest,
    pub location: String,
    pub path: String,
    pub enabled: bool,
    harvesters: Vec<CompiledHarvester>,
}

struct CompiledHarvester {
    def: Harvester,
    include: GlobSet,
    exclude: GlobSet,
    regex: Option<Regex>,
}

impl Spore {
    /// A summary for the CLI, the GUI and MCP. `settings` is the workspace's
    /// answers, so the caller learns which required ones are still blank.
    pub fn info_with(&self, settings: &BTreeMap<String, String>) -> SporeInfo {
        let mut info = self.info();
        info.settings = self
            .manifest
            .settings
            .iter()
            .map(|d| aneural_core::spore::SettingValue {
                key: d.key.clone(),
                label: if d.label.is_empty() {
                    d.key.clone()
                } else {
                    d.label.clone()
                },
                description: d.description.clone(),
                example: d.example.clone(),
                required: d.required,
                value: settings
                    .get(&d.key)
                    .filter(|v| !v.trim().is_empty())
                    .cloned(),
            })
            .collect();
        info.missing_settings = info
            .settings
            .iter()
            .filter(|s| s.required && s.value.is_none())
            .map(|s| s.key.clone())
            .collect();
        info
    }

    pub fn info(&self) -> SporeInfo {
        SporeInfo {
            id: self.manifest.id(),
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            display_name: if self.manifest.display_name.is_empty() {
                self.manifest.name.clone()
            } else {
                self.manifest.display_name.clone()
            },
            description: self.manifest.description.clone(),
            enabled: self.enabled,
            location: self.location.clone(),
            path: self.path.clone(),
            node_kinds: self
                .manifest
                .node_types
                .iter()
                .map(|n| n.kind.clone())
                .collect(),
            tier: self.manifest.tier().label().to_string(),
            consent_lines: self
                .manifest
                .capabilities
                .iter()
                .map(|c| c.consent_line())
                .collect(),
            missing_settings: Vec::new(),
            settings: Vec::new(),
        }
    }

    pub fn node_types(&self) -> Vec<NodeTypeDef> {
        self.manifest
            .node_types
            .iter()
            .cloned()
            .map(|mut d| {
                d.provider = Source::spore(&self.manifest.id());
                if d.label.is_empty() {
                    d.label = d.kind.clone();
                }
                d
            })
            .collect()
    }

    /// This spore's HTTP harvesters. They are not reachable through the
    /// file-driven path at all, so the refresh loop asks for them by name.
    pub fn http_harvesters(&self) -> impl Iterator<Item = &Harvester> {
        self.harvesters
            .iter()
            .map(|h| &h.def)
            .filter(|d| matches!(d, Harvester::Http { .. }))
    }

    /// Whether any harvester of this spore applies to the given path.
    pub fn applies_to(&self, rel: &str) -> bool {
        self.harvesters.iter().any(|h| h.matches(rel))
    }
}

impl CompiledHarvester {
    fn matches(&self, rel: &str) -> bool {
        // An HTTP harvester declares no `include`, and an empty include set
        // means "every file" for the file-driven kinds — so it has to be ruled
        // out explicitly or it would claim every path in the workspace.
        if matches!(self.def, Harvester::Http { .. }) {
            return false;
        }
        let inc = self.include.is_empty() || self.include.is_match(rel);
        inc && !self.exclude.is_match(rel)
    }
}

/// `compile` needs a `&Vec<String>` to hand the glob builder for the kinds that
/// declare no globs at all.
static EMPTY: Vec<String> = Vec::new();

/// Compile a manifest; returns human-readable errors for bad globs/regexes.
pub fn compile(
    manifest: SporeManifest,
    location: &str,
    path: &str,
    enabled: bool,
) -> Result<Spore, SporeError> {
    let mut errs = manifest.validate();
    let mut harvesters = Vec::new();
    for h in &manifest.harvesters {
        let (include, exclude, pattern) = match h {
            Harvester::Regex {
                include,
                exclude,
                pattern,
                ..
            } => (include, exclude, Some(pattern)),
            Harvester::TreeSitter {
                include, exclude, ..
            }
            | Harvester::Markdown {
                include, exclude, ..
            }
            | Harvester::Sqlite {
                include, exclude, ..
            } => (include, exclude, None),
            // An HTTP harvester has no file behind it, so it has no globs. It
            // is still compiled in, because the refresh loop has to find it.
            Harvester::Http { .. } => (&EMPTY, &EMPTY, None),
            Harvester::Wasm { .. } => continue,
        };
        let include = match globset(include) {
            Ok(g) => g,
            Err(e) => {
                errs.push(format!("harvester `{}`: bad include glob: {e}", h.id()));
                continue;
            }
        };
        let exclude = match globset(exclude) {
            Ok(g) => g,
            Err(e) => {
                errs.push(format!("harvester `{}`: bad exclude glob: {e}", h.id()));
                continue;
            }
        };
        let regex = match pattern {
            Some(p) => match Regex::new(p) {
                Ok(r) => Some(r),
                Err(e) => {
                    errs.push(format!("harvester `{}`: bad regex: {e}", h.id()));
                    continue;
                }
            },
            None => None,
        };
        harvesters.push(CompiledHarvester {
            def: h.clone(),
            include,
            exclude,
            regex,
        });
    }
    if !errs.is_empty() {
        return Err(SporeError::Invalid {
            name: manifest.name.clone(),
            problems: errs,
        });
    }
    Ok(Spore {
        manifest,
        location: location.into(),
        path: path.into(),
        enabled,
        harvesters,
    })
}

/// Every problem with a manifest, not just the first. `compile` collapses them
/// into one error for logging; callers that show a user a list want them apart.
pub fn compile_report(manifest: SporeManifest, path: &str) -> Vec<String> {
    match compile(manifest, "check", path, true) {
        Ok(_) => Vec::new(),
        Err(SporeError::Invalid { problems, .. }) => problems,
        Err(e) => vec![e.to_string()],
    }
}

pub fn globset(patterns: &[String]) -> Result<GlobSet, globset::Error> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p)?);
    }
    b.build()
}

/// Load builtin + workspace spores. Invalid workspace spores are returned as errors, not fatal.
pub fn load_all(ws: &Workspace, enabled: &[String]) -> (Vec<Spore>, Vec<SporeError>) {
    let mut spores = Vec::new();
    let mut errors = Vec::new();
    for (name, json) in BUILTIN_SPORES {
        let manifest: SporeManifest = serde_json::from_str(json).expect("builtin spore json");
        let on = enabled
            .iter()
            .any(|e| SporesConfig::entry_matches(e, &manifest.id(), name));
        match compile(
            manifest,
            "builtin",
            &format!("spores/{name}/spore.json"),
            on,
        ) {
            Ok(s) => spores.push(s),
            Err(e) => errors.push(e),
        }
    }
    if let Ok(rd) = std::fs::read_dir(ws.spores_dir()) {
        let mut dirs: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let mf = dir.join("spore.json");
            if !mf.exists() {
                continue;
            }
            let rel = ws.rel(&mf).unwrap_or_else(|| mf.display().to_string());
            let text = match std::fs::read_to_string(&mf) {
                Ok(t) => t,
                Err(e) => {
                    errors.push(SporeError::Io(e));
                    continue;
                }
            };
            let manifest: SporeManifest = match serde_json::from_str(&text) {
                Ok(m) => m,
                Err(e) => {
                    errors.push(SporeError::Invalid {
                        name: rel.clone(),
                        problems: vec![e.to_string()],
                    });
                    continue;
                }
            };
            let on = enabled
                .iter()
                .any(|e| SporesConfig::entry_matches(e, &manifest.id(), &manifest.name));
            match compile(manifest, "workspace", &rel, on) {
                Ok(s) => {
                    // A workspace spore shadows a builtin only when it is the
                    // same spore: `bob.comments` must not displace
                    // `aneural.comments` just by sharing a name.
                    let id = s.manifest.id();
                    spores.retain(|b: &Spore| b.manifest.id() != id);
                    spores.push(s);
                }
                Err(e) => errors.push(e),
            }
        }
    }
    (spores, errors)
}

/// Resolves `[[Wiki Link]]` names to workspace-relative markdown paths.
#[derive(Default, Clone)]
pub struct MarkdownIndex {
    by_stem: HashMap<String, Vec<String>>,
}

impl MarkdownIndex {
    pub fn insert(&mut self, rel: &str) {
        if let Some(stem) = stem_of(rel) {
            let v = self.by_stem.entry(stem).or_default();
            if !v.iter().any(|p| p == rel) {
                v.push(rel.to_string());
                v.sort();
            }
        }
    }

    pub fn remove(&mut self, rel: &str) {
        if let Some(stem) = stem_of(rel)
            && let Some(v) = self.by_stem.get_mut(&stem)
        {
            v.retain(|p| p != rel);
        }
    }

    /// Prefer a file in the same directory as `from`, then the shortest path.
    pub fn resolve(&self, target: &str, from: &str) -> Option<String> {
        let key = target.trim().trim_end_matches(".md").to_lowercase();
        let key = key.rsplit('/').next().unwrap_or(&key).to_string();
        let candidates = self.by_stem.get(&key)?;
        let from_dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        candidates
            .iter()
            .find(|c| c.rsplit_once('/').map(|(d, _)| d).unwrap_or("") == from_dir)
            .or_else(|| candidates.iter().min_by_key(|c| c.len()))
            .cloned()
    }
}

fn stem_of(rel: &str) -> Option<String> {
    let base = rel.rsplit('/').next()?;
    let (stem, ext) = base.rsplit_once('.')?;
    if !matches!(ext, "md" | "mdx" | "markdown") {
        return None;
    }
    Some(stem.to_lowercase())
}

/// Whether a harvester opens the file itself rather than being handed its bytes.
/// These run even for files too large for the engine to read.
pub fn opens_its_own_file(h: &Harvester) -> bool {
    matches!(h, Harvester::Sqlite { .. })
}

/// Run only the harvesters that open the file themselves. Used for files past
/// `walk::MAX_PARSE_BYTES`, where a dev database routinely lands.
pub fn harvest_large(spores: &[Spore], rel: &str, abs: &std::path::Path) -> Harvest {
    let mut out = Harvest::default();
    for spore in spores.iter().filter(|s| s.enabled) {
        let src = Source::spore(&spore.manifest.id());
        for h in spore
            .harvesters
            .iter()
            .filter(|h| opens_its_own_file(&h.def) && h.matches(rel))
        {
            if let Harvester::Sqlite {
                emit,
                references,
                sample_rows,
                ..
            } = &h.def
            {
                sqlite::harvest(
                    abs,
                    rel,
                    emit,
                    references.as_ref(),
                    *sample_rows,
                    &src,
                    &mut out,
                );
            }
        }
    }
    dedupe(&mut out);
    out
}

/// Whether any enabled spore would harvest this path without reading it.
pub fn has_large_file_harvester(spores: &[Spore], rel: &str) -> bool {
    spores.iter().filter(|s| s.enabled).any(|s| {
        s.harvesters
            .iter()
            .any(|h| opens_its_own_file(&h.def) && h.matches(rel))
    })
}

/// Output of harvesting one file with one or more spores.
#[derive(Default, Debug)]
pub struct Harvest {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Run every enabled harvester of every enabled spore over one file.
pub fn harvest_file(
    spores: &[Spore],
    md_index: &MarkdownIndex,
    rel: &str,
    abs: &std::path::Path,
    source: &[u8],
) -> Harvest {
    let mut out = Harvest::default();
    let text = String::from_utf8_lossy(source);
    for spore in spores.iter().filter(|s| s.enabled) {
        let src = Source::spore(&spore.manifest.id());
        for h in spore.harvesters.iter().filter(|h| h.matches(rel)) {
            match &h.def {
                Harvester::Regex { emit, .. } => {
                    let re = h.regex.as_ref().expect("compiled regex");
                    harvest_regex(re, emit, &src, rel, &text, &mut out);
                }
                Harvester::TreeSitter {
                    language,
                    query,
                    emit,
                    ..
                } => {
                    harvest_tree_sitter(language, query, emit, &src, rel, source, &mut out);
                }
                Harvester::Markdown {
                    granularity,
                    emit,
                    wikilinks,
                    annotate,
                    ..
                } => {
                    harvest_markdown(
                        *granularity,
                        emit.as_ref(),
                        wikilinks.as_ref(),
                        annotate.as_ref(),
                        &src,
                        rel,
                        &text,
                        md_index,
                        &mut out,
                    );
                }
                Harvester::Sqlite {
                    emit,
                    references,
                    sample_rows,
                    ..
                } => {
                    sqlite::harvest(
                        abs,
                        rel,
                        emit,
                        references.as_ref(),
                        *sample_rows,
                        &src,
                        &mut out,
                    );
                }
                // Driven by the refresh loop, not by the walker.
                Harvester::Http { .. } | Harvester::Wasm { .. } => {}
            }
        }
    }
    dedupe(&mut out);
    out
}

fn dedupe(h: &mut Harvest) {
    let mut seen = std::collections::HashSet::new();
    h.nodes.retain(|n| seen.insert(n.id.clone()));
    let mut seen_e = std::collections::HashSet::new();
    h.edges.retain(|e| {
        seen_e.insert((
            e.kind.clone(),
            e.src.clone(),
            e.dst.clone(),
            e.source.clone(),
        ))
    });
}

pub(crate) fn base_vars(rel: &str) -> Vars {
    let mut v = Vars::new();
    v.insert("file".into(), rel.to_string());
    v.insert(
        "basename".into(),
        rel.rsplit('/').next().unwrap_or(rel).to_string(),
    );
    v
}

pub(crate) fn emit_node(
    emit: &Emit,
    source: &str,
    rel: &str,
    vars: &Vars,
    out: &mut Harvest,
) -> Option<NodeId> {
    let id_str = render(&emit.node.id, vars);
    let id = NodeId::parse(&id_str).ok()?;
    let mut node = Node::new(
        id.clone(),
        emit.node.kind.clone(),
        render(&emit.node.label, vars).trim().to_string(),
        source,
    )
    .with_origin(rel);
    if let serde_json::Value::Object(map) = &mut node.props {
        for (k, tpl) in &emit.node.props {
            let v = render(tpl, vars);
            if !v.is_empty() {
                map.insert(k.clone(), coerce(&v));
            }
        }
    }
    if node.label.is_empty() {
        node.label = id.fragment().unwrap_or(id.path_part()).to_string();
    }
    out.nodes.push(node);
    for e in &emit.edges {
        let src = if e.src == "$node" {
            id.clone()
        } else {
            NodeId::new(render(&e.src, vars))
        };
        let dst = if e.dst == "$node" {
            id.clone()
        } else {
            NodeId::new(render(&e.dst, vars))
        };
        if NodeId::parse(src.as_str()).is_err() || NodeId::parse(dst.as_str()).is_err() {
            continue;
        }
        let mut edge = Edge::new(e.kind.clone(), src, dst, source).with_origin(rel);
        if let serde_json::Value::Object(map) = &mut edge.props {
            for (k, tpl) in &e.props {
                let v = render(tpl, vars);
                if !v.is_empty() {
                    map.insert(k.clone(), coerce(&v));
                }
            }
        }
        out.edges.push(edge);
    }
    Some(id)
}

fn harvest_regex(re: &Regex, emit: &Emit, source: &str, rel: &str, text: &str, out: &mut Harvest) {
    for (i, line) in text.lines().enumerate() {
        if let Some(caps) = re.captures(line) {
            let mut vars = base_vars(rel);
            vars.insert("line".into(), (i + 1).to_string());
            vars.insert(
                "match".into(),
                caps.get(0)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
            );
            for name in re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    vars.insert(name.to_string(), m.as_str().trim().to_string());
                }
            }
            emit_node(emit, source, rel, &vars, out);
        }
    }
}

fn harvest_tree_sitter(
    language: &str,
    query: &str,
    emit: &Emit,
    source: &str,
    rel: &str,
    bytes: &[u8],
    out: &mut Harvest,
) {
    let path = Path::new(rel);
    let Ok(matches) = aneural_lang::run_query(language, Some(path), query, bytes) else {
        return;
    };
    for caps in matches {
        let mut vars = base_vars(rel);
        if let Some(first) = caps.first() {
            vars.insert("line".into(), first.line.to_string());
        }
        for c in caps {
            vars.insert(c.name.clone(), c.text.trim().to_string());
        }
        emit_node(emit, source, rel, &vars, out);
    }
}

#[allow(clippy::too_many_arguments)]
fn harvest_markdown(
    granularity: MarkdownGranularity,
    emit: Option<&Emit>,
    wikilinks: Option<&aneural_core::spore::WikiLinks>,
    annotate: Option<&aneural_core::spore::Annotate>,
    source: &str,
    rel: &str,
    text: &str,
    md_index: &MarkdownIndex,
    out: &mut Harvest,
) {
    let fallback = rel
        .rsplit('/')
        .next()
        .unwrap_or(rel)
        .trim_end_matches(".md")
        .to_string();
    let doc = markdown::parse(text, &fallback);
    let mut vars = base_vars(rel);
    vars.insert("title".into(), doc.title.clone());
    vars.insert("body".into(), doc.body.trim().to_string());
    vars.insert("line".into(), (doc.body_offset + 1).to_string());
    for (k, v) in &doc.frontmatter.scalars {
        vars.insert(format!("fm.{k}"), v.clone());
    }
    for (k, v) in &doc.frontmatter.lists {
        vars.insert(format!("fm.{k}"), v.join(", "));
    }
    let file_id = NodeId::file(rel);

    let link_edge = |from: &NodeId, target: &str, line: u32, out: &mut Harvest| {
        let Some(wl) = wikilinks else { return };
        if let Some(dst_rel) = md_index.resolve(target, rel) {
            let dst = NodeId::file(&dst_rel);
            if &dst == from {
                return;
            }
            out.edges.push(
                Edge::new(wl.edge_kind.clone(), from.clone(), dst, source)
                    .with_origin(rel)
                    .with_prop("via", "wikilink")
                    .with_prop("line", line as i64),
            );
        }
    };

    match granularity {
        MarkdownGranularity::Document => {
            let anchor = match emit {
                Some(e) => emit_node(e, source, rel, &vars, out).unwrap_or_else(|| file_id.clone()),
                None => file_id.clone(),
            };
            for l in &doc.links {
                link_edge(&anchor, &l.target, l.line, out);
            }
            if let (Some(a), Some(_)) = (annotate, emit)
                && let Some(targets) = doc.frontmatter.lists.get(&a.frontmatter_key)
            {
                for t in targets {
                    let dst = NodeId::file(t.trim_start_matches("./"));
                    out.edges.push(
                        Edge::new(a.edge_kind.clone(), anchor.clone(), dst, source)
                            .with_origin(rel)
                            .with_prop("via", "frontmatter"),
                    );
                }
            }
        }
        MarkdownGranularity::Heading => {
            let Some(e) = emit else { return };
            let mut names: BTreeMap<String, usize> = BTreeMap::new();
            for (i, section) in doc.sections.iter().enumerate() {
                let mut v = vars.clone();
                let mut heading = section.heading.clone();
                // disambiguate duplicate headings
                let n = names.entry(aneural_core::slug(&heading)).or_insert(0);
                *n += 1;
                if *n > 1 {
                    heading = format!("{heading} ({n})");
                }
                v.insert("heading".into(), heading);
                v.insert("line".into(), section.line.to_string());
                v.insert("body".into(), section.body.clone());
                if let Some(id) = emit_node(e, source, rel, &v, out) {
                    let next = doc.sections.get(i + 1).map(|s| s.line);
                    for l in markdown::links_in(&doc, section, next) {
                        link_edge(&id, &l.target, l.line, out);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a workspace spore and load everything with the given `enabled` list.
    fn load_with(enabled: &[&str], extra: &[(&str, &str)]) -> (Vec<Spore>, Vec<SporeError>) {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::at(tmp.path());
        ws.init(Some("t"), false).unwrap();
        for (dir, json) in extra {
            let d = ws.spores_dir().join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("spore.json"), json).unwrap();
        }
        let enabled: Vec<String> = enabled.iter().map(|s| s.to_string()).collect();
        load_all(&ws, &enabled)
    }

    fn is_on(spores: &[Spore], id: &str) -> bool {
        spores.iter().any(|s| s.manifest.id() == id && s.enabled)
    }

    #[test]
    fn builtins_are_first_party_and_enable_by_bare_or_qualified_name() {
        let (spores, errs) = load_with(&["comments", "aneural.plans"], &[]);
        assert!(errs.is_empty(), "{errs:?}");

        // The bare name is what every existing workspace has on disk.
        assert!(is_on(&spores, "aneural.comments"));
        // The qualified id is what new workspaces write.
        assert!(is_on(&spores, "aneural.plans"));
        assert!(!is_on(&spores, "aneural.icebox"));
    }

    #[test]
    fn a_third_party_spore_cannot_shadow_a_builtin_by_name() {
        // Same `name`, different publisher: it must load *alongside*
        // `aneural.comments`, not replace it.
        let impostor = r##"{
          "publisher": "bob", "name": "comments", "version": "0.1.0",
          "displayName": "Not the real one", "description": "d",
          "nodeTypes": [{ "kind": "BobComment", "icon": "LuCircleDot", "color": "#fff" }],
          "harvesters": [{
            "id": "h", "kind": "markdown", "include": ["**/*.md"],
            "emit": { "node": { "kind": "BobComment", "id": "bob.comments.item:{file}", "label": "{title}" }, "edges": [] }
          }]
        }"##;
        let (spores, errs) =
            load_with(&["comments", "bob.comments"], &[("bob-comments", impostor)]);
        assert!(errs.is_empty(), "{errs:?}");

        assert!(is_on(&spores, "aneural.comments"), "the builtin survives");
        assert!(is_on(&spores, "bob.comments"), "and the impostor loads too");
    }

    #[test]
    fn a_workspace_spore_still_shadows_the_builtin_it_replaces() {
        let fork = r##"{
          "publisher": "aneural", "name": "comments", "version": "9.9.9",
          "displayName": "Local fork", "description": "d",
          "nodeTypes": [{ "kind": "Comment", "icon": "LuCircleDot", "color": "#fff" }],
          "harvesters": [{
            "id": "h", "kind": "markdown", "include": ["**/*.md"],
            "emit": { "node": { "kind": "Comment", "id": "comment:{file}", "label": "{title}" }, "edges": [] }
          }]
        }"##;
        let (spores, errs) = load_with(&["aneural.comments"], &[("comments", fork)]);
        assert!(errs.is_empty(), "{errs:?}");

        let loaded: Vec<&Spore> = spores
            .iter()
            .filter(|s| s.manifest.id() == "aneural.comments")
            .collect();
        assert_eq!(loaded.len(), 1, "exactly one wins");
        assert_eq!(loaded[0].manifest.version, "9.9.9");
        assert_eq!(loaded[0].location, "workspace");
    }

    fn builtins() -> Vec<Spore> {
        BUILTIN_SPORES
            .iter()
            .map(|(name, json)| {
                compile(serde_json::from_str(json).unwrap(), "builtin", name, true).unwrap()
            })
            .collect()
    }

    #[test]
    fn builtin_spores_compile() {
        let s = builtins();
        assert_eq!(s.len(), BUILTIN_SPORES.len());
        assert!(s.iter().any(|s| s.manifest.name == "comments"));
        assert!(s.iter().all(|s| s.manifest.is_first_party()));
    }

    #[test]
    fn comments_harvest() {
        let spores = builtins();
        let src = b"const a = 1; // TODO: make it two\n# not code\n/* FIXME slow */\n// claude: keep this\n";
        let h = harvest_file(
            &spores,
            &MarkdownIndex::default(),
            "src/a.ts",
            std::path::Path::new("src/a.ts"),
            src,
        );
        let labels: Vec<_> = h.nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, vec!["make it two", "slow", "keep this"]);
        assert_eq!(h.nodes[0].props["tag"], "TODO");
        assert_eq!(h.nodes[0].props["line"], 1);
        assert_eq!(h.nodes[2].props["tag"], "CLAUDE");
        assert_eq!(h.edges.len(), 3);
        assert_eq!(h.edges[0].dst, NodeId::file("src/a.ts"));
        assert_eq!(h.nodes[0].origin.as_deref(), Some("src/a.ts"));
    }

    #[test]
    fn plans_and_icebox_and_links() {
        let spores = builtins();
        let mut idx = MarkdownIndex::default();
        idx.insert("README.md");
        idx.insert(".aneural/notes/Architecture.md");
        idx.insert(".aneural/plans/p.md");
        idx.insert(".aneural/icebox/ideas.md");

        let plan = b"---\ntitle: Plan A\nstatus: open\ntargets:\n  - apps/web/src/index.ts\n---\nSee [[Architecture]]\n";
        let h = harvest_file(
            &spores,
            &idx,
            ".aneural/plans/p.md",
            std::path::Path::new(".aneural/plans/p.md"),
            plan,
        );
        let plan_node = h.nodes.iter().find(|n| n.kind == "Plan").unwrap();
        assert_eq!(plan_node.id, NodeId::new("plan:.aneural/plans/p.md"));
        assert_eq!(plan_node.label, "Plan A");
        assert_eq!(plan_node.props["status"], "open");
        assert!(
            h.edges
                .iter()
                .any(|e| e.kind == "ANNOTATES" && e.dst == NodeId::file("apps/web/src/index.ts"))
        );
        assert!(h.edges.iter().any(|e| e.kind == "RELATES_TO"
            && e.src == plan_node.id
            && e.dst == NodeId::file(".aneural/notes/Architecture.md")));
        // the generic links harvester also links the plan *file* to Architecture
        assert!(
            h.edges
                .iter()
                .any(|e| e.src == NodeId::file(".aneural/plans/p.md")
                    && e.dst == NodeId::file(".aneural/notes/Architecture.md"))
        );

        let ice =
            b"# Icebox\n\n## Replace router\nSee [[README]].\n\n## Rate limit\n\n## Rate limit\n";
        let h = harvest_file(
            &spores,
            &idx,
            ".aneural/icebox/ideas.md",
            std::path::Path::new(".aneural/icebox/ideas.md"),
            ice,
        );
        let ideas: Vec<_> = h.nodes.iter().filter(|n| n.kind == "Idea").collect();
        assert_eq!(ideas.len(), 3);
        assert_eq!(
            ideas[0].id,
            NodeId::new("idea:.aneural/icebox/ideas.md#replace-router")
        );
        assert_eq!(ideas[2].label, "Rate limit (2)");
        assert!(h.edges.iter().any(|e| e.src == ideas[0].id
            && e.dst == NodeId::file("README.md")
            && e.props["via"] == "wikilink"));

        let note = b"# Architecture\n\nBack to [[README]] and [[Missing]].\n";
        let h = harvest_file(
            &spores,
            &idx,
            ".aneural/notes/Architecture.md",
            std::path::Path::new(".aneural/notes/Architecture.md"),
            note,
        );
        let n = h.nodes.iter().find(|n| n.kind == "Note").unwrap();
        assert_eq!(n.label, "Architecture");
        assert_eq!(
            h.edges
                .iter()
                .filter(|e| e.props["via"] == "wikilink")
                .count(),
            1
        );
    }

    #[test]
    fn the_database_spore_harvests_a_real_sqlite_file() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("app.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);
             CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id));",
        )
        .unwrap();
        drop(conn);

        let spores = builtins();
        let h = harvest_file(&spores, &MarkdownIndex::default(), "app.db", &db, b"");

        let tables: Vec<&str> = h
            .nodes
            .iter()
            .filter(|n| n.kind == "Table")
            .map(|n| n.label.as_str())
            .collect();
        assert_eq!(tables, vec!["posts", "users"]);

        let posts = h.nodes.iter().find(|n| n.label == "posts").unwrap();
        assert_eq!(posts.props["columns"], "id, user_id");
        assert_eq!(posts.props["rowCount"], 0);
        assert!(posts.props.get("sample").is_none(), "rows stay opt-in");
        assert_eq!(posts.origin.as_deref(), Some("app.db"));

        assert!(h.edges.iter().any(|e| e.kind == "REFERENCES"
            && e.src == NodeId::new("aneural.database.table:app.db#posts")
            && e.dst == NodeId::new("aneural.database.table:app.db#users")));
    }

    #[test]
    fn markdown_index_prefers_same_dir() {
        let mut idx = MarkdownIndex::default();
        idx.insert("docs/README.md");
        idx.insert("README.md");
        assert_eq!(
            idx.resolve("readme", "docs/x.md").as_deref(),
            Some("docs/README.md")
        );
        assert_eq!(
            idx.resolve("README", "src/x.md").as_deref(),
            Some("README.md")
        );
        idx.remove("README.md");
        assert_eq!(
            idx.resolve("README", "src/x.md").as_deref(),
            Some("docs/README.md")
        );
    }
}
