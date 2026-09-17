//! Aneural indexing engine: walks a workspace, parses manifests and imports,
//! runs spore harvesters, persists to the SQLite store and streams
//! [`GraphDelta`]s to whoever is listening (the GUI, the CLI, the MCP server).

pub mod analysis;
pub mod doctor;
pub mod spores;
pub mod walk;
pub mod watch;

pub use aneural_core;
pub use aneural_lang;
pub use aneural_store;
pub use doctor::Diagnostic;

use aneural_core::config::{Config, NodeTypeDef};
use aneural_core::graph::{DeltaPhase, IndexStats};
use aneural_core::kinds::NodeKind;
use aneural_core::net::{EnvSecrets, Fetcher, SecretStore};
use aneural_core::spore::{Harvester, SporeInfo};
use aneural_core::{Edge, GraphDelta, Node, NodeId, Workspace};
use aneural_lang::Resolver;
use aneural_store::{FileRecord, NodeQuery, Store};
use crossbeam_channel::{Receiver, Sender};
use globset::GlobSet;
use serde::{Deserialize, Serialize};
use spores::{MarkdownIndex, Spore};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use walk::Entry;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] aneural_core::Error),
    #[error(transparent)]
    Store(#[from] aneural_store::Error),
    #[error("walk: {0}")]
    Walk(#[from] ignore::Error),
    #[error("watch: {0}")]
    Watch(#[from] notify::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Events streamed by the engine.
#[derive(Clone, Debug)]
pub enum EngineEvent {
    Delta(GraphDelta),
    /// The installed spores, emitted on open and after any change to them, so
    /// the GUI never has to hold an `Engine` of its own to see them.
    Spores {
        spores: Vec<SporeInfo>,
        errors: Vec<String>,
    },
    Progress {
        phase: &'static str,
        done: u64,
        total: u64,
    },
    IndexComplete(IndexStats),
    Watching,
    Error(String),
}

/// Commands accepted by [`Engine::watch_loop`].
#[derive(Clone, Debug)]
pub enum EngineCommand {
    /// Re-run a full index (respecting fingerprints unless `force`).
    Reindex {
        force: bool,
    },
    /// Re-read the config and reload spores from disk, then report them. Used
    /// after the marketplace installs or removes one.
    ReloadSpores,
    /// Turn one spore on or off and converge the graph to match.
    SetSporeEnabled {
        id: String,
        on: bool,
    },
    /// Re-fetch HTTP harvesters now. `id` narrows it to one spore; `force`
    /// ignores the refresh interval, which is what a user pressing a button
    /// means.
    RefreshHttp {
        id: Option<String>,
        force: bool,
    },
    Stop,
}

/// What one pass of the HTTP refresh loop did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshStats {
    pub harvesters: u64,
    pub skipped: u64,
    pub nodes: u64,
    pub edges: u64,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HarvestStats {
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
}

pub struct Engine {
    ws: Workspace,
    config: Config,
    store: Store,
    resolver: Resolver,
    spores: Vec<Spore>,
    spore_errors: Vec<String>,
    ignore: GlobSet,
    md_index: MarkdownIndex,
    repos: BTreeSet<String>,
    /// Supplied by the host binary. `None` means this program does not make web
    /// requests, and every HTTP harvester reports that rather than failing
    /// silently — see `aneural_core::net`.
    fetcher: Option<Box<dyn Fetcher>>,
    secrets: Box<dyn SecretStore>,
    /// When each HTTP harvester may next run, keyed by its synthetic origin.
    next_refresh: HashMap<String, Instant>,
}

const BATCH: usize = 200;

impl Engine {
    /// Open the engine on a workspace, using the on-disk cache.
    pub fn open(ws: Workspace) -> Result<Self> {
        let store = Store::open(&ws.db_path())?;
        Self::with_store(ws, store)
    }

    /// Open with an in-memory store (tests, one-shot queries).
    pub fn open_in_memory(ws: Workspace) -> Result<Self> {
        Self::with_store(ws, Store::open_in_memory()?)
    }

    fn with_store(ws: Workspace, store: Store) -> Result<Self> {
        let config = ws.load_config()?;
        let resolver = Resolver::new(ws.root(), &config.typescript);
        let (spores, errs) = spores::load_all(&ws, &config.spores.enabled);
        let ignore = spores::globset(&config.ignore)
            .map_err(|e| Error::Other(format!("config.ignore: {e}")))?;
        let mut engine = Engine {
            ws,
            config,
            store,
            resolver,
            spores,
            spore_errors: errs.iter().map(|e| e.to_string()).collect(),
            ignore,
            md_index: MarkdownIndex::default(),
            repos: BTreeSet::new(),
            fetcher: None,
            secrets: Box::new(EnvSecrets),
            next_refresh: HashMap::new(),
        };
        engine.warm_from_store()?;
        Ok(engine)
    }

    /// Rebuild the in-memory helpers (repo set, markdown index) from the cache.
    fn warm_from_store(&mut self) -> Result<()> {
        for n in self.store.query_nodes(&NodeQuery {
            kinds: vec![NodeKind::REPO.into()],
            ..Default::default()
        })? {
            if let Some(p) = n.path {
                self.repos.insert(p);
            }
        }
        for rec in self.store.all_files()? {
            if rec.lang.as_deref() == Some("markdown") {
                self.md_index.insert(&rec.path);
            }
        }
        Ok(())
    }

    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn store(&self) -> &Store {
        &self.store
    }
    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }
    pub fn spore_errors(&self) -> &[String] {
        &self.spore_errors
    }

    pub fn spores(&self) -> Vec<SporeInfo> {
        self.spores
            .iter()
            .map(|sp| {
                let settings = self
                    .config
                    .spores
                    .settings_for(&sp.manifest.id(), &sp.manifest.name);
                sp.info_with(&settings)
            })
            .collect()
    }

    /// Builtin + spore + workspace node types with config overrides applied.
    pub fn node_types(&self) -> Result<Vec<NodeTypeDef>> {
        let extra: Vec<NodeTypeDef> = self
            .spores
            .iter()
            .filter(|s| s.enabled)
            .flat_map(Spore::node_types)
            .collect();
        Ok(self.ws.compose_node_types(&self.config, extra)?)
    }

    // ---- indexing ---------------------------------------------------------

    /// Full index. Emits every node/edge (so a fresh consumer sees the whole
    /// graph), skipping analysis of unchanged files unless `force`.
    pub fn index_full(
        &mut self,
        force: bool,
        sink: &mut dyn FnMut(EngineEvent),
    ) -> Result<IndexStats> {
        let started = Instant::now();
        let mut stats = IndexStats::default();
        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut files: Vec<Entry> = Vec::new();
        self.repos.clear();

        // Phase 1: structure
        let mut delta = GraphDelta::new(DeltaPhase::Initial);
        let roots: Vec<PathBuf> = self.config.roots.iter().map(|r| self.ws.abs(r)).collect();
        for root in roots {
            for result in walk::walker(&self.ws, &self.config, &root)? {
                let dent = match result {
                    Ok(d) => d,
                    Err(e) => {
                        sink(EngineEvent::Error(e.to_string()));
                        continue;
                    }
                };
                let Some(entry) = walk::entry_for(&self.ws, dent.path()) else {
                    continue;
                };
                if entry.is_repo {
                    self.repos.insert(entry.rel.clone());
                }
                seen_paths.insert(entry.rel.clone());
                let repo = walk::repo_for(&entry.rel, &self.repos);
                delta.nodes.push(walk::node_for(
                    &self.ws,
                    &self.config,
                    &entry,
                    repo.as_ref(),
                ));
                if let Some(e) = walk::contains_edge_for(&entry.rel, entry.is_dir) {
                    delta.edges.push(e);
                }
                if !entry.is_dir {
                    if is_markdown(&entry.rel) {
                        self.md_index.insert(&entry.rel);
                    }
                    files.push(entry);
                }
                if delta.nodes.len() >= BATCH {
                    self.flush(&mut delta, sink)?;
                    sink(EngineEvent::Progress {
                        phase: "walk",
                        done: seen_paths.len() as u64,
                        total: 0,
                    });
                }
            }
        }
        self.flush(&mut delta, sink)?;
        stats.files_scanned = files.len() as u64;

        // Phase 2: content (manifests, imports, spores)
        let total = files.len() as u64;
        for (i, entry) in files.iter().enumerate() {
            match self.process_file(entry, force)? {
                Processed::Indexed(d) => {
                    stats.files_indexed += 1;
                    self.emit(d, sink);
                }
                Processed::Unchanged(d) => {
                    stats.files_skipped += 1;
                    self.emit(d, sink);
                }
            }
            if i % 50 == 0 {
                sink(EngineEvent::Progress {
                    phase: "analyze",
                    done: i as u64 + 1,
                    total,
                });
            }
        }

        // Phase 3: prune what disappeared since the last run
        let mut removed = GraphDelta::new(DeltaPhase::Initial);
        for rec in self.store.all_files()? {
            if !seen_paths.contains(&rec.path) {
                let d = self.remove_path(&rec.path)?;
                removed.merge(d);
            }
        }
        let fs_kinds = vec![
            NodeKind::DIRECTORY.into(),
            NodeKind::REPO.into(),
            NodeKind::FILE.into(),
            NodeKind::MANIFEST.into(),
        ];
        let stale: Vec<NodeId> = self
            .store
            .query_nodes(&NodeQuery {
                kinds: fs_kinds,
                ..Default::default()
            })?
            .into_iter()
            .filter(|n| n.path.as_ref().is_none_or(|p| !seen_paths.contains(p)))
            .map(|n| n.id)
            .collect();
        if !stale.is_empty() {
            self.store.delete_nodes(&stale)?;
            removed.removed_node_ids.extend(stale);
        }
        for id in self.store.gc_orphan_packages()? {
            removed.removed_node_ids.push(id);
        }
        removed.initial_complete = true;
        sink(EngineEvent::Delta(removed));

        let counts = self.store.counts()?;
        stats.nodes = counts.nodes;
        stats.edges = counts.edges;
        stats.unresolved = counts.unresolved;
        stats.duration_ms = started.elapsed().as_millis() as u64;
        self.store
            .meta_set("last_index_at", &aneural_core::now_rfc3339())?;
        self.store
            .meta_set("engine_version", env!("CARGO_PKG_VERSION"))?;
        sink(EngineEvent::IndexComplete(stats.clone()));
        Ok(stats)
    }

    fn flush(&mut self, delta: &mut GraphDelta, sink: &mut dyn FnMut(EngineEvent)) -> Result<()> {
        if delta.is_empty() {
            return Ok(());
        }
        self.store.apply_delta(delta)?;
        let out = std::mem::replace(delta, GraphDelta::new(delta.phase));
        sink(EngineEvent::Delta(out));
        Ok(())
    }

    fn emit(&self, delta: GraphDelta, sink: &mut dyn FnMut(EngineEvent)) {
        if !delta.is_empty() {
            sink(EngineEvent::Delta(delta));
        }
    }

    /// Whether a workspace-relative path is excluded by config or internal.
    pub fn is_ignored(&self, rel: &str) -> bool {
        walk::is_internal(rel)
            || rel.split('/').any(|c| c == ".git")
            || self.ignore.is_match(rel)
            || rel.split('/').any(|c| c == ".DS_Store")
    }

    /// Everything derived from one file's contents.
    fn process_file(&mut self, entry: &Entry, force: bool) -> Result<Processed> {
        let rel = entry.rel.as_str();
        let lang = walk::node_for(&self.ws, &self.config, entry, None)
            .prop_str("lang")
            .map(String::from);
        let record = self.store.file_record(rel)?;
        let unchanged_meta = record
            .as_ref()
            .is_some_and(|r| r.mtime == entry.mtime && r.size == entry.size as i64);
        if unchanged_meta && !force {
            return Ok(Processed::Unchanged(self.cached_delta(rel)?));
        }
        if entry.size > walk::MAX_PARSE_BYTES {
            self.store.upsert_file(&FileRecord {
                path: rel.into(),
                mtime: entry.mtime,
                size: entry.size as i64,
                fingerprint: "skipped".into(),
                lang,
                indexed_at: aneural_core::now_millis(),
            })?;
            // Some harvesters open the file themselves instead of being handed
            // its bytes — a dev database is routinely past this cap and is still
            // worth indexing, because reading its schema costs a few queries.
            if spores::has_large_file_harvester(&self.spores, rel) {
                let h = spores::harvest_large(&self.spores, rel, &entry.abs);
                let repo = walk::repo_for(rel, &self.repos);
                let mut nodes = h.nodes;
                nodes.push(walk::node_for(&self.ws, &self.config, entry, repo.as_ref()));
                let mut delta = self.store.replace_origin(rel, &nodes, &h.edges)?;
                delta.phase = DeltaPhase::Live;
                return Ok(Processed::Indexed(delta));
            }
            return Ok(Processed::Unchanged(GraphDelta::new(DeltaPhase::Live)));
        }
        let bytes = match std::fs::read(&entry.abs) {
            Ok(b) => b,
            Err(_) => return Ok(Processed::Unchanged(GraphDelta::new(DeltaPhase::Live))),
        };
        let fingerprint = blake3::hash(&bytes).to_hex().to_string();
        if !force
            && record
                .as_ref()
                .is_some_and(|r| r.fingerprint == fingerprint)
        {
            self.store.upsert_file(&FileRecord {
                path: rel.into(),
                mtime: entry.mtime,
                size: entry.size as i64,
                fingerprint,
                lang,
                indexed_at: aneural_core::now_millis(),
            })?;
            return Ok(Processed::Unchanged(self.cached_delta(rel)?));
        }

        let mut nodes: Vec<Node> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();
        let mut unresolved = Vec::new();

        if let Some(l) = lang.as_deref()
            && l != "markdown"
            && self.config.language_enabled(l)
        {
            let a = analysis::analyze(&self.ws, &self.resolver, rel, &entry.abs, &bytes);
            nodes.extend(a.nodes);
            edges.extend(a.edges);
            unresolved = a.unresolved;
        }
        if self.spores.iter().any(|s| s.enabled && s.applies_to(rel)) {
            let h = spores::harvest_file(&self.spores, &self.md_index, rel, &entry.abs, &bytes);
            nodes.extend(h.nodes);
            edges.extend(h.edges);
        }
        // refresh the file node's fingerprint + repo
        let repo = walk::repo_for(rel, &self.repos);
        let mut file_node = walk::node_for(&self.ws, &self.config, entry, repo.as_ref());
        file_node.fingerprint = Some(fingerprint.clone());
        nodes.push(file_node);

        let mut delta = self.store.replace_origin(rel, &nodes, &edges)?;
        delta.phase = DeltaPhase::Live;
        self.store.record_unresolved(&unresolved)?;
        self.store.upsert_file(&FileRecord {
            path: rel.into(),
            mtime: entry.mtime,
            size: entry.size as i64,
            fingerprint,
            lang,
            indexed_at: aneural_core::now_millis(),
        })?;
        Ok(Processed::Indexed(delta))
    }

    /// The nodes/edges previously derived from `rel`, as additions.
    fn cached_delta(&self, rel: &str) -> Result<GraphDelta> {
        let mut d = GraphDelta::new(DeltaPhase::Initial);
        d.nodes = self.store.nodes_by_origin(rel)?;
        d.edges = self.store.edges_by_origin(rel)?;
        // package nodes referenced by these edges live without an origin
        for e in &d.edges {
            if e.dst.prefix() == "pkg"
                && let Some(n) = self.store.get_node(&e.dst)?
            {
                d.nodes.push(n);
            }
        }
        Ok(d)
    }

    /// Remove a file or directory (and everything derived from it) from the graph.
    fn remove_path(&mut self, rel: &str) -> Result<GraphDelta> {
        let mut delta = GraphDelta::new(DeltaPhase::Live);
        for origin in self.store.origins_under(rel)? {
            let d = self.store.delete_origin(&origin)?;
            delta.merge(d);
            self.store.delete_file(&origin)?;
            self.md_index.remove(&origin);
        }
        let ids = self.store.node_ids_under(rel)?;
        if !ids.is_empty() {
            self.store.delete_nodes(&ids)?;
            delta.removed_node_ids.extend(ids);
        }
        self.repos
            .retain(|r| r != rel && !r.starts_with(&format!("{rel}/")));
        Ok(delta)
    }

    /// Make sure every ancestor directory of `rel` exists as a node.
    fn ensure_ancestors(&mut self, rel: &str, delta: &mut GraphDelta) -> Result<()> {
        let mut chain: Vec<String> = Vec::new();
        let mut cur = rel
            .rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_else(|| ".".into());
        loop {
            chain.push(cur.clone());
            if cur == "." {
                break;
            }
            cur = cur
                .rsplit_once('/')
                .map(|(p, _)| p.to_string())
                .unwrap_or_else(|| ".".into());
        }
        chain.reverse();
        for dir in chain {
            if self.store.get_node(&NodeId::dir(&dir))?.is_some() {
                continue;
            }
            let Some(entry) = walk::entry_for(&self.ws, &self.ws.abs(&dir)) else {
                continue;
            };
            if entry.is_repo {
                self.repos.insert(entry.rel.clone());
            }
            let repo = walk::repo_for(&entry.rel, &self.repos);
            delta.nodes.push(walk::node_for(
                &self.ws,
                &self.config,
                &entry,
                repo.as_ref(),
            ));
            if let Some(e) = walk::contains_edge_for(&entry.rel, true) {
                delta.edges.push(e);
            }
        }
        Ok(())
    }

    /// React to filesystem changes (absolute paths).
    pub fn index_paths(
        &mut self,
        paths: &[PathBuf],
        sink: &mut dyn FnMut(EngineEvent),
    ) -> Result<()> {
        let mut delta = GraphDelta::new(DeltaPhase::Live);
        let mut rels: Vec<String> = paths
            .iter()
            .filter_map(|p| self.ws.rel(p))
            .filter(|r| !self.is_ignored(r))
            .collect();
        rels.sort();
        rels.dedup();
        for rel in rels {
            let abs = self.ws.abs(&rel);
            if !abs.exists() {
                let d = self.remove_path(&rel)?;
                delta.merge(d);
                continue;
            }
            let Some(entry) = walk::entry_for(&self.ws, &abs) else {
                continue;
            };
            self.ensure_ancestors(&rel, &mut delta)?;
            if entry.is_dir {
                // a new directory: index its subtree
                if self.store.get_node(&NodeId::dir(&rel))?.is_none() {
                    let mut sub = GraphDelta::new(DeltaPhase::Live);
                    let mut files = Vec::new();
                    for result in walk::walker(&self.ws, &self.config, &abs)? {
                        let Ok(dent) = result else { continue };
                        let Some(e) = walk::entry_for(&self.ws, dent.path()) else {
                            continue;
                        };
                        if e.is_repo {
                            self.repos.insert(e.rel.clone());
                        }
                        let repo = walk::repo_for(&e.rel, &self.repos);
                        sub.nodes
                            .push(walk::node_for(&self.ws, &self.config, &e, repo.as_ref()));
                        if let Some(edge) = walk::contains_edge_for(&e.rel, e.is_dir) {
                            sub.edges.push(edge);
                        }
                        if !e.is_dir {
                            if is_markdown(&e.rel) {
                                self.md_index.insert(&e.rel);
                            }
                            files.push(e);
                        }
                    }
                    self.store.apply_delta(&sub)?;
                    delta.merge(sub);
                    for f in files {
                        delta.merge(self.process_file(&f, false)?.into_delta());
                    }
                }
                continue;
            }
            if is_markdown(&rel) {
                self.md_index.insert(&rel);
            }
            let is_new = self.store.get_node(&NodeId::file(&rel))?.is_none();
            if is_new && let Some(e) = walk::contains_edge_for(&rel, false) {
                delta.edges.push(e);
            }
            delta.merge(self.process_file(&entry, is_new)?.into_delta());
        }
        if !delta.is_empty() {
            // structural additions collected above (ancestors, CONTAINS) need persisting too
            let structural = GraphDelta {
                nodes: delta
                    .nodes
                    .iter()
                    .filter(|n| n.origin.is_none())
                    .cloned()
                    .collect(),
                edges: delta
                    .edges
                    .iter()
                    .filter(|e| e.origin.is_none())
                    .cloned()
                    .collect(),
                ..GraphDelta::new(DeltaPhase::Live)
            };
            self.store.apply_delta(&structural)?;
            for id in self.store.gc_orphan_packages()? {
                delta.removed_node_ids.push(id);
            }
            sink(EngineEvent::Delta(delta));
        }
        Ok(())
    }

    /// Block, applying filesystem changes as they happen, until `Stop`.
    pub fn watch_loop(
        &mut self,
        sink: &mut dyn FnMut(EngineEvent),
        commands: Receiver<EngineCommand>,
    ) -> Result<()> {
        let watcher = watch::Watcher::new(self.ws.root(), Duration::from_millis(300))?;
        sink(EngineEvent::Watching);
        // A first pass so an HTTP spore has data without waiting an interval.
        if self.has_http_spores()
            && let Err(e) = self.refresh_http(None, false, sink)
        {
            sink(EngineEvent::Error(e.to_string()));
        }
        loop {
            // Sleep until the next harvester is due rather than polling: a
            // workspace with no HTTP spores must not wake up at all.
            let tick = match self.next_http_due() {
                Some(d) => crossbeam_channel::after(d.max(Duration::from_secs(1))),
                None => crossbeam_channel::never(),
            };
            crossbeam_channel::select! {
                recv(tick) -> _ => {
                    if let Err(e) = self.refresh_http(None, false, sink) {
                        sink(EngineEvent::Error(e.to_string()));
                    }
                }
                recv(watcher.rx) -> msg => match msg {
                    Ok(paths) => {
                        if let Err(e) = self.index_paths(&paths, sink) {
                            sink(EngineEvent::Error(e.to_string()));
                        }
                    }
                    Err(_) => return Ok(()),
                },
                recv(commands) -> cmd => match cmd {
                    Ok(EngineCommand::Reindex { force }) => {
                        if let Err(e) = self.index_full(force, sink) {
                            sink(EngineEvent::Error(e.to_string()));
                        }
                    }
                    Ok(EngineCommand::ReloadSpores) => {
                        if let Err(e) = self.reload_spores() {
                            sink(EngineEvent::Error(e.to_string()));
                        }
                        sink(EngineEvent::Spores {
                            spores: self.spores(),
                            errors: self.spore_errors.clone(),
                        });
                        if let Err(e) = self.index_full(false, sink) {
                            sink(EngineEvent::Error(e.to_string()));
                        }
                    }
                    Ok(EngineCommand::SetSporeEnabled { id, on }) => {
                        if let Err(e) = self.set_spore_enabled(&id, on, sink) {
                            sink(EngineEvent::Error(e.to_string()));
                        }
                    }
                    Ok(EngineCommand::RefreshHttp { id, force }) => {
                        if let Err(e) = self.refresh_http(id.as_deref(), force, sink) {
                            sink(EngineEvent::Error(e.to_string()));
                        }
                    }
                    Ok(EngineCommand::Stop) | Err(_) => return Ok(()),
                },
            }
        }
    }

    /// Reload spores from disk after the config or `.aneural/spores` changed.
    pub fn reload_spores(&mut self) -> Result<()> {
        self.config = self.ws.load_config()?;
        let (spores, errs) = spores::load_all(&self.ws, &self.config.spores.enabled);
        self.spores = spores;
        self.spore_errors = errs.iter().map(|e| e.to_string()).collect();
        // Settings may have changed under a spore that is already installed, so
        // a reload is a reason to re-fetch rather than wait out the interval.
        self.next_refresh.clear();
        Ok(())
    }

    /// Turn a spore on or off, converging the graph without a full reindex.
    ///
    /// The two directions are not symmetric: enabling *adds* nodes, so it
    /// re-harvests the files the spore applies to; disabling has to *retract*
    /// them, which is a delete by producer.
    pub fn set_spore_enabled(
        &mut self,
        id: &str,
        on: bool,
        sink: &mut dyn FnMut(EngineEvent),
    ) -> Result<()> {
        if on {
            self.harvest_spore(id, sink)?;
            // An HTTP spore has no files to walk, so enabling it means fetching.
            self.refresh_http(Some(id), true, sink)?;
        } else {
            let Some(idx) = self
                .spores
                .iter()
                .position(|s| s.manifest.id() == id || s.manifest.name == id)
            else {
                return Err(Error::Other(format!("unknown spore `{id}`")));
            };
            self.spores[idx].enabled = false;
            let source = aneural_core::kinds::Source::spore(&self.spores[idx].manifest.id());
            let prefix = format!("spore://{}/", self.spores[idx].manifest.id());
            self.next_refresh.retain(|org, _| !org.starts_with(&prefix));
            let delta = self.store.delete_by_source(&source)?;
            self.emit(delta, sink);
        }
        sink(EngineEvent::Spores {
            spores: self.spores(),
            errors: self.spore_errors.clone(),
        });
        Ok(())
    }

    // ---- HTTP (tier 1) harvesters -----------------------------------------

    /// Supply the thing that is actually allowed to open a socket.
    ///
    /// Without one, HTTP harvesters report that this program cannot make web
    /// requests. That is deliberate: the engine is linked by the MCP server and
    /// by one-shot CLI queries, and neither should acquire the ability to call
    /// out just by being linked.
    pub fn set_fetcher(&mut self, fetcher: Box<dyn Fetcher>) {
        self.fetcher = Some(fetcher);
    }

    /// Override where `{secret.*}` comes from. Defaults to the environment.
    pub fn set_secrets(&mut self, secrets: Box<dyn SecretStore>) {
        self.secrets = secrets;
    }

    /// Whether any enabled spore has an HTTP harvester at all.
    pub fn has_http_spores(&self) -> bool {
        self.spores
            .iter()
            .filter(|s| s.enabled)
            .any(|s| s.http_harvesters().next().is_some())
    }

    /// Re-fetch every HTTP harvester that is due (or all of `only`'s, ignoring
    /// the schedule, when a user asked for it explicitly).
    pub fn refresh_http(
        &mut self,
        only: Option<&str>,
        force: bool,
        sink: &mut dyn FnMut(EngineEvent),
    ) -> Result<RefreshStats> {
        let mut stats = RefreshStats::default();
        let now = Instant::now();

        // Snapshot the work first: the harvest borrows the store immutably and
        // applying the result borrows it mutably.
        struct Job {
            manifest: aneural_core::SporeManifest,
            harvester: Harvester,
        }
        let jobs: Vec<Job> = self
            .spores
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| only.is_none_or(|id| s.manifest.id() == id || s.manifest.name == id))
            .flat_map(|s| {
                s.http_harvesters().map(|h| Job {
                    manifest: s.manifest.clone(),
                    harvester: h.clone(),
                })
            })
            .collect();

        for job in jobs {
            let Harvester::Http {
                id,
                request,
                select,
                max_pages,
                refresh_seconds,
                emit,
                expand,
            } = &job.harvester
            else {
                continue;
            };
            let org = spores::http::origin(&job.manifest.id(), id);
            if !force && self.next_refresh.get(&org).is_some_and(|due| *due > now) {
                stats.skipped += 1;
                continue;
            }

            let settings = self
                .config
                .spores
                .settings_for(&job.manifest.id(), &job.manifest.name);
            let source = aneural_core::kinds::Source::spore(&job.manifest.id());

            let report = {
                let store = &self.store;
                let known = |id: &str| {
                    NodeId::parse(id)
                        .ok()
                        .and_then(|n| store.get_node(&n).ok().flatten())
                        .is_some()
                };
                let fetcher: &dyn Fetcher = match &self.fetcher {
                    Some(f) => f.as_ref(),
                    None => &aneural_core::net::NoFetcher,
                };
                let cx = spores::http::Context {
                    fetcher,
                    secrets: self.secrets.as_ref(),
                    settings: &settings,
                    known: &known,
                };
                spores::http::harvest(
                    &job.manifest,
                    id,
                    request,
                    select,
                    *max_pages,
                    emit,
                    expand.as_ref(),
                    &source,
                    &cx,
                )
            };

            self.next_refresh.insert(
                org.clone(),
                now + Duration::from_secs(*refresh_seconds).max(Duration::from_secs(
                    aneural_core::spore::MIN_REFRESH_SECONDS,
                )),
            );

            for problem in &report.problems {
                let line = format!("{}/{id}: {problem}", job.manifest.id());
                sink(EngineEvent::Error(line.clone()));
                stats.problems.push(line);
            }

            // A failed fetch must not wipe what the last good one found: only
            // converge the graph when we actually have an answer.
            if !report.problems.is_empty() && report.harvest.nodes.is_empty() {
                continue;
            }

            let delta =
                self.store
                    .replace_origin(&org, &report.harvest.nodes, &report.harvest.edges)?;
            stats.harvesters += 1;
            stats.nodes += report.harvest.nodes.len() as u64;
            stats.edges += report.harvest.edges.len() as u64;
            self.emit(delta, sink);
        }
        Ok(stats)
    }

    /// How long until the soonest HTTP harvester is due, if any.
    pub fn next_http_due(&self) -> Option<Duration> {
        let now = Instant::now();
        self.spores
            .iter()
            .filter(|s| s.enabled)
            .flat_map(|s| {
                let id = s.manifest.id();
                s.http_harvesters().map(move |h| {
                    let org = spores::http::origin(&id, h.id());
                    match self.next_refresh.get(&org) {
                        Some(due) => due.saturating_duration_since(now),
                        // Never fetched in this session: due immediately.
                        None => Duration::ZERO,
                    }
                })
            })
            .min()
    }

    /// Re-run one spore's harvesters across every indexed file it applies to.
    pub fn harvest_spore(
        &mut self,
        name: &str,
        sink: &mut dyn FnMut(EngineEvent),
    ) -> Result<HarvestStats> {
        let Some(idx) = self
            .spores
            .iter()
            .position(|s| s.manifest.id() == name || s.manifest.name == name)
        else {
            return Err(Error::Other(format!("unknown spore `{name}`")));
        };
        let source = aneural_core::kinds::Source::spore(&self.spores[idx].manifest.id());
        self.spores[idx].enabled = true;
        let mut stats = HarvestStats::default();
        for rec in self.store.all_files()? {
            if !self.spores[idx].applies_to(&rec.path) {
                continue;
            }
            let Some(entry) = walk::entry_for(&self.ws, &self.ws.abs(&rec.path)) else {
                continue;
            };
            if let Processed::Indexed(d) = self.process_file(&entry, true)? {
                stats.files += 1;
                stats.nodes += d.nodes.iter().filter(|n| n.source == source).count() as u64;
                stats.edges += d.edges.iter().filter(|e| e.source == source).count() as u64;
                self.emit(d, sink);
            }
        }
        Ok(stats)
    }

    // ---- doctor -----------------------------------------------------------

    pub fn doctor(&self) -> Result<Vec<Diagnostic>> {
        let mut out = Vec::new();
        if let Err(e) = aneural_lang::abi_check() {
            out.push(Diagnostic::new("error", "grammar", e.to_string()));
        }
        for e in &self.spore_errors {
            out.push(Diagnostic::new("error", "spore", e.clone()));
        }
        for l in &self.config.languages {
            if !aneural_core::config::Language::ALL.contains(&l.as_str()) {
                out.push(Diagnostic::new(
                    "warning",
                    "config",
                    format!("unknown language `{l}` in config.languages"),
                ));
            }
        }
        for s in &self.config.spores.enabled {
            if !self.spores.iter().any(|sp| &sp.manifest.name == s) {
                out.push(Diagnostic::new(
                    "warning",
                    "spore",
                    format!("enabled spore `{s}` is not installed"),
                ));
            }
        }
        for def in self.node_types()? {
            if !aneural_icons::is_valid(&def.icon) {
                out.push(Diagnostic::new(
                    "warning",
                    "icon",
                    format!(
                        "node type `{}` uses unknown icon `{}` (falling back to {})",
                        def.kind,
                        def.icon,
                        NodeTypeDef::FALLBACK_ICON
                    ),
                ));
            }
        }
        for rec in self.store.all_files()? {
            if !self.ws.abs(&rec.path).exists() {
                out.push(
                    Diagnostic::new(
                        "warning",
                        "cache",
                        "indexed file no longer exists (run `aneural index`)",
                    )
                    .at(rec.path),
                );
            }
        }
        for u in self.store.list_unresolved(Some(500))? {
            out.push(
                Diagnostic::new(
                    "info",
                    "unresolved",
                    format!("`{}` (line {}): {}", u.specifier, u.line, u.reason),
                )
                .at(u.origin),
            );
        }
        Ok(out)
    }
}

enum Processed {
    Indexed(GraphDelta),
    Unchanged(GraphDelta),
}

impl Processed {
    fn into_delta(self) -> GraphDelta {
        match self {
            Processed::Indexed(d) | Processed::Unchanged(d) => d,
        }
    }
}

fn is_markdown(rel: &str) -> bool {
    matches!(rel.rsplit('.').next(), Some("md" | "mdx" | "markdown"))
}

/// Convenience for the GUI: run a full index then watch, forwarding events on
/// `tx`, until `commands` yields `Stop` or is dropped.
pub fn run(root: &Path, tx: Sender<EngineEvent>, commands: Receiver<EngineCommand>) {
    run_with(root, tx, commands, None)
}

/// As [`run`], but with something that can make web requests on a spore's
/// behalf. A host that passes `None` — the MCP server, a one-shot query — simply
/// has no tier-1 spores, and they say so rather than reporting an empty result.
pub fn run_with(
    root: &Path,
    tx: Sender<EngineEvent>,
    commands: Receiver<EngineCommand>,
    fetcher: Option<Box<dyn Fetcher>>,
) {
    let ws = Workspace::at(root);
    let mut sink = |ev: EngineEvent| {
        let _ = tx.send(ev);
    };
    let mut engine = match Engine::open(ws) {
        Ok(e) => e,
        Err(e) => {
            sink(EngineEvent::Error(e.to_string()));
            return;
        }
    };
    if let Some(f) = fetcher {
        engine.set_fetcher(f);
    }
    sink(EngineEvent::Spores {
        spores: engine.spores(),
        errors: engine.spore_errors().to_vec(),
    });
    if let Err(e) = engine.index_full(false, &mut sink) {
        sink(EngineEvent::Error(e.to_string()));
    }
    if let Err(e) = engine.watch_loop(&mut sink, commands) {
        sink(EngineEvent::Error(e.to_string()));
    }
}
