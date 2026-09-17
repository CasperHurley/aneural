//! File-type icons Bootstrap does not ship, built from Bootstrap's own parts.
//!
//! `BsFiletype*` has no page for `.ts`, `.rs`, `.go` or `.toml`. Rather than
//! fall back to a brand mark, each icon here is a Bootstrap page outline plus
//! letters lifted verbatim from other `BsFiletype*` glyphs, moved into place
//! with a `translate`. Same strokes, same letterforms, same baseline — so a
//! Rust file sits beside a Python file looking like a sibling, not a logo.

#![allow(non_upper_case_globals)]

use icondata_core::IconData;

/// Page with a notch at the bottom right for a label ending before `x = 8`
/// (from `BsFiletypeJs`).
macro_rules! PAGE_SHORT {
    () => {
        "M14 4.5V14a2 2 0 0 1-2 2H8v-1h4a1 1 0 0 0 1-1V4.5h-2A1.5 1.5 0 0 1 9.5 3V1H4a1 1 0 0 0-1 1v9H2V2a2 2 0 0 1 2-2h5.5z"
    };
}

/// Page cut off above a label that runs the full width (from `BsFiletypeJson`).
macro_rules! PAGE_WIDE {
    () => {
        "M14 4.5V11h-1V4.5h-2A1.5 1.5 0 0 1 9.5 3V1H4a1 1 0 0 0-1 1v9H2V2a2 2 0 0 1 2-2h5.5z"
    };
}

// Letters, each at the position it has in its source icon.

/// `T` from `BsFiletypeTsx`, spanning x 0–3.06.
macro_rules! T {
    () => {
        "M1.928 15.931v-3.337h1.136v-.662H0v.662h1.134v3.337z"
    };
}
/// `S` from `BsFiletypeTsx`, spanning x 3.17–6.3.
macro_rules! S {
    () => {
        "M3.172 14.841a1.13 1.13 0 0 0 .401.823q.193.162.478.252.283.091.665.091.507 0 .858-.158.354-.158.54-.44a1.17 1.17 0 0 0 .187-.656q0-.336-.135-.56a1 1 0 0 0-.375-.357 2 2 0 0 0-.566-.21l-.62-.144a1 1 0 0 1-.405-.176.37.37 0 0 1-.144-.299q0-.234.185-.384.188-.152.513-.152.213 0 .369.068a.6.6 0 0 1 .246.181.56.56 0 0 1 .12.258h.75a1.1 1.1 0 0 0-.2-.566 1.2 1.2 0 0 0-.5-.41 1.8 1.8 0 0 0-.78-.152q-.438 0-.776.15-.336.149-.527.421-.19.273-.19.639 0 .302.122.524.124.223.352.367.228.143.54.213l.617.144q.311.073.463.193a.39.39 0 0 1 .152.326.5.5 0 0 1-.084.29.56.56 0 0 1-.255.193 1.1 1.1 0 0 1-.413.07q-.177 0-.32-.04a.8.8 0 0 1-.249-.115.58.58 0 0 1-.255-.384z"
    };
}
/// `R` from `BsFiletypeRb`, spanning x 0–3.08.
macro_rules! R {
    () => {
        "M0 11.85h1.597q.446 0 .758.158.315.155.478.44.167.284.167.668a1.18 1.18 0 0 1-.727 1.122l.803 1.611h-.885l-.7-1.491H.782v1.491H0zm.782.621v1.292h.689q.327 0 .518-.158.195-.159.194-.475 0-.32-.194-.489a.74.74 0 0 0-.507-.17z"
    };
}
/// `G` from `BsFiletypeGif`, spanning x 0–3.28.
macro_rules! G {
    () => {
        "M3.278 13.124a1.4 1.4 0 0 0-.14-.492 1.3 1.3 0 0 0-.314-.407 1.5 1.5 0 0 0-.48-.275 1.9 1.9 0 0 0-.636-.1q-.542 0-.926.229a1.5 1.5 0 0 0-.583.632 2.1 2.1 0 0 0-.199.95v.506q0 .408.105.745.105.336.32.58.213.243.533.377.323.132.753.132.402 0 .697-.111a1.29 1.29 0 0 0 .788-.77q.097-.261.097-.551v-.797H1.717v.589h.823v.255q0 .199-.09.363a.67.67 0 0 1-.273.264 1 1 0 0 1-.457.096.87.87 0 0 1-.519-.146.9.9 0 0 1-.305-.413 1.8 1.8 0 0 1-.096-.615v-.499q0-.547.234-.85.237-.3.665-.301a1 1 0 0 1 .3.044q.136.044.236.126a.7.7 0 0 1 .17.19.8.8 0 0 1 .097.25z"
    };
}
/// `O` from `BsFiletypeOtf`, spanning x 0–3.43.
macro_rules! O {
    () => {
        "M2.622 13.666v.522q0 .384-.117.641a.86.86 0 0 1-.322.387.9.9 0 0 1-.47.126.9.9 0 0 1-.47-.126.87.87 0 0 1-.32-.386 1.55 1.55 0 0 1-.117-.642v-.522q0-.386.117-.641a.87.87 0 0 1 .32-.387.87.87 0 0 1 .47-.129q.265 0 .47.13a.86.86 0 0 1 .322.386q.117.255.117.641M3.425 14.185v-.513q0-.565-.205-.972a1.46 1.46 0 0 0-.59-.63q-.38-.22-.916-.22-.534 0-.92.22a1.44 1.44 0 0 0-.589.627Q0 13.103 0 13.672v.513q0 .563.205.973.205.406.589.627.386.216.92.216.536 0 .917-.216a1.47 1.47 0 0 0 .589-.627q.204-.41.205-.973"
    };
}
/// `M` from `BsFiletypeMd`, spanning x 0–3.95.
macro_rules! M {
    () => {
        "M.706 13.189v2.66H0V11.85h.806l1.14 2.596h.026l1.14-2.596h.8v3.999h-.716v-2.66h-.038l-.946 2.159h-.516l-.952-2.16H.706Z"
    };
}
/// `L` from `BsFiletypeXls`, spanning x 3.77–6.25.
macro_rules! L {
    () => {
        "M6.254 15.257H4.557v-3.325h-.79v4h2.487z"
    };
}

