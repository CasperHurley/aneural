//! The app's circadian rhythm.
//!
//! One number, `Vibe::night`, runs everything: 0 in broad daylight, 1 in the
//! small hours. It is read off the user's wall clock, and it drives the
//! palette ([`crate::theme`]), the glow around the nodes, the breath in the
//! graph and the drift of the spores. At 0 all of that is switched off and
//! the app is exactly the tool it is at noon; the further into the night, the
//! more it looks like the thing it is named after. The one motion that keeps
//! going by day is the float of nodes that relate to others rather than sit in
//! the tree: that is how their relation is shown, not atmosphere.
//!
//! The clock can be overruled — `gui.circadian` in `config.json`, or the dial
//! in the top bar — because a demo at 2am and a screenshot for a deck both
//! want to choose.

use crate::graph::{Drift, GraphNode, GraphState};
use crate::theme::Palette;
use crate::workspace::WorkspaceRes;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// One breath of the graph, in seconds. Slow enough that it is felt rather
/// than watched: about the rate of a calm human breath.
const BREATH_PERIOD: f32 = 9.0;
/// How far a breath travels across the graph, in world units, so the whole
/// mesh does not pulse in lockstep like a metronome.
const BREATH_WAVELENGTH: f32 = 900.0;
/// How far a node can wander from where the layout put it, at the deepest
/// point of the night. Well under a node's radius, so nothing ever looks
/// misplaced — only unsettled. In screen pixels: zoomed out on a whole
/// codebase, a wander measured in world units would be too small to see.
const DRIFT_PIXELS: f32 = 4.0;
/// Normalises the two wanders below so their sum tops out at exactly 1.
const DRIFT_NORM: f32 = 0.487;
/// How far a floating node strays from where the layout holds it, day or
/// night. Wider than a node, so it visibly hovers instead of sitting still.
const FLOAT_PIXELS: f32 = 11.0;
/// The slowest and quickest a floating node takes to circle once, in seconds.
const FLOAT_SECONDS: (f32, f32) = (26.0, 44.0);
/// Seconds between readings of the wall clock. Nothing here moves fast.
const CLOCK_INTERVAL: f32 = 2.0;

/// Whether the look follows the clock, or is held at one end of the day.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VibeMode {
    #[default]
    Auto,
    Day,
    Night,
}

impl VibeMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "day" | "daylight" | "off" => VibeMode::Day,
            "night" => VibeMode::Night,
            _ => VibeMode::Auto,
        }
    }

    /// The dial cycles: follow the clock, hold the day, hold the night.
    pub fn next(self) -> Self {
        match self {
            VibeMode::Auto => VibeMode::Day,
            VibeMode::Day => VibeMode::Night,
            VibeMode::Night => VibeMode::Auto,
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            VibeMode::Auto => "following your clock — click to hold it at daylight",
            VibeMode::Day => "held at daylight — click to hold it at night",
            VibeMode::Night => "held at night — click to follow your clock again",
        }
    }
}

/// How dark it is out, where the graph is in its breath, and the palette that
/// follows from both.
#[derive(Resource)]
pub struct Vibe {
    pub mode: VibeMode,
    /// Has the user touched the dial this session? Until they do, each
    /// workspace's own `gui.circadian` applies as it is opened; afterwards
    /// their choice outlives the workspace they made it in.
    pub touched: bool,
    /// The mode the last reading was taken in, so that turning the dial shows
    /// immediately instead of at the next reading.
    last_mode: VibeMode,
    /// 0 in broad daylight, 1 in the small hours. Eased, so that a mode
    /// change is a dusk rather than a switch.
    pub night: f32,
    /// Where `night` is heading, straight off the clock.
    target: f32,
    /// Is the night still coming on? Only used to name the hour.
    rising: bool,
    /// Seconds since the app started: the clock every slow motion runs on.
    pub clock: f32,
    pub palette: Palette,
    /// The last local offset the `time` crate would give us. It refuses in a
    /// threaded process on some platforms, so the one read at startup — while
    /// the process was still single-threaded — is kept as the fallback.
    offset: Option<time::UtcOffset>,
    next_reading: f32,
}

