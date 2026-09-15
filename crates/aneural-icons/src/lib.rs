//! Curated [`icondata`](https://docs.rs/icondata) registry for Aneural node icons.
//!
//! Icons are compile-time statics in the `icondata_*` set crates, so this crate
//! exposes a curated name table (a typo is a compile error) plus the default
//! kind/extension → icon mapping the GUI and CLI share. The optional `raster`
//! feature rasterizes an icon to RGBA via `resvg` for use as a sprite.

mod defaults;
mod registry;
mod svg;

#[cfg(feature = "raster")]
mod raster;

pub use icondata_core::{Icon, IconData};

pub use defaults::{default_icon, ecosystem_icon};
pub use registry::{is_valid, lookup, names, ICONS};
pub use svg::to_svg;

#[cfg(feature = "raster")]
pub use raster::{rasterize, rasterize_named, Rgba};

/// Icon name used when a configured icon does not exist in the registry.
pub const FALLBACK_ICON: &str = "LuCircleDot";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown icon `{0}`")]
    UnknownIcon(String),
    #[error("invalid icon size {0}px")]
    InvalidSize(u32),
    #[error("svg parse error: {0}")]
    Svg(String),
    #[error("could not allocate a {0}x{0} pixmap")]
    Pixmap(u32),
}
