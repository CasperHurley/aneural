//! Derive `.aneural/state/focus.json` from the UI state (debounced writes).

use crate::engine::IndexStatus;
use crate::filters::Filters;
use crate::graph::{GraphNode, Hidden};
use crate::picking::Selection;
use crate::workspace::WorkspaceRes;
use aneural_core::Focus;
use aneural_core::focus::{Direction, Neighborhood};
use bevy::prelude::*;
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
pub struct FocusState {
    pub notes: String,
    last_written: Option<Focus>,
    pending: Option<(Focus, Instant)>,
    wrote_initial: bool,
}

pub struct FocusPlugin;

impl Plugin for FocusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FocusState>()
            .add_systems(Update, derive_focus)
            .add_systems(Last, flush_on_exit);
    }
}

fn same(a: &Focus, b: &Focus) -> bool {
    a.filters == b.filters
        && a.selection == b.selection
        && a.neighborhood == b.neighborhood
        && a.visible_node_ids == b.visible_node_ids
        && a.notes == b.notes
}

pub fn build_focus(
    ws: &WorkspaceRes,
    filters: &Filters,
    selection: &Selection,
    notes: &str,
    nodes: &Query<(&GraphNode, Has<Hidden>)>,
) -> Focus {
    let mut f = Focus::new(ws.ws.root().to_string_lossy());
    f.filters.kinds = filters.kinds_list(&ws.kinds());
    f.filters.edge_kinds = filters.edge_kinds_list();
    f.filters.repos = filters.repos.iter().cloned().collect();
    f.filters.repos.sort();
    f.filters.query = filters.query.clone();
    f.selection.primary = selection.primary.clone();
    f.selection.pinned = selection.pinned.clone();
    f.neighborhood = Neighborhood {
        depth: filters.neighborhood_depth,
        direction: Direction::Both,
    };
    let mut visible: Vec<_> = nodes
        .iter()
        .filter(|(_, h)| !h)
        .map(|(n, _)| n.id.clone())
        .collect();
    visible.sort();
    visible.truncate(2000);
    f.visible_node_ids = visible;
    f.notes = notes.to_string();
    f
}

fn derive_focus(
    ws: Res<WorkspaceRes>,
    filters: Res<Filters>,
    selection: Res<Selection>,
    status: Res<IndexStatus>,
    mut state: ResMut<FocusState>,
    nodes: Query<(&GraphNode, Has<Hidden>)>,
) {
    if !status.complete {
        return;
    }
    let now = Instant::now();
    let focus = build_focus(&ws, &filters, &selection, &state.notes, &nodes);
    let changed = state.last_written.as_ref().is_none_or(|w| !same(w, &focus));
    if changed && !state.wrote_initial {
        // first write right after the initial index completes
        write(&ws, &mut state, focus);
        return;
    }
    if changed {
        let already_pending = state.pending.as_ref().is_some_and(|(p, _)| same(p, &focus));
        if !already_pending {
            state.pending = Some((focus, now));
        }
    } else {
        state.pending = None;
    }
    if let Some((_, since)) = &state.pending
        && now.duration_since(*since) >= Duration::from_millis(300)
    {
        let (f, _) = state.pending.take().unwrap();
        write(&ws, &mut state, f);
    }
}

fn write(ws: &WorkspaceRes, state: &mut FocusState, mut focus: Focus) {
    focus.updated_at = aneural_core::now_rfc3339();
    match ws.ws.write_focus(&focus) {
        Ok(()) => {
            state.last_written = Some(focus);
            state.wrote_initial = true;
        }
        Err(e) => warn!("focus.json: {e}"),
    }
}

fn flush_on_exit(
    mut exit: MessageReader<AppExit>,
    ws: Res<WorkspaceRes>,
    mut state: ResMut<FocusState>,
    filters: Res<Filters>,
    selection: Res<Selection>,
    nodes: Query<(&GraphNode, Has<Hidden>)>,
) {
    if exit.read().next().is_none() {
        return;
    }
    let focus = build_focus(&ws, &filters, &selection, &state.notes, &nodes);
    write(&ws, &mut state, focus);
}