impl Vibe {
    pub fn new(mode: VibeMode) -> Self {
        let mut offset = time::UtcOffset::current_local_offset().ok();
        if offset.is_none() {
            debug!("no local UTC offset available; the clock will read as UTC");
        }
        let (target, rising) = reading(mode, &mut offset);
        Vibe {
            mode,
            touched: false,
            last_mode: mode,
            night: target,
            target,
            rising,
            clock: 0.0,
            palette: Palette::at(target),
            offset,
            next_reading: CLOCK_INTERVAL,
        }
    }

    /// Turn the dial. What the user picks holds for the session, whatever
    /// the next workspace's config says.
    pub fn set_mode(&mut self, mode: VibeMode) {
        self.mode = mode;
        self.touched = true;
    }

    /// Take the mode from a workspace that has just been opened, unless the
    /// user has already said what they want.
    pub fn adopt(&mut self, configured: &str) {
        if !self.touched {
            self.mode = VibeMode::parse(configured);
        }
    }

    /// The name of the hour, as the app would tell it.
    pub fn phase(&self) -> &'static str {
        let n = self.night;
        if self.rising {
            match n {
                n if n < 0.12 => "Daylight",
                n if n < 0.32 => "Golden hour",
                n if n < 0.55 => "Dusk",
                n if n < 0.78 => "Nightfall",
                n if n < 0.93 => "Deep night",
                _ => "The witching hour",
            }
        } else {
            match n {
                n if n < 0.12 => "Daylight",
                n if n < 0.40 => "Dawn",
                n if n < 0.72 => "First light",
                n if n < 0.93 => "Small hours",
                _ => "The witching hour",
            }
        }
    }

    /// The graph's breath at a point: one slow wave rippling outward from the
    /// middle, so the mesh moves like something alive rather than something
    /// on a timer. Returns roughly -1..1.
    pub fn breath_at(&self, p: Vec2) -> f32 {
        let phase = self.clock / BREATH_PERIOD - p.length() / BREATH_WAVELENGTH;
        (TAU * phase).sin() * 0.78 + (TAU * 2.0 * phase + 1.1).sin() * 0.22
    }

    pub fn breath(&self) -> f32 {
        self.breath_at(Vec2::ZERO)
    }

    /// Where a node sits relative to where the layout put it: two slow
    /// wanders at incommensurate rates, so it never repeats a path.
    pub fn drift(&self, seed: u32) -> Vec2 {
        let amp = self.night * DRIFT_PIXELS;
        if amp < 0.01 {
            return Vec2::ZERO;
        }
        let (t, p1, p2) = (
            self.clock,
            (seed & 0xFFFF) as f32 / 65535.0 * TAU,
            ((seed >> 16) & 0xFFFF) as f32 / 65535.0 * TAU,
        );
        let slow = Vec2::new((t * 0.12 + p1).sin(), (t * 0.10 + p2).cos());
        let quick = Vec2::new((t * 0.30 + p2).sin(), (t * 0.27 + p1).cos());
        (slow + quick * 0.45) * amp * DRIFT_NORM
    }

    /// Where a floating node hovers relative to its place: a slow lap around
    /// it whose reach swells and shrinks, each node at its own pace and phase.
    pub fn float(&self, seed: u32) -> Vec2 {
        let (fast, slow) = FLOAT_SECONDS;
        let u = |bits: u32| (bits & 0x3FF) as f32 / 1023.0;
        let period = fast + (slow - fast) * u(seed >> 3);
        // half of them circle the other way
        let turn = if seed & 1 == 0 { 1.0 } else { -1.0 };
        let angle = turn * TAU * self.clock / period + u(seed >> 13) * TAU;
        let reach = 0.7 + 0.3 * (self.clock * 0.17 + u(seed >> 23) * TAU).sin();
        Vec2::from_angle(angle) * reach * FLOAT_PIXELS
    }
}

/// The night level the clock calls for, and whether it is on its way up.
fn reading(mode: VibeMode, offset: &mut Option<time::UtcOffset>) -> (f32, bool) {
    match mode {
        VibeMode::Day => (0.0, false),
        VibeMode::Night => (1.0, true),
        VibeMode::Auto => {
            let h = local_hour(offset);
            let now = night_at_hour(h);
            (now, night_at_hour(h + 0.05) >= now)
        }
    }
}

