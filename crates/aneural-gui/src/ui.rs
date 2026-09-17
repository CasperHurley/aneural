//! egui panels: top bar, filters, inspector, spores.

use crate::camera::{CanvasRect, FrameRequest, PanGrab, UiCapture};
use crate::circadian::Vibe;
use crate::engine::IndexStatus;
use crate::filters::{Filters, PointedKind};
use crate::focus::FocusState;
use crate::graph::{GraphEdge, GraphNode, GraphState, Hidden};
use crate::picking::{Hovered, Selection};
use crate::switch::{OpenRequest, Recents};
use crate::theme;
use crate::workspace::WorkspaceRes;
use aneural_core::NodeId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use std::collections::BTreeMap;

fn own_the_primary_context(mut settings: ResMut<bevy_egui::EguiGlobalSettings>) {
    settings.auto_create_primary_context = false;
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            // We put `PrimaryEguiContext` on the main camera ourselves
            // (`camera.rs`), so bevy_egui must not also attach one: two
            // contexts claiming `EguiPrimaryContextPass` is a panic.
            .add_systems(PreStartup, own_the_primary_context)
            .init_resource::<PanelsOpen>()
            .init_resource::<Styled>()
            .add_systems(
                EguiPrimaryContextPass,
                (follow_palette, panels, canvas_cursor).chain(),
            );
    }
}

/// Which side panels are showing. Collapsing one hands its width to the canvas.
#[derive(Resource)]
pub struct PanelsOpen {
    pub left: bool,
    pub right: bool,
}

impl Default for PanelsOpen {
    fn default() -> Self {
        PanelsOpen {
            left: true,
            right: true,
        }
    }
}

/// egui honours `Visuals::interact_cursor` for buttons only, so every other
/// clickable widget asks for the hand itself.
trait Hand {
    fn hand(self) -> Self;
}

