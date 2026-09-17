//! Mycelium palette: the daylit one the app wears at noon, the
//! bioluminescent one it becomes after dark, and the crossfade between them.
//!
//! Everything here is a pure function of one number — `night`, 0 in broad
//! daylight and 1 in the small hours — which [`crate::circadian`] reads off
//! the wall clock. At `night = 0` every value below is exactly the colour the
//! app has always used, so daylight is never a new look, only the absence of
//! the other one.

use bevy::prelude::*;

/// The canvas behind the graph: pure black, so nodes and edges carry the
/// colour on their own.
pub const BACKGROUND: &str = "#000000";
pub const PANEL: &str = "#131a16";
pub const TEXT: &str = "#d9d2c5";
pub const ACCENT: &str = "#9fd18f";
pub const DIM: &str = "#4c6b5a";
pub const SELECTION: &str = "#f5c542";
pub const HOVER: &str = "#1c2a22";
/// The icon sitting on a node's disc, drawn as a hole punched in it.
pub const ICON_INK: &str = "#0d1210";
/// Something the user should look at before acting: an index error, a spore
/// asking for more than the declarative tier.
pub const WARNING: &str = "#e6785a";
pub const FALLBACK_NODE: &str = "#9aa0a6";

/// After dark the same palette, lit from within: cooler, wetter, and pulled
/// towards the green-cyan that fungi actually glow in.
const NIGHT_BACKGROUND: &str = "#02070a";
const NIGHT_PANEL: &str = "#0a1a1d";
const NIGHT_TEXT: &str = "#cbe7df";
const NIGHT_ACCENT: &str = "#57efb4";
const NIGHT_DIM: &str = "#2c6b61";
const NIGHT_SELECTION: &str = "#ffd86b";
const NIGHT_HOVER: &str = "#13302a";
const NIGHT_ICON_INK: &str = "#02100e";
const NIGHT_WARNING: &str = "#ff9270";

/// Edge kinds, each with the colour it has by day and the one it glows in by
/// night. Order is only lookup order; the last row catches unknown kinds.
const EDGES: &[(&str, &str, &str)] = &[
    ("CONTAINS", "#4c6b5a", "#2f7f6d"),
    ("IMPORTS", "#9fd18f", "#6df2b5"),
    ("RE_EXPORTS", "#7fd1c7", "#5ff0e6"),
    ("REFERENCES", "#7e8fa5", "#7fb2f0"),
    ("DEPENDS_ON", "#6f7d99", "#8f9cf0"),
    ("ANNOTATES", "#e8c170", "#ffd98a"),
    ("RELATES_TO", "#a58cd6", "#c39bff"),
    ("", "#8a948f", "#79c9b4"),
];

pub fn hex(s: &str) -> Color {
    Srgba::hex(s.trim_start_matches('#'))
        .map(Color::from)
        .unwrap_or_else(|_| Color::srgb(0.6, 0.63, 0.65))
}

pub fn hex_or(s: &str, fallback: &str) -> Color {
    Srgba::hex(s.trim_start_matches('#'))
        .map(Color::from)
        .unwrap_or_else(|_| hex(fallback))
}

/// Straight-line blend in sRGB. The two ends of every pair below are close
/// enough in luminance that the cheap mix reads as clean as a perceptual one.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b, t) = (a.to_srgba(), b.to_srgba(), t.clamp(0.0, 1.0));
    Color::srgba(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        a.alpha + (b.alpha - a.alpha) * t,
    )
}

/// Every colour the chrome is drawn in, at one point in the day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub night: f32,
    pub background: Color,
    pub panel: Color,
    pub text: Color,
    pub accent: Color,
    pub dim: Color,
    pub selection: Color,
    pub hover: Color,
    pub icon_ink: Color,
    pub warning: Color,
    /// Edge colours in the order of [`EDGES`], resolved once so that drawing
    /// a few thousand hyphae does not re-parse a few thousand hex strings.
    edges: [Color; EDGES.len()],
}

