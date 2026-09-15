//! Spore loading and harvesting. First-party spores are embedded in the binary;
//! workspace spores live in `.aneural/spores/<name>/spore.json`.

pub mod markdown;
pub mod template;

use aneural_core::config::NodeTypeDef;
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
];

#[derive(Debug, thiserror::Error)]
pub enum SporeError {
    #[error("{name}: {message}")]
    Invalid { name: String, message: String },
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
    pub fn info(&self) -> SporeInfo {
        SporeInfo {
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
        }
    }

    pub fn node_types(&self) -> Vec<NodeTypeDef> {
        self.manifest
            .node_types
            .iter()
            .cloned()
            .map(|mut d| {
                d.provider = Source::spore(&self.manifest.name);
                if d.label.is_empty() {
                    d.label = d.kind.clone();
                }
                d
            })
            .collect()
    }

    /// Whether any harvester of this spore applies to the given path.
    pub fn applies_to(&self, rel: &str) -> bool {
        self.harvesters.iter().any(|h| h.matches(rel))
    }
}

impl CompiledHarvester {
    fn matches(&self, rel: &str) -> bool {
        let inc = self.include.is_empty() || self.include.is_match(rel);
        inc && !self.exclude.is_match(rel)
    }
}

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
            } => (include, exclude, None),
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
            message: errs.join("; "),
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
        let on = enabled.iter().any(|e| e == name);
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
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let on = enabled.iter().any(|e| e == &manifest.name);
            match compile(manifest, "workspace", &rel, on) {
                Ok(s) => {
                    // workspace spores shadow builtins of the same name
                    spores.retain(|b: &Spore| b.manifest.name != s.manifest.name);
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
    source: &[u8],
) -> Harvest {
    let mut out = Harvest::default();
    let text = String::from_utf8_lossy(source);
    for spore in spores.iter().filter(|s| s.enabled) {
        let src = Source::spore(&spore.manifest.name);
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
                Harvester::Wasm { .. } => {}
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

fn base_vars(rel: &str) -> Vars {
    let mut v = Vars::new();
    v.insert("file".into(), rel.to_string());
    v.insert(
        "basename".into(),
        rel.rsplit('/').next().unwrap_or(rel).to_string(),
    );
    v
}

fn emit_node(
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
        assert_eq!(s.len(), 4);
        assert!(s.iter().any(|s| s.manifest.name == "comments"));
    }

    #[test]
    fn comments_harvest() {
        let spores = builtins();
        let src = b"const a = 1; // TODO: make it two\n# not code\n/* FIXME slow */\n// claude: keep this\n";
        let h = harvest_file(&spores, &MarkdownIndex::default(), "src/a.ts", src);
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
        let h = harvest_file(&spores, &idx, ".aneural/plans/p.md", plan);
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
        let h = harvest_file(&spores, &idx, ".aneural/icebox/ideas.md", ice);
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
        let h = harvest_file(&spores, &idx, ".aneural/notes/Architecture.md", note);
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