impl Hand for egui::Response {
    fn hand(self) -> Self {
        self.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

/// The step of the night the panels were last painted at, so that a palette
/// that moves over hours is not rebuilt sixty times a second.
#[derive(Resource, Default)]
struct Styled(Option<u8>);

/// The chrome follows the same clock the graph does: warm and matte through
/// the day, cooler and wetter after dark.
fn follow_palette(mut contexts: EguiContexts, vibe: Res<Vibe>, mut styled: ResMut<Styled>) {
    let step = (vibe.palette.night * 48.0).round() as u8;
    if styled.0 == Some(step) {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    styled.0 = Some(step);
    let p = &vibe.palette;
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme::egui_color(p.panel);
    visuals.window_fill = theme::egui_color(p.panel);
    visuals.override_text_color = Some(theme::egui_color(p.text));
    visuals.selection.bg_fill = theme::egui_color(p.dim);
    visuals.hyperlink_color = theme::egui_color(p.accent);
    visuals.widgets.active.bg_fill = theme::egui_color(p.dim);
    visuals.widgets.hovered.bg_fill = theme::egui_color(p.hover);
    // a hand over anything clickable, the way a web app behaves
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    ctx.set_visuals(visuals);
}

/// The hour of the day as the app is wearing it: a disc whose rays retract as
/// the light goes and which a bite turns into a crescent. Clicking it decides
/// how much of the clock the interface follows.
fn time_of_day_dial(
    ui: &mut egui::Ui,
    night: f32,
    color: egui::Color32,
    behind: egui::Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::click());
    let (c, r, day) = (rect.center(), 5.0, 1.0 - night);
    let painter = ui.painter();
    if day > 0.02 {
        let reach = 1.6 + 2.4 * day;
        let stroke = egui::Stroke::new(1.1, color.gamma_multiply(day));
        for i in 0..8 {
            let a = std::f32::consts::TAU * i as f32 / 8.0;
            let dir = egui::vec2(a.cos(), a.sin());
            painter.line_segment([c + dir * (r + 1.8), c + dir * (r + 1.8 + reach)], stroke);
        }
    }
    painter.circle_filled(c, r, color);
    // The bite slides in from the side: clear of the disc in daylight, most of
    // the way across it in the small hours.
    let bite = r * (2.0 - 1.15 * night);
    if bite < r * 2.0 {
        painter.circle_filled(c + egui::vec2(bite, -bite * 0.25), r, behind);
    }
    resp
}

/// splitmix64's finaliser: turns a counter into well-spread bits, so "every
/// so often" does not fall into a visible pattern.
fn mix(n: u64) -> u64 {
    let mut h = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    h
}

/// Where the pupil sits during a glance, over `u` in 0..1 of the open eye:
/// a wait, a dart to one side, a pause, a dart across, a pause, and back to
/// the middle. The waits and the side it looks first vary with the cycle.
fn gaze_offset(u: f32, seed: u64) -> f32 {
    let byte = |shift: u32| ((seed >> shift) & 0xFF) as f32 / 255.0;
    let delay = 0.08 + 0.22 * byte(8);
    let hold = 0.10 + 0.20 * byte(16);
    let dart = 0.11;
    let side = if (seed >> 24) & 1 == 0 { 2.6 } else { -2.6 };
    let mut keys = [
        (0.0, 0.0),
        (delay, 0.0),
        (delay + dart, side),
        (delay + dart + hold, side),
        (delay + 2.0 * dart + hold, -side),
        (delay + 2.0 * dart + 2.0 * hold, -side),
        (delay + 3.0 * dart + 2.0 * hold, 0.0),
    ];
    // squeeze the whole sequence back inside the blink if the waits ran long
    let squeeze = (0.95 / keys[keys.len() - 1].0).min(1.0);
    for k in &mut keys {
        k.0 *= squeeze;
    }
    let mut prev = keys[0];
    for &k in &keys[1..] {
        if u <= k.0 {
            let x = ((u - prev.0) / (k.0 - prev.0).max(1e-4)).clamp(0.0, 1.0);
            return prev.1 + (k.1 - prev.1) * x * x * (3.0 - 2.0 * x);
        }
        prev = k;
    }
    0.0
}

/// The workspace name doubles as the menu for opening another one.
fn workspace_menu(ui: &mut egui::Ui, name: &str, recents: &Recents, request: &mut OpenRequest) {
    ui.menu_button(egui::RichText::new(format!("{name} ⏷")).strong(), |ui| {
        if ui.button("Open a folder…").hand().clicked() {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title("Open a directory to grow")
                .pick_folder()
            {
                request.0 = Some(dir);
            }
            ui.close();
        }
        if recents.0.len() > 1 {
            ui.separator();
            ui.label(egui::RichText::new("Recent").weak().small());
            for path in recents.0.iter().skip(1) {
                let label = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("/")
                    .to_string();
                if ui
                    .button(label)
                    .on_hover_text(path.display().to_string())
                    .hand()
                    .clicked()
                {
                    request.0 = Some(path.clone());
                    ui.close();
                }
            }
        }
    })
    .response
    .on_hover_text("Open another workspace");
}

/// The watcher's telltale: a pupil that every few seconds opens into an eye
/// and closes again, so the status line shows it is awake.
fn watching_eye(ui: &mut egui::Ui, color: egui::Color32) -> egui::Response {
    const CYCLE: f64 = 6.5;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 12.0), egui::Sense::hover());
    let time = ui.input(|i| i.time);
    let (cycle, phase) = ((time / CYCLE) as u64, time % CYCLE);
    // on some cycles the eye has a look around, and stays open long enough for it
    let seed = mix(cycle);
    let glances = seed.is_multiple_of(3);
    let blink = if glances { 3.4 } else { 1.9 };
    // one smooth open-and-shut at the top of each cycle
    let open = if phase < blink {
        (std::f64::consts::PI * phase / blink).sin() as f32
    } else {
        0.0
    };
    let gaze = if glances && open > 0.0 {
        gaze_offset((phase / blink) as f32, seed)
    } else {
        0.0
    };
    let c = rect.center();
    let painter = ui.painter();
    if open > 0.01 {
        let (w, h) = (7.5, 5.5 * open);
        // a lid is a parabola from corner to corner, mirrored above and below
        let lid = |sign: f32| -> Vec<egui::Pos2> {
            (0..=12)
                .map(|i| {
                    let x = -w + 2.0 * w * (i as f32 / 12.0);
                    egui::pos2(c.x + x, c.y + sign * h * (1.0 - (x / w).powi(2)))
                })
                .collect()
        };
        let stroke = egui::Stroke::new(1.2, color.gamma_multiply(open));
        painter.add(egui::Shape::line(lid(-1.0), stroke));
        painter.add(egui::Shape::line(lid(1.0), stroke));
    }
    painter.circle_filled(c + egui::vec2(gaze, 0.0), 3.0 - 1.0 * open, color);
    resp
}

