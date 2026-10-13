//! The package cache (ADR-0010):
//! the heads, the spatial index, and a contribution that
//! reads only the wells near a section.
//!
//! Every object is built in memory; the shape of each assertion is the one
//! the section's own rule gives (*the head alone decides*), the
//! values are the fixtures'. The counts on a real model are in a downstream test.
//!
//! # What it would take to make each one fail
//!
//! - a node whose bounds are one short (`Index::rebuild`: the union of the
//!   children left out of the node's box): the tree-equals-scan test finds an
//!   item the tree no longer reaches;
//! - a zero margin in `read::corridor_windows`: the well at the corridor's edge
//!   is not read, and `the_catalogue_is_the_same_…` and
//!   `a_well_at_the_edge_of_the_band_is_read` both go red;
//! - folding the head-only stubs of a far wellbore again (`assemble::fold_wells`
//!   ignoring `WellRead::head_only`): a `Merged` row appears among the far wells
//!   and `far_wells_are_each_refused_on_their_own_row` goes red (no merging far away).

use std::collections::BTreeSet;

use energyml_core::registry::DynObject;
use energyml_domain::reproject::NoReprojection;
use energyml_domain::source::Prefetched;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_section::assemble::assemble;
use energyml_section::crosses::traces;
use energyml_section::model::{
    Asked, Catalogue, Contribution, CrossWidths, Crosses, Inputs, Outcome, System,
};
use energyml_section::read::{Bound, PackageCache, PackageContext, Scene, contribution_bounded};
use energyml_section::spatial::{Index, Rect};
use energyml_storage::array::ArrayLocation;

// -- the index against a linear scan ---------------------------------------------

/// A small deterministic generator: no dependency, the same data every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn rect(&mut self, span: f64, size: f64) -> Rect {
        let (x, y) = (self.next() * span, self.next() * span);
        let (w, h) = (self.next() * size, self.next() * size);
        // One in five is a point: the heads of wells are.
        if self.next() < 0.2 {
            Rect::point([x, y]).unwrap()
        } else {
            Rect::new([x, y], [x + w, y + h]).unwrap()
        }
    }
}

fn scan(items: &[(Rect, usize)], window: &Rect) -> BTreeSet<usize> {
    items
        .iter()
        .filter(|(rect, _)| rect.intersects(window))
        .map(|(_, id)| *id)
        .collect()
}

fn tree(index: &Index<usize>, window: &Rect) -> Vec<usize> {
    index.query(window).into_iter().copied().collect()
}

/// Whatever the tree answers is what a scan answers — the build, the overflow
/// list, and the rebuild past its size — over many random queries, large and
/// small, and boxes that only touch.
#[test]
fn the_rtree_equals_a_linear_scan_on_random_boxes() {
    let mut random = Lcg(7);
    let mut items: Vec<(Rect, usize)> = (0..1500).map(|id| (random.rect(1000.0, 40.0), id)).collect();
    let mut index = Index::build(items.clone());
    assert_eq!(index.rebuilds(), 1);

    let check = |index: &Index<usize>, items: &[(Rect, usize)], random: &mut Lcg, queries: usize| {
        for _ in 0..queries {
            let size = if random.next() < 0.5 { 30.0 } else { 400.0 };
            let window = random.rect(1000.0, size);
            let found = tree(index, &window);
            let unique: BTreeSet<usize> = found.iter().copied().collect();
            assert_eq!(found.len(), unique.len(), "an item is answered once");
            assert_eq!(unique, scan(items, &window), "window {window:?}");
        }
    };
    check(&index, &items, &mut random, 400);

    // A box that only touches: closed boxes meet at a point.
    let probe = Rect::new([2000.0, 2000.0], [2010.0, 2010.0]).unwrap();
    index.insert(Rect::new([2010.0, 2010.0], [2020.0, 2020.0]).unwrap(), 9999);
    items.push((Rect::new([2010.0, 2010.0], [2020.0, 2020.0]).unwrap(), 9999));
    assert_eq!(tree(&index, &probe), vec![9999]);

    // Items added after the build wait in the overflow, and are found.
    for id in 1500..1560 {
        let rect = random.rect(1000.0, 40.0);
        index.insert(rect, id);
        items.push((rect, id));
    }
    assert!(
        index.overflow() > 0 && index.rebuilds() == 1,
        "still in the overflow"
    );
    check(&index, &items, &mut random, 200);

    // Past its share the overflow is folded in, and the answers do not move.
    for id in 1560..2000 {
        let rect = random.rect(1000.0, 40.0);
        index.insert(rect, id);
        items.push((rect, id));
    }
    assert!(index.rebuilds() > 1, "the overflow outgrew its share");
    assert!(index.overflow() < 440);
    check(&index, &items, &mut random, 400);
    assert_eq!(index.len(), items.len());
}

