//! Mycelium-dark palette.

use bevy::prelude::*;

/// The canvas behind the graph: pure black, so nodes and edges carry the
/// colour on their own.
pub const BACKGROUND: &str = "#000000";
pub const PANEL: &str = "#131a16";
pub const TEXT: &str = "#d9d2c5";
pub const ACCENT: &str = "#9fd18f";
pub const DIM: &str = "#4c6b5a";
pub const SELECTION: &str = "#f5c542";
pub const FALLBACK_NODE: &str = "#9aa0a6";

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

pub fn edge_color(kind: &str) -> Color {
    match kind {
        "CONTAINS" => hex("#4c6b5a"),
        "IMPORTS" => hex("#9fd18f"),
        "RE_EXPORTS" => hex("#7fd1c7"),
        "REFERENCES" => hex("#7e8fa5"),
        "DEPENDS_ON" => hex("#6f7d99"),
        "ANNOTATES" => hex("#e8c170"),
        "RELATES_TO" => hex("#a58cd6"),
        _ => hex("#8a948f"),
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

pub fn egui_hex(s: &str) -> bevy_egui::egui::Color32 {
    egui_color(hex(s))
}