/// The canvas is not egui, so it sets its own cursor: a hand over a node, an
/// open palm over the background, a closed one while the view is being dragged.
fn canvas_cursor(
    mut contexts: EguiContexts,
    canvas: Res<CanvasRect>,
    windows: Query<&Window, With<PrimaryWindow>>,
    capture: Res<UiCapture>,
    hovered: Res<Hovered>,
    grab: Res<PanGrab>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    // a pan that began on the canvas keeps its cursor even if the drag wanders
    // over a panel; otherwise the panels are egui's and its own cursors win
    let panning = grab.active
        || (!capture.pointer
            && (mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle)));
    let over_canvas = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .is_some_and(|p| canvas.contains(p));
    if !panning && (!over_canvas || capture.pointer) {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    ctx.set_cursor_icon(if panning {
        egui::CursorIcon::Grabbing
    } else if hovered.0.is_some() {
        egui::CursorIcon::PointingHand
    } else {
        egui::CursorIcon::Grab
    });
}

#[allow(clippy::too_many_arguments)]
fn panels(
    mut contexts: EguiContexts,
    ws: Res<WorkspaceRes>,
    status: Res<IndexStatus>,
    graph: Res<GraphState>,
    mut filters: ResMut<Filters>,
    mut selection: ResMut<Selection>,
    mut focus: ResMut<FocusState>,
    mut frame: ResMut<FrameRequest>,
    mut canvas: ResMut<CanvasRect>,
    mut open: ResMut<PanelsOpen>,
    mut open_request: ResMut<OpenRequest>,
    mut vibe: ResMut<Vibe>,
    recents: Res<Recents>,
    mut market: ResMut<crate::marketplace::Marketplace>,
    nodes: Query<(&GraphNode, Has<Hidden>)>,
    edges: Query<&GraphEdge>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let palette = vibe.palette;
    let accent = theme::egui_color(palette.accent);
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let ctx = &mut root;

    // counts per kind
    let mut kind_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (n, hidden) in &nodes {
        let c = kind_counts.entry(n.kind.clone()).or_default();
        c.0 += 1;
        if !hidden {
            c.1 += 1;
        }
    }
    let mut edge_counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &edges {
        *edge_counts.entry(e.kind.clone()).or_default() += 1;
    }

    // The bar carries the app's name and what it is doing, so it gets room to
    // breathe rather than egui's default two pixels top and bottom.
    egui::Panel::top("top")
        .frame(
            egui::Frame::side_top_panel(ctx.style())
                .inner_margin(egui::Margin::symmetric(14, 9)),
        )
        .show(ctx, |ui| {
        ui.horizontal(|ui| {
            // With the panel open its own corner holds the button; this is
            // just the way back once it is gone.
            if !open.left
                && ui
                    .button("⏵")
                    .on_hover_text("Show the filters")
                    .clicked()
            {
                open.left = true;
            }
            ui.label(egui::RichText::new("🍄 Aneural").color(accent).strong());
            workspace_menu(ui, &ws.name(), &recents, &mut open_request);
            if ui
                .button("spores")
                .on_hover_text("Browse the Open Spores Marketplace")
                .hand()
                .clicked()
            {
                market.open = true;
                if market.listings.is_empty() {
                    market.state = crate::marketplace::MarketState::Loading;
                    market
                        .pending
                        .push(crate::marketplace::worker::RegistryCommand::Refresh { force: false });
                }
            }
            ui.separator();
            // one indicator with two states: reading files, or idle and watching
            if status.busy {
                ui.add(egui::Spinner::new().size(12.0).color(accent));
                // phase and counts belong to the initial index; a live batch
                // from the watcher has neither
                let progress = if !status.complete && status.total > 0 {
                    format!("indexing {} {}/{}", status.phase, status.done, status.total)
                } else {
                    "indexing".to_string()
                };
                ui.label(progress)
                    .on_hover_text("Reading the workspace: files, imports and spores.");
            } else if status.watching {
                let eye = watching_eye(ui, accent);
                let text = ui.label(egui::RichText::new("watching").color(accent));
                (eye | text).on_hover_text(
                    "Everything is in the graph. Files you add, edit or delete are picked up on their own.",
                );
            }
            ui.label(format!(
                "{} nodes · {} edges",
                graph.node_count, graph.edge_count
            ));
            if let Some(err) = &status.last_error {
                ui.label(
                    egui::RichText::new(format!("⚠ {err}"))
                        .color(theme::egui_color(palette.warning)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !open.right
                    && ui
                        .button("⏴")
                        .on_hover_text("Show the inspector")
                        .clicked()
                {
                    open.right = true;
                }
                let dial = time_of_day_dial(
                    ui,
                    palette.night,
                    accent,
                    theme::egui_color(palette.panel),
                );
                if dial
                    .on_hover_text(format!("{} — {}", vibe.phase(), vibe.mode.tip()))
                    .hand()
                    .clicked()
                {
                    let next = vibe.mode.next();
                    vibe.set_mode(next);
                }
            });
        });
    });

    let mut close_left = false;
    let mut pointed = None;
    egui::Panel::left("filters").resizable(true).default_size(240.0).show_collapsible(ctx, &mut open.left, |ui| {
        close_left = panel_header(ui, "Filters", "⏴", "Hide the filters");
        let resp = ui.add(egui::TextEdit::singleline(&mut filters.query).hint_text("search label / path"));
        if resp.changed() {
            filters.dirty = true;
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(egui::RichText::new("Node kinds").strong());
            for kind in ws.kinds() {
                let style = ws.style(&kind);
                let (total, shown) = kind_counts.get(&kind).copied().unwrap_or((0, 0));
                let mut on = filters.kind_visible(&kind);
                let row = ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 5.0, theme::egui_color(style.color));
                    if ui.checkbox(&mut on, format!("{} ({shown}/{total})", style.label)).hand().changed() {
                        filters.toggle_kind(&kind);
                    }
                });
                if row.response.contains_pointer() {
                    pointed = Some(PointedKind::Node(kind.clone()));
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Edge kinds").strong());
            for kind in aneural_core::kinds::EdgeKind::ALL.iter().filter(|k| aneural_core::kinds::EdgeKind::is_toggleable(k)) {
                let mut on = !filters.hidden_edge_kinds.contains(*kind);
                let label = aneural_core::kinds::EdgeKind::label(kind);
                let row = ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 4.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 1.0, theme::egui_color(palette.edge(kind)));
                    if ui.checkbox(&mut on, format!("{label} ({})", edge_counts.get(*kind).copied().unwrap_or(0))).hand().changed() {
                        filters.toggle_edge_kind(kind);
                    }
                });
                if row.response.contains_pointer() {
                    pointed = Some(PointedKind::Edge(kind.to_string()));
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Repos").strong());
            let mut repos: Vec<(NodeId, String)> = nodes.iter().filter(|(n, _)| n.kind == "Repo").map(|(n, _)| (n.id.clone(), n.path.clone().unwrap_or_else(|| n.label.clone()))).collect();
            repos.sort_by(|a, b| a.1.cmp(&b.1));
            if repos.is_empty() {
                ui.label(egui::RichText::new("no git repos found").weak());
            }
            for (id, path) in repos {
                let mut on = filters.repos.is_empty() || filters.repos.contains(&id);
                if ui.checkbox(&mut on, path).hand().changed() {
                    let all: Vec<NodeId> = nodes.iter().filter(|(n, _)| n.kind == "Repo").map(|(n, _)| n.id.clone()).collect();
                    if filters.repos.is_empty() {
                        filters.repos = all.into_iter().collect();
                    }
                    if on {
                        filters.repos.insert(id);
                    } else {
                        filters.repos.remove(&id);
                    }
                    filters.dirty = true;
                }
            }
            if !filters.repos.is_empty() && ui.small_button("all repos").clicked() {
                filters.repos.clear();
                filters.dirty = true;
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Click: select · Shift-click: pin · Drag node: move · Drag canvas: pan · Wheel: zoom · F: frame").weak().small());
        });
    });

    let mut close_right = false;
    egui::Panel::right("inspector").resizable(true).default_size(310.0).show_collapsible(ctx, &mut open.right, |ui| {
        close_right = panel_header(ui, "Inspector", "⏵", "Hide the inspector");
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut select_next: Option<NodeId> = None;
            match selection.primary.clone() {
                None => {
                    ui.label(egui::RichText::new("Select a node to inspect it. The selection and filters define the context served over MCP.").weak());
                }
                Some(id) => {
                    let found = nodes.iter().find(|(n, _)| n.id == id).map(|(n, _)| n.clone());
                    match found {
                        None => {
                            ui.label(egui::RichText::new(format!("{id} (gone)")).weak());
                        }
                        Some(n) => {
                            let style = ws.style(&n.kind);
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 6.0, theme::egui_color(style.color));
                                ui.label(egui::RichText::new(&n.label).strong().size(16.0));
                            });
                            ui.label(egui::RichText::new(format!("{} · {}", style.label, n.id)).weak().small());
                            if let Some(p) = &n.path {
                                ui.label(p);
                                ui.horizontal(|ui| {
                                    if ui.button("Open in editor").clicked() {
                                        open_in_editor(&ws.ws.abs(p));
                                    }
                                    if ui.button("Pin").clicked() {
                                        selection.toggle_pin(n.id.clone());
                                    }
                                });
                            } else if ui.button("Pin").clicked() {
                                selection.toggle_pin(n.id.clone());
                            }
                            if let Some(r) = &n.repo_id {
                                ui.label(egui::RichText::new(format!("repo: {}", r.path_part())).weak());
                            }
                            if let serde_json::Value::Object(map) = &n.props
                                && !map.is_empty() {
                                    ui.separator();
                                    egui::Grid::new("props").num_columns(2).striped(true).show(ui, |ui| {
                                        for (k, v) in map {
                                            ui.label(egui::RichText::new(k).weak());
                                            let text = match v {
                                                serde_json::Value::String(s) => s.clone(),
                                                other => other.to_string(),
                                            };
                                            ui.add(egui::Label::new(text).wrap());
                                            ui.end_row();
                                        }
                                    });
                                }
                            ui.separator();
                            ui.label(egui::RichText::new("Edges").strong());
                            let mut rows: Vec<(String, bool, NodeId)> = graph.neighbors(&n.id).iter().map(|(other, kind)| (kind.clone(), true, other.clone())).collect();
                            // direction: look up actual edge direction
                            for row in rows.iter_mut() {
                                row.1 = graph.edges.contains_key(&(row.0.clone(), n.id.clone(), row.2.clone()));
                            }
                            rows.sort();
                            for (kind, outgoing, other) in rows.into_iter().take(200) {
                                let label = nodes.iter().find(|(x, _)| x.id == other).map(|(x, _)| x.label.clone()).unwrap_or_else(|| other.to_string());
                                let arrow = if outgoing { "→" } else { "←" };
                                ui.horizontal(|ui| {
                                    let name = aneural_core::kinds::EdgeKind::label(&kind);
                                    ui.label(egui::RichText::new(format!("{arrow} {name}")).color(theme::egui_color(palette.edge(&kind))).small());
                                    if ui.link(label).clicked() {
                                        select_next = Some(other.clone());
                                    }
                                });
                            }
                        }
                    }
                }
            }
            if let Some(id) = select_next {
                selection.primary = Some(id);
            }
            ui.separator();
            ui.label(egui::RichText::new("Pinned").strong());
            let mut unpin: Option<NodeId> = None;
            for id in &selection.pinned {
                ui.horizontal(|ui| {
                    if ui.small_button("✕").clicked() {
                        unpin = Some(id.clone());
                    }
                    let label = nodes.iter().find(|(x, _)| &x.id == id).map(|(x, _)| x.label.clone()).unwrap_or_else(|| id.to_string());
                    ui.label(label);
                });
            }
            if let Some(id) = unpin {
                selection.toggle_pin(id);
            }
            if selection.pinned.is_empty() {
                ui.label(egui::RichText::new("shift-click nodes to pin them into the context").weak().small());
            }
            ui.separator();
            ui.label(egui::RichText::new("Notes for the assistant").strong());
            ui.add(egui::TextEdit::multiline(&mut focus.notes).desired_rows(4).hint_text("what should Claude know about this focus?"));
        });
    });

    open.left &= !close_left;
    if filters.pointed != pointed {
        filters.pointed = pointed;
    }
    open.right &= !close_right;

    // whatever the panels left over is the graph canvas
    let rect = ctx.available_rect_before_wrap();

    // Acts on the canvas belong on the canvas, tucked into the bottom corner
    // of whatever the panels have left over.
    let size = egui::vec2(30.0, 30.0);
    let tools = egui::vec2(size.x * 2.0 + 6.0, size.y);
    egui::Area::new("canvas tools".into())
        .order(egui::Order::Foreground)
        .fixed_pos(rect.max - tools - egui::vec2(12.0, 12.0))
        .show(&ctx.ctx().clone(), |ui| {
            ui.horizontal(|ui| {
                let on = filters.focus_mode;
                if ui
                    .add_sized(
                        size,
                        egui::Button::new(egui::RichText::new("🔘").size(15.0)).selected(on),
                    )
                    .on_hover_text(if on {
                        "Showing everything again"
                    } else {
                        "Focus: show only what is selected and its neighbours, and share                          that alone with the assistant"
                    })
                    .clicked()
                {
                    filters.focus_mode = !on;
                    filters.dirty = true;
                }
                if ui
                    .add_sized(size, egui::Button::new(egui::RichText::new("⛶").size(15.0)))
                    .on_hover_text("Fit the whole graph in view (F)")
                    .clicked()
                {
                    frame.0 = true;
                }
            });
        });

    let (min, max) = (
        Vec2::new(rect.min.x, rect.min.y),
        Vec2::new(rect.max.x, rect.max.y),
    );
    if canvas.min != min || canvas.max != max {
        canvas.min = min;
        canvas.max = max;
    }
}

/// A panel's title with its own collapse button tucked into the far corner.
/// Returns whether the button was clicked; the caller closes the panel, since
/// `show_collapsible` holds the flag while the body is running.
fn panel_header(ui: &mut egui::Ui, title: &str, chevron: &str, tip: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui.small_button(chevron).on_hover_text(tip).clicked();
        });
    });
    close
}

fn open_in_editor(path: &std::path::Path) {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "code".into());
    let _ = std::process::Command::new(editor).arg(path).spawn();
}
