//! A geodesy backend for [`energyml_domain::reproject::Reprojector`].
//!
//! One backend, everywhere, wasm included. It is [`proj4rs`] for the maths and
//! [`crs_definitions`] for the EPSG table, plus the one rule that makes the
//! pair trustworthy: **a definition that says nothing about its datum is
//! refused, not applied.**
//!
//! ```no_run
//! use energyml_domain::reproject::{self, Wgs84};
//! use energyml_proj::Proj4;
//! # fn run(crs: &energyml_domain::crs::Crs, point: [f64; 3]) -> energyml_domain::Result<()> {
//! let lonlat = reproject::to_epsg(crs, point, Wgs84::EPSG, &Proj4::new())?;
//! # Ok(())
//! # }
//! ```
//!
//! # Why the table matters more than the library
//!
//! Reprojecting has two halves, and only one of them is arithmetic.
//!
//! The **projection** - UTM, Lambert, stereographic - is pure trigonometry.
//! Give it an ellipsoid and a formula and it needs nothing else. proj4rs does
//! this as well as PROJ does.
//!
//! The **datum** is not arithmetic, it is *data*. EPSG:23031 is ED50 / UTM 31N,
//! and the ED50 ellipsoid is not centred where WGS 84's is - about 178 m away
//! in space. Crossing from one to the other needs seven measured numbers:
//!
//! ```text
//! +towgs84=-87,-98,-121,0,0,0,0
//! ```
//!
//! Those numbers have to come from somewhere. `pyproj` ships the whole EPSG
//! database and never makes you think about it. A Rust crate does not, and
//! **`crs-definitions` 0.4.0 - the version proj4rs itself pins - carries none
//! of them.** Of nine systems checked it documents a datum for exactly one,
//! EPSG:32631, which is already on WGS 84 and needs no shift. 0.5.0 carries all
//! nine. This crate depends on 0.5.0 directly for that reason.
//!
//! Measured on EPSG:23031 at `500000 E, 4650000 N`, the corpus CRS:
//!
//! | source definition | longitude | latitude |
//! |---|---|---|
//! | the 0.4.0 string, no datum | 3.00000000 | 42.00127347 |
//! | the 0.5.0 string, with `+towgs84` | 2.99887374 | 42.00020043 |
//!
//! **151 m apart** - 93 m east, 119 m north. Nothing errors, and every
//! coordinate looks reasonable.
//!
//! # How close this is to PROJ
//!
//! Close enough to stop worrying about the engine. Measured against PROJ 9.5.1
//! through pyproj 3.7.2, same proj4 string on both sides, one point per system
//! (the table is `tests/conformance.rs`, and it runs on every build):
//!
//! | EPSG | | EPSG | |
//! |---|---|---|---|
//! | 23031 ED50 / UTM 31N | 0.0014 mm | 28992 Amersfoort / RD New | 0.0050 mm |
//! | 32631 WGS 84 / UTM 31N | 0.0052 mm | 21781 CH1903 / LV03 | 0.0039 mm |
//! | 2154 RGF93 / Lambert-93 | 0.1061 mm | 25831 ETRS89 / UTM 31N | 0.0998 mm |
//! | 27700 OSGB36 / Nat. Grid | 3.0084 mm | 26910 NAD83 / UTM 10N | 0.0039 mm |
//! | 31370 Lambert 72 | 0.0020 mm | | |
//!
//! Millimetres. The 151 m was never the engine.
//!
//! # What this backend is *not*
//!
//! A proj4 string carries **one** Helmert transformation for a whole datum.
//! The EPSG registry usually knows several for a given area, and sometimes a
//! correction grid (NTv2, NADCON) that is better than any of them. Comparing
//! the proj4 string against PROJ's own best *locally available* path:
//!
//! | system | difference | what EPSG says its own transformation is worth |
//! |---|---|---|
//! | EPSG:23031 ED50 | **3.7 m** | ED50 → WGS 84 (1) is ±10 m, (2) is ±6 m |
//! | EPSG:28992 Amersfoort | 5 mm | ±1 m |
//! | the other seven | 0 | |
//!
//! Read that second column before reading the first. The disagreement is
//! **smaller than the accuracy the registry claims for the transformation
//! itself** - a few metres, on a conversion nobody says is good to better than
//! six. That is the accuracy this backend buys, and for placing a reservoir
//! model on a map it is ample.
//!
//! Two of the nine also have a *better* transformation that PROJ could not use
//! here either, because the grid was not installed: EPSG:23031 wants
//! `es_ign_SPED2ETV2`, EPSG:27700 wants `uk_os_OSTN15_NTv2_OSGBtoETRS`. Their
//! zeroes in the table above mean "PROJ fell back to the same Helmert", not
//! "there is nothing better".
//!
//! For survey work none of this is good enough, and the answer there is
//! [`Proj4::with_definition`] - supply the transformation your survey specifies
//! and this crate uses it verbatim.
//!
//! Grid-based transformations are **refused by name** rather than approximated:
//! proj4rs's nadgrid support is experimental and the grids are megabytes of
//! data this crate does not ship.
//!
//! # The third coordinate is horizontal-only, on purpose
//!
//! A Helmert transformation moves the whole earth-centred frame, so it changes
//! **ellipsoidal height** as well as longitude and latitude. Handed the corpus
//! point at 1234.5 m depth, proj4rs faithfully returns it 60.8 m higher:
//!
//! ```text
//! ED50 → WGS 84, z in    0.0 m  →  z out    60.8 m
//!                z in -1234.5 m  →  z out -1173.7 m
//! ```
//!
//! That is correct geodesy and wrong for EnergyML. The Z of a representation is
//! a depth or an elevation against a **vertical CRS** - mean sea level, a
//! seabed, a rig floor - which is a separate object with its own EPSG code, and
//! which `to_epsg` is not given. Letting the horizontal datum shift it would
//! quietly mix two vertical references and move every horizon 60 m.
//!
//! So this backend converts the horizontal pair with the height taken as zero
//! and **returns the Z exactly as it came in**. The cost of using zero rather
//! than the true height is measured, not assumed: **2.9 cm** horizontally over
//! 1234.5 m of depth. Converting the vertical is a separate job, and when it is
//! done it belongs behind its own seam.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use energyml_domain::crs::Point;
use energyml_domain::error::{Error, Result};
use energyml_domain::reproject::Reprojector;

