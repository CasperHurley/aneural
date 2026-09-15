//! Engine thread + event drain with a per-frame growth budget.

use crate::graph::{GraphState, apply_delta};
use crate::workspace::WorkspaceRes;
use aneural_core::GraphDelta;
use aneural_engine::{EngineCommand, EngineEvent};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender};
use std::collections::VecDeque;

#[derive(Resource)]
pub struct EngineRx(pub Receiver<EngineEvent>);

#[derive(Resource)]
pub struct EngineTx(pub Sender<EngineCommand>);

#[derive(Resource, Default, Debug)]
pub struct IndexStatus {
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub complete: bool,
    pub watching: bool,
    pub last_error: Option<String>,
    pub last_stats: Option<aneural_core::graph::IndexStats>,
    /// Bumped whenever graph content changes (layout wakes up, filters recompute).
    pub generation: u64,
}

#[derive(Resource, Default)]
pub struct PendingDeltas(pub VecDeque<GraphDelta>);

pub struct EnginePlugin;

impl Plugin for EnginePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IndexStatus>()
            .init_resource::<PendingDeltas>()
            .add_systems(Startup, spawn_engine)
            .add_systems(PreUpdate, drain_events)
            .add_systems(Last, stop_on_exit);
    }
}

fn spawn_engine(mut commands: Commands, ws: Res<WorkspaceRes>) {
    let (ev_tx, ev_rx) = crossbeam_channel::unbounded::<EngineEvent>();
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EngineCommand>();
    let root = ws.ws.root().to_path_buf();
    std::thread::Builder::new()
        .name("aneural-engine".into())
        .spawn(move || aneural_engine::run(&root, ev_tx, cmd_rx))
        .expect("spawn engine thread");
    commands.insert_resource(EngineRx(ev_rx));
    commands.insert_resource(EngineTx(cmd_tx));
}

#[allow(clippy::too_many_arguments)]
fn drain_events(
    mut commands: Commands,
    rx: Option<Res<EngineRx>>,
    mut status: ResMut<IndexStatus>,
    mut pending: ResMut<PendingDeltas>,
    mut graph: ResMut<GraphState>,
    ws: Res<WorkspaceRes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    atlas: Res<crate::render::IconAtlas>,
    mut nodes: Query<(&mut crate::graph::GraphNode, &crate::graph::Pos)>,
) {
    let Some(rx) = rx else { return };
    while let Ok(ev) = rx.0.try_recv() {
        match ev {
            EngineEvent::Delta(d) => pending.0.push_back(d),
            EngineEvent::Progress { phase, done, total } => {
                status.phase = phase.to_string();
                status.done = done;
                status.total = total;
            }
            EngineEvent::IndexComplete(stats) => {
                status.complete = true;
                status.phase = "complete".into();
                status.last_stats = Some(stats);
            }
            EngineEvent::Watching => status.watching = true,
            EngineEvent::Error(e) => {
                warn!("engine: {e}");
                status.last_error = Some(e);
            }
        }
    }
    if pending.0.is_empty() {
        return;
    }
    let mut budget = ws.config.gui.growth_budget_per_frame.max(1) as usize;
    while budget > 0 {
        let Some(mut delta) = pending.0.pop_front() else {
            break;
        };
        if delta.nodes.len() + delta.edges.len() > budget {
            // split: apply the first `budget` items, park the rest
            let mut rest = GraphDelta::new(delta.phase);
            rest.initial_complete = delta.initial_complete;
            delta.initial_complete = false;
            if delta.nodes.len() > budget {
                rest.nodes = delta.nodes.split_off(budget);
                rest.edges = std::mem::take(&mut delta.edges);
            } else {
                let keep = budget - delta.nodes.len();
                rest.edges = delta.edges.split_off(keep.min(delta.edges.len()));
            }
            budget = 0;
            pending.0.push_front(rest);
            apply_delta(
                &mut commands,
                &mut graph,
                &ws,
                &mut meshes,
                &mut materials,
                &atlas,
                &mut nodes,
                delta,
            );
        } else {
            budget -= delta.nodes.len() + delta.edges.len();
            apply_delta(
                &mut commands,
                &mut graph,
                &ws,
                &mut meshes,
                &mut materials,
                &atlas,
                &mut nodes,
                delta,
            );
        }
        status.generation += 1;
    }
}

fn stop_on_exit(mut exit: MessageReader<AppExit>, tx: Option<Res<EngineTx>>) {
    if exit.read().next().is_some()
        && let Some(tx) = tx
    {
        let _ = tx.0.send(EngineCommand::Stop);
    }
}
