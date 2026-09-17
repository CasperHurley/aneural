//! Manual picking: cursor → world → nearest node. Click selects, shift-click
//! pins, left-drag moves (and pins while held). Left-drag on empty canvas is
//! a pan (see `camera`); a background click without movement clears selection.

use crate::camera::{CanvasRect, MainCamera, PanGrab, UiCapture};
use crate::graph::{Drift, GraphNode, GraphState, Hidden, Pinned, Pos, Vel};
use crate::layout::LayoutParams;
use crate::workspace::node_radius;
use aneural_core::NodeId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

#[derive(Resource, Default, Debug)]
pub struct Selection {
    pub primary: Option<NodeId>,
    pub pinned: Vec<NodeId>,
}

impl Selection {
    pub fn toggle_pin(&mut self, id: NodeId) {
        if let Some(i) = self.pinned.iter().position(|p| p == &id) {
            self.pinned.remove(i);
        } else {
            self.pinned.push(id);
        }
    }
}

#[derive(Resource, Default, Debug)]
pub struct Hovered(pub Option<NodeId>);

#[derive(Resource, Default)]
pub struct DragState {
    pub entity: Option<Entity>,
    moved: bool,
    was_pinned: bool,
    offset: Vec2,
}

pub struct PickingPlugin;

impl Plugin for PickingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Hovered>()
            .init_resource::<DragState>()
            .add_systems(Update, pick);
    }
}

fn cursor_world(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cam: &Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) -> Option<Vec2> {
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let (camera, gt) = cam.single().ok()?;
    camera.viewport_to_world_2d(gt, cursor).ok()
}

fn nearest(
    world: Vec2,
    nodes: &Query<(Entity, &GraphNode, &Pos, Option<&Drift>, Has<Pinned>), Without<Hidden>>,
) -> Option<(Entity, NodeId, Vec2, bool)> {
    let mut best: Option<(f32, Entity, NodeId, Vec2, bool)> = None;
    for (e, gn, p, drift, pinned) in nodes {
        let r = node_radius(&gn.kind) + 4.0;
        // Hit where the node is drawn, but hand back where the layout has it,
        // so a drag starts without a jump.
        let drawn = p.0 + drift.map(|d| d.offset).unwrap_or(Vec2::ZERO);
        let d = drawn.distance(world);
        if d <= r && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, e, gn.id.clone(), p.0, pinned));
        }
    }
    best.map(|(_, e, id, p, pinned)| (e, id, p, pinned))
}

#[allow(clippy::too_many_arguments)]
pub fn pick(
    mut commands: Commands,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    capture: Res<UiCapture>,
    canvas: Res<CanvasRect>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut queries: ParamSet<(
        Query<(Entity, &GraphNode, &Pos, Option<&Drift>, Has<Pinned>), Without<Hidden>>,
        Query<(&mut Pos, &mut Vel), With<GraphNode>>,
    )>,
    mut selection: ResMut<Selection>,
    mut hovered: ResMut<Hovered>,
    mut drag: ResMut<DragState>,
    mut layout: ResMut<LayoutParams>,
    graph: Res<GraphState>,
    grab: Res<PanGrab>,
) {
    let world = cursor_world(&windows, &cam);
    // The graph is drawn under the panels as well as beside them, so a node
    // behind one must not answer the pointer: the canvas rectangle decides.
    let on_canvas = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .is_some_and(|p| canvas.contains(p));
    let over_ui = capture.pointer || !on_canvas;

    // dragging in progress
    if let Some(e) = drag.entity {
        if mouse.pressed(MouseButton::Left) {
            let mut positions = queries.p1();
            if let (Some(w), Ok((mut pos, mut vel))) = (world, positions.get_mut(e)) {
                let target = w + drag.offset;
                if pos.0.distance(target) > 0.5 {
                    drag.moved = true;
                }
                pos.0 = target;
                vel.0 = Vec2::ZERO;
                layout.nudge();
            }
            return;
        }
        // release
        let entity = e;
        drag.entity = None;
        if !drag.moved {
            // click
            if let Some(id) = graph
                .by_id
                .iter()
                .find(|(_, en)| **en == entity)
                .map(|(id, _)| id.clone())
            {
                if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                    selection.toggle_pin(id);
                } else {
                    selection.primary = Some(id);
                }
            }
            if !drag.was_pinned {
                commands.entity(entity).remove::<Pinned>();
            }
        }
        // a dragged node stays pinned where the user left it
        return;
    }

    // a background click (press + release without panning) clears selection
    if mouse.just_released(MouseButton::Left)
        && grab.active
        && !grab.moved
        && !(keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight))
    {
        selection.primary = None;
    }

    // hover (suppressed while panning)
    let hit = if over_ui || grab.active {
        None
    } else {
        world.and_then(|w| nearest(w, &queries.p0()))
    };
    let new_hover = hit.as_ref().map(|(_, id, _, _)| id.clone());
    if hovered.0 != new_hover {
        hovered.0 = new_hover;
    }

    if over_ui {
        return;
    }
    if mouse.just_pressed(MouseButton::Left)
        && let Some((e, _, p, was_pinned)) = hit
    {
        drag.entity = Some(e);
        drag.moved = false;
        drag.was_pinned = was_pinned;
        drag.offset = p - world.unwrap_or(p);
        commands.entity(e).insert(Pinned);
    }
}