/// What a proj4 definition says about its datum.
///
/// The distinction the whole crate turns on. A string in the [`Silent`] state
/// projects perfectly well and lands in the wrong place.
///
/// [`Silent`]: DatumState::Silent
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatumState {
    /// `+datum=`, `+towgs84=` or `+nadgrids=@null` - the shift is known,
    /// including when it is known to be zero.
    Stated,
    /// `+nadgrids=` naming a real grid file. Known, and not available here.
    NeedsGrid,
    /// Nothing at all. The ellipsoid will be taken as already aligned with
    /// WGS 84, which for anything older than GPS it is not.
    Silent,
}

/// Read a proj4 string's datum state.
///
/// Public because it is worth asking of a definition you are about to supply,
/// before it is used on ten thousand points.
///
/// ```
/// use energyml_proj::{datum_state, DatumState};
///
/// assert_eq!(datum_state("+proj=utm +zone=31 +ellps=intl"), DatumState::Silent);
/// assert_eq!(
///     datum_state("+proj=utm +zone=31 +ellps=intl +towgs84=-87,-98,-121,0,0,0,0"),
///     DatumState::Stated,
/// );
/// ```
pub fn datum_state(proj4: &str) -> DatumState {
    let mut state = DatumState::Silent;
    for token in proj4.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match key {
            "+datum" | "+towgs84" => state = DatumState::Stated,
            // `@null` is proj4 for "this system is already on WGS 84" - it is
            // how Web Mercator is written, and it is a statement, not silence.
            "+nadgrids" if value == "@null" => state = DatumState::Stated,
            "+nadgrids" => return DatumState::NeedsGrid,
            _ => {}
        }
    }
    state
}