/// Local time as an hour with a fraction, 0..24.
fn local_hour(offset: &mut Option<time::UtcOffset>) -> f32 {
    let now = time::OffsetDateTime::now_utc();
    // Re-ask every time: it is cheap, and it is how a session that runs
    // across a daylight-saving change keeps up. The startup value covers the
    // platforms that refuse once there are threads.
    if let Ok(o) = time::UtcOffset::current_local_offset() {
        *offset = Some(o);
    }
    let t = offset.map(|o| now.to_offset(o)).unwrap_or(now).time();
    t.hour() as f32 + t.minute() as f32 / 60.0 + t.second() as f32 / 3600.0
}

/// The curve of the day: flat through working hours, a long ramp through the
/// evening, deepest around 2:30am, and back up through the dawn. Smoothstep
/// between the keys, so there is no hour where the look visibly steps.
fn night_at_hour(hour: f32) -> f32 {
    const KEYS: &[(f32, f32)] = &[
        (0.0, 0.93),
        (2.5, 1.00),
        (4.5, 0.93),
        (6.0, 0.60),
        (7.5, 0.20),
        (8.5, 0.00),
        (16.5, 0.00),
        (18.0, 0.22),
        (19.5, 0.50),
        (21.0, 0.72),
        (23.0, 0.88),
        (24.0, 0.93),
    ];
    let hour = hour.rem_euclid(24.0);
    let mut prev = KEYS[0];
    for &k in &KEYS[1..] {
        if hour <= k.0 {
            let x = ((hour - prev.0) / (k.0 - prev.0).max(1e-4)).clamp(0.0, 1.0);
            return prev.1 + (k.1 - prev.1) * x * x * (3.0 - 2.0 * x);
        }
        prev = k;
    }
    KEYS[KEYS.len() - 1].1
}

pub struct CircadianPlugin;

impl Plugin for CircadianPlugin {
    fn build(&self, app: &mut App) {
        let mode = app
            .world()
            .get_resource::<WorkspaceRes>()
            .map(|ws| VibeMode::parse(&ws.config.gui.circadian))
            .unwrap_or_default();
        app.insert_resource(Vibe::new(mode))
            .add_systems(PreUpdate, (tick, drift_nodes).chain())
            .add_systems(Update, follow_clear_color);
    }
}

/// Advance the slow clock, re-read the wall clock now and then, and ease the
/// look towards it.
fn tick(time: Res<Time>, mut vibe: ResMut<Vibe>) {
    let dt = time.delta_secs();
    let vibe = vibe.bypass_change_detection();
    vibe.clock += dt;
    if vibe.clock >= vibe.next_reading || vibe.mode != vibe.last_mode {
        vibe.next_reading = vibe.clock + CLOCK_INTERVAL;
        vibe.last_mode = vibe.mode;
        let mode = vibe.mode;
        let (target, rising) = reading(mode, &mut vibe.offset);
        vibe.target = target;
        vibe.rising = rising;
    }
    // A mode change is a jump in the target; easing turns it into a dusk that
    // takes a couple of seconds.
    let k = 1.0 - (-dt * 1.4).exp();
    let next = vibe.night + (vibe.target - vibe.night) * k;
    if (next - vibe.night).abs() > 1e-4 {
        vibe.night = next;
        vibe.palette = Palette::at(next);
    } else if vibe.night != vibe.target {
        vibe.night = vibe.target;
        vibe.palette = Palette::at(vibe.target);
    }
}

/// Hand every node its wander for this frame. `Drift` is read by
/// `sync_transforms`, picking and the hyphae, so all of them agree.
fn drift_nodes(
    vibe: Res<Vibe>,
    graph: Res<GraphState>,
    camera: Query<&Projection, With<crate::camera::MainCamera>>,
    mut nodes: Query<(Entity, &GraphNode, &mut Drift)>,
) {
    // Both wanders are set in screen pixels, so a graph zoomed out to fit a
    // whole codebase moves as visibly as one filling the window.
    let Ok(Projection::Orthographic(ortho)) = camera.single() else {
        return;
    };
    let zoom = ortho.scale.max(1e-3);
    let night = vibe.night >= 0.01;
    for (e, gn, mut d) in &mut nodes {
        let mut offset = Vec2::ZERO;
        if graph.floats(e, &gn.id) {
            offset += vibe.float(d.seed) * zoom;
        }
        if night {
            offset += vibe.drift(d.seed) * zoom;
        }
        // Still nodes are left untouched, so a quiet graph is not re-sent
        // to the renderer every frame.
        if d.offset != offset {
            d.offset = offset;
        }
    }
}

