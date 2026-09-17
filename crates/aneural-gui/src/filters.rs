//! Visibility filters and focus mode → `Hidden` markers.

use crate::engine::IndexStatus;
use crate::graph::{GraphNode, GraphState, Hidden};
use crate::picking::Selection;
use aneural_core::NodeId;
use aneural_core::kinds::EdgeKind;
use bevy::prelude::*;
use std::collections::HashSet;

/// A link between two nodes that each already have more than this many of the
/// same kind is held back until one of its ends is picked up. Ordinary code
/// structure is well under it, so only a genuine mesh is thinned out and small
/// workspaces lose nothing.
const CROWD_LIMIT: u32 = 8;

/// How far focus reaches around what is selected: the immediate neighbours,
/// and no dial to turn.
pub const FOCUS_DEPTH: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum PointedKind {
    Node(String),
    Edge(String),
}

#[derive(Resource, Debug, Clone)]
pub struct Filters {
    /// Kinds hidden (empty = show all). Stored as an exclusion set so new kinds default to visible.
    pub hidden_kinds: HashSet<String>,
    /// Edge kinds hidden. Only toggleable kinds ever land here.
    pub hidden_edge_kinds: HashSet<String>,
    /// Repo ids shown (empty = all).
    pub repos: HashSet<NodeId>,
    pub query: String,
    /// The kind row the pointer is resting on in the Filters drawer, lighting
    /// up everything of that kind while it stays there.
    pub pointed: Option<PointedKind>,
    /// Show only what is selected and its neighbours — the same slice that
    /// goes to the assistant as the focus.
    pub focus_mode: bool,
    pub last_generation: u64,
    pub dirty: bool,
}

impl Default for Filters {
    fn default() -> Self {
        Filters {
            hidden_kinds: HashSet::new(),
            hidden_edge_kinds: HashSet::new(),
            repos: HashSet::new(),
            query: String::new(),
            pointed: None,
            focus_mode: false,
            last_generation: 0,
            dirty: true,
        }
    }
}

impl Filters {
    pub fn kind_visible(&self, kind: &str) -> bool {
        !self.hidden_kinds.contains(kind)
    }
    /// Is this link one of the ones a crowd holds back? `crowd` is the smaller
    /// of the two ends' link counts for this kind.
    pub fn decluttered(&self, kind: &str, crowd: u32) -> bool {
        kind != "CONTAINS" && crowd > CROWD_LIMIT
    }
    pub fn edge_visible(&self, kind: &str) -> bool {
        !EdgeKind::is_toggleable(kind) || !self.hidden_edge_kinds.contains(kind)
    }
    pub fn toggle_kind(&mut self, kind: &str) {
        if !self.hidden_kinds.remove(kind) {
            self.hidden_kinds.insert(kind.to_string());
        }
        self.dirty = true;
    }
    pub fn toggle_edge_kind(&mut self, kind: &str) {
        if !self.hidden_edge_kinds.remove(kind) {
            self.hidden_edge_kinds.insert(kind.to_string());
        }
        self.dirty = true;
    }
    /// Kinds as an inclusion list for focus.json (empty = all).
    pub fn kinds_list(&self, all_kinds: &[String]) -> Vec<String> {
        if self.hidden_kinds.is_empty() {
            Vec::new()
        } else {
            all_kinds
                .iter()
                .filter(|k| self.kind_visible(k))
                .cloned()
                .collect()
        }
    }
    pub fn edge_kinds_list(&self) -> Vec<String> {
        let all = EdgeKind::ALL;
        if self.hidden_edge_kinds.is_empty() {
            Vec::new()
        } else {
            all.iter()
                .filter(|k| self.edge_visible(k))
                .map(|k| k.to_string())
                .collect()
        }
    }
}

pub struct FiltersPlugin;

impl Plugin for FiltersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Filters>()
            .add_systems(Update, apply_filters);
    }
}

fn apply_filters(
    mut commands: Commands,
    mut filters: ResMut<Filters>,
    selection: Res<Selection>,
    status: Res<IndexStatus>,
    graph: Res<GraphState>,
    nodes: Query<(Entity, &GraphNode, Has<Hidden>)>,
) {
    let graph_changed = status.generation != filters.last_generation;
    if !filters.dirty && !selection.is_changed() && !graph_changed {
        return;
    }
    filters.last_generation = status.generation;
    filters.dirty = false;
    let q = filters.query.trim().to_lowercase();
    let focus_set: Option<HashSet<NodeId>> = if filters.focus_mode {
        let mut roots: Vec<NodeId> = selection.pinned.clone();
        if let Some(p) = &selection.primary
            && !roots.contains(p)
        {
            roots.push(p.clone());
        }
        if roots.is_empty() {
            None
        } else {
            let f = filters.clone();
            Some(graph.neighborhood(&roots, FOCUS_DEPTH, &|k| f.edge_visible(k)))
        }
    } else {
        None
    };
    for (e, gn, was_hidden) in &nodes {
        let mut visible = filters.kind_visible(&gn.kind);
        if visible && !filters.repos.is_empty() {
            let in_repo = gn
                .repo_id
                .as_ref()
                .is_some_and(|r| filters.repos.contains(r))
                || filters.repos.contains(&gn.id);
            visible = in_repo || gn.kind == "Package";
        }
        if visible && !q.is_empty() {
            visible = gn.label.to_lowercase().contains(&q)
                || gn
                    .path
                    .as_deref()
                    .is_some_and(|p| p.to_lowercase().contains(&q));
        }
        if visible && let Some(set) = &focus_set {
            visible = set.contains(&gn.id);
        }
        if visible && was_hidden {
            commands.entity(e).remove::<Hidden>();
        } else if !visible && !was_hidden {
            commands.entity(e).insert(Hidden);
        }
    }
}