/// Reprojection between EPSG systems, in pure Rust.
///
/// Built with [`Proj4::new`]; definitions come from `crs-definitions` 0.5.0
/// unless you supply your own with [`Proj4::with_definition`] or
/// [`Proj4::with_definitions_json`], which take precedence - the same
/// arrangement as every other registry in this workspace, so a built-in can
/// always be replaced without forking anything.
#[derive(Debug, Clone)]
pub struct Proj4 {
    definitions: BTreeMap<u64, String>,
    require_datum: bool,
}

impl Default for Proj4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Proj4 {
    /// The built-in table, with the datum check on.
    pub fn new() -> Self {
        Self {
            definitions: BTreeMap::new(),
            require_datum: true,
        }
    }

    /// Supply the proj4 string for one EPSG code, replacing the built-in.
    ///
    /// This is the escape hatch for accuracy: a transformation from a national
    /// registry, or the one your survey contract names, used verbatim.
    ///
    /// ```
    /// use energyml_proj::Proj4;
    ///
    /// // The NTv2-free Helmert the Dutch registry publishes, rather than the
    /// // generic one.
    /// let backend = Proj4::new().with_definition(
    ///     28992,
    ///     "+proj=sterea +lat_0=52.15616055555555 +lon_0=5.38763888888889 \
    ///      +k=0.9999079 +x_0=155000 +y_0=463000 +ellps=bessel \
    ///      +towgs84=565.4171,50.3319,465.5524,-0.398957,0.343988,-1.87740,4.0725 \
    ///      +units=m +no_defs",
    /// );
    /// # let _ = backend;
    /// ```
    #[must_use]
    pub fn with_definition(mut self, epsg: u64, proj4: impl Into<String>) -> Self {
        self.definitions.insert(epsg, proj4.into());
        self
    }

    /// Supply several, as a JSON object of `"code": "proj4 string"`.
    ///
    /// So that a deployment can carry its own systems in a file next to the
    /// metadata spec, rather than in a rebuild.
    ///
    /// ```
    /// use energyml_proj::Proj4;
    ///
    /// let backend = Proj4::new().with_definitions_json(
    ///     r#"{ "23031": "+proj=utm +zone=31 +ellps=intl +towgs84=-84,-107,-120,0,0,0,0 +units=m" }"#,
    /// ).unwrap();
    /// # let _ = backend;
    /// ```
    pub fn with_definitions_json(mut self, text: &str) -> Result<Self> {
        for (code, proj4) in parse_definitions(text)? {
            self.definitions.insert(code, proj4);
        }
        Ok(self)
    }

    /// Turn off the datum check.
    ///
    /// Only sound when every system in play is already on WGS 84 - a package
    /// written entirely in EPSG:326xx, 4326 or 3857. Anywhere else this is the
    /// switch that puts the model a hundred metres from where it belongs,
    /// which is why it has to be asked for.
    #[must_use]
    pub fn allowing_a_silent_datum(mut self) -> Self {
        self.require_datum = false;
        self
    }

    /// The proj4 string this backend would use for a code.
    pub fn definition(&self, epsg: u64) -> Result<&str> {
        if let Some(own) = self.definitions.get(&epsg) {
            return Ok(own);
        }
        let code = u16::try_from(epsg).ok();
        code.and_then(crs_definitions::from_code)
            .map(|def| def.proj4)
            .ok_or_else(|| {
                Error::unsupported(
                    format!("EPSG:{epsg}"),
                    "not in the built-in table; supply it with `Proj4::with_definition`",
                )
            })
    }

