//! Where every node goes: a mycelium spreading out from the workspace root.
//!
//! The shape is decided, not simulated. Each node gets a slice of the circle
//! in proportion to how much grows from it, and sits further out than its
//! parent, inside its parent's slice. Slices never overlap, so a branch cannot
//! fold back across another and nothing grows towards the middle. Nodes
//! outside the folder tree (packages, comments, notes, tables) grow from the
//! file they belong to, or from the deepest folder shared by everything they
//! touch. The simulation only eases nodes to their places and nudges apart any
//! that would land on top of each other.

use crate::engine::IndexStatus;
use crate::graph::{GraphNode, GraphState, Hidden, Pinned, Pos, Vel, hash01};
use aneural_core::NodeId;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

/// The shortest a hypha grows from its parent.
const STEP: f32 = 70.0;
/// The longest, however narrow its slice.
const MAX_STEP: f32 = 320.0;
/// Room a node wants along its arc, so neighbours in narrow slices keep apart.
const MIN_GAP: f32 = 30.0;
/// How much further out every other leaf of a parent sits, as a share of
/// [`STEP`]: a folder of many files fans into two staggered rows instead of
/// one packed arc.
const STAGGER: f32 = 0.5;
/// Nodes closer than this push apart.
const PERSONAL_SPACE: f32 = 34.0;

#[derive(Resource)]
pub struct LayoutParams {
    /// How hard a node is drawn to its place.
    pub pull: f32,
    /// How hard two nodes inside each other's personal space push apart.
    pub push: f32,
    pub damping: f32,
    pub max_speed: f32,
    pub epsilon: f32,
    /// How much the nodes are still allowed to jostle, 1 down to nothing.
    /// Every tick cools it a little, so the layout always comes to rest.
    pub alpha: f32,
    pub alpha_decay: f32,
    pub frozen: bool,
    pub last_generation: u64,
}

impl LayoutParams {
    /// Reheat: every node heads back to its place and settles again.
    pub fn stir(&mut self) {
        self.frozen = false;
        self.alpha = 1.0;
    }

    /// Just warm enough to carry a dragged node's branch along with it.
    pub fn nudge(&mut self) {
        self.frozen = false;
        self.alpha = self.alpha.max(0.3);
    }
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams {
            pull: 0.08,
            push: 3.0,
            damping: 0.8,
            max_speed: 40.0,
            epsilon: 0.05,
            alpha: 1.0,
            alpha_decay: 0.02,
            frozen: false,
            last_generation: 0,
        }
    }
}

pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LayoutParams>()
            .insert_resource(Time::<Fixed>::from_hz(64.0))
            .add_systems(FixedUpdate, step);
    }
}

/// A node's place in the mycelium.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// The node it grows from; `None` for a root.
    pub host: Option<NodeId>,
    /// Where it sits while nothing has been dragged.
    pub at: Vec2,
}

