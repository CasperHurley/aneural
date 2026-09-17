//! `@aneural/core`: Node.js bindings over the Aneural engine and store.
//!
//! Every function opens the workspace fresh (SQLite open is cheap), so the
//! addon is stateless except for [`WatchHandle`]. Complex values cross the
//! boundary as plain JSON-shaped objects.

#![allow(clippy::too_many_arguments)]

use aneural_core::Workspace;
use aneural_engine::{Engine, EngineCommand, EngineEvent};
use aneural_store::Store;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn err(e: impl std::fmt::Display) -> Error {
    Error::from_reason(e.to_string())
}

/// Serde round-trip between the addon's JS-shaped structs and the core types.
/// Absent optional fields arrive as `null`; strip them so `#[serde(default)]`
/// applies on the receiving side.
fn convert<T: Serialize, U: for<'de> Deserialize<'de>>(value: T) -> Result<U> {
    let mut v = serde_json::to_value(value).map_err(err)?;
    strip_nulls(&mut v);
    serde_json::from_value(v).map_err(err)
}

fn strip_nulls(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            map.retain(|_, val| !val.is_null());
            map.values_mut().for_each(strip_nulls);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

fn ws(root: &str) -> Workspace {
    Workspace::at(root)
}

fn open_store(root: &str) -> Result<Store> {
    let ws = ws(root);
    if !ws.exists() {
        return Err(err(format!(
            "no .aneural workspace at {root} (run `aneural init`)"
        )));
    }
    Store::open(&ws.db_path()).map_err(err)
}

fn open_engine(root: &str) -> Result<Engine> {
    let ws = ws(root);
    if !ws.exists() {
        return Err(err(format!(
            "no .aneural workspace at {root} (run `aneural init`)"
        )));
    }
    Engine::open(ws).map_err(err)
}

// ---- types -------------------------------------------------------------

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub path: Option<String>,
    pub repo_id: Option<String>,
    pub props: serde_json::Value,
    pub fingerprint: Option<String>,
    pub source: String,
    pub origin: Option<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub kind: String,
    pub src: String,
    pub dst: String,
    pub props: serde_json::Value,
    pub source: String,
    pub origin: Option<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub files_scanned: u32,
    pub files_indexed: u32,
    pub files_skipped: u32,
    pub nodes: u32,
    pub edges: u32,
    pub unresolved: u32,
    pub duration_ms: u32,
}

#[napi(object)]
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeQuery {
    pub kinds: Option<Vec<String>>,
    pub repo: Option<String>,
    pub path_prefix: Option<String>,
    pub text: Option<String>,
    pub limit: Option<u32>,
}

#[napi(object)]
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EdgeQuery {
    pub src: Option<String>,
    pub dst: Option<String>,
    pub kinds: Option<Vec<String>>,
    pub limit: Option<u32>,
}