#[test]
fn the_rtree_handles_the_small_and_the_degenerate() {
    let empty: Index<usize> = Index::new();
    assert!(
        empty.is_empty()
            && empty
                .query(&Rect::new([0.0, 0.0], [1.0, 1.0]).unwrap())
                .is_empty()
    );
    // One item, then every point on one spot.
    let one = Index::build(vec![(Rect::point([1.0, 1.0]).unwrap(), 0)]);
    assert_eq!(tree(&one, &Rect::new([0.0, 0.0], [1.0, 1.0]).unwrap()), vec![0]);
    let same = Index::build((0..40).map(|id| (Rect::point([5.0, 5.0]).unwrap(), id)).collect());
    assert_eq!(tree(&same, &Rect::point([5.0, 5.0]).unwrap()).len(), 40);
    assert!(tree(&same, &Rect::point([5.0, 5.1]).unwrap()).is_empty());
    // A non-finite box is not indexable: the caller decides.
    assert!(Rect::point([f64::NAN, 0.0]).is_none() && Rect::of_points(&[]).is_none());
    // A polygon is queried by its bounding box.
    let polygon = [[0.0, 0.0], [4.0, 1.0], [2.0, 3.0]];
    assert_eq!(Rect::of_points(&polygon), Rect::new([0.0, 0.0], [4.0, 3.0]));
}

// -- a package of many wells ---------------------------------------------------------

const PCRS: &str = "aaaaaaaa-0000-4000-8000-00000000000a";
const VCRS: &str = "22222222-2222-4222-8222-222222222222";

fn uuid(kind: u8, n: usize) -> String {
    format!("{kind:02x}000000-0000-4000-8000-{n:012x}")
}

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: "x".to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn datum(uuid: &str, x: f64, y: f64) -> eml23::ReferencePointInACrs {
    eml23::ReferencePointInACrs {
        uuid: uuid.to_string(),
        vertical_crs: Some(dor(VCRS, "eml23.VerticalCrs")),
        vertical_coordinate: Some(40.0),
        horizontal_coordinates: Some(eml23::AbstractHorizontalCoordinates::ProjectedCoordinates(
            eml23::ProjectedCoordinates {
                coordinate1: x,
                coordinate2: y,
                crs: dor(PCRS, "eml23.ProjectedCrs"),
            },
        )),
        ..Default::default()
    }
}

fn trajectory(
    id: &str,
    datum_uuid: &str,
    represents: Option<&str>,
) -> resqml22::WellboreTrajectoryRepresentation {
    resqml22::WellboreTrajectoryRepresentation {
        uuid: id.to_string(),
        citation: citation(&format!("{id} trajectory")),
        represented_object: represents.map(|uuid| dor(uuid, "resqml22.WellboreInterpretation")),
        md_interval: eml23::MdInterval {
            md_min: 2.5,
            md_max: 65.0,
            uom: eml23::LengthUomExt::M,
            datum: Some(dor(datum_uuid, "eml23.ReferencePointInACrs")),
            ..Default::default()
        },
        md_domain: Some(resqml22::MdDomain::Driller),
        ..Default::default()
    }
}

