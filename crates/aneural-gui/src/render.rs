//! Node visuals (circle mesh + icon sprite + label), hyphae edge drawing, and
//! the bioluminescence that comes over all of it after dark.

use crate::camera::MainCamera;
use crate::circadian::Vibe;
use crate::graph::{Drift, GraphEdge, GraphNode, GraphState, GrowIn, Hidden, Pos};
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

/// One soft radial falloff, shared by every node's halo, so the whole glow
/// costs a single texture and batches as a single draw.
#[derive(Resource, Default)]
pub struct GlowTexture(pub Handle<Image>);

#[derive(Resource, Default)]
pub struct MeshCache {
    pub materials: HashMap<String, Handle<ColorMaterial>>,
}

#[derive(Component)]
pub struct NodeLabel;

/// The icon punched into a node's disc. It darkens as the disc brightens, so
/// it stays legible against a glowing node.
#[derive(Component)]
pub struct NodeIcon;

/// The node's disc, carrying the colour it is drawn in so that
/// [`node_colors`] can put it back after fading or lighting it.
#[derive(Component)]
pub struct NodeBody(pub Color);

/// The halo behind a node: invisible by day, and after dark the light the
/// node is giving off. Carries the node's daylight colour, so the halo can be
/// relit from it as the night comes on.
#[derive(Component)]
pub struct GlowHalo(pub Color);

#[derive(Component)]
pub struct SelectionRing;

