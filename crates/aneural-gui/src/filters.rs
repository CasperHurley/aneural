//! Visibility filters and focus mode → `Hidden` markers.

use crate::engine::IndexStatus;
use crate::graph::{GraphNode, GraphState, Hidden};
use crate::picking::Selection;
use aneural_core::NodeId;
use bevy::prelude::*;
use std::collections::HashSet;

#[derive(Resource, Debug, Clone)]
pub struct Filters {
    /// Kinds hidden (empty = show all). Stored as an exclusion set so new kinds default to visible.
    pub hidden_kinds: HashSet<String>,
    pub hidden_edge_kinds: HashSet<String>,
    /// Repo ids shown (empty = all).
    pub repos: HashSet<NodeId>,
    pub query: String,
    pub show_structural_edges: bool,
    pub focus_mode: bool,
    pub neighborhood_depth: u32,
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
            show_structural_edges: true,
            focus_mode: false,
            neighborhood_depth: 1,
            last_generation: 0,
            dirty: true,
        }
    }
}

impl Filters {
    pub fn kind_visible(&self, kind: &str) -> bool {
        !self.hidden_kinds.contains(kind)
    }
    pub fn edge_visible(&self, kind: &str) -> bool {
        if kind == "CONTAINS" && !self.show_structural_edges {
            return false;
        }
        !self.hidden_edge_kinds.contains(kind)
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
        let all = aneural_core::kinds::EdgeKind::ALL;
        if self.hidden_edge_kinds.is_empty() && self.show_structural_edges {
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
            Some(graph.neighborhood(&roots, filters.neighborhood_depth, &|k| f.edge_visible(k)))
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
