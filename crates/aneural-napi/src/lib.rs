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
    pub name: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub enabled: bool,
    pub location: String,
    pub path: String,
    pub node_kinds: Vec<String>,
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

/// Validate a `spore.json`; returns a list of problems (empty = valid).
#[napi]
pub fn validate_spore(manifest_path: String) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(&manifest_path).map_err(err)?;
    let manifest: aneural_core::spore::SporeManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => return Ok(vec![format!("invalid JSON: {e}")]),
    };
    let rel = manifest_path.clone();
    match aneural_engine::spores::compile(manifest, "check", &rel, true) {
        Ok(_) => Ok(vec![]),
        Err(e) => Ok(vec![e.to_string()]),
    }
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
    pub message: Option<String>,
}