    /// The definition, checked.
    fn checked(&self, epsg: u64) -> Result<&str> {
        let proj4 = self.definition(epsg)?;
        match datum_state(proj4) {
            DatumState::Stated => Ok(proj4),
            DatumState::NeedsGrid => Err(Error::unsupported(
                format!("EPSG:{epsg}"),
                format!(
                    "its definition needs a correction grid (`{}`), which this backend does not \
                     carry; supply a Helmert form instead (`--crs-definitions`, or \
                     `Proj4::with_definition`)",
                    grid_named(proj4).unwrap_or("+nadgrids"),
                ),
            )),
            // The failure this crate exists to make loud. Refusing costs the
            // caller a line of code; not refusing costs them a model in the
            // wrong place, with every coordinate looking plausible.
            DatumState::Silent if self.require_datum => Err(Error::unsupported(
                format!("EPSG:{epsg}"),
                format!(
                    "its definition states no datum (`{proj4}`), so the shift to WGS 84 would be \
                     silently skipped and the model would land some hundred metres out - supply \
                     one (`--crs-definitions`, or `Proj4::with_definition`), or say the risk is \
                     accepted (`--allow-silent-datum`, or `allowing_a_silent_datum()`) if every \
                     system here is already on WGS 84"
                ),
            )),
            DatumState::Silent => Ok(proj4),
        }
    }

    fn build(&self, epsg: u64) -> Result<proj4rs::Proj> {
        let proj4 = self.checked(epsg)?;
        proj4rs::Proj::from_proj_string(proj4)
            .map_err(|e| Error::malformed(format!("EPSG:{epsg}"), format!("proj4rs rejected `{proj4}`: {e}")))
    }
}

/// Which grid a definition asks for, for the error message.
fn grid_named(proj4: &str) -> Option<&str> {
    proj4
        .split_whitespace()
        .find_map(|token| token.strip_prefix("+nadgrids="))
}

/// `{ "23031": "+proj=..." }`, by hand.
///
/// A dependency on `serde_json` for one flat object of strings would be the
/// only reason this crate needed one, and this parser is small enough to read
/// in full.
fn parse_definitions(text: &str) -> Result<Vec<(u64, String)>> {
    let bad = |reason: &str| Error::malformed("CRS definitions JSON", reason.to_string());
    let body = text.trim();
    let body = body
        .strip_prefix('{')
        .and_then(|b| b.strip_suffix('}'))
        .ok_or_else(|| bad("expected a JSON object of \"epsg code\": \"proj4 string\""))?;

    let mut out = Vec::new();
    let mut rest = body.trim();
    while !rest.is_empty() {
        let (key, after) = json_string(rest).ok_or_else(|| bad("expected a quoted EPSG code"))?;
        let after = after
            .trim_start()
            .strip_prefix(':')
            .ok_or_else(|| bad("expected `:` after an EPSG code"))?;
        let (value, after) =
            json_string(after.trim_start()).ok_or_else(|| bad("expected a quoted proj4 string"))?;
        let code: u64 = key
            .trim()
            .parse()
            .map_err(|_| bad(&format!("`{key}` is not an EPSG code")))?;
        out.push((code, value));
        rest = after.trim_start();
        if let Some(next) = rest.strip_prefix(',') {
            rest = next.trim_start();
        } else if !rest.is_empty() {
            return Err(bad("expected `,` between entries"));
        }
    }
    Ok(out)
}

