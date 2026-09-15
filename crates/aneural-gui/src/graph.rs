//! In-memory graph ECS state and delta application.

use crate::render::{IconAtlas, spawn_node_visuals};
use crate::workspace::WorkspaceRes;
use aneural_core::{Edge, GraphDelta, Node, NodeId};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Component, Clone, Debug)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: String,
    pub label: String,
    pub path: Option<String>,
    pub repo_id: Option<NodeId>,
    pub props: serde_json::Value,
}

#[derive(Component, Clone, Debug)]
pub struct GraphEdge {
    pub kind: String,
    pub src: Entity,
    pub dst: Entity,
    pub src_id: NodeId,
    pub dst_id: NodeId,
    pub seed: u32,
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Pos(pub Vec2);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Vel(pub Vec2);

#[derive(Component, Clone, Copy, Debug)]
pub struct Mass(pub f32);

#[derive(Component)]
pub struct Pinned;

#[derive(Component)]
pub struct Hidden;

#[derive(Component, Clone, Copy, Debug)]
pub struct GrowIn {
    pub t: f32,
    pub dur: f32,
}

impl Default for GrowIn {
    fn default() -> Self {
        GrowIn { t: 0.0, dur: 0.45 }
    }
}

pub type EdgeKey = (String, NodeId, NodeId);

#[derive(Resource, Default)]
pub struct GraphState {
    pub by_id: HashMap<NodeId, Entity>,
    pub edges: HashMap<EdgeKey, Entity>,
    pub pending_edges: Vec<Edge>,
    /// Adjacency (undirected) for BFS/focus mode: id → (neighbor id, edge kind).
    pub adjacency: HashMap<NodeId, Vec<(NodeId, String)>>,
    /// Parent (CONTAINS src) per node, for sprouting and gravity.
    pub parent: HashMap<NodeId, NodeId>,
    pub node_count: usize,
    pub edge_count: usize,
}

impl GraphState {
    pub fn neighbors(&self, id: &NodeId) -> &[(NodeId, String)] {
        self.adjacency.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// BFS up to `depth` over edges whose kind passes `allow`.
    pub fn neighborhood(
        &self,
        roots: &[NodeId],
        depth: u32,
        allow: &dyn Fn(&str) -> bool,
    ) -> HashSet<NodeId> {
        let mut seen: HashSet<NodeId> = roots.iter().cloned().collect();
        let mut frontier: Vec<NodeId> = roots.to_vec();
        for _ in 0..depth {
            let mut next = Vec::new();
            for id in &frontier {
                for (n, kind) in self.neighbors(id) {
                    if allow(kind) && seen.insert(n.clone()) {
                        next.push(n.clone());
                    }
                }
            }
            frontier = next;
        }
        seen
    }
}

/// Deterministic pseudo-random in [0,1) from a string and a salt.
pub fn hash01(s: &str, salt: u32) -> f32 {
    let mut h: u32 = 2166136261 ^ salt.wrapping_mul(16777619);
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    (h as f32) / (u32::MAX as f32)
}

pub fn seed_of(s: &str) -> u32 {
    (hash01(s, 7) * u32::MAX as f32) as u32
}

#[allow(clippy::too_many_arguments)]
pub fn apply_delta(
    commands: &mut Commands,
    graph: &mut GraphState,
    ws: &WorkspaceRes,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    atlas: &IconAtlas,
    existing: &mut Query<(&mut GraphNode, &Pos)>,
    delta: GraphDelta,
) {
    // removals first
    for e in &delta.removed_edges {
        remove_edge(
            commands,
            graph,
            &(e.kind.clone(), e.src.clone(), e.dst.clone()),
        );
    }
    for id in &delta.removed_node_ids {
        remove_node(commands, graph, id);
    }
    // nodes
    for node in delta.nodes {
        upsert_node(
            commands, graph, ws, meshes, materials, atlas, existing, node,
        );
    }
    // edges
    for edge in delta.edges {
        add_edge(commands, graph, edge);
    }
    // retry parked edges
    if !graph.pending_edges.is_empty() {
        let parked = std::mem::take(&mut graph.pending_edges);
        for e in parked {
            add_edge(commands, graph, e);
        }
    }
}

fn parent_pos(
    graph: &GraphState,
    id: &NodeId,
    existing: &Query<(&mut GraphNode, &Pos)>,
) -> Option<Vec2> {
    let parent = graph.parent.get(id).cloned().or_else(|| {
        // derive parent from the path when the CONTAINS edge hasn't arrived
        let p = id.path_part();
        if p == "." || id.prefix() == "pkg" {
            return None;
        }
        let parent = p.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        Some(NodeId::dir(parent))
    })?;
    let e = graph.by_id.get(&parent)?;
    existing.get(*e).ok().map(|(_, p)| p.0)
}

#[allow(clippy::too_many_arguments)]
fn upsert_node(
    commands: &mut Commands,
    graph: &mut GraphState,
    ws: &WorkspaceRes,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    atlas: &IconAtlas,
    existing: &mut Query<(&mut GraphNode, &Pos)>,
    node: Node,
) {
    if let Some(&e) = graph.by_id.get(&node.id) {
        if let Ok((mut gn, _)) = existing.get_mut(e) {
            gn.label = node.label;
            gn.props = node.props;
            gn.kind = node.kind;
            gn.repo_id = node.repo_id;
            gn.path = node.path;
        }
        return;
    }
    let anchor = parent_pos(graph, &node.id, existing).unwrap_or(Vec2::ZERO);
    let angle = hash01(node.id.as_str(), 1) * std::f32::consts::TAU;
    let radius = 25.0 + 30.0 * hash01(node.id.as_str(), 2);
    let pos = anchor + Vec2::from_angle(angle) * radius;
    let gn = GraphNode {
        id: node.id.clone(),
        kind: node.kind.clone(),
        label: node.label.clone(),
        path: node.path.clone(),
        repo_id: node.repo_id.clone(),
        props: node.props.clone(),
    };
    let mass = match node.kind.as_str() {
        "Repo" => 4.0,
        "Directory" => 2.0,
        _ => 1.0,
    };
    let entity = commands
        .spawn((
            gn,
            Pos(pos),
            Vel::default(),
            Mass(mass),
            GrowIn::default(),
            Transform::from_translation(pos.extend(0.0)).with_scale(Vec3::splat(0.01)),
            Visibility::default(),
        ))
        .id();
    spawn_node_visuals(commands, entity, &node, ws, meshes, materials, atlas);
    graph.by_id.insert(node.id, entity);
    graph.node_count += 1;
}

fn remove_node(commands: &mut Commands, graph: &mut GraphState, id: &NodeId) {
    let Some(entity) = graph.by_id.remove(id) else {
        return;
    };
    let keys: Vec<EdgeKey> = graph
        .edges
        .keys()
        .filter(|(_, s, d)| s == id || d == id)
        .cloned()
        .collect();
    for k in keys {
        remove_edge(commands, graph, &k);
    }
    graph.adjacency.remove(id);
    graph.parent.remove(id);
    graph.pending_edges.retain(|e| &e.src != id && &e.dst != id);
    commands.entity(entity).despawn();
    graph.node_count = graph.node_count.saturating_sub(1);
}

fn add_edge(commands: &mut Commands, graph: &mut GraphState, edge: Edge) {
    let key: EdgeKey = (edge.kind.clone(), edge.src.clone(), edge.dst.clone());
    if graph.edges.contains_key(&key) {
        return;
    }
    let (Some(&s), Some(&d)) = (graph.by_id.get(&edge.src), graph.by_id.get(&edge.dst)) else {
        if edge.src != edge.dst {
            graph.pending_edges.push(edge);
        }
        return;
    };
    if s == d {
        return;
    }
    let seed = seed_of(&format!("{}{}{}", edge.kind, edge.src, edge.dst));
    let entity = commands
        .spawn((
            GraphEdge {
                kind: edge.kind.clone(),
                src: s,
                dst: d,
                src_id: edge.src.clone(),
                dst_id: edge.dst.clone(),
                seed,
            },
            GrowIn { t: 0.0, dur: 0.6 },
        ))
        .id();
    graph
        .adjacency
        .entry(edge.src.clone())
        .or_default()
        .push((edge.dst.clone(), edge.kind.clone()));
    graph
        .adjacency
        .entry(edge.dst.clone())
        .or_default()
        .push((edge.src.clone(), edge.kind.clone()));
    if edge.kind == "CONTAINS" {
        graph.parent.insert(edge.dst.clone(), edge.src.clone());
    }
    graph.edges.insert(key, entity);
    graph.edge_count += 1;
}

fn remove_edge(commands: &mut Commands, graph: &mut GraphState, key: &EdgeKey) {
    let Some(entity) = graph.edges.remove(key) else {
        return;
    };
    let (kind, src, dst) = key;
    if let Some(v) = graph.adjacency.get_mut(src) {
        v.retain(|(n, k)| !(n == dst && k == kind));
    }
    if let Some(v) = graph.adjacency.get_mut(dst) {
        v.retain(|(n, k)| !(n == src && k == kind));
    }
    if kind == "CONTAINS" && graph.parent.get(dst) == Some(src) {
        graph.parent.remove(dst);
    }
    commands.entity(entity).despawn();
    graph.edge_count = graph.edge_count.saturating_sub(1);
}

/// Advance grow-in tweens; apply scale to nodes.
pub fn tick_grow_in(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut GrowIn, Option<&mut Transform>)>,
) {
    let dt = time.delta_secs();
    for (e, mut g, transform) in &mut q {
        g.t = (g.t + dt / g.dur).min(1.0);
        let done = g.t >= 1.0;
        if let Some(mut t) = transform {
            t.scale = if done {
                Vec3::ONE
            } else {
                Vec3::splat(ease_out_back(g.t).max(0.01))
            };
        }
        if done {
            commands.entity(e).remove::<GrowIn>();
        }
    }
}

pub fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Copy layout positions into transforms.
pub fn sync_transforms(mut q: Query<(&Pos, &mut Transform), With<GraphNode>>) {
    for (p, mut t) in &mut q {
        t.translation.x = p.0.x;
        t.translation.y = p.0.y;
    }
}
