//! Node visuals (circle mesh + icon sprite + label) and hyphae edge drawing.

use crate::camera::MainCamera;
use crate::graph::{GraphEdge, GraphNode, GraphState, GrowIn, Hidden, Pos};
use crate::picking::{Hovered, Selection};
use crate::theme;
use crate::workspace::{WorkspaceRes, node_radius};
use aneural_core::Node;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
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

/// The node's disc, carrying the colour it is drawn in so that
/// [`focus_context`] can put it back after fading it.
#[derive(Component)]
pub struct NodeBody(pub Color);

#[derive(Component)]
pub struct SelectionRing;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconAtlas>()
            .init_resource::<MeshCache>()
            .add_systems(Startup, (build_icon_atlas, hyphae_behind_nodes))
            .add_systems(
                Update,
                (
                    draw_edges,
                    label_visibility,
                    selection_ring,
                    hover_scale,
                    focus_context,
                ),
            );
    }
}

/// 2D gizmos are always queued last, whatever their depth, so the hyphae would
/// paint over the nodes. Park them on their own render layer: the main camera
/// draws that layer, then `NodeCamera` redraws the nodes over the top.
fn hyphae_behind_nodes(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = RenderLayers::layer(crate::camera::HYPHAE_LAYER);
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
        NodeBody(style.color),
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

/// Cubic Bézier hyphae sample points between two positions, sampled into
/// `segments` pieces. A hypha only a few pixels long needs two.
pub fn hypha_points(a: Vec2, b: Vec2, seed: u32, t_max: f32, segments: usize) -> Vec<Vec2> {
    let chord = b - a;
    let len = chord.length().max(1.0);
    let perp = chord.perp() / len;
    let n1 = ((seed % 1000) as f32 / 1000.0) * 2.0 - 1.0;
    let n2 = (((seed / 1000) % 1000) as f32 / 1000.0) * 2.0 - 1.0;
    let c1 = a + chord * 0.3 + perp * len * BOW * n1;
    let c2 = a + chord * 0.7 + perp * len * BOW * n2;
    let segments = segments.max(1);
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

/// How far a hypha's control points swing off the straight chord, as a
/// fraction of its length. Also the padding the cull box needs.
const BOW: f32 = 0.18;

/// Roughly how many screen pixels of hypha each sample covers. Curves read as
/// curves at this rate, and a graph of thousands stops paying for the rest.
const PIXELS_PER_SAMPLE: f32 = 26.0;

/// What a hypha fades to when something else has the focus.
const GHOST: f32 = 0.05;

fn draw_edges(
    mut gizmos: Gizmos,
    edges: Query<(&GraphEdge, Option<&GrowIn>)>,
    nodes: Query<(&Pos, Has<Hidden>), With<GraphNode>>,
    camera: Query<(&Transform, &Projection), With<MainCamera>>,
    graph: Res<GraphState>,
    filters: Res<crate::filters::Filters>,
    selection: Res<Selection>,
    hovered: Res<Hovered>,
) {
    let Ok((cam, Projection::Orthographic(ortho))) = camera.single() else {
        return;
    };
    let eye = cam.translation.truncate();
    let half = ortho.area.size() * 0.5 * 1.1;
    let (view_min, view_max) = (eye - half, eye + half);
    let scale = ortho.scale.max(1e-3);

    // Focus and context: with one node in hand its own hyphae stay lit and the
    // rest of the mesh drops to a whisper, so a single thread can be followed
    // across a crowd instead of vanishing into it.
    let focus = selection
        .primary
        .as_ref()
        .or(hovered.0.as_ref())
        .and_then(|id| graph.by_id.get(id))
        .copied();

    for (e, grow) in &edges {
        if !filters.edge_visible(&e.kind) {
            continue;
        }
        let lit = focus.is_some_and(|f| f == e.src || f == e.dst);
        let structural = e.kind == "CONTAINS";
        // Hairball control. A link between two nodes that are each already
        // tangled in dozens says very little on its own and costs a stroke
        // through the middle of everything, so it waits until one of its ends
        // is picked up. The folder tree is never cut: it is the skeleton.
        let crowd = graph
            .kind_degree(e.src, &e.kind)
            .min(graph.kind_degree(e.dst, &e.kind));
        if !lit && filters.decluttered(&e.kind, crowd) {
            continue;
        }
        let (Ok((a, ha)), Ok((b, hb))) = (nodes.get(e.src), nodes.get(e.dst)) else {
            continue;
        };
        if ha || hb {
            continue;
        }
        let (a, b) = (a.0, b.0);

        let len = (b - a).length();
        let pad = Vec2::splat(len * BOW);
        let (lo, hi) = (a.min(b) - pad, a.max(b) + pad);
        if hi.x < view_min.x || lo.x > view_max.x || hi.y < view_min.y || lo.y > view_max.y {
            continue;
        }

        let t = grow.map(|g| g.t).unwrap_or(1.0);
        let segments = ((len / scale / PIXELS_PER_SAMPLE).ceil() as usize).clamp(2, 20);
        let pts = hypha_points(a, b, e.seed, t, segments);
        let base = theme::edge_color(&e.kind);
        let alpha = match (lit, focus.is_some(), structural) {
            (true, _, _) => 1.0,
            (false, true, _) => GHOST,
            (false, false, true) => 0.3,
            (false, false, false) => 0.4,
        };
        // The glow doubles a hypha's ink, so it is spent only on the thread
        // the user is actually looking at.
        if lit {
            let off = (b - a).perp().normalize_or_zero() * 1.5;
            let glow = base.with_alpha(0.3);
            gizmos.linestrip_2d(pts.iter().map(|p| *p + off), glow);
            gizmos.linestrip_2d(pts.iter().map(|p| *p - off), glow);
        }
        gizmos.linestrip_2d(pts.iter().copied(), base.with_alpha(alpha));
    }
}

/// Fade every node that is not the focused one or one of its neighbours. The
/// canvas is black, so darkening towards it reads as a fade without paying for
/// transparency on a thousand meshes.
fn focus_context(
    selection: Res<Selection>,
    hovered: Res<Hovered>,
    graph: Res<GraphState>,
    bodies: Query<(&ChildOf, &NodeBody, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut last: Local<Option<Entity>>,
) {
    let id = selection.primary.as_ref().or(hovered.0.as_ref());
    let focus = id.and_then(|id| graph.by_id.get(id)).copied();
    if *last == focus {
        return;
    }
    *last = focus;

    let mut keep = bevy::ecs::entity::EntityHashSet::default();
    if let (Some(f), Some(id)) = (focus, id) {
        keep.insert(f);
        for (n, _) in graph.neighbors(id) {
            if let Some(&e) = graph.by_id.get(n) {
                keep.insert(e);
            }
        }
    }
    for (parent, body, mat) in &bodies {
        let faded = focus.is_some() && !keep.contains(&parent.parent());
        let want = if faded { dimmed(body.0) } else { body.0 };
        // Moving from one node to its neighbour leaves almost every other node
        // exactly as it was, so look before writing: an untouched material is
        // not re-uploaded.
        if materials.get(&mat.0).is_some_and(|m| m.color == want) {
            continue;
        }
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.color = want;
        }
    }
}

/// Towards the black canvas, which on this background reads as a fade.
fn dimmed(c: Color) -> Color {
    let s = c.to_srgba();
    Color::srgba(s.red * 0.22, s.green * 0.22, s.blue * 0.22, s.alpha)
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