/// One JSON string literal, and what follows it.
fn json_string(text: &str) -> Option<(String, &str)> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((out, &text[index + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

impl Reprojector for Proj4 {
    fn reproject(&self, point: Point, from: u64, to: u64) -> Result<Point> {
        Ok(self.reproject_all(&[point], from, to)?.remove(0))
    }

    /// Builds each projection **once** for the whole array.
    ///
    /// Worth the override: parsing a proj4 string per point turns a surface of
    /// ten thousand nodes into ten thousand string parses.
    fn reproject_all(&self, points: &[Point], from: u64, to: u64) -> Result<Vec<Point>> {
        if from == to {
            return Ok(points.to_vec());
        }
        let source = self.build(from)?;
        let target = self.build(to)?;
        // proj4rs speaks radians on the geographic side, both ways.
        let scale_in = if source.is_latlong() {
            f64::to_radians
        } else {
            std::convert::identity as fn(f64) -> f64
        };
        let scale_out = if target.is_latlong() {
            f64::to_degrees
        } else {
            std::convert::identity as fn(f64) -> f64
        };

        points
            .iter()
            .map(|point| {
                // The third coordinate goes in as **zero** and comes back
                // untouched. See `elevation` below: a Helmert is a rotation of
                // the whole earth-centred frame, so it changes ellipsoidal
                // height too - and the Z of an EnergyML representation is not
                // an ellipsoidal height. It is a depth or an elevation against
                // a vertical CRS that this call knows nothing about.
                let mut work = (scale_in(point[0]), scale_in(point[1]), 0.0);
                proj4rs::transform::transform(&source, &target, &mut work).map_err(|e| {
                    Error::malformed(
                        format!("EPSG:{from} to EPSG:{to}"),
                        format!("proj4rs refused ({}, {}): {e}", point[0], point[1]),
                    )
                })?;
                Ok([scale_out(work.0), scale_out(work.1), point[2]])
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_with_no_datum_is_refused_rather_than_applied() {
        // 4267 NAD27: `crs-definitions` gives it `+datum=NAD27`, so build one
        // that is genuinely silent to exercise the check.
        let backend = Proj4::new().with_definition(99999, "+proj=utm +zone=31 +ellps=intl +units=m");
        let error = backend
            .reproject([500000.0, 4650000.0, 0.0], 99999, 4326)
            .unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("states no datum"), "{message}");
        assert!(message.contains("99999"), "{message}");
    }

    #[test]
    fn the_silent_datum_check_can_be_turned_off_deliberately() {
        let backend = Proj4::new()
            .with_definition(99999, "+proj=utm +zone=31 +ellps=intl +units=m")
            .allowing_a_silent_datum();
        let out = backend
            .reproject([500000.0, 4650000.0, 0.0], 99999, 4326)
            .unwrap();
        // The no-shift answer: exactly on the central meridian.
        assert!((out[0] - 3.0).abs() < 1e-9, "{out:?}");
    }

    #[test]
    fn a_grid_based_definition_names_the_grid_it_would_need() {
        let backend = Proj4::new().with_definition(
            99998,
            "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 \
             +ellps=airy +nadgrids=OSTN15_NTv2_OSGBtoETRS.gsb +units=m",
        );
        let error = backend
            .reproject([400000.0, 300000.0, 0.0], 99998, 4326)
            .unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("correction grid"), "{message}");
        assert!(message.contains("OSTN15"), "{message}");
    }

    #[test]
    fn web_mercators_at_null_counts_as_a_statement() {
        // `+nadgrids=@null` is how proj4 spells "already on WGS 84". Reading it
        // as silence would refuse EPSG:3857, which is the one system every web
        // map needs.
        assert_eq!(datum_state(crs_definitions::EPSG_3857.proj4), DatumState::Stated);
    }

    #[test]
    fn an_unknown_code_says_so_rather_than_falling_back() {
        let error = Proj4::new().definition(999_999).unwrap_err();
        assert!(format!("{error}").contains("999999"), "{error}");
    }

    #[test]
    fn the_same_system_is_a_copy_and_needs_no_definition() {
        // Not even a lookup: a caller asking 99997 → 99997 gets its points back.
        let out = Proj4::new()
            .reproject_all(&[[1.0, 2.0, 3.0]], 99997, 99997)
            .unwrap();
        assert_eq!(out, vec![[1.0, 2.0, 3.0]]);
    }

    #[test]
    fn definitions_can_be_supplied_as_json() {
        let backend = Proj4::new()
            .with_definitions_json(
                r#"{
                     "23031": "+proj=utm +zone=31 +ellps=intl +towgs84=-84,-107,-120,0,0,0,0 +units=m",
                     "32631": "+proj=utm +zone=31 +datum=WGS84 +units=m"
                   }"#,
            )
            .unwrap();
        assert!(backend.definition(23031).unwrap().contains("-84,-107,-120"));
        assert!(backend.definition(32631).unwrap().contains("WGS84"));
    }

    #[test]
    fn a_supplied_definition_beats_the_built_in_one() {
        let backend = Proj4::new().with_definition(23031, "+proj=utm +zone=31 +datum=WGS84 +units=m");
        assert!(backend.definition(23031).unwrap().contains("datum=WGS84"));
    }

    #[test]
    fn malformed_definition_json_is_an_error_naming_what_was_expected() {
        let error = Proj4::new().with_definitions_json(r#"["23031"]"#).unwrap_err();
        assert!(format!("{error}").contains("JSON object"), "{error}");
    }

    #[test]
    fn a_definition_proj4rs_cannot_parse_is_reported_with_the_string() {
        let backend = Proj4::new().with_definition(99996, "+proj=nonesuch +datum=WGS84");
        let error = backend.reproject([0.0, 0.0, 0.0], 99996, 4326).unwrap_err();
        assert!(format!("{error}").contains("nonesuch"), "{error}");
    }

    #[test]
    fn the_elevation_passes_through_untouched() {
        // Not a formality. proj4rs, given this point at depth, returns it
        // 60.8 m higher - the ellipsoidal-height half of the ED50 Helmert. A
        // RESQML Z is a depth against a vertical CRS, not an ellipsoidal
        // height, so that shift must not reach it.
        let out = Proj4::new()
            .reproject([500000.0, 4650000.0, -1234.5], 23031, 4326)
            .unwrap();
        assert_eq!(out[2], -1234.5);
    }

    #[test]
    fn taking_the_height_as_zero_costs_three_centimetres() {
        // The price of the decision above, measured rather than assumed: the
        // horizontal answer depends slightly on the height fed to the Helmert,
        // and this build always feeds zero.
        let backend = Proj4::new();
        let flat = backend
            .reproject([500000.0, 4650000.0, 0.0], 23031, 4326)
            .unwrap();
        let deep = backend
            .reproject([500000.0, 4650000.0, -1234.5], 23031, 4326)
            .unwrap();
        assert_eq!(flat[0], deep[0]);
        assert_eq!(flat[1], deep[1]);

        // What proj4rs would have said with the true height, in metres on the
        // ground. If this ever grows past a few centimetres the decision needs
        // revisiting, so the number is asserted and not merely written down.
        let source = backend.build(23031).unwrap();
        let target = backend.build(4326).unwrap();
        let mut with_height = (500000.0, 4650000.0, -1234.5);
        proj4rs::transform::transform(&source, &target, &mut with_height).unwrap();
        let metres_per_degree = 111_320.0;
        let east = (with_height.0.to_degrees() - flat[0]) * metres_per_degree * flat[1].to_radians().cos();
        let north = (with_height.1.to_degrees() - flat[1]) * metres_per_degree;
        let offset = east.hypot(north);
        assert!(offset < 0.05, "{offset} m");
        assert!(
            offset > 0.01,
            "{offset} m - suspiciously small, is the datum shift running?"
        );
    }

    #[test]
    fn degrees_go_in_as_well_as_out() {
        // 4326 → 23031 → 4326 must return the same point: it is the only test
        // that exercises the radian conversion on the *input* side.
        let backend = Proj4::new();
        let start = [2.998873735512756, 42.00020043060782, 0.0];
        let there = backend.reproject(start, 4326, 23031).unwrap();
        let back = backend.reproject(there, 23031, 4326).unwrap();

        // Not exact, and it should not be: the inverse of a Helmert is
        // iterative. Measured at ~1.2 mm on the ground, so a centimetre is a
        // tolerance with room and still far tighter than anything that would
        // hide a missing datum.
        let metres_per_degree = 111_320.0;
        let east = (back[0] - start[0]) * metres_per_degree * start[1].to_radians().cos();
        let north = (back[1] - start[1]) * metres_per_degree;
        let offset = east.hypot(north);
        assert!(offset < 0.01, "{offset} m round trip, from {back:?}");

        // And the forward leg really did land on the corpus easting/northing,
        // which is what proves the degrees were read as degrees.
        assert!((there[0] - 500000.0).abs() < 0.01, "{there:?}");
        assert!((there[1] - 4650000.0).abs() < 0.01, "{there:?}");
    }
}