macro_rules! drawn {
    ($(#[$doc:meta])* $name:ident, $page:ident, [$(($letter:ident, $dx:literal)),* $(,)?]) => {
        $(#[$doc])*
        pub static $name: &IconData = &IconData {
            style: None,
            x: None,
            y: None,
            width: Some("16"),
            height: Some("16"),
            view_box: Some("0 0 16 16"),
            stroke_linecap: None,
            stroke_linejoin: None,
            stroke_width: None,
            stroke: None,
            fill: Some("currentColor"),
            data: concat!(
                r#"<path fill-rule="evenodd" d=""#, $page!(), r#"" />"#,
                $(
                    r#"<path fill-rule="evenodd" transform="translate("#, $dx, r#" 0)" d=""#,
                    $letter!(), r#"" />"#,
                )*
            ),
        };
    };
}

drawn!(
    /// `.ts`: `BsFiletypeTsx` without the `x`, on the shorter label's page.
    AnFiletypeTs, PAGE_SHORT, [(T, "0"), (S, "0")]
);
drawn!(
    /// `.rs`
    AnFiletypeRs, PAGE_SHORT, [(R, "0"), (S, "0.2")]
);
drawn!(
    /// `.go`
    AnFiletypeGo, PAGE_SHORT, [(G, "0"), (O, "3.53")]
);
drawn!(
    /// `.toml`
    AnFiletypeToml, PAGE_WIDE, [(T, "1.2"), (O, "4.384"), (M, "8.109"), (L, "8.588")]
);