#[napi(object)]
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NeighborhoodOptions {
    pub depth: Option<u32>,
    /// `in` | `out` | `both`
    pub direction: Option<String>,
    pub edge_kinds: Option<Vec<String>>,
    pub limit: Option<u32>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub nodes: u32,
    pub edges: u32,
    pub files: u32,
    pub unresolved: u32,
    pub by_kind: Vec<KindCount>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
pub struct KindCount {
    pub kind: String,
    pub count: u32,
}

#[napi(object)]
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Focus {
    pub version: u32,
    pub updated_at: String,
    pub workspace: String,
    pub filters: serde_json::Value,
    pub selection: serde_json::Value,
    pub neighborhood: serde_json::Value,
    pub visible_node_ids: Vec<String>,
    pub notes: String,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTypeDef {
    pub kind: String,
    pub label: String,
    pub icon: String,
    pub color: String,
    pub shape: String,
    pub description: String,
    pub provider: String,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeInfo {
    /// `publisher.name`, the marketplace identity.
    pub id: String,
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub enabled: bool,
    pub location: String,
    pub path: String,
    pub node_kinds: Vec<String>,
    /// `declarative` | `http` | `sandboxed` | `native`.
    pub tier: String,
    #[napi(ts_type = "string[]")]
    #[serde(default)]
    pub consent_lines: Vec<String>,
    #[napi(ts_type = "string[]")]
    #[serde(default)]
    pub missing_settings: Vec<String>,
    #[serde(default)]
    pub settings: Vec<SporeSetting>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub level: String,
    pub category: String,
    pub message: String,
    pub path: Option<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unresolved {
    pub origin: String,
    pub specifier: String,
    pub line: u32,
    pub reason: String,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarvestStats {
    pub files: u32,
    pub nodes: u32,
    pub edges: u32,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub root: String,
    pub name: String,
    pub aneural_dir: String,
    pub config_path: String,
    pub db_path: String,
    pub focus_path: String,
    pub initialized: bool,
    pub indexed: bool,
}

#[napi(object)]
#[derive(Default)]
pub struct InitOptions {
    pub name: Option<String>,
    pub force: Option<bool>,
}

#[napi(object)]
#[derive(Default)]
pub struct IndexOptions {
    /// Re-analyse every file even if its fingerprint is unchanged.
    pub full: Option<bool>,
}

// ---- workspace ---------------------------------------------------------

#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Walk up from `startDir` to the nearest directory containing `.aneural/`.
#[napi]
pub fn find_workspace(start_dir: String) -> Option<String> {
    Workspace::find(start_dir)
        .ok()
        .map(|w| w.root().display().to_string())
}

fn info_for(ws: &Workspace) -> Result<WorkspaceInfo> {
    let config = ws.load_config().map_err(err)?;
    Ok(WorkspaceInfo {
        root: ws.root().display().to_string(),
        name: config.name,
        aneural_dir: ws.aneural_dir().display().to_string(),
        config_path: ws.config_path().display().to_string(),
        db_path: ws.db_path().display().to_string(),
        focus_path: ws.focus_path().display().to_string(),
        initialized: ws.exists(),
        indexed: ws.db_path().exists(),
    })
}

#[napi]
pub fn workspace_info(root: String) -> Result<WorkspaceInfo> {
    info_for(&ws(&root))
}

/// Create `.aneural/` with a default config.
#[napi]
pub fn init_workspace(root: String, opts: Option<InitOptions>) -> Result<WorkspaceInfo> {
    let opts = opts.unwrap_or_default();
    let ws = ws(&root);
    std::fs::create_dir_all(ws.root()).map_err(err)?;
    let ws = Workspace::at(ws.root());
    ws.init(opts.name.as_deref(), opts.force.unwrap_or(false))
        .map_err(err)?;
    info_for(&ws)
}

/// The effective config (defaults applied).
#[napi]
pub fn load_config(root: String) -> Result<serde_json::Value> {
    serde_json::to_value(ws(&root).load_config().map_err(err)?).map_err(err)
}

// ---- indexing ----------------------------------------------------------

pub struct IndexTask {
    root: String,
    full: bool,
}

impl Task for IndexTask {
    type Output = aneural_core::graph::IndexStats;
    type JsValue = IndexStats;

    fn compute(&mut self) -> Result<Self::Output> {
        let mut engine = open_engine(&self.root)?;
        engine.index_full(self.full, &mut |_| {}).map_err(err)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        convert(output)
    }
}

/// Index the workspace (incremental unless `opts.full`). Resolves with stats.
#[napi(ts_return_type = "Promise<IndexStats>")]
pub fn index_workspace(root: String, opts: Option<IndexOptions>) -> AsyncTask<IndexTask> {
    AsyncTask::new(IndexTask {
        root,
        full: opts.and_then(|o| o.full).unwrap_or(false),
    })
}

/// Blocking variant of [`index_workspace`].
#[napi]
pub fn index_workspace_sync(root: String, opts: Option<IndexOptions>) -> Result<IndexStats> {
    let mut engine = open_engine(&root)?;
    let stats = engine
        .index_full(opts.and_then(|o| o.full).unwrap_or(false), &mut |_| {})
        .map_err(err)?;
    convert(stats)
}

// ---- graph reads -------------------------------------------------------

#[napi]
pub fn query_nodes(root: String, query: Option<NodeQuery>) -> Result<Vec<Node>> {
    let store = open_store(&root)?;
    let q: aneural_store::NodeQuery = convert(query.unwrap_or_default())?;
    convert(store.query_nodes(&q).map_err(err)?)
}

#[napi]
pub fn get_node(root: String, id: String) -> Result<Option<Node>> {
    let store = open_store(&root)?;
    convert(
        store
            .get_node(&aneural_core::NodeId::new(id))
            .map_err(err)?,
    )
}

#[napi]
pub fn get_nodes(root: String, ids: Vec<String>) -> Result<Vec<Node>> {
    let store = open_store(&root)?;
    let ids: Vec<aneural_core::NodeId> = ids.into_iter().map(aneural_core::NodeId::new).collect();
    convert(store.get_nodes(&ids).map_err(err)?)
}

#[napi]
pub fn get_edges(root: String, query: Option<EdgeQuery>) -> Result<Vec<Edge>> {
    let store = open_store(&root)?;
    let q: aneural_store::EdgeQuery = convert(query.unwrap_or_default())?;
    convert(store.get_edges(&q).map_err(err)?)
}

/// BFS neighbourhood of the given node ids.
#[napi]
pub fn neighborhood(
    root: String,
    ids: Vec<String>,
    opts: Option<NeighborhoodOptions>,
) -> Result<Subgraph> {
    let store = open_store(&root)?;
    let opts = opts.unwrap_or_default();
    let direction = match opts.direction.as_deref() {
        Some("in") => Some(aneural_core::focus::Direction::In),
        Some("out") => Some(aneural_core::focus::Direction::Out),
        _ => Some(aneural_core::focus::Direction::Both),
    };
    let q = aneural_store::NeighborhoodQuery {
        depth: opts.depth.unwrap_or(1),
        direction,
        edge_kinds: opts.edge_kinds.unwrap_or_default(),
        limit: opts.limit,
    };
    let ids: Vec<aneural_core::NodeId> = ids.into_iter().map(aneural_core::NodeId::new).collect();
    convert(store.neighborhood(&ids, &q).map_err(err)?)
}

/// The whole graph. Use sparingly on large workspaces.
#[napi]
pub fn snapshot(root: String) -> Result<Subgraph> {
    let store = open_store(&root)?;
    convert(store.snapshot().map_err(err)?)
}

#[napi]
pub fn counts(root: String) -> Result<Counts> {
    let store = open_store(&root)?;
    let c = store.counts().map_err(err)?;
    Ok(Counts {
        nodes: c.nodes as u32,
        edges: c.edges as u32,
        files: c.files as u32,
        unresolved: c.unresolved as u32,
        by_kind: c
            .by_kind
            .into_iter()
            .map(|(kind, count)| KindCount {
                kind,
                count: count as u32,
            })
            .collect(),
    })
}

#[napi]
pub fn list_unresolved(root: String, limit: Option<u32>) -> Result<Vec<Unresolved>> {
    let store = open_store(&root)?;
    convert(store.list_unresolved(limit).map_err(err)?)
}

// ---- focus -------------------------------------------------------------

#[napi]
pub fn read_focus(root: String) -> Result<Option<Focus>> {
    convert(ws(&root).read_focus().map_err(err)?)
}

#[napi]
pub fn write_focus(root: String, focus: Focus) -> Result<()> {
    let f: aneural_core::Focus = convert(focus)?;
    ws(&root).write_focus(&f).map_err(err)
}

/// Everything a spore emitted over a fixture directory.
#[napi(object)]
pub struct HarvestResult {
    pub nodes: serde_json::Value,
    pub edges: serde_json::Value,
}

// ---- marketplace types -------------------------------------------------

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryStatus {
    pub name: String,
    pub url: String,
    pub ok: bool,
    pub spore_count: u32,
    pub error: Option<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub publisher: String,
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    /// Which configured registry provided it.
    pub registry: String,
    /// Other registries that also list it, so a shadow is never invisible.
    pub also_in: Vec<String>,
    /// `declarative` | `http` | `sandboxed` | `native`.
    pub tier: String,
    pub node_kinds: Vec<String>,
    pub first_party: bool,
    pub score: u32,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub id: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub registry: String,
    pub repo: String,
    pub tier: String,
    pub requires_consent: bool,
    /// Plain-English lines describing what the spore will be allowed to do.
    pub consent_lines: Vec<String>,
    pub previous_version: Option<String>,
    pub node_kinds: Vec<String>,
    pub readme: Option<String>,
    pub first_party: bool,
    /// Values this spore needs before it can do anything, straight from the
    /// downloaded manifest rather than from the listing.
    pub settings: Vec<SporeSetting>,
    /// Declared secrets that are not resolvable in this environment.
    pub missing_secrets: Vec<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeSetting {
    pub key: String,
    pub label: String,
    pub description: String,
    pub example: Option<String>,
    pub required: bool,
    /// The value recorded in this workspace, if any.
    pub value: Option<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub harvesters: u32,
    pub skipped: u32,
    pub nodes: u32,
    pub edges: u32,
    pub problems: Vec<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub id: String,
    pub version: String,
    pub dir: String,
    pub tier: String,
    pub enabled: bool,
    pub previous_version: Option<String>,
}

#[napi(object)]
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeDrift {
    pub id: String,
    /// `modified` | `missing` | `notInLock` | `notInstalled`.
    pub kind: String,
    pub file: Option<String>,
}

// ---- schema ------------------------------------------------------------

/// Builtin + spore + workspace node types with config overrides applied.
#[napi]
pub fn list_node_types(root: String) -> Result<Vec<NodeTypeDef>> {
    let engine = open_engine(&root)?;
    convert(engine.node_types().map_err(err)?)
}

#[napi]
pub fn list_spores(root: String) -> Result<Vec<SporeInfo>> {
    let engine = open_engine(&root)?;
    convert(engine.spores())
}

/// Validate a `spore.json`; returns every problem (empty = valid).
#[napi]
pub fn validate_spore(manifest_path: String) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(&manifest_path).map_err(err)?;
    let manifest: aneural_core::spore::SporeManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => return Ok(vec![format!("invalid JSON: {e}")]),
    };
    Ok(aneural_engine::spores::compile_report(
        manifest,
        &manifest_path,
    ))
}

/// Run one spore over a directory of fixture files and return everything it
/// emitted. No workspace, no cache, no store — this is the tight loop a spore
/// author (or an agent writing one) iterates in.
#[napi]
pub fn test_spore(manifest_path: String, fixtures_dir: String) -> Result<HarvestResult> {
    let text = std::fs::read_to_string(&manifest_path).map_err(err)?;
    let manifest: aneural_core::spore::SporeManifest =
        serde_json::from_str(&text).map_err(|e| err(format!("invalid JSON: {e}")))?;
    let spore =
        aneural_engine::spores::compile(manifest, "check", &manifest_path, true).map_err(err)?;

    let root = PathBuf::from(&fixtures_dir);
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_files(&root, &root, &mut files)?;
    files.sort();

    // Wiki-links resolve against the whole fixture set, so index it first.
    let mut index = aneural_engine::spores::MarkdownIndex::default();
    for (rel, _) in &files {
        index.insert(rel);
    }

    let spores = [spore];
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for (rel, abs) in &files {
        let bytes = std::fs::read(abs).unwrap_or_default();
        let h = aneural_engine::spores::harvest_file(&spores, &index, rel, abs, &bytes);
        nodes.extend(h.nodes);
        edges.extend(h.edges);
    }
    // Deterministic order, so a snapshot diff means a real change.
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| (&a.kind, &a.src, &a.dst).cmp(&(&b.kind, &b.src, &b.dst)));

    Ok(HarvestResult {
        nodes: serde_json::to_value(&nodes).map_err(err)?,
        edges: serde_json::to_value(&edges).map_err(err)?,
    })
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push((rel.to_string_lossy().replace('\\', "/"), path.clone()));
        }
    }
    Ok(())
}

// ---- marketplace -------------------------------------------------------

fn federation(root: &str) -> Result<aneural_registry::Federation> {
    let config = ws(root).load_config().map_err(err)?;
    aneural_registry::federation(&config.spores).map_err(err)
}

/// Refresh every configured registry, reporting each one separately so a dead
/// private index never hides the official one.
#[napi]
pub fn registry_refresh(root: String, force: Option<bool>) -> Result<Vec<RegistryStatus>> {
    let statuses = federation(&root)?.refresh(force.unwrap_or(false));
    Ok(statuses
        .into_iter()
        .map(|s| RegistryStatus {
            name: s.name,
            url: s.url,
            ok: s.ok,
            spore_count: s.spore_count as u32,
            error: s.error,
        })
        .collect())
}

/// Search every configured registry. An empty query lists everything.
#[napi]
pub fn search_spores(root: String, query: String) -> Result<Vec<SearchHit>> {
    let fed = federation(&root)?;
    fed.refresh(false);
    let indexes = fed.indexes();
    let sources: Vec<aneural_registry::search::Source<'_>> = indexes
        .iter()
        .map(|(name, index)| aneural_registry::search::Source {
            registry: name,
            entries: &index.spores,
        })
        .collect();
    aneural_registry::search(&sources, &query)
        .into_iter()
        .map(|h| {
            Ok(SearchHit {
                id: h.entry.id.clone(),
                publisher: h.entry.publisher().to_string(),
                name: h.entry.name().to_string(),
                version: h.entry.version.clone(),
                display_name: h.entry.display_name.clone(),
                description: h.entry.description.clone(),
                registry: h.registry,
                also_in: h.also_in,
                tier: h.entry.tier().label().to_string(),
                node_kinds: h.entry.node_kinds.clone(),
                first_party: h.entry.first_party,
                score: h.score,
            })
        })
        .collect()
}

/// Resolve, download and verify a spore without writing anything. The returned
/// plan is what the consent sheet renders.
#[napi]
pub fn plan_install_spore(root: String, id: String) -> Result<InstallPlan> {
    let fed = federation(&root)?;
    fed.refresh(false);
    let plan = aneural_registry::plan(Path::new(&root), &fed, &id).map_err(err)?;
    Ok(InstallPlan {
        id: plan.id.clone(),
        version: plan.entry.version.clone(),
        display_name: plan.manifest.display_name.clone(),
        description: plan.manifest.description.clone(),
        registry: plan.registry.clone(),
        repo: plan.entry.repo.clone(),
        tier: plan.tier.label().to_string(),
        requires_consent: plan.requires_consent(),
        consent_lines: plan.consent_lines(),
        previous_version: plan.previous.clone(),
        node_kinds: plan
            .manifest
            .node_types
            .iter()
            .map(|n| n.kind.clone())
            .collect(),
        readme: plan
            .files
            .get(aneural_registry::README_FILE)
            .map(|b| String::from_utf8_lossy(b).to_string()),
        first_party: plan.manifest.is_first_party(),
        settings: {
            // Anything the workspace already recorded, so re-installing does
            // not look like it is asking for something it has.
            let recorded = ws(&root)
                .load_config()
                .map(|c| c.spores.settings_for(&plan.id, plan.manifest.name.as_str()))
                .unwrap_or_default();
            plan.manifest
                .settings
                .iter()
                .map(|d| SporeSetting {
                    key: d.key.clone(),
                    label: if d.label.is_empty() {
                        d.key.clone()
                    } else {
                        d.label.clone()
                    },
                    description: d.description.clone(),
                    example: d.example.clone(),
                    required: d.required,
                    value: recorded.get(&d.key).cloned(),
                })
                .collect()
        },
        missing_secrets: {
            use aneural_core::net::{EnvSecrets, SecretStore};
            let declared: Vec<String> = plan
                .manifest
                .capabilities
                .iter()
                .flat_map(|c| match c {
                    aneural_core::spore::Capability::Secret { names, .. } => names.clone(),
                    _ => Vec::new(),
                })
                .collect();
            EnvSecrets.missing(&declared)
        },
    })
}

// ---- settings and refresh ---------------------------------------------

/// Record (or, with a null value, clear) one of a spore's declared settings.
#[napi]
pub fn set_spore_setting(
    root: String,
    id: String,
    key: String,
    value: Option<String>,
) -> Result<()> {
    aneural_registry::set_setting(Path::new(&root), &id, &key, value.as_deref()).map_err(err)?;
    Ok(())
}

/// The settings recorded for one spore in this workspace.
#[napi]
pub fn get_spore_settings(root: String, id: String) -> Result<serde_json::Value> {
    let ws = ws(&root);
    let config = ws.load_config().map_err(err)?;
    let name = id.split_once('.').map(|(_, n)| n).unwrap_or(&id);
    serde_json::to_value(config.spores.settings_for(&id, name)).map_err(err)
}

/// Re-fetch every HTTP harvester now, writing the results into the graph.
///
/// This is the one napi entry point that makes web requests, and it does so
/// only because it explicitly hands the engine a fetcher. `indexWorkspace`,
/// `queryNodes` and everything the MCP server calls do not.
#[napi]
pub fn refresh_spores(
    root: String,
    id: Option<String>,
    force: Option<bool>,
) -> Result<RefreshResult> {
    let mut engine = open_engine(&root)?;
    engine.set_fetcher(Box::new(aneural_registry::UreqFetcher::new()));
    let stats = engine
        .refresh_http(id.as_deref(), force.unwrap_or(true), &mut |_| {})
        .map_err(err)?;
    Ok(RefreshResult {
        harvesters: stats.harvesters as u32,
        skipped: stats.skipped as u32,
        nodes: stats.nodes as u32,
        edges: stats.edges as u32,
        problems: stats.problems,
    })
}

/// Install a spore the user has consented to. Re-resolves and re-verifies, so a
/// stale plan can never be committed.
#[napi]
pub fn install_spore(root: String, id: String, enable: Option<bool>) -> Result<InstallResult> {
    let fed = federation(&root)?;
    fed.refresh(false);
    let plan = aneural_registry::plan(Path::new(&root), &fed, &id).map_err(err)?;
    let grants = aneural_registry::Grants {
        capabilities: plan.manifest.capabilities.clone(),
    };
    let out = aneural_registry::commit(Path::new(&root), &plan, &grants, enable.unwrap_or(true))
        .map_err(err)?;
    Ok(InstallResult {
        id: out.id,
        version: out.version,
        dir: out.dir.display().to_string(),
        tier: out.tier.label().to_string(),
        enabled: out.enabled,
        previous_version: out.previous,
    })
}

#[napi]
pub fn uninstall_spore(root: String, id: String) -> Result<()> {
    aneural_registry::uninstall(Path::new(&root), &id).map_err(err)
}

#[napi]
pub fn set_spore_enabled(root: String, id: String, enabled: bool) -> Result<()> {
    aneural_registry::set_enabled(Path::new(&root), &id, enabled).map_err(err)?;
    Ok(())
}

/// Compare installed files against the lockfile.
#[napi]
pub fn verify_spores(root: String) -> Result<Vec<SporeDrift>> {
    let lock = aneural_registry::Lockfile::load(Path::new(&root)).map_err(err)?;
    Ok(lock
        .verify(Path::new(&root))
        .map_err(err)?
        .into_iter()
        .map(|d| {
            use aneural_registry::Drift::*;
            match d {
                Modified { id, file } => SporeDrift {
                    id,
                    kind: "modified".into(),
                    file: Some(file),
                },
                Missing { id, file } => SporeDrift {
                    id,
                    kind: "missing".into(),
                    file: Some(file),
                },
                NotInLock { id } => SporeDrift {
                    id,
                    kind: "notInLock".into(),
                    file: None,
                },
                NotInstalled { id } => SporeDrift {
                    id,
                    kind: "notInstalled".into(),
                    file: None,
                },
            }
        })
        .collect())
}

/// Bring a v1 `spores` config block up to date. With `write: false` this only
/// reports what would change, so reading never dirties the working tree.
#[napi]
pub fn migrate_spores_config(root: String, write: bool) -> Result<Vec<String>> {
    let workspace = ws(&root);
    let mut config = workspace.load_config().map_err(err)?;
    let changes = config.spores.migrate(&aneural_registry::FIRST_PARTY_NAMES);
    if write && !changes.is_empty() {
        workspace.save_config(&config).map_err(err)?;
    }
    Ok(changes
        .into_iter()
        .map(|c| format!("{} -> {}", c.from, c.to))
        .collect())
}

/// Validate a registry index file: structure, then fetch and verify every listed
/// file, then cross-check each manifest against its listing. What registry CI runs.
#[napi]
pub fn validate_registry_index(index_path: String) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(&index_path).map_err(err)?;
    let index: aneural_registry::Index =
        serde_json::from_str(&text).map_err(|e| err(format!("invalid JSON: {e}")))?;

    // Entries resolve relative to the index, so a registry can be checked from a
    // checkout without publishing it anywhere first.
    let path = PathBuf::from(&index_path);
    let root = path.parent().map(PathBuf::from).unwrap_or_default();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "index.json".into());
    let client = aneural_registry::StaticIndex::new(
        "check",
        index_path.clone(),
        aneural_registry::MixedTransport::new(root),
    )
    .fetching(name);

    Ok(aneural_registry::index::validate_index(&index, &client))
}

