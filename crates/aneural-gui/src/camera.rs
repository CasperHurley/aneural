//! Camera, pan/zoom, framing, hotkeys.
//!
//! Panning: left-drag on empty canvas (right/middle drag anywhere) via
//! `bevy_pancam`; left-drag on a node is handled by `picking` instead.
//! Framing: while the graph is first growing the camera follows the graph's
//! bounds so it stays centered; the follow stops on the first manual pan/zoom
//! or once indexing is complete and the layout has settled. `F` re-frames.

use crate::engine::{EngineTx, IndexStatus};
use crate::graph::{GraphNode, Hidden, Pos};
use crate::layout::LayoutParams;
use crate::picking::{DragState, Hovered};
use aneural_engine::EngineCommand;
use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;
use bevy_pancam::{PanCam, PanCamPlugin, PanCamSystems};

const MIN_SCALE: f32 = 0.15;
const MAX_SCALE: f32 = 12.0;
/// Pixels the cursor must travel before a background press counts as a pan.
const PAN_DEADZONE: f32 = 4.0;

#[derive(Component)]
pub struct MainCamera;

/// Second pass over the same view. 2D gizmos are always queued last, so the
/// hyphae would paint over the nodes; instead the main camera draws only the
/// gizmo layer and this one redraws the nodes on top of them.
#[derive(Component)]
pub struct NodeCamera;

/// The render layer the hyphae gizmos live on (the main camera's own).
pub const HYPHAE_LAYER: usize = 1;

#[derive(Resource, Default)]
pub struct UiCapture {
    pub pointer: bool,
    pub keyboard: bool,
}

/// One-shot "frame everything now".
#[derive(Resource, Default)]
pub struct FrameRequest(pub bool);

/// Smoothly keep the whole graph in view while it grows.
#[derive(Resource)]
pub struct AutoFollow(pub bool);

impl Default for AutoFollow {
    fn default() -> Self {
        AutoFollow(true)
    }
}

/// The window region not covered by egui panels, in logical pixels
/// (screen coordinates, y down). Written by `ui::panels`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct CanvasRect {
    pub min: Vec2,
    pub max: Vec2,
}

impl CanvasRect {
    fn size(&self) -> Vec2 {
        self.max - self.min
    }
    fn center(&self) -> Vec2 {
        (self.min + self.max) / 2.0
    }
    /// Is a window-space point (logical pixels, y down) on the canvas?
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

/// A left-button press that started on empty canvas (a pan, or a click that
/// clears the selection if the cursor never moved).
#[derive(Resource, Default, Debug)]
pub struct PanGrab {
    pub active: bool,
    pub moved: bool,
    start: Vec2,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PanCamPlugin)
            .init_resource::<UiCapture>()
            .init_resource::<FrameRequest>()
            .init_resource::<AutoFollow>()
            .init_resource::<PanGrab>()
            .init_resource::<CanvasRect>()
            .add_systems(Startup, spawn_camera)
            .add_systems(bevy_egui::EguiPrimaryContextPass, read_ui_capture)
            .add_systems(
                Update,
                (
                    (gate_pancam, hotkeys)
                        .after(crate::picking::pick)
                        .before(PanCamSystems),
                    (frame_all, follow_graph, sync_node_camera)
                        .chain()
                        .after(PanCamSystems),
                ),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        MainCamera,
        Camera2d,
        RenderLayers::layer(HYPHAE_LAYER),
        PanCam {
            grab_buttons: vec![MouseButton::Left, MouseButton::Right, MouseButton::Middle],
            zoom_to_cursor: true,
            min_scale: MIN_SCALE,
            max_scale: MAX_SCALE,
            ..default()
        },
    ));
    commands.spawn((
        NodeCamera,
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(0),
    ));
}

/// Keep the node pass looking through the same lens as the main camera.
fn sync_node_camera(
    main: Query<(&Transform, &Projection), (With<MainCamera>, Without<NodeCamera>)>,
    mut overlay: Query<(&mut Transform, &mut Projection), With<NodeCamera>>,
) {
    let (Ok((t, p)), Ok((mut ot, mut op))) = (main.single(), overlay.single_mut()) else {
        return;
    };
    *ot = *t;
    *op = p.clone();
}

fn read_ui_capture(mut contexts: EguiContexts, mut capture: ResMut<UiCapture>) {
    if let Ok(ctx) = contexts.ctx_mut() {
        capture.pointer = ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui();
        capture.keyboard = ctx.egui_wants_keyboard_input();
    }
}

