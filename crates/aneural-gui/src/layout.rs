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
    pub frozen: bool,
    pub last_generation: u64,
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
        params.frozen = false;
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
    let index: HashMap<Entity, usize> = snapshot
        .iter()
        .enumerate()
        .map(|(i, (e, ..))| (*e, i))
        .collect();
    let mut force = vec![Vec2::ZERO; n];

    // repulsion — spatial hash for large graphs
    let cell = 120.0_f32;
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
            let f = params.repulsion * snapshot[j].2.sqrt() / dist2;
            force[i] += d.normalize_or_zero() * f.min(60.0);
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
        let stretch = dist - rest_length(&e.kind);
        let f = d / dist * stretch * params.spring;
        force[i] += f;
        force[j] -= f;
        if e.kind == "CONTAINS" {
            // child is pulled gently toward its parent
            force[j] -= d * params.gravity;
        }
    }

    // centering
    let centroid = snapshot.iter().map(|s| s.1).sum::<Vec2>() / n as f32;
    for (i, s) in snapshot.iter().enumerate() {
        force[i] -= (s.1 - centroid) * params.center_pull;
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
        let mut v = (vel.0 + force[i]) * params.damping;
        if v.length() > params.max_speed {
            v = v.normalize() * params.max_speed;
        }
        pos.0 += v;
        vel.0 = v;
        energy += v.length_squared();
    }
    if energy / (n as f32) < params.epsilon {
        params.frozen = true;
    }
    let _ = &graph;
}