fn nowhere(_: &dyn energyml_core::value::Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn lambert() -> System {
    System {
        epsg: 2154,
        unit: "m".to_string(),
    }
}

/// The package: 14 columns by 8 rows of vertical wells, one every 100 m from
/// (-200, -200), with a position nudged off the grid by the generator. Every
/// fifth well is one wellbore with a **second trajectory collared 30 m to the
/// east** — the two members of a wellbore that a corridor's edge can separate.
struct Package {
    source: Prefetched,
    /// Every well's uri, and where its head is.
    wells: Vec<(String, [f64; 2])>,
}

fn package() -> Package {
    let mut random = Lcg(11);
    let mut objects: Vec<Box<dyn DynObject>> = vec![
        Box::new(eml23::VerticalCrs {
            uuid: VCRS.to_string(),
            direction: eml23::VerticalDirection::Up,
            abstract_vertical_crs: Some(eml23::AbstractVerticalCrs::VerticalEpsgCrs(
                eml23::VerticalEpsgCrs { epsg_code: 5714 },
            )),
            ..Default::default()
        }),
        Box::new(eml23::ProjectedCrs {
            uuid: PCRS.to_string(),
            abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(
                eml23::ProjectedEpsgCrs { epsg_code: 2154 },
            )),
            ..Default::default()
        }),
    ];
    let mut wells = Vec::new();
    let mut add =
        |objects: &mut Vec<Box<dyn DynObject>>, n: usize, at: [f64; 2], represents: Option<&str>| {
            let (d, t) = (uuid(0x10, n), uuid(0x70, n));
            objects.push(Box::new(datum(&d, at[0], at[1])));
            objects.push(Box::new(trajectory(&t, &d, represents)));
            wells.push((t, at));
        };
    let mut n = 0;
    for column in 0..14 {
        for row in 0..8 {
            let at = [
                -200.0 + column as f64 * 100.0 + random.next() * 60.0,
                -200.0 + row as f64 * 100.0 + random.next() * 60.0,
            ];
            n += 1;
            if n % 5 == 0 {
                let (feature, interpretation) = (uuid(0x20, n), uuid(0x30, n));
                objects.push(Box::new(resqml22::WellboreFeature {
                    uuid: feature.clone(),
                    citation: citation("wellbore"),
                    ..Default::default()
                }));
                objects.push(Box::new(resqml22::WellboreInterpretation {
                    uuid: interpretation.clone(),
                    citation: citation("interpretation"),
                    interpreted_feature: dor(&feature, "resqml22.WellboreFeature"),
                    ..Default::default()
                }));
                add(&mut objects, n, at, Some(&interpretation));
                add(
                    &mut objects,
                    n + 10_000,
                    [at[0] + 30.0, at[1]],
                    Some(&interpretation),
                );
            } else {
                add(&mut objects, n, at, None);
            }
        }
    }
    let mut source = Prefetched::new();
    for object in objects {
        source.insert(object, &nowhere);
    }
    Package { source, wells }
}

fn scene(system: &System) -> Scene<'_> {
    Scene {
        system: Some(system),
        time_factor: None,
        reprojector: &NoReprojection,
    }
}

/// The long section along y = 250 over 1 000 m, 90 m each side, and cross
/// sections across it — a corridor that cuts the grid in a band.
fn inputs(package: &Package, left: f64, right: f64) -> Inputs {
    Inputs {
        trace: vec![[0.0, 250.0], [500.0, 280.0], [1000.0, 250.0]],
        system: Some(lambert()),
        left,
        right,
        crosses: Crosses::Count {
            count: 2,
            common: CrossWidths {
                before: 60.0,
                after: 60.0,
                left: 40.0,
                right: 40.0,
            },
            own: Vec::new(),
        },
        objects: vec![Asked {
            store: "s1".to_string(),
            revision: 0,
            wells: package.wells.iter().map(|(uri, _)| uri.clone()).collect(),
            surfaces: Vec::new(),
        }],
        time_factor: None,
    }
}

fn contribution(package: &Package, inputs: &Inputs, cache: &mut PackageCache, bound: Bound) -> Contribution {
    let (sections, _) = traces(inputs).expect("the sections");
    let system = lambert();
    contribution_bounded(
        &package.source,
        &inputs.objects[0],
        &sections,
        &scene(&system),
        &PackageContext::default(),
        cache,
        bound,
    )
}

fn catalogue(inputs: &Inputs, contribution: &Contribution) -> Catalogue {
    assemble(inputs, std::slice::from_ref(contribution)).expect("assembles")
}

/// The wells whose head is within `reach` of a section's trace — what the
/// window must hold at the least, computed here by the distance to a segment,
/// not by a box.
fn near_a_trace(package: &Package, inputs: &Inputs) -> usize {
    let (sections, _) = traces(inputs).expect("the sections");
    let distance = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
        let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1])).clamp(0.0, 1.0);
        (p[0] - a[0] - t * ab[0]).hypot(p[1] - a[1] - t * ab[1])
    };
    package
        .wells
        .iter()
        .filter(|(_, at)| {
            sections.iter().any(|st| {
                st.trace
                    .windows(2)
                    .any(|s| distance(*at, s[0], s[1]) <= st.left.max(st.right))
            })
        })
        .count()
}

/// The uuids of the wells told by their head alone.
fn far_of(contribution: &Contribution) -> BTreeSet<String> {
    contribution
        .wells
        .iter()
        .filter(|w| w.head_only)
        .map(|w| w.object.uuid.clone())
        .collect()
}

