//! Assemble a standalone SVG document from an [`IconData`].

use icondata_core::Icon;
use std::fmt::Write;

/// Build an SVG document for `icon` rendered at `px` × `px` in `color`
/// (any CSS colour). `currentColor` in the icon data is replaced by `color`;
/// icons without any fill or stroke (e.g. Simple Icons) are filled with it.
pub fn to_svg(icon: Icon, px: u32, color: &str) -> String {
    let view_box = icon.view_box.unwrap_or("0 0 24 24");
    let mut out = String::with_capacity(icon.data.len() + 256);
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view_box}" width="{px}" height="{px}""#
    );
    let fill = match (icon.fill, icon.stroke) {
        (Some(f), _) => f,
        (None, Some(_)) => "none",
        (None, None) => "currentColor",
    };
    let _ = write!(out, r#" fill="{}""#, swap(fill, color));
    if let Some(s) = icon.stroke {
        let _ = write!(out, r#" stroke="{}""#, swap(s, color));
    }
    if let Some(v) = icon.stroke_width {
        let _ = write!(out, r#" stroke-width="{v}""#);
    }
    if let Some(v) = icon.stroke_linecap {
        let _ = write!(out, r#" stroke-linecap="{v}""#);
    }
    if let Some(v) = icon.stroke_linejoin {
        let _ = write!(out, r#" stroke-linejoin="{v}""#);
    }
    if let Some(v) = icon.style {
        let _ = write!(out, r#" style="{v}""#);
    }
    out.push('>');
    out.push_str(&icon.data.replace("currentColor", color));
    out.push_str("</svg>");
    out
}

fn swap<'a>(value: &'a str, color: &'a str) -> &'a str {
    if value == "currentColor" { color } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lucide_is_stroked_and_simple_icons_are_filled() {
        let folder = to_svg(icondata_lu::LuFolder, 32, "#fff");
        assert!(folder.contains(r#"width="32""#));
        assert!(folder.contains(r#"fill="none""#));
        assert!(folder.contains(r##"stroke="#fff""##));
        assert!(folder.contains(r#"stroke-width="2""#));
        assert!(!folder.contains("currentColor"));

        let rust = to_svg(icondata_si::SiRust, 64, "white");
        assert!(rust.contains(r#"fill="white""#));
        assert!(!rust.contains("stroke="));
        assert!(!rust.contains("currentColor"));

        let repo = to_svg(icondata_vs::VsRepo, 16, "red");
        assert!(repo.contains(r#"viewBox="0 0 16 16""#));
        assert!(repo.contains(r#"fill="red""#));
        assert!(!repo.contains("currentColor"));
    }
}