/// Lay `nodes` out as a radial tree. `parent` is the folder tree and
/// `neighbors` everything a node is linked to. Hosts come before the nodes
/// that grow from them.
pub fn radial(
    nodes: &[NodeId],
    parent: &HashMap<NodeId, NodeId>,
    neighbors: &dyn Fn(&NodeId) -> Vec<NodeId>,
) -> Vec<(NodeId, Place)> {
    let present: HashSet<&NodeId> = nodes.iter().collect();
    let in_tree = |id: &NodeId| id.is_dir() || id.is_file();

    // The folder tree first: a node's parent, or failing that (its CONTAINS
    // edge has not arrived yet) the nearest folder above it by path.
    let mut host: HashMap<NodeId, Option<NodeId>> = HashMap::new();
    for id in nodes.iter().filter(|id| in_tree(id)) {
        let h = parent
            .get(id)
            .filter(|p| present.contains(p))
            .cloned()
            .or_else(|| folder_above(id.path_part(), &present));
        host.insert(id.clone(), h);
    }
    let chain = |id: &NodeId| -> Vec<NodeId> {
        let mut out = vec![id.clone()];
        while let Some(Some(h)) = host.get(out.last().unwrap()) {
            if out.len() > host.len() || out.contains(h) {
                break;
            }
            out.push(h.clone());
        }
        out
    };

    // Everything else grows from the deepest folder-tree node shared by all
    // it touches: the file for a comment, the folder two importers share for
    // a package.
    let mut extra: Vec<(NodeId, Option<NodeId>)> = Vec::new();
    for id in nodes.iter().filter(|id| !in_tree(id)) {
        let touched: Vec<NodeId> = neighbors(id)
            .into_iter()
            .filter(|n| in_tree(n) && present.contains(n))
            .collect();
        let h = match touched.split_first() {
            Some((first, rest)) => {
                let mut common = chain(first);
                for other in rest {
                    let theirs: HashSet<NodeId> = chain(other).into_iter().collect();
                    let keep = common.iter().position(|a| theirs.contains(a));
                    common = keep.map(|i| common.split_off(i)).unwrap_or_default();
                }
                common.into_iter().next()
            }
            None => {
                let own = NodeId::file(id.path_part());
                if present.contains(&own) {
                    Some(own)
                } else {
                    folder_above(id.path_part(), &present)
                }
            }
        };
        extra.push((id.clone(), h));
    }
    host.extend(extra);

    let mut children: HashMap<Option<NodeId>, Vec<NodeId>> = HashMap::new();
    for (id, h) in &host {
        children.entry(h.clone()).or_default().push(id.clone());
    }
    for list in children.values_mut() {
        list.sort();
    }
    let kids = |id: &Option<NodeId>| children.get(id).map(Vec::as_slice).unwrap_or(&[]);

    // How much grows from each node, counted in leaves: the width its slice needs.
    let mut order: Vec<NodeId> = Vec::new();
    let mut stack: Vec<NodeId> = kids(&None).to_vec();
    while let Some(id) = stack.pop() {
        stack.extend(kids(&Some(id.clone())).iter().cloned());
        order.push(id);
    }
    let mut weight: HashMap<NodeId, f32> = HashMap::new();
    for id in order.iter().rev() {
        let below: f32 = kids(&Some(id.clone())).iter().map(|k| weight[k]).sum();
        weight.insert(id.clone(), below.max(1.0));
    }

    let mut out: Vec<(NodeId, Place)> = Vec::with_capacity(host.len());
    // (host, its radius, where its slice starts, how wide it is)
    let mut todo: Vec<(Option<NodeId>, f32, f32, f32)> = Vec::new();
    match kids(&None) {
        [root] => {
            out.push((
                root.clone(),
                Place {
                    host: None,
                    at: Vec2::ZERO,
                },
            ));
            todo.push((Some(root.clone()), 0.0, 0.0, TAU));
        }
        _ => todo.push((None, 0.0, 0.0, TAU)),
    }
    while let Some((h, radius, start, span)) = todo.pop() {
        let list = kids(&h);
        let total: f32 = list.iter().map(|k| weight[k]).sum();
        let mut cursor = start;
        let mut leaves = 0;
        for k in list {
            let slice = span * weight[k] / total;
            let mut r = (radius + STEP)
                .max(MIN_GAP / slice.max(1e-3))
                .min(radius + MAX_STEP);
            if kids(&Some(k.clone())).is_empty() {
                if leaves % 2 == 1 {
                    r += STEP * STAGGER;
                }
                leaves += 1;
            }
            // A little wander keeps it organic without leaving the slice.
            let name = k.as_str();
            r += (hash01(name, 5) - 0.5) * STEP * 0.2;
            let angle = cursor + slice * (0.5 + (hash01(name, 6) - 0.5) * 0.3);
            out.push((
                k.clone(),
                Place {
                    host: h.clone(),
                    at: Vec2::from_angle(angle) * r,
                },
            ));
            todo.push((Some(k.clone()), r, cursor, slice));
            cursor += slice;
        }
    }
    out
}

/// The nearest folder above `path` that is in the graph.
fn folder_above(path: &str, present: &HashSet<&NodeId>) -> Option<NodeId> {
    let mut path = path;
    while path != "." && !path.is_empty() {
        path = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        let dir = NodeId::dir(path);
        if present.contains(&dir) {
            return Some(dir);
        }
    }
    None
}

/// The places last worked out, and the shape of the graph they were worked
/// out for.
#[derive(Default)]
struct Plan {
    shape: (u64, usize, usize),
    /// (node, the node it grows from, where it sits) in host-first order.
    places: Vec<(Entity, Option<Entity>, Vec2)>,
}

