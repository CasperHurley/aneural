//! Rasterize icons to RGBA8 (white on transparent) with `resvg`.

use crate::{Error, to_svg};
use icondata_core::Icon;

/// Non-premultiplied RGBA8 pixels, row-major, `width * height * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Render `icon` white on a transparent `px` × `px` canvas. Tint at draw time.
pub fn rasterize(icon: Icon, px: u32) -> Result<Rgba, Error> {
    if px == 0 || px > 4096 {
        return Err(Error::InvalidSize(px));
    }
    let svg = to_svg(icon, px, "#ffffff");
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())
        .map_err(|e| Error::Svg(e.to_string()))?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px).ok_or(Error::Pixmap(px))?;
    // The document already carries width/height = px, so usvg scales the
    // viewBox into it; render with the identity transform.
    let size = tree.size();
    let transform =
        tiny_skia::Transform::from_scale(px as f32 / size.width(), px as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut data = Vec::with_capacity((px * px * 4) as usize);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Rgba {
        width: px,
        height: px,
        data,
    })
}

/// [`rasterize`] by registry name.
pub fn rasterize_named(name: &str, px: u32) -> Result<Rgba, Error> {
    let icon = crate::lookup(name).ok_or_else(|| Error::UnknownIcon(name.to_string()))?;
    rasterize(icon, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(img: &Rgba) -> (usize, usize) {
        let opaque = img.data.chunks(4).filter(|p| p[3] > 0).count();
        (opaque, img.data.len() / 4)
    }

    #[test]
    fn lucide_and_simple_icons_render_at_64() {
        for name in [
            "LuLeaf",
            "SiRust",
            "VsRepo",
            "BsFiletypeTsx",
            "AnFiletypeToml",
        ] {
            let img = rasterize_named(name, 64).unwrap();
            assert_eq!((img.width, img.height), (64, 64));
            assert_eq!(img.data.len(), 64 * 64 * 4);
            let (opaque, total) = coverage(&img);
            assert!(
                opaque > total / 50,
                "{name}: only {opaque}/{total} pixels drawn"
            );
            assert!(opaque < total, "{name}: canvas fully opaque");
            // white glyph: every drawn pixel is white
            assert!(
                img.data
                    .chunks(4)
                    .filter(|p| p[3] > 0)
                    .all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255),
                "{name}"
            );
        }
    }

    #[test]
    fn errors() {
        assert!(matches!(
            rasterize_named("Nope", 16),
            Err(Error::UnknownIcon(_))
        ));
        assert!(matches!(
            rasterize(icondata_lu::LuLeaf, 0),
            Err(Error::InvalidSize(0))
        ));
    }
}