impl Palette {
    pub fn at(night: f32) -> Self {
        let night = night.clamp(0.0, 1.0);
        let at = |day: &str, dark: &str| mix(hex(day), hex(dark), night);
        let mut edges = [Color::WHITE; EDGES.len()];
        for (slot, (_, day, dark)) in edges.iter_mut().zip(EDGES) {
            *slot = at(day, dark);
        }
        Palette {
            night,
            background: at(BACKGROUND, NIGHT_BACKGROUND),
            panel: at(PANEL, NIGHT_PANEL),
            text: at(TEXT, NIGHT_TEXT),
            accent: at(ACCENT, NIGHT_ACCENT),
            dim: at(DIM, NIGHT_DIM),
            selection: at(SELECTION, NIGHT_SELECTION),
            hover: at(HOVER, NIGHT_HOVER),
            icon_ink: at(ICON_INK, NIGHT_ICON_INK),
            warning: at(WARNING, NIGHT_WARNING),
            edges,
        }
    }

    pub fn edge(&self, kind: &str) -> Color {
        let i = EDGES
            .iter()
            .position(|(k, ..)| *k == kind)
            .unwrap_or(EDGES.len() - 1);
        self.edges[i]
    }

    /// A node's own colour, after dark: richer and brighter, as if it were
    /// lit from inside. No hue is moved. An earlier version leaned every hue
    /// towards the green of foxfire and it read beautifully right up until
    /// two kinds five degrees apart — a file and a repo — became the same
    /// colour. The glow is what makes the app look fungal; the discs are what
    /// makes it readable, and they keep their own colours for it.
    pub fn bioluminesce(&self, c: Color) -> Color {
        if self.night < 0.01 {
            return c;
        }
        let h = Hsla::from(c);
        // towards full, never past it, so the order of two colours survives
        let lift = |v: f32, by: f32| v + by * (1.0 - v) * self.night;
        Color::from(Hsla::new(
            h.hue,
            lift(h.saturation, 0.20),
            lift(h.lightness, 0.10),
            h.alpha,
        ))
    }

    /// What a node's halo is lit in: mostly foxfire, with enough of the node's
    /// own colour left in it to tell a cluster of one kind from another.
    pub fn glow(&self, c: Color) -> Color {
        mix(self.bioluminesce(c), self.accent, 0.5 * self.night)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette::at(0.0)
    }
}

pub fn egui_color(c: Color) -> bevy_egui::egui::Color32 {
    let s = c.to_srgba();
    bevy_egui::egui::Color32::from_rgba_unmultiplied(
        (s.red * 255.0) as u8,
        (s.green * 255.0) as u8,
        (s.blue * 255.0) as u8,
        (s.alpha * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daylight_is_the_palette_the_app_already_had() {
        let day = Palette::at(0.0);
        assert_eq!(day.panel, hex(PANEL));
        assert_eq!(day.accent, hex(ACCENT));
        assert_eq!(day.edge("IMPORTS"), hex("#9fd18f"));
        // and nothing glows in it
        let c = hex("#8fae6b");
        assert_eq!(day.bioluminesce(c), c);
    }

    #[test]
    fn unknown_edge_kinds_fall_back() {
        let p = Palette::at(1.0);
        assert_eq!(p.edge("NOT_A_KIND"), p.edge(""));
    }

    #[test]
    fn night_lifts_colour_without_collapsing_it() {
        let night = Palette::at(1.0);
        // the builtin node palette
        let kinds = ["#8fae6b", "#d9d2c5", "#e0a458", "#7d8aa5", "#a58cd6"];
        let day: Vec<Color> = kinds.iter().map(|k| hex(k)).collect();
        let lit: Vec<Color> = day.iter().map(|c| night.bioluminesce(*c)).collect();
        for (before, after) in day.iter().zip(&lit) {
            let (before, after) = (Hsla::from(*before), Hsla::from(*after));
            assert!(after.saturation >= before.saturation);
            assert!(after.lightness >= before.lightness);
        }
        // and the kinds are still as easy to tell apart as they were at noon
        for i in 0..kinds.len() {
            for j in i + 1..kinds.len() {
                let (was, now) = (apart(day[i], day[j]), apart(lit[i], lit[j]));
                assert!(
                    now > was * 0.7,
                    "{} and {} were {was:.3} apart and are now {now:.3}",
                    kinds[i],
                    kinds[j]
                );
            }
        }
    }

    /// Plain sRGB distance: rough, but enough to catch two kinds turning into
    /// the same colour.
    fn apart(a: Color, b: Color) -> f32 {
        let (a, b) = (a.to_srgba(), b.to_srgba());
        Vec3::new(a.red - b.red, a.green - b.green, a.blue - b.blue).length()
    }
}