/// Everything a node needs in order to be drawn for the first time. Bundled
/// because a sprouting node wants five unrelated things and threading them
/// one by one through the delta path buries the code that matters.
pub struct Visuals<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<ColorMaterial>,
    pub atlas: &'a IconAtlas,
    pub glow: &'a GlowTexture,
    pub vibe: &'a Vibe,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconAtlas>()
            .init_resource::<GlowTexture>()
            .init_resource::<MeshCache>()
            .add_systems(
                Startup,
                (build_icon_atlas, build_glow_texture, hyphae_behind_nodes),
            )
            .add_systems(
                Update,
                (
                    (draw_spore_motes, draw_edges).chain(),
                    hidden_visibility,
                    label_visibility,
                    selection_ring,
                    hover_scale,
                    node_colors,
                    night_chrome,
                    breathe_halos,
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

/// White, with the alpha falling off from the middle. A sprite tinted with
/// the node's colour then reads as that colour's light.
fn build_glow_texture(mut glow: ResMut<GlowTexture>, mut images: ResMut<Assets<Image>>) {
    const SIZE: usize = 96;
    let mut data = vec![0u8; SIZE * SIZE * 4];
    let centre = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = Vec2::new(x as f32 - centre, y as f32 - centre).length() / centre;
            // A steep-ish falloff: a small bright core and a wide, very faint
            // skirt, which is what a light source in fog actually looks like.
            let a = (1.0 - d).clamp(0.0, 1.0).powf(2.6);
            let i = (y * SIZE + x) * 4;
            data[i..i + 3].fill(255);
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    glow.0 = images.add(Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));
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
    v: &mut Visuals,
) {
    let style = ws.style(&node.kind);
    let r = node_radius(&node.kind);
    let palette = &v.vibe.palette;
    let mesh = v.meshes.add(shape_mesh(&style.shape, r));
    let mat = v
        .materials
        .add(ColorMaterial::from_color(palette.bioluminesce(style.color)));
    let icon = icon_for(node, ws);
    // Behind the disc, so the node sits in its own light rather than under it.
    let halo = r * 4.0;
    commands.spawn((
        GlowHalo(style.color),
        Sprite {
            image: v.glow.0.clone(),
            color: palette.glow(style.color).with_alpha(0.0),
            custom_size: Some(Vec2::splat(halo)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.25),
        Visibility::Hidden,
        ChildOf(entity),
    ));
    let mut body = commands.spawn((
        NodeBody(style.color),
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));
    body.insert(ChildOf(entity));
    if let Some(img) = v.atlas.by_name.get(icon) {
        let size = r * 1.2;
        commands.spawn((
            NodeIcon,
            Sprite {
                image: img.clone(),
                color: palette.icon_ink,
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
        TextColor(palette.text),
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

/// How long one pulse takes to travel a hypha, at the fast and slow ends.
/// Each hypha picks its own from its seed, so the mesh never flashes in time.
const PULSE_SECONDS: (f32, f32) = (7.0, 14.0);
/// How much of a hypha one pulse lights at a time.
const PULSE_WIDTH: f32 = 0.16;

/// Where a hypha's pulse is along its length right now, in a cycle that
/// carries it off the far end and leaves a dark gap before the next.
fn pulse_head(clock: f32, seed: u32) -> f32 {
    let (fast, slow) = PULSE_SECONDS;
    let cycle = fast + (slow - fast) * ((seed >> 7) % 1000) as f32 / 1000.0;
    let phase = ((clock / cycle) + (seed % 1000) as f32 / 1000.0).fract();
    // 0..1 of the cycle maps past both ends, so for most of it the hypha is dark
    phase * 2.6 - 0.8
}

#[allow(clippy::too_many_arguments)]
fn draw_edges(
    mut gizmos: Gizmos,
    edges: Query<(&GraphEdge, Option<&GrowIn>)>,
    nodes: Query<(&Pos, Option<&Drift>, Has<Hidden>), With<GraphNode>>,
    camera: Query<(&Transform, &Projection), With<MainCamera>>,
    graph: Res<GraphState>,
    filters: Res<crate::filters::Filters>,
    selection: Res<Selection>,
    hovered: Res<Hovered>,
    vibe: Res<Vibe>,
) {
    let Ok((cam, Projection::Orthographic(ortho))) = camera.single() else {
        return;
    };
    let eye = cam.translation.truncate();
    let half = ortho.area.size() * 0.5 * 1.1;
    let (view_min, view_max) = (eye - half, eye + half);
    let scale = ortho.scale.max(1e-3);
    let palette = &vibe.palette;
    let night = palette.night;
    // After dark the whole mesh dims and lifts together with the breath, and
    // each hypha carries a slow pulse of light along itself.
    let swell = 1.0 + 0.10 * night * vibe.breath();
    let pulsing = night > 0.04;

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
        let (Ok((a, da, ha)), Ok((b, db, hb))) = (nodes.get(e.src), nodes.get(e.dst)) else {
            continue;
        };
        if ha || hb {
            continue;
        }
        // The ends wander with the nodes, so a hypha stays rooted in both.
        let drift = |d: Option<&Drift>| d.map(|d| d.offset).unwrap_or(Vec2::ZERO);
        let (a, b) = (a.0 + drift(da), b.0 + drift(db));

        let len = (b - a).length();
        let pad = Vec2::splat(len * BOW);
        let (lo, hi) = (a.min(b) - pad, a.max(b) + pad);
        if hi.x < view_min.x || lo.x > view_max.x || hi.y < view_min.y || lo.y > view_max.y {
            continue;
        }

        let t = grow.map(|g| g.t).unwrap_or(1.0);
        let segments = ((len / scale / PIXELS_PER_SAMPLE).ceil() as usize).clamp(2, 20);
        let pts = hypha_points(a, b, e.seed, t, segments);
        let base = palette.edge(&e.kind);
        let alpha = match (lit, focus.is_some(), structural) {
            (true, _, _) => 1.0,
            (false, true, _) => GHOST,
            (false, false, true) => 0.3,
            (false, false, false) => 0.4,
        } * swell;
        // The glow doubles a hypha's ink, so it is spent only on the thread
        // the user is actually looking at.
        if lit {
            let off = (b - a).perp().normalize_or_zero() * 1.5;
            let glow = base.with_alpha(0.3);
            gizmos.linestrip_2d(pts.iter().map(|p| *p + off), glow);
            gizmos.linestrip_2d(pts.iter().map(|p| *p - off), glow);
        }
        let head = pulse_head(vibe.clock, e.seed);
        // Most hyphae are between pulses at any moment; those cost nothing
        // extra and are drawn flat.
        if !pulsing || !(-PULSE_WIDTH * 3.0..1.0 + PULSE_WIDTH * 3.0).contains(&head) {
            gizmos.linestrip_2d(pts.iter().copied(), base.with_alpha(alpha.min(1.0)));
            continue;
        }
        let last = (pts.len() - 1).max(1) as f32;
        gizmos.linestrip_gradient_2d(pts.iter().enumerate().map(|(i, p)| {
            let u = (i as f32 / last) * t;
            let g = (-((u - head) / PULSE_WIDTH).powi(2)).exp() * night;
            (
                *p,
                theme::mix(base, palette.accent, g * 0.55)
                    .with_alpha((alpha * (1.0 + g * 1.3)).min(1.0)),
            )
        }));
    }
}

/// How many motes drift across the canvas at the deepest point of the night.
/// They are kept small and faint on purpose: a mote the size of a node is not
/// atmosphere, it is a node the user cannot click.
const MOTES: usize = 30;

/// splitmix64's finaliser over an index and a salt, as a float in 0..1: a
/// mote's whole character, without keeping anything between frames.
fn rand01(i: usize, salt: u64) -> f32 {
    let mut h = (i as u64 + 1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(salt.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 11) as f32 / (1u64 << 53) as f32
}

/// Spores in the air between the viewer and the mesh. They hang in front of
/// the lens rather than in the world, so panning the graph does not shake
/// them, and there is nothing to keep track of between frames.
fn draw_spore_motes(
    mut gizmos: Gizmos,
    camera: Query<(&Transform, &Projection), With<MainCamera>>,
    vibe: Res<Vibe>,
) {
    let night = vibe.palette.night;
    if night < 0.06 {
        return;
    }
    let Ok((cam, Projection::Orthographic(ortho))) = camera.single() else {
        return;
    };
    let size = ortho.area.size();
    let origin = cam.translation.truncate() - size * 0.5;
    let count = (MOTES as f32 * night) as usize;
    let t = vibe.clock;
    for i in 0..count {
        let (x0, y0, speed, size_seed) = (rand01(i, 1), rand01(i, 2), rand01(i, 3), rand01(i, 4));
        // A slow rise with a lazy sideways sway, wrapped in the view: a spore
        // that leaves the top comes back in at the bottom.
        let rise = (y0 + t * (0.004 + 0.010 * speed)).fract();
        let sway = (x0 + (t * 0.05 + y0 * std::f32::consts::TAU).sin() * 0.02).rem_euclid(1.0);
        let p = origin + size * Vec2::new(sway, rise);
        // Constant size on screen, whatever the zoom.
        let r = (0.5 + 1.0 * size_seed) * ortho.scale;
        let twinkle = 0.45 + 0.55 * ((t * 0.6 + size_seed * 20.0).sin() * 0.5 + 0.5);
        let alpha = night * 0.12 * twinkle;
        gizmos
            .circle_2d(p, r, vibe.palette.accent.with_alpha(alpha))
            .resolution(8);
    }
}

/// Fade every node that is not the focused one or one of its neighbours, and
/// light the rest by however much of the night there is. The canvas is black,
/// so darkening towards it reads as a fade without paying for transparency on
/// a thousand meshes.
fn node_colors(
    selection: Res<Selection>,
    hovered: Res<Hovered>,
    graph: Res<GraphState>,
    vibe: Res<Vibe>,
    bodies: Query<(&ChildOf, &NodeBody, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut last: Local<Option<(Option<Entity>, u8)>>,
) {
    let id = selection.primary.as_ref().or(hovered.0.as_ref());
    let focus = id.and_then(|id| graph.by_id.get(id)).copied();
    // The night moves too slowly to repaint a thousand materials every frame;
    // a step of it is a fine grain to notice.
    let step = quantized_night(&vibe);
    if *last == Some((focus, step)) {
        return;
    }
    *last = Some((focus, step));

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
        let lit = vibe.palette.bioluminesce(body.0);
        let faded = focus.is_some() && !keep.contains(&parent.parent());
        let want = if faded { dimmed(lit) } else { lit };
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

/// The night in 1/48ths: the grain at which the palette is worth repainting.
fn quantized_night(vibe: &Vibe) -> u8 {
    (vibe.palette.night * 48.0).round() as u8
}

/// Labels, icons and the selection ring follow the palette. Like the node
/// bodies, only when it has actually moved.
fn night_chrome(
    vibe: Res<Vibe>,
    mut icons: Query<&mut Sprite, With<NodeIcon>>,
    mut labels: Query<&mut TextColor, With<NodeLabel>>,
    cache: Res<MeshCache>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut last: Local<Option<u8>>,
) {
    let step = quantized_night(&vibe);
    if *last == Some(step) {
        return;
    }
    *last = Some(step);
    for mut sprite in &mut icons {
        sprite.color = vibe.palette.icon_ink;
    }
    for mut label in &mut labels {
        label.0 = vibe.palette.text;
    }
    if let Some(handle) = cache.materials.get("selection")
        && let Some(mut m) = materials.get_mut(handle)
    {
        m.color = vibe.palette.selection;
    }
}

/// The halo behind each node: dark and hidden by day, and after dark a glow
/// that swells and settles on the breath rippling out across the graph.
fn breathe_halos(
    vibe: Res<Vibe>,
    positions: Query<&Pos>,
    mut halos: Query<(
        &ChildOf,
        &GlowHalo,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
    mut lit: Local<bool>,
) {
    let night = vibe.palette.night;
    if night < 0.02 {
        if !*lit {
            return;
        }
        *lit = false;
        for (_, _, _, _, mut visibility) in &mut halos {
            *visibility = Visibility::Hidden;
        }
        return;
    }
    *lit = true;
    for (parent, halo, mut sprite, mut transform, mut visibility) in &mut halos {
        // A halo is spawned dark, and nodes keep sprouting long after the
        // first frame, so this cannot be done once on the way into the night.
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
        let at = positions
            .get(parent.parent())
            .map(|p| p.0)
            .unwrap_or_default();
        let breath = vibe.breath_at(at);
        sprite.color = vibe
            .palette
            .bioluminesce(halo.0)
            .with_alpha(night * (0.30 + 0.13 * breath));
        transform.scale = Vec3::splat(1.0 + 0.07 * night * breath);
    }
}

/// Towards the black canvas, which on this background reads as a fade.
fn dimmed(c: Color) -> Color {
    let s = c.to_srgba();
    Color::srgba(s.red * 0.22, s.green * 0.22, s.blue * 0.22, s.alpha)
}

/// `Hidden` is the marker the filters put on a node; this is what stops it
/// being drawn — the disc, its icon, its label and its halo with it.
fn hidden_visibility(mut nodes: Query<(&mut Visibility, Has<Hidden>), With<GraphNode>>) {
    for (mut visibility, hidden) in &mut nodes {
        let want = if hidden {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != want {
            *visibility = want;
        }
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
    vibe: Res<Vibe>,
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
        .or_insert_with(|| materials.add(ColorMaterial::from_color(vibe.palette.selection)))
        .clone();
    commands.spawn((
        SelectionRing,
        Mesh2d(mesh),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 0.0, 0.5),
        ChildOf(e),
    ));
}

/// A node grows under the cursor, and after dark every node rises and falls a
/// little on the breath travelling out across the graph.
fn hover_scale(
    hovered: Res<Hovered>,
    graph: Res<crate::graph::GraphState>,
    vibe: Res<Vibe>,
    positions: Query<&Pos>,
    mut bodies: Query<(&ChildOf, &mut Transform), With<NodeBody>>,
) {
    let target = hovered
        .0
        .as_ref()
        .and_then(|id| graph.by_id.get(id))
        .copied();
    let swell = 0.035 * vibe.palette.night;
    for (parent, mut t) in &mut bodies {
        let want = if Some(parent.parent()) == target {
            1.25
        } else {
            1.0
        };
        let breath = if swell > 0.0005 {
            let at = positions
                .get(parent.parent())
                .map(|p| p.0)
                .unwrap_or_default();
            1.0 + swell * vibe.breath_at(at)
        } else {
            1.0
        };
        t.scale = Vec3::splat(want * breath);
    }
}