/// Decide each frame whether `bevy_pancam` may move the camera, and track
/// background left-presses so they pan instead of picking.
fn gate_pancam(
    capture: Res<UiCapture>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    hovered: Res<Hovered>,
    drag: Res<DragState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut grab: ResMut<PanGrab>,
    mut follow: ResMut<AutoFollow>,
    mut cams: Query<&mut PanCam>,
) {
    let cursor = windows.single().ok().and_then(Window::cursor_position);

    if mouse.just_pressed(MouseButton::Left) {
        grab.active = !capture.pointer && hovered.0.is_none() && drag.entity.is_none();
        grab.moved = false;
        grab.start = cursor.unwrap_or_default();
    }
    if grab.active {
        if let Some(c) = cursor
            && c.distance(grab.start) > PAN_DEADZONE
        {
            grab.moved = true;
        }
        if !mouse.pressed(MouseButton::Left) {
            grab.active = false;
        }
    }

    let side_pan = (mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle))
        && !capture.pointer;
    let scrolled = wheel.read().next().is_some() && !capture.pointer;
    if (grab.active && grab.moved) || side_pan || scrolled {
        follow.0 = false;
    }

    // Left button: only pan for a grab that began on empty canvas. Other
    // buttons and keys: pan unless egui owns the pointer or a node is being dragged.
    let enabled = drag.entity.is_none()
        && if mouse.pressed(MouseButton::Left) {
            grab.active
        } else {
            !capture.pointer
        };
    for mut cam in &mut cams {
        cam.enabled = enabled;
    }
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<UiCapture>,
    mut frame: ResMut<FrameRequest>,
    mut follow: ResMut<AutoFollow>,
    mut layout: ResMut<LayoutParams>,
    tx: Option<Res<EngineTx>>,
) {
    if capture.keyboard {
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        frame.0 = true;
    }
    if keys.just_pressed(KeyCode::Space) {
        layout.stir();
    }
    if keys.just_pressed(KeyCode::KeyR)
        && let Some(tx) = tx
    {
        let _ = tx.0.send(EngineCommand::Reindex { force: false });
    }
    if keys.any_pressed([
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
    ]) {
        follow.0 = false;
    }
}

/// Camera translation and orthographic scale that fit every visible node
/// inside the canvas (the window minus egui panels).
fn fit(
    nodes: &Query<&Pos, (With<GraphNode>, Without<Hidden>)>,
    windows: &Query<&Window, With<PrimaryWindow>>,
    canvas: &CanvasRect,
) -> Option<(Vec2, f32)> {
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    let mut count = 0;
    for p in nodes {
        min = min.min(p.0);
        max = max.max(p.0);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let size = (max - min).max(Vec2::splat(100.0)) + Vec2::splat(160.0);
    let center = (min + max) / 2.0;
    let win = windows
        .single()
        .ok()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let (canvas_size, canvas_center) = if canvas.size().x > 50.0 && canvas.size().y > 50.0 {
        (canvas.size(), canvas.center())
    } else {
        (win, win / 2.0)
    };
    let scale = (size.x / canvas_size.x)
        .max(size.y / canvas_size.y)
        .clamp(MIN_SCALE, MAX_SCALE);
    // the camera looks at the window centre; shift it so the graph centre
    // lands on the canvas centre instead (screen y is down, world y is up)
    let offset = canvas_center - win / 2.0;
    let translation = center - Vec2::new(offset.x, -offset.y) * scale;
    Some((translation, scale))
}

fn frame_all(
    mut frame: ResMut<FrameRequest>,
    nodes: Query<&Pos, (With<GraphNode>, Without<Hidden>)>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    canvas: Res<CanvasRect>,
) {
    if !frame.0 {
        return;
    }
    frame.0 = false;
    let Some((center, scale)) = fit(&nodes, &windows, &canvas) else {
        return;
    };
    let Ok((mut t, mut proj)) = cam.single_mut() else {
        return;
    };
    t.translation.x = center.x;
    t.translation.y = center.y;
    if let Projection::Orthographic(o) = &mut *proj {
        o.scale = scale;
    }
}

/// While `AutoFollow` is on, ease the camera toward the graph's bounds every
/// frame so the graph grows in the middle of the window. Stops (after a final
/// snap) once the index is complete and the layout has come to rest.
fn follow_graph(
    time: Res<Time>,
    mut follow: ResMut<AutoFollow>,
    status: Res<IndexStatus>,
    layout: Res<LayoutParams>,
    nodes: Query<&Pos, (With<GraphNode>, Without<Hidden>)>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    canvas: Res<CanvasRect>,
) {
    if !follow.0 {
        return;
    }
    let Some((center, scale)) = fit(&nodes, &windows, &canvas) else {
        return;
    };
    let Ok((mut t, mut proj)) = cam.single_mut() else {
        return;
    };
    let settled = status.complete && layout.frozen;
    // exponential ease; snap on the final frame
    let k = if settled {
        1.0
    } else {
        1.0 - (-time.delta_secs() * 3.0).exp()
    };
    let cur = t.translation.truncate();
    let next = cur.lerp(center, k);
    t.translation.x = next.x;
    t.translation.y = next.y;
    if let Projection::Orthographic(o) = &mut *proj {
        o.scale += (scale - o.scale) * k;
    }
    if settled {
        follow.0 = false;
    }
}
