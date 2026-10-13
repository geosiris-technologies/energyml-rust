//! The CRS view, against every coordinate system in the reference packages.
//!
//! The geometry corpus test reads CRSs the way an application does — through
//! the representation that names one. This one goes at them directly, so a
//! coordinate system nothing points at is still read, and so the two RESQML
//! versions are compared against each other rather than merely both passing.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::crs::{self, CrsKind, VerticalDirection, VerticalQuantity};
use energyml_storage::EpcStore;

/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice — the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The public reference packages; see `fixtures.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

/// Every CRS object in the corpus reads, whichever version wrote it.
#[test]
fn every_coordinate_system_reads() {
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: no EPC in testdata/");
        return;
    }
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut with_epsg = 0usize;
    let mut time = 0usize;
    let mut downward = 0usize;
    let mut components = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        for summary in store.resources() {
            if !summary.qualified_type.contains("Crs") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let read = crs::read_with(object.as_reflect(), &store)
                .unwrap_or_else(|e| panic!("{}: {} — {e}", path.display(), summary.uuid));

            *kinds.entry(read.kind.to_string()).or_default() += 1;
            if read.is_component() {
                components += 1;
            }
            if read.projected.epsg().is_some() {
                with_epsg += 1;
            }
            if read.vertical_quantity == VerticalQuantity::Time {
                time += 1;
            }
            if read.vertical_direction == VerticalDirection::Down {
                downward += 1;
            }

            // Whatever it says, it must say something usable: finite numbers,
            // and a unit this build can act on if one is stated at all.
            assert!(read.areal_rotation.is_finite());
            assert!(read.x_offset.is_finite() && read.y_offset.is_finite() && read.z_offset.is_finite());
            for unit in [&read.horizontal_uom, &read.vertical_uom].into_iter().flatten() {
                assert!(
                    energyml_domain::units::Dimension::of(unit).is_some(),
                    "{}: {} states `{unit}`, which this build cannot convert",
                    path.display(),
                    summary.uuid
                );
            }
            // A system that names nothing is `Absent`; one that says it does
            // not know is `Unknown`. Conflating them would hide whether the
            // file was silent or explicit.
            assert!(read.projected != CrsKind::Epsg(0));

            // **A time axis is measured in time.** The form of this assertion
            // is the spec's, not the corpus's: 2.0.1 puts the axis unit in
            // `TimeUom` beside a `VerticalUom` that describes the datum, and
            // eml 2.3 divides the same two facts between `VerticalAxis.Uom`
            // and the referenced `VerticalCrs` — so whichever release it came
            // from, the unit of the axis has to come back a time.
            //
            // It failed on every eml 2.3 compound time CRS until 06/09, and
            // silently: the referenced `VerticalCrs` overwrote the axis unit
            // with its datum's length, so `vertical_axis_uom` answered `m` for
            // an axis in seconds. That is the factor of a thousand its own doc
            // comment warns about, and nothing in the crate had caught it
            // because both readings are perfectly valid units.
            if read.vertical_quantity == VerticalQuantity::Time && !read.is_component() {
                if let Some(unit) = read.vertical_axis_uom() {
                    assert_eq!(
                        energyml_domain::units::Dimension::of(unit),
                        energyml_domain::units::Dimension::of("s"),
                        "{}: {} is a time system whose axis unit `{unit}` is not a time",
                        path.display(),
                        summary.uuid
                    );
                }
            }
        }
    }

    eprintln!("CRS objects read: {kinds:?}");
    eprintln!(
        "{with_epsg} with an EPSG code, {downward} counting downward, \
         {time} seismic time system(s), {components} half-systems"
    );

    assert!(!kinds.is_empty(), "the corpus holds no CRS at all");
    // Both structural shapes must be exercised, or this test proves only that
    // one of them works: RESQML 2.0.1 is one self-contained object, eml 2.3 is
    // three joined by references.
    assert!(
        kinds
            .keys()
            .any(|k| k.contains("LocalDepth3dCrs") || k.contains("LocalTime3dCrs")),
        "no RESQML 2.0.1 CRS in the corpus: {kinds:?}"
    );
    assert!(
        kinds.keys().any(|k| k.contains("Compound")),
        "no eml 2.3 compound CRS in the corpus: {kinds:?}"
    );
    assert!(with_epsg > 0, "not one CRS resolved to an EPSG code");
    assert!(
        time > 0,
        "the corpus has a LocalTime3dCrs; if none is seen, the depth/time \
         distinction is not actually being read"
    );
}

/// The two versions describe the same place the same way.
///
/// The reference packages are the same model written twice, once in RESQML
/// 2.0.1 and once in 2.2. The CRSs are spelled completely differently — one
/// object against three, `XOffset` against `OriginProjectedCoordinate1`,
/// `ZIncreasingDownward` against `VerticalAxis.Direction` — so if the view is
/// doing its job, both come out identical. Nothing else in the suite compares
/// the two versions against *each other*.
#[test]
fn the_two_versions_agree_on_the_same_model() {
    let (Some(old), Some(new)) = (
        crate::fixtures::find("testingPackageCpp.epc"),
        crate::fixtures::find("testingPackageCpp22.epc"),
    ) else {
        eprintln!("skipped: the paired fesapi packages are not in the fixtures");
        return;
    };

    let read_all = |path: &std::path::Path| {
        let store = EpcStore::open(path, registry()).expect("the package opens");
        let mut found = Vec::new();
        for summary in store.resources() {
            if !summary.qualified_type.contains("Crs") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Ok(read) = crs::read_with(object.as_reflect(), &store) else {
                continue;
            };
            // Whole systems only, and depth ones at that. eml 2.3 promotes the
            // halves of a system to top-level objects, so a package contains
            // `LocalEngineering2dCrs` and `VerticalCrs` objects that are not
            // systems in their own right; and 2.0.1 keeps time in a separate
            // type that 2.2 folds into the same one.
            if !read.is_component() && read.vertical_quantity == VerticalQuantity::Length {
                found.push(read);
            }
        }
        found
    };

    let (old, new) = (read_all(&old), read_all(&new));
    assert!(
        !old.is_empty() && !new.is_empty(),
        "no depth CRS in one of the pair"
    );

    let epsg = |list: &[energyml_domain::Crs]| -> Vec<Option<u64>> {
        let mut codes: Vec<_> = list.iter().map(|c| c.projected.epsg()).collect();
        codes.sort();
        codes.dedup();
        codes
    };
    assert_eq!(
        epsg(&old),
        epsg(&new),
        "the same model in two versions must name the same projected system"
    );

    for (a, b) in old.iter().zip(new.iter()) {
        assert_eq!(a.x_offset, b.x_offset, "XOffset vs OriginProjectedCoordinate1");
        assert_eq!(a.y_offset, b.y_offset, "YOffset vs OriginProjectedCoordinate2");
        assert_eq!(a.z_offset, b.z_offset, "ZOffset vs OriginVerticalCoordinate");
        assert_eq!(a.areal_rotation, b.areal_rotation, "ArealRotation vs Azimuth");
        assert_eq!(
            a.vertical_direction, b.vertical_direction,
            "ZIncreasingDownward vs VerticalAxis.Direction"
        );
        // A point placed by either must land in the same place. This is the
        // assertion an application actually depends on.
        let point = [1234.5, -678.9, 250.0];
        assert_eq!(a.to_projected(point), b.to_projected(point));
        assert_eq!(a.to_metres(point).unwrap(), b.to_metres(point).unwrap());
    }
}
