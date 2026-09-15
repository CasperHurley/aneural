//! egui panels: top bar, filters, inspector, spores.

use crate::camera::{CanvasRect, FrameRequest, PanGrab, UiCapture};
use crate::engine::IndexStatus;
use crate::filters::Filters;
use crate::focus::FocusState;
use crate::graph::{GraphEdge, GraphNode, GraphState, Hidden};
use crate::layout::LayoutParams;
use crate::picking::{Hovered, Selection};
use crate::theme;
use crate::workspace::WorkspaceRes;
use aneural_core::NodeId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use std::collections::BTreeMap;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default()).add_systems(
            EguiPrimaryContextPass,
            (style_once, panels, canvas_cursor).chain(),
        );
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

#[derive(Resource, Default)]
struct Styled(bool);

fn style_once(mut contexts: EguiContexts, styled: Option<ResMut<Styled>>, mut commands: Commands) {
    if styled.as_ref().is_some_and(|s| s.0) {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme::egui_hex(theme::PANEL);
    visuals.window_fill = theme::egui_hex(theme::PANEL);
    visuals.override_text_color = Some(theme::egui_hex(theme::TEXT));
    visuals.selection.bg_fill = theme::egui_hex(theme::DIM);
    visuals.hyperlink_color = theme::egui_hex(theme::ACCENT);
    visuals.widgets.active.bg_fill = theme::egui_hex(theme::DIM);
    visuals.widgets.hovered.bg_fill = theme::egui_hex("#1c2a22");
    // a hand over anything clickable, the way a web app behaves
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    ctx.set_visuals(visuals);
    commands.insert_resource(Styled(true));
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
    mut layout: ResMut<LayoutParams>,
    mut canvas: ResMut<CanvasRect>,
    nodes: Query<(&GraphNode, Has<Hidden>)>,
    edges: Query<&GraphEdge>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let accent = theme::egui_hex(theme::ACCENT);
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

    egui::Panel::top("top").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("🍄 Aneural").color(accent).strong());
            ui.label(egui::RichText::new(ws.name()).strong());
            ui.separator();
            let phase = if !status.complete {
                if status.total > 0 {
                    format!("Growing… {} {}/{}", status.phase, status.done, status.total)
                } else {
                    format!("Growing… {}", status.phase)
                }
            } else {
                "Grown".to_string()
            };
            ui.label(phase);
            if status.watching {
                ui.label(egui::RichText::new("● watching").color(accent));
            }
            ui.label(format!(
                "{} nodes · {} edges",
                graph.node_count, graph.edge_count
            ));
            if let Some(err) = &status.last_error {
                ui.label(
                    egui::RichText::new(format!("⚠ {err}"))
                        .color(egui::Color32::from_rgb(230, 120, 90)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("⛶")
                    .on_hover_text("Fit the whole graph in view (F)")
                    .clicked()
                {
                    frame.0 = true;
                }
                if ui
                    .button("🔄")
                    .on_hover_text("Stir the layout and let it settle again (Space)")
                    .clicked()
                {
                    layout.frozen = false;
                }
            });
        });
    });

    egui::Panel::left("filters").resizable(true).default_size(240.0).show(ctx, |ui| {
        ui.heading("Filters");
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
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 5.0, theme::egui_color(style.color));
                    if ui.checkbox(&mut on, format!("{} ({shown}/{total})", style.label)).hand().changed() {
                        filters.toggle_kind(&kind);
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new("Edge kinds").strong());
            let mut structural = filters.show_structural_edges;
            if ui.checkbox(&mut structural, format!("Contains (folder tree) ({})", edge_counts.get("CONTAINS").copied().unwrap_or(0))).hand().changed() {
                filters.show_structural_edges = structural;
                filters.dirty = true;
            }
            for kind in aneural_core::kinds::EdgeKind::ALL.iter().filter(|k| **k != "CONTAINS") {
                let mut on = !filters.hidden_edge_kinds.contains(*kind);
                let label = aneural_core::kinds::EdgeKind::label(kind);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 4.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 1.0, theme::egui_color(theme::edge_color(kind)));
                    if ui.checkbox(&mut on, format!("{label} ({})", edge_counts.get(*kind).copied().unwrap_or(0))).hand().changed() {
                        filters.toggle_edge_kind(kind);
                    }
                });
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
            ui.separator();
            ui.label(egui::RichText::new("Focus").strong());
            let mut fm = filters.focus_mode;
            if ui.checkbox(&mut fm, "focus mode (selection + neighbours only)").hand().changed() {
                filters.focus_mode = fm;
                filters.dirty = true;
            }
            let mut depth = filters.neighborhood_depth;
            if ui.add(egui::Slider::new(&mut depth, 0..=4).text("depth")).hand().changed() {
                filters.neighborhood_depth = depth;
                filters.dirty = true;
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Click: select · Shift-click: pin · Drag node: move · Drag canvas: pan · Wheel: zoom · F: frame").weak().small());
        });
    });

    egui::Panel::right("inspector").resizable(true).default_size(310.0).show(ctx, |ui| {
        ui.heading("Inspector");
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
                                    ui.label(egui::RichText::new(format!("{arrow} {name}")).color(theme::egui_color(theme::edge_color(&kind))).small());
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

    // whatever the panels left over is the graph canvas
    let rect = ctx.available_rect_before_wrap();
    let (min, max) = (
        Vec2::new(rect.min.x, rect.min.y),
        Vec2::new(rect.max.x, rect.max.y),
    );
    if canvas.min != min || canvas.max != max {
        canvas.min = min;
        canvas.max = max;
    }
}

fn open_in_editor(path: &std::path::Path) {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "code".into());
    let _ = std::process::Command::new(editor).arg(path).spawn();
}