/// The canvas is black by day and very nearly black by night: enough of a
/// tint to say the light has changed, not enough to wash out the glow.
fn follow_clear_color(vibe: Res<Vibe>, mut clear: ResMut<ClearColor>) {
    if clear.0 != vibe.palette.background {
        clear.0 = vibe.palette.background;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_working_day_is_untouched() {
        for hour in [9.0, 12.0, 14.5, 16.0] {
            assert_eq!(night_at_hour(hour), 0.0, "{hour} should be plain daylight");
        }
    }

    #[test]
    fn night_deepens_through_the_evening_and_lifts_by_morning() {
        let evening = [17.0, 18.5, 20.0, 22.0, 23.5];
        for pair in evening.windows(2) {
            assert!(
                night_at_hour(pair[1]) > night_at_hour(pair[0]),
                "{pair:?} should be getting darker"
            );
        }
        assert!(night_at_hour(2.5) > 0.99);
        let morning = [3.0, 5.0, 6.5, 7.5, 8.5];
        for pair in morning.windows(2) {
            assert!(
                night_at_hour(pair[1]) < night_at_hour(pair[0]),
                "{pair:?} should be getting lighter"
            );
        }
    }

    #[test]
    fn the_curve_wraps_and_stays_in_range() {
        for i in 0..=2400 {
            let h = i as f32 / 100.0;
            let n = night_at_hour(h);
            assert!((0.0..=1.0).contains(&n), "{h}h gave {n}");
        }
        assert!((night_at_hour(24.0) - night_at_hour(0.0)).abs() < 1e-5);
        assert_eq!(night_at_hour(25.0), night_at_hour(1.0));
        assert_eq!(night_at_hour(-1.0), night_at_hour(23.0));
    }

    #[test]
    fn the_dial_cycles_and_sticks() {
        let mut vibe = Vibe::new(VibeMode::Auto);
        assert!(!vibe.touched);
        // a workspace with a pinned look is adopted while the dial is untouched
        vibe.adopt("night");
        assert_eq!(vibe.mode, VibeMode::Night);
        for expected in [
            VibeMode::Auto,
            VibeMode::Day,
            VibeMode::Night,
            VibeMode::Auto,
        ] {
            vibe.set_mode(vibe.mode.next());
            assert_eq!(vibe.mode, expected);
        }
        // but once the user has chosen, opening another workspace leaves it
        vibe.adopt("day");
        assert_eq!(vibe.mode, VibeMode::Auto);
    }

    #[test]
    fn held_modes_ignore_the_clock() {
        let mut offset = None;
        assert_eq!(reading(VibeMode::Day, &mut offset).0, 0.0);
        assert_eq!(reading(VibeMode::Night, &mut offset).0, 1.0);
    }

    #[test]
    fn drift_stays_under_a_node() {
        let mut vibe = Vibe::new(VibeMode::Night);
        vibe.night = 1.0;
        for step in 0..4000 {
            vibe.clock = step as f32 * 0.05;
            for seed in [0u32, 7, 913_377, u32::MAX] {
                assert!(vibe.drift(seed).length() <= DRIFT_PIXELS + 1e-3);
            }
        }
    }

    #[test]
    fn floating_keeps_its_distance_and_moves() {
        let mut vibe = Vibe::new(VibeMode::Day);
        for seed in [0u32, 7, 913_377, u32::MAX] {
            vibe.clock = 0.0;
            let start = vibe.float(seed);
            vibe.clock = 3.0;
            let later = vibe.float(seed);
            assert!(start.distance(later) > 0.5, "seed {seed} stood still");
            for step in 0..2000 {
                vibe.clock = step as f32 * 0.05;
                let r = vibe.float(seed).length();
                assert!((0.4 * FLOAT_PIXELS - 1e-3..=FLOAT_PIXELS + 1e-3).contains(&r));
            }
        }
    }

    #[test]
    fn daylight_is_perfectly_still() {
        let mut vibe = Vibe::new(VibeMode::Day);
        vibe.night = 0.0;
        vibe.clock = 123.4;
        assert_eq!(vibe.drift(42), Vec2::ZERO);
    }
}