/// **The drawn part of the catalogue does not change**: everything but the rows
/// and the `PlanWell`s of the far wells is identical, JSON for JSON.
fn same_but_far(a: &Catalogue, b: &Catalogue, far: &BTreeSet<String>, what: &str) {
    let (mut a, mut b) = (serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
    for catalogue in [&mut a, &mut b] {
        catalogue["objects"]
            .as_array_mut()
            .unwrap()
            .retain(|row| !far.contains(row["object"]["uuid"].as_str().unwrap()));
        catalogue["plan"]["wells"]
            .as_array_mut()
            .unwrap()
            .retain(|well| !far.contains(well["object"]["uuid"].as_str().unwrap()));
    }
    assert_eq!(a, b, "{what}");
}

/// Far wells, in the bounded catalogue: one refused row each, never `Merged`,
/// and a `PlanWell` each.
fn far_wells_stand_alone(bounded: &Catalogue, far: &BTreeSet<String>) {
    for uuid in far {
        let rows: Vec<_> = bounded
            .objects
            .iter()
            .filter(|row| row.object.uuid == *uuid)
            .collect();
        assert_eq!(rows.len(), 1, "one row for the far well {uuid}");
        assert!(
            matches!(&rows[0].outcome, Outcome::Refused { reason } if reason.contains("farther than")),
            "{uuid}: {:?}",
            rows[0].outcome
        );
        assert_eq!(
            bounded
                .plan
                .wells
                .iter()
                .filter(|w| w.object.uuid == *uuid)
                .count(),
            1
        );
    }
    assert!(
        !bounded
            .objects
            .iter()
            .any(|row| far.contains(&row.object.uuid) && matches!(row.outcome, Outcome::Merged { .. })),
        "no far well is folded into another"
    );
}

/// **The drawing does not change.** Read everything, or read what is near and
/// stand the far wellbores in by their heads: the same sections, the same plan,
/// the same near rows — and the far wells, member by member, refused as far.
#[test]
fn the_catalogue_is_the_same_with_and_without_the_corridor_filter() {
    let package = package();
    for (left, right) in [(90.0, 90.0), (120.0, 40.0), (30.0, 30.0)] {
        let inputs = inputs(&package, left, right);
        let mut all = PackageCache::new();
        let mut near = PackageCache::new();
        let full = contribution(&package, &inputs, &mut all, Bound::Everything);
        let bounded = contribution(&package, &inputs, &mut near, Bound::Corridor);

        let (a, b) = (catalogue(&inputs, &full), catalogue(&inputs, &bounded));
        let far = far_of(&bounded);
        same_but_far(&a, &b, &far, &format!("widths {left}/{right}"));
        far_wells_stand_alone(&b, &far);
        assert_eq!(far.len(), package.wells.len() - near.work().wells_read);

        // The rows say what they said: far wells refused as such, by name.
        let refused_far = a
            .objects
            .iter()
            .filter(
                |row| matches!(&row.outcome, Outcome::Refused { reason } if reason.contains("farther than")),
            )
            .count();
        assert!(
            refused_far > 10,
            "the fixture has wells far from every section: {refused_far}"
        );

        // The work: all of them read in full, against only those near.
        assert_eq!(all.work().wells_read, package.wells.len());
        let read = near.work().wells_read;
        let must = near_a_trace(&package, &inputs);
        assert!(
            read >= must,
            "a well within reach of a trace was not read ({read} < {must})"
        );
        assert!(
            read < package.wells.len(),
            "the filter read {read} of {}",
            package.wells.len()
        );
        // The wells of the wellbores far from every section are the ones left unread.
        assert!(
            package.wells.len() - read > 30,
            "{} wells unread",
            package.wells.len() - read
        );
        assert_eq!(near.work().heads_computed, package.wells.len());
    }
}

/// A well whose head is 0.9 of the width off the trace is **kept** by the
/// section's rule, so the filter must read it — a box grown by nothing would
/// have left it out.
#[test]
fn a_well_at_the_edge_of_the_band_is_read() {
    let mut package = package();
    let edge = uuid(0x70, 77_777);
    let d = uuid(0x10, 77_777);
    // On the long section's first leg, (0,250) to (500,280): 81 m above a foot
    // at x = 300 — a perpendicular distance of about 80.9, inside the 90 m.
    let at = [300.0, 250.0 + 300.0 * 30.0 / 500.0 + 81.0];
    let mut extra: Vec<Box<dyn DynObject>> = vec![
        Box::new(datum(&d, at[0], at[1])),
        Box::new(trajectory(&edge, &d, None)),
    ];
    for object in extra.drain(..) {
        package.source.insert(object, &nowhere);
    }
    package.wells.push((edge.clone(), at));

    let inputs = inputs(&package, 90.0, 90.0);
    let mut cache = PackageCache::new();
    let bounded = contribution(&package, &inputs, &mut cache, Bound::Corridor);
    assert!(
        bounded
            .wells
            .iter()
            .any(|w| w.object.uuid == edge && !w.path.is_empty()),
        "the edge well was read in full"
    );
    let row = catalogue(&inputs, &bounded)
        .objects
        .into_iter()
        .find(|row| row.object.uuid == edge)
        .expect("its row");
    assert!(
        matches!(row.outcome, Outcome::Drawn { .. }),
        "kept by the corridor: {:?}",
        row.outcome
    );
}

/// The work is counted, and kept: the second call computes no head and reads no
/// well; a trace moved reads only the wells it newly reaches.
#[test]
fn heads_are_computed_once_and_a_moved_trace_reads_only_what_it_newly_reaches() {
    let package = package();
    let inputs_a = inputs(&package, 60.0, 60.0);
    let mut cache = PackageCache::new();
    contribution(&package, &inputs_a, &mut cache, Bound::Corridor);
    let first = cache.work();
    assert_eq!(first.heads_computed, package.wells.len());
    assert!(first.spatial_queries >= 1);

    contribution(&package, &inputs_a, &mut cache, Bound::Corridor);
    let second = cache.work();
    assert_eq!(
        (second.heads_computed, second.wells_read),
        (first.heads_computed, first.wells_read)
    );
    assert!(second.spatial_queries > first.spatial_queries && second.reused > first.reused);

    // Wider: the same wells are reused, the others newly read.
    let inputs_b = inputs(&package, 160.0, 160.0);
    contribution(&package, &inputs_b, &mut cache, Bound::Corridor);
    let third = cache.work();
    assert_eq!(third.heads_computed, first.heads_computed, "no head twice");
    assert!(third.wells_read > first.wells_read, "wider reaches more wells");
    let mut fresh = PackageCache::new();
    contribution(&package, &inputs_b, &mut fresh, Bound::Corridor);
    assert_eq!(
        third.wells_read,
        fresh.work().wells_read,
        "every well read once, none twice"
    );
}

/// Without a section there is nothing to bound by: every well is read.
#[test]
fn without_a_section_everything_is_read() {
    let package = package();
    let inputs = inputs(&package, 60.0, 60.0);
    let system = lambert();
    let mut cache = PackageCache::new();
    let out = contribution_bounded(
        &package.source,
        &inputs.objects[0],
        &[],
        &scene(&system),
        &PackageContext::default(),
        &mut cache,
        Bound::Corridor,
    );
    assert_eq!(out.wells.len(), package.wells.len());
    assert!(out.wells.iter().all(|w| !w.path.is_empty()));
    assert_eq!(cache.work().heads_computed, 0);
}

/// A wellbore whose two trajectories are both far from every section is not
/// folded: each member has its own refused row and its own `PlanWell`, none is
/// `Merged` (no merging far away). The pairs of the package
/// are the ones numbered 5, 10, … with a twin at +10 000.
#[test]
fn far_wells_are_each_refused_on_their_own_row() {
    let package = package();
    let inputs = inputs(&package, 30.0, 30.0);
    let mut cache = PackageCache::new();
    let bounded = contribution(&package, &inputs, &mut cache, Bound::Corridor);
    let far = far_of(&bounded);
    let twins: Vec<&String> = far
        .iter()
        .filter(|uuid| {
            uuid.ends_with(&format!("{:012x}", 10_000 + 5))
                || uuid
                    .rsplit('-')
                    .next()
                    .is_some_and(|n| usize::from_str_radix(n, 16).is_ok_and(|n| n > 10_000))
        })
        .collect();
    assert!(
        !twins.is_empty(),
        "a far wellbore with two members exists in the fixture"
    );
    let b = catalogue(&inputs, &bounded);
    far_wells_stand_alone(&b, &far);
    // Each twin and its partner both have a row of their own.
    for twin in twins {
        let n = usize::from_str_radix(twin.rsplit('-').next().unwrap(), 16).unwrap() - 10_000;
        let partner = uuid(0x70, n);
        assert!(far.contains(&partner), "the partner of a far twin is far too");
    }
}
