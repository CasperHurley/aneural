//! Force-directed layout with parent gravity (keeps the tree radial/fungal).

use crate::engine::IndexStatus;
use crate::graph::{GraphEdge, GraphNode, GraphState, Hidden, Mass, Pinned, Pos, Vel};
use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Resource)]
pub struct LayoutParams {
    pub repulsion: f32,
    pub spring: f32,
    pub gravity: f32,
    pub center_pull: f32,
    pub damping: f32,
    pub max_speed: f32,
    pub epsilon: f32,
    /// How hard the forces push, 1 down to nothing. Every tick cools it a
    /// little, so the layout always arrives somewhere instead of boiling.
    pub alpha: f32,
    pub alpha_decay: f32,
    pub frozen: bool,
    pub last_generation: u64,
}

impl LayoutParams {
    /// Reheat: the graph rearranges from wherever it is and settles again.
    pub fn stir(&mut self) {
        self.frozen = false;
        self.alpha = 1.0;
    }

    /// Just warm enough to follow a dragged node without flinging the rest.
    pub fn nudge(&mut self) {
        self.frozen = false;
        self.alpha = self.alpha.max(0.3);
    }
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams {
            repulsion: 2600.0,
            spring: 0.05,
            gravity: 0.02,
            center_pull: 0.002,
            damping: 0.85,
            max_speed: 40.0,
            epsilon: 0.15,
            alpha: 1.0,
            alpha_decay: 0.022,
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

fn rest_length(kind: &str) -> f32 {
    match kind {
        "CONTAINS" => 60.0,
        "IMPORTS" | "RE_EXPORTS" => 140.0,
        _ => 110.0,
    }
}

fn step(
    mut params: ResMut<LayoutParams>,
    status: Res<IndexStatus>,
    graph: Res<GraphState>,
    mut nodes: Query<
        (Entity, &mut Pos, &mut Vel, &Mass, Has<Pinned>, Has<Hidden>),
        With<GraphNode>,
    >,
    edges: Query<&GraphEdge>,
) {
    if status.generation != params.last_generation {
        params.last_generation = status.generation;
        params.stir();
    }
    if params.frozen {
        return;
    }
    let snapshot: Vec<(Entity, Vec2, f32, bool)> = nodes
        .iter()
        .filter(|(_, _, _, _, _, hidden)| !hidden)
        .map(|(e, p, _, m, pinned, _)| (e, p.0, m.0, pinned))
        .collect();
    let n = snapshot.len();
    if n == 0 {
        return;
    }
    // A thousand nodes need more room than a hundred: stretch the rest lengths
    // and push harder, so a big workspace spreads instead of balling up.
    let spread = (n as f32 / 120.0).sqrt().clamp(1.0, 3.0);
    let index: HashMap<Entity, usize> = snapshot
        .iter()
        .enumerate()
        .map(|(i, (e, ..))| (*e, i))
        .collect();
    let mut force = vec![Vec2::ZERO; n];

    // repulsion — spatial hash for large graphs
    let cell = 120.0_f32 * spread;
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, (_, p, _, _)) in snapshot.iter().enumerate() {
        grid.entry(((p.x / cell).floor() as i32, (p.y / cell).floor() as i32))
            .or_default()
            .push(i);
    }
    let use_grid = n > 600;
    for i in 0..n {
        let pi = snapshot[i].1;
        let visit = |j: usize, force: &mut Vec<Vec2>| {
            if j == i {
                return;
            }
            let d = pi - snapshot[j].1;
            let dist2 = d.length_squared().max(4.0);
            if use_grid && dist2 > (cell * 2.0) * (cell * 2.0) {
                return;
            }
            // symmetric in (i, j) so the pair exerts no net force or torque
            let f =
                params.repulsion * spread * spread * (snapshot[i].2 * snapshot[j].2).sqrt() / dist2;
            force[i] += d.normalize_or_zero() * f.min(60.0 * spread);
        };
        if use_grid {
            let cx = (pi.x / cell).floor() as i32;
            let cy = (pi.y / cell).floor() as i32;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(bucket) = grid.get(&(cx + dx, cy + dy)) {
                        for &j in bucket {
                            visit(j, &mut force);
                        }
                    }
                }
            }
        } else {
            for j in 0..n {
                visit(j, &mut force);
            }
        }
    }

    // springs + parent gravity
    for e in &edges {
        let (Some(&i), Some(&j)) = (index.get(&e.src), index.get(&e.dst)) else {
            continue;
        };
        let d = snapshot[j].1 - snapshot[i].1;
        let dist = d.length().max(0.01);
        let stretch = dist - rest_length(&e.kind) * spread;
        let f = d / dist * (stretch * params.spring).clamp(-30.0, 30.0);
        force[i] += f;
        force[j] -= f;
        if e.kind == "CONTAINS" {
            // child and parent are pulled gently together (equal and opposite,
            // so the tree does not pick up angular momentum)
            force[j] -= d * params.gravity;
            force[i] += d * params.gravity;
        }
    }

    // centering
    let centroid = snapshot.iter().map(|s| s.1).sum::<Vec2>() / n as f32;
    for (i, s) in snapshot.iter().enumerate() {
        force[i] -= (s.1 - centroid) * params.center_pull / spread;
    }

    // integrate velocities
    let mut next_v = vec![Vec2::ZERO; n];
    for (i, s) in snapshot.iter().enumerate() {
        if s.3 {
            continue; // pinned
        }
        let vel = nodes.get(s.0).map(|(_, _, v, ..)| v.0).unwrap_or_default();
        let mut v = (vel + force[i] * params.alpha) * params.damping;
        if v.length() > params.max_speed {
            v = v.normalize() * params.max_speed;
        }
        next_v[i] = v;
    }

    // Remove any rigid-body motion of the whole graph (drift + rotation about
    // the centroid). Without this, tiny asymmetries accumulate into a slow
    // perpetual spin that never settles and makes nodes wander off-screen.
    let free: Vec<usize> = (0..n).filter(|&i| !snapshot[i].3).collect();
    if free.len() > 1 {
        let m = free.len() as f32;
        let com = free.iter().map(|&i| snapshot[i].1).sum::<Vec2>() / m;
        let mean_v = free.iter().map(|&i| next_v[i]).sum::<Vec2>() / m;
        let mut ang_mom = 0.0;
        let mut inertia = 0.0;
        for &i in &free {
            let r = snapshot[i].1 - com;
            ang_mom += r.perp_dot(next_v[i]);
            inertia += r.length_squared();
        }
        let omega = if inertia > 1.0 {
            ang_mom / inertia
        } else {
            0.0
        };
        for &i in &free {
            let r = snapshot[i].1 - com;
            next_v[i] -= mean_v + r.perp() * omega;
        }
    }

    let mut energy = 0.0;
    for (e, mut pos, mut vel, _, pinned, hidden) in &mut nodes {
        if hidden {
            continue;
        }
        let Some(&i) = index.get(&e) else { continue };
        if pinned {
            vel.0 = Vec2::ZERO;
            continue;
        }
        let v = next_v[i];
        pos.0 += v;
        vel.0 = v;
        energy += v.length_squared();
    }
    params.alpha *= 1.0 - params.alpha_decay;
    // cold, or still: either way it has arrived
    if params.alpha < 0.02 || energy / (n as f32) < params.epsilon {
        params.frozen = true;
        debug!(
            "layout settled ({n} nodes, alpha {:.3}, energy/n {:.3})",
            params.alpha,
            energy / (n as f32)
        );
        for (_, _, mut vel, ..) in &mut nodes {
            vel.0 = Vec2::ZERO;
        }
    }
    let _ = &graph;
}
