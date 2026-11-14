//! The drawing module names no format crate and no other module of this crate,
//! so that it can be moved into a crate of its own without a rewrite.
//!
//! The sources are the repository's own files, frozen at compile time: nothing
//! is read at run time. A new file of `src/draw/` goes into `SOURCES` below.

const SOURCES: &[(&str, &str)] = &[
    ("draw/mod.rs", include_str!("../../src/draw/mod.rs")),
    ("draw/hit.rs", include_str!("../../src/draw/hit.rs")),
    ("draw/list.rs", include_str!("../../src/draw/list.rs")),
    ("draw/png.rs", include_str!("../../src/draw/png.rs")),
    ("draw/raster.rs", include_str!("../../src/draw/raster.rs")),
    ("draw/spread.rs", include_str!("../../src/draw/spread.rs")),
    ("draw/svg.rs", include_str!("../../src/draw/svg.rs")),
    ("draw/text.rs", include_str!("../../src/draw/text.rs")),
    ("draw/ticks.rs", include_str!("../../src/draw/ticks.rs")),
    ("draw/layered/mod.rs", include_str!("../../src/draw/layered/mod.rs")),
    ("draw/layered/cycles.rs", include_str!("../../src/draw/layered/cycles.rs")),
    ("draw/layered/layering.rs", include_str!("../../src/draw/layered/layering.rs")),
    ("draw/layered/ordering.rs", include_str!("../../src/draw/layered/ordering.rs")),
    ("draw/layered/place.rs", include_str!("../../src/draw/layered/place.rs")),
    ("draw/layered/route.rs", include_str!("../../src/draw/layered/route.rs")),
];

#[test]
fn the_draw_module_names_no_format_crate() {
    // Breaks if a source gains `use energyml_domain::color::Rgba;` or a path
    // such as `crate::layout`.
    let forbidden = ["energyml_core", "energyml_domain", "energyml_storage", "super::super"];
    for (name, source) in SOURCES {
        for word in forbidden {
            assert!(!source.contains(word), "{name} names `{word}`: the drawing module knows no format");
        }
        for (at, _) in source.match_indices("crate::") {
            let rest = &source[at + "crate::".len()..];
            assert!(
                rest.starts_with("draw"),
                "{name} reaches `crate::` outside `crate::draw`: {:?}",
                rest.chars().take(24).collect::<String>()
            );
        }
    }
}