/// The disclaimer every surface must show before installing third-party code.
#[napi]
pub fn marketplace_disclaimer() -> String {
    aneural_registry::DISCLAIMER.to_string()
}

/// Re-run one spore's harvesters over every file it applies to.
#[napi]
pub fn harvest_spore(root: String, name: String) -> Result<HarvestStats> {
    let mut engine = open_engine(&root)?;
    convert(engine.harvest_spore(&name, &mut |_| {}).map_err(err)?)
}

#[napi]
pub fn doctor(root: String) -> Result<Vec<Diagnostic>> {
    let engine = open_engine(&root)?;
    convert(engine.doctor().map_err(err)?)
}

/// Names accepted by `NodeTypeDef.icon`.
#[napi]
pub fn list_icons() -> Vec<String> {
    aneural_icons::names().map(String::from).collect()
}

/// Read a workspace file (UTF-8, lossy), optionally a 1-based inclusive line range.
#[napi]
pub fn read_file(
    root: String,
    rel_path: String,
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> Result<String> {
    let ws = ws(&root);
    let clean = aneural_core::id::canonical_rel(Path::new(&rel_path));
    if clean.starts_with("..") {
        return Err(err("path escapes the workspace"));
    }
    let abs: PathBuf = ws.abs(&clean);
    if !abs.starts_with(ws.root()) {
        return Err(err("path escapes the workspace"));
    }
    let bytes = std::fs::read(&abs).map_err(err)?;
    let text = String::from_utf8_lossy(&bytes);
    match (start_line, end_line) {
        (None, None) => Ok(text.into_owned()),
        (s, e) => {
            let s = s.unwrap_or(1).max(1) as usize;
            let e = e.map(|e| e as usize).unwrap_or(usize::MAX);
            Ok(text
                .lines()
                .enumerate()
                .filter(|(i, _)| (s..=e).contains(&(i + 1)))
                .map(|(_, l)| l)
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }
}

// ---- watch -------------------------------------------------------------

/// A running watcher; call `stop()` to end it.
#[napi]
pub struct WatchHandle {
    tx: Option<crossbeam_channel::Sender<EngineCommand>>,
}

#[napi]
impl WatchHandle {
    #[napi]
    pub fn stop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(EngineCommand::Stop);
        }
    }

    #[napi(getter)]
    pub fn active(&self) -> bool {
        self.tx.is_some()
    }
}

/// Index, then watch the workspace; `callback` receives `{type: "delta"|"progress"|"indexComplete"|"watching"|"error", ...}` objects.
#[napi]
pub fn watch(
    root: String,
    #[napi(ts_arg_type = "(event: WatchEvent) => void")] callback: ThreadsafeFunction<
        serde_json::Value,
        (),
        serde_json::Value,
        Status,
        false,
    >,
) -> Result<WatchHandle> {
    let ws = ws(&root);
    if !ws.exists() {
        return Err(err(format!(
            "no .aneural workspace at {root} (run `aneural init`)"
        )));
    }
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EngineCommand>();
    let (ev_tx, ev_rx) = crossbeam_channel::unbounded::<EngineEvent>();
    let root_path = ws.root().to_path_buf();
    std::thread::Builder::new()
        .name("aneural-engine".into())
        .spawn(move || aneural_engine::run(&root_path, ev_tx, cmd_rx))
        .map_err(err)?;
    std::thread::Builder::new()
        .name("aneural-watch-forward".into())
        .spawn(move || {
            while let Ok(ev) = ev_rx.recv() {
                let value = match ev {
                    EngineEvent::Delta(d) => serde_json::json!({ "type": "delta", "delta": d }),
                    EngineEvent::Progress { phase, done, total } => serde_json::json!({ "type": "progress", "phase": phase, "done": done, "total": total }),
                    EngineEvent::IndexComplete(s) => serde_json::json!({ "type": "indexComplete", "stats": s }),
                    EngineEvent::Watching => serde_json::json!({ "type": "watching" }),
                    EngineEvent::Spores { spores, errors } => {
                        serde_json::json!({ "type": "spores", "spores": spores, "errors": errors })
                    }
                    EngineEvent::Error(e) => serde_json::json!({ "type": "error", "message": e }),
                };
                if callback.call(value, ThreadsafeFunctionCallMode::NonBlocking) == Status::Closing {
                    break;
                }
            }
        })
        .map_err(err)?;
    Ok(WatchHandle { tx: Some(cmd_tx) })
}

/// Reserved TS type for watch events (documented in index.d.ts header).
#[napi(object)]
pub struct WatchEvent {
    #[napi(js_name = "type")]
    pub kind: String,
    pub delta: Option<serde_json::Value>,
    pub phase: Option<String>,
    pub done: Option<u32>,
    pub total: Option<u32>,
    pub stats: Option<serde_json::Value>,
    pub spores: Option<serde_json::Value>,
    pub errors: Option<Vec<String>>,
    pub message: Option<String>,
}
