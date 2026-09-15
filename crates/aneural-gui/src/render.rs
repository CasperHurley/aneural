//! Node visuals (circle mesh + icon sprite + label) and hyphae edge drawing.

use crate::camera::MainCamera;
use crate::graph::{GraphEdge, GraphNode, GrowIn, Hidden, Pos};
use crate::picking::{Hovered, Selection};
use crate::theme;
use crate::workspace::{WorkspaceRes, node_radius};
use aneural_core::Node;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;
use std::path::Path;

#[derive(Resource, Default)]
pub struct IconAtlas {
    pub by_name: HashMap<&'static str, Handle<Image>>,
}

#[derive(Resource, Default)]
pub struct MeshCache {
    pub materials: HashMap<String, Handle<ColorMaterial>>,
}

#[derive(Component)]
pub struct NodeLabel;

#[derive(Component)]
pub struct NodeBody;

#[derive(Component)]
pub struct SelectionRing;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconAtlas>()
            .init_resource::<MeshCache>()
            .add_systems(Startup, build_icon_atlas)
            .add_systems(
                Update,
                (draw_edges, label_visibility, selection_ring, hover_scale),
            );
    }
}

fn build_icon_atlas(mut atlas: ResMut<IconAtlas>, mut images: ResMut<Assets<Image>>) {
    for name in aneural_icons::names() {
        let Ok(rgba) = aneural_icons::rasterize_named(name, 64) else {
            continue;
        };
        let image = Image::new(
            Extent3d {
                width: rgba.width,
                height: rgba.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba.data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        atlas.by_name.insert(name, images.add(image));
    }
    info!("rasterised {} icons", atlas.by_name.len());
}

pub fn icon_for(node: &Node, ws: &WorkspaceRes) -> &'static str {
    if node.kind == "File"
        && let Some(p) = &node.path
    {
        return aneural_icons::default_icon("File", Some(Path::new(p)));
    }
    ws.style(&node.kind).icon
}

pub fn spawn_node_visuals(
    commands: &mut Commands,
    entity: Entity,
    node: &Node,
    ws: &WorkspaceRes,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    atlas: &IconAtlas,
) {
    let style = ws.style(&node.kind);
    let r = node_radius(&node.kind);
    let mesh = meshes.add(shape_mesh(&style.shape, r));
    let mat = materials.add(ColorMaterial::from_color(style.color));
    let icon = icon_for(node, ws);
    let mut body = commands.spawn((
        NodeBody,
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));
    body.insert(ChildOf(entity));
    if let Some(img) = atlas.by_name.get(icon) {
        let size = r * 1.2;
        commands.spawn((
            Sprite {
                image: img.clone(),
                color: theme::hex("#0d1210"),
                custom_size: Some(Vec2::splat(size)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 2.0),
            ChildOf(entity),
        ));
    }
    let label = if node.label.chars().count() > 28 {
        format!("{}…", node.label.chars().take(27).collect::<String>())
    } else {
        node.label.clone()
    };
    commands.spawn((
        NodeLabel,
        Text2d(label),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(theme::hex(theme::TEXT)),
        Transform::from_xyz(0.0, -(r + 9.0), 1.5),
        Visibility::Hidden,
        ChildOf(entity),
    ));
}

/// Mesh for a node shape name (`circle` | `hexagon` | `pill` | `square` | `diamond`).
pub fn shape_mesh(shape: &str, r: f32) -> Mesh {
    match shape {
        "hexagon" => RegularPolygon::new(r * 1.1, 6).into(),
        "square" => Rectangle::new(r * 1.8, r * 1.8).into(),
        "pill" => Capsule2d::new(r * 0.55, r * 1.2).into(),
        "diamond" => Rhombus::new(r * 2.2, r * 1.8).into(),
        _ => Circle::new(r).into(),
    }
}

/// Cubic Bézier hyphae sample points between two positions.
pub fn hypha_points(a: Vec2, b: Vec2, seed: u32, t_max: f32) -> Vec<Vec2> {
    let chord = b - a;
    let len = chord.length().max(1.0);
    let perp = chord.perp() / len;
    let n1 = ((seed % 1000) as f32 / 1000.0) * 2.0 - 1.0;
    let n2 = (((seed / 1000) % 1000) as f32 / 1000.0) * 2.0 - 1.0;
    let c1 = a + chord * 0.3 + perp * len * 0.18 * n1;
    let c2 = a + chord * 0.7 + perp * len * 0.18 * n2;
    let segments = 20;
    let count = ((segments as f32 * t_max).ceil() as usize).clamp(1, segments);
    let mut pts = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let t = (i as f32 / segments as f32).min(t_max);
        let u = 1.0 - t;
        let p = a * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + b * (t * t * t);
        pts.push(p);
    }
    pts
}

fn draw_edges(
    mut gizmos: Gizmos,
    edges: Query<(&GraphEdge, Option<&GrowIn>)>,
    nodes: Query<(&Pos, Has<Hidden>), With<GraphNode>>,
    filters: Res<crate::filters::Filters>,
    selection: Res<Selection>,
) {
    for (e, grow) in &edges {
        if !filters.edge_visible(&e.kind) {
            continue;
        }
        let (Ok((a, ha)), Ok((b, hb))) = (nodes.get(e.src), nodes.get(e.dst)) else {
            continue;
        };
        if ha || hb {
            continue;
        }
        let t = grow.map(|g| g.t).unwrap_or(1.0);
        let pts = hypha_points(a.0, b.0, e.seed, t);
        let base = theme::edge_color(&e.kind);
        let highlighted = selection
            .primary
            .as_ref()
            .is_some_and(|p| p == &e.src_id || p == &e.dst_id);
        let core_alpha = if e.kind == "CONTAINS" { 0.35 } else { 0.7 };
        let glow_alpha = if highlighted { 0.35 } else { 0.08 };
        let core = base.with_alpha(if highlighted { 1.0 } else { core_alpha });
        let glow = base.with_alpha(glow_alpha);
        // glow: two parallel offset strips
        if e.kind != "CONTAINS" || highlighted {
            let off = (b.0 - a.0).perp().normalize_or_zero() * 1.5;
            gizmos.linestrip_2d(pts.iter().map(|p| *p + off), glow);
            gizmos.linestrip_2d(pts.iter().map(|p| *p - off), glow);
        }
        gizmos.linestrip_2d(pts.iter().copied(), core);
    }
}

fn label_visibility(
    camera: Query<&Projection, With<MainCamera>>,
    ws: Res<WorkspaceRes>,
    nodes: Query<(Entity, &Children, Has<Hidden>), With<GraphNode>>,
    mut labels: Query<&mut Visibility, With<NodeLabel>>,
    selection: Res<Selection>,
    hovered: Res<Hovered>,
    graph: Res<crate::graph::GraphState>,
) {
    let Ok(Projection::Orthographic(ortho)) = camera.single() else {
        return;
    };
    let show = ortho.scale < ws.config.gui.label_zoom_threshold;
    let selected = selection
        .primary
        .as_ref()
        .and_then(|id| graph.by_id.get(id))
        .copied();
    let hovered = hovered
        .0
        .as_ref()
        .and_then(|id| graph.by_id.get(id))
        .copied();
    for (entity, children, hidden) in &nodes {
        let forced = selected == Some(entity) || hovered == Some(entity);
        for c in children.iter() {
            if let Ok(mut v) = labels.get_mut(c) {
                let want = !hidden && (show || forced);
                *v = if want {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

fn selection_ring(
    mut commands: Commands,
    selection: Res<Selection>,
    graph: Res<crate::graph::GraphState>,
    nodes: Query<(&Pos, &GraphNode)>,
    ring: Query<Entity, With<SelectionRing>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut cache: ResMut<MeshCache>,
) {
    if !selection.is_changed() && !ring.is_empty() {
        // keep ring following the node
        return;
    }
    for r in &ring {
        commands.entity(r).despawn();
    }
    let Some(id) = &selection.primary else { return };
    let Some(&e) = graph.by_id.get(id) else {
        return;
    };
    let Ok((_, gn)) = nodes.get(e) else { return };
    let r = node_radius(&gn.kind) + 4.0;
    let mesh = meshes.add(Circle::new(r));
    let mat = cache
        .materials
        .entry("selection".into())
        .or_insert_with(|| materials.add(ColorMaterial::from_color(theme::hex(theme::SELECTION))))
        .clone();
    commands.spawn((
        SelectionRing,
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 0.0, 0.5),
        ChildOf(e),
    ));
}

fn hover_scale(
    hovered: Res<Hovered>,
    graph: Res<crate::graph::GraphState>,
    mut bodies: Query<(&ChildOf, &mut Transform), With<NodeBody>>,
) {
    let target = hovered
        .0
        .as_ref()
        .and_then(|id| graph.by_id.get(id))
        .copied();
    for (parent, mut t) in &mut bodies {
        let want = if Some(parent.parent()) == target {
            1.25
        } else {
            1.0
        };
        t.scale = Vec3::splat(want);
    }
}