fn step(
    mut params: ResMut<LayoutParams>,
    status: Res<IndexStatus>,
    graph: Res<GraphState>,
    mut plan: Local<Plan>,
    mut nodes: Query<(Entity, &mut Pos, &mut Vel, Has<Pinned>, Has<Hidden>), With<GraphNode>>,
) {
    if status.generation != params.last_generation {
        params.last_generation = status.generation;
        params.stir();
    }
    let shape = (status.generation, graph.node_count, graph.edge_count);
    if plan.shape != shape {
        plan.shape = shape;
        let ids: Vec<NodeId> = graph.by_id.keys().cloned().collect();
        let neighbors = |id: &NodeId| graph.neighbors(id).iter().map(|(n, _)| n.clone()).collect();
        plan.places = radial(&ids, &graph.parent, &neighbors)
            .into_iter()
            .filter_map(|(id, p)| {
                let e = *graph.by_id.get(&id)?;
                let host = p.host.and_then(|h| graph.by_id.get(&h).copied());
                Some((e, host, p.at))
            })
            .collect();
        params.stir();
    }
    if params.frozen {
        return;
    }

    // A dragged node stays where it was left and takes its branch with it:
    // everything growing from it shifts by however far it was moved.
    let mut shift: HashMap<Entity, Vec2> = HashMap::with_capacity(plan.places.len());
    let mut target: HashMap<Entity, Vec2> = HashMap::with_capacity(plan.places.len());
    for (e, host, at) in &plan.places {
        let inherited = host
            .and_then(|h| shift.get(&h).copied())
            .unwrap_or(Vec2::ZERO);
        let own = match nodes.get(*e) {
            Ok((_, pos, _, true, _)) => pos.0 - *at,
            _ => inherited,
        };
        shift.insert(*e, own);
        target.insert(*e, *at + inherited);
    }

    let snapshot: Vec<(Entity, Vec2, bool)> = nodes
        .iter()
        .filter(|(.., hidden)| !hidden)
        .map(|(e, p, _, pinned, _)| (e, p.0, pinned))
        .collect();
    let n = snapshot.len();
    if n == 0 {
        return;
    }
    let mut force: Vec<Vec2> = snapshot
        .iter()
        .map(|(e, p, _)| (target.get(e).copied().unwrap_or(*p) - *p) * params.pull)
        .collect();

    // Personal space, found through a grid so only near neighbours are compared.
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let cell = |p: Vec2| {
        (
            (p.x / PERSONAL_SPACE).floor() as i32,
            (p.y / PERSONAL_SPACE).floor() as i32,
        )
    };
    for (i, (_, p, _)) in snapshot.iter().enumerate() {
        grid.entry(cell(*p)).or_default().push(i);
    }
    for (i, (_, p, _)) in snapshot.iter().enumerate() {
        let (cx, cy) = cell(*p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    if i == j {
                        continue;
                    }
                    let d = *p - snapshot[j].1;
                    let dist = d.length();
                    if dist >= PERSONAL_SPACE {
                        continue;
                    }
                    // Two nodes on the very same spot part in directions of
                    // their own rather than not at all.
                    let away = if dist > 1e-3 {
                        d / dist
                    } else {
                        Vec2::from_angle((i as f32 - j as f32) * 2.399)
                    };
                    force[i] += away * params.push * params.alpha * (1.0 - dist / PERSONAL_SPACE);
                }
            }
        }
    }

    let mut energy = 0.0;
    for (i, (e, _, pinned)) in snapshot.iter().enumerate() {
        let Ok((_, mut pos, mut vel, ..)) = nodes.get_mut(*e) else {
            continue;
        };
        if *pinned {
            vel.0 = Vec2::ZERO;
            continue;
        }
        let v = ((vel.0 + force[i]) * params.damping).clamp_length_max(params.max_speed);
        pos.0 += v;
        vel.0 = v;
        energy += v.length_squared();
    }
    params.alpha *= 1.0 - params.alpha_decay;
    // Still and cool: it has arrived.
    if params.alpha < 0.5 && energy / (n as f32) < params.epsilon {
        params.frozen = true;
        debug!("layout settled ({n} nodes, alpha {:.3})", params.alpha);
        for (_, _, mut vel, ..) in &mut nodes {
            vel.0 = Vec2::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Links = HashMap<NodeId, Vec<NodeId>>;

    /// A workspace shaped like the ones people have: a few top-level folders,
    /// one deep and busy, files at every level, and a package and a comment
    /// hanging off the tree.
    fn workspace() -> (Vec<NodeId>, HashMap<NodeId, NodeId>, Links) {
        let dirs = [
            "apps",
            "apps/web",
            "apps/web/src",
            "apps/web/src/lib",
            "services",
            "services/api",
            "docs",
        ];
        let files = [
            "README.md",
            "apps/web/package.json",
            "apps/web/src/index.ts",
            "apps/web/src/app.ts",
            "apps/web/src/types.ts",
            "apps/web/src/styles.css",
            "apps/web/src/lib/util.ts",
            "apps/web/src/lib/format.ts",
            "services/api/main.py",
            "services/api/db.py",
            "docs/overview.md",
        ];
        let up = |p: &str| {
            p.rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or(".")
                .to_string()
        };
        let mut nodes = vec![NodeId::dir(".")];
        let mut parent = HashMap::new();
        for d in dirs {
            nodes.push(NodeId::dir(d));
            parent.insert(NodeId::dir(d), NodeId::dir(up(d)));
        }
        for f in files {
            nodes.push(NodeId::file(f));
            parent.insert(NodeId::file(f), NodeId::dir(up(f)));
        }
        let react = NodeId::package("npm", "react");
        let todo = NodeId::new("comment:apps/web/src/app.ts#abc");
        nodes.push(react.clone());
        nodes.push(todo.clone());
        let mut links = Links::new();
        links.insert(
            react,
            vec![
                NodeId::file("apps/web/src/index.ts"),
                NodeId::file("apps/web/src/lib/util.ts"),
            ],
        );
        links.insert(todo, vec![NodeId::file("apps/web/src/app.ts")]);
        (nodes, parent, links)
    }

    fn lay_out() -> HashMap<NodeId, Place> {
        let (nodes, parent, links) = workspace();
        radial(&nodes, &parent, &|id| {
            links.get(id).cloned().unwrap_or_default()
        })
        .into_iter()
        .collect()
    }

    #[test]
    fn everything_grows_outward_from_the_root() {
        let places = lay_out();
        assert_eq!(places[&NodeId::dir(".")].at, Vec2::ZERO);
        for (id, p) in &places {
            if let Some(h) = &p.host {
                let (mine, theirs) = (p.at.length(), places[h].at.length());
                assert!(
                    mine > theirs + STEP * 0.5,
                    "{id} ({mine}) is not outside {h} ({theirs})"
                );
            }
        }
    }

    #[test]
    fn hyphae_never_cross() {
        let places = lay_out();
        let segments: Vec<(&NodeId, Vec2, Vec2)> = places
            .iter()
            .filter_map(|(id, p)| Some((id, places[p.host.as_ref()?].at, p.at)))
            .collect();
        for (i, (a, a0, a1)) in segments.iter().enumerate() {
            for (b, b0, b1) in &segments[i + 1..] {
                // hyphae that share an end meet there, which is not a crossing
                let shared = [a0, a1]
                    .iter()
                    .any(|p| p.distance(*b0) < 1e-3 || p.distance(*b1) < 1e-3);
                if !shared {
                    assert!(!crosses(*a0, *a1, *b0, *b1), "{a} crosses {b}");
                }
            }
        }
    }

    #[test]
    fn strays_grow_from_what_they_touch() {
        let places = lay_out();
        let host = |id: NodeId| places[&id].host.clone();
        assert_eq!(
            host(NodeId::new("comment:apps/web/src/app.ts#abc")),
            Some(NodeId::file("apps/web/src/app.ts"))
        );
        assert_eq!(
            host(NodeId::package("npm", "react")),
            Some(NodeId::dir("apps/web/src")),
            "the deepest folder both importers share"
        );
    }

    #[test]
    fn hosts_come_before_what_grows_from_them() {
        let (nodes, parent, links) = workspace();
        let order = radial(&nodes, &parent, &|id| {
            links.get(id).cloned().unwrap_or_default()
        });
        assert_eq!(order.len(), nodes.len());
        let mut seen = HashSet::new();
        for (id, p) in &order {
            if let Some(h) = &p.host {
                assert!(seen.contains(h), "{id} placed before its host {h}");
            }
            seen.insert(id.clone());
        }
    }

    fn crosses(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
        let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
        side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
    }
}
