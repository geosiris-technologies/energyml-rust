//! Naming the objects, so that importing the same file twice writes the same
//! package.
//!
//! # Why not fresh uuids
//!
//! An import is not a one-off. A SIGEOL export is re-pulled when the referential
//! moves, and the same file is imported into a package somebody has already
//! edited. With random uuids, the second run produces a package that shares not
//! one identifier with the first: every reference from outside breaks, a diff
//! shows everything changed, and nothing can say whether the data moved or only
//! its names did.
//!
//! So every uuid is **derived**, uuid v5 over a namespace and a name — the same
//! choice a version translation makes for the objects it splits, and for the same
//! reason: no clock, no randomness, nothing that would differ between two
//! machines or fail to build for wasm.
//!
//! # What each name is derived from, and why it is that
//!
//! The rule: **derive from what the producer owns, never from what we compute.**
//!
//! - a **unit** is named from its BRGM referential URL. That uuid is permanent,
//!   the referential exists to provide it, and two exports that spell « Marnes
//!   et Caillasses » differently still land on one object. When the referential
//!   gives nothing, the unit's title is the fallback — worse, and stated;
//! - a **surface** is named from the unit, the stage and the side, because that
//!   triple is what the group *is*;
//! - the **CRS objects** are named from their EPSG codes alone. Two imports into
//!   two packages then produce the same 4326 object, which is what makes it
//!   recognisable as the same system rather than as a coincidence;
//! - the **activity** is named from the source file, the stage and the run's own
//!   timestamp — the one place a fresh identity is right, because two imports of
//!   one file are two events and a package should record both.
//!
//! # The namespace is fixed and must never move
//!
//! Changing it renames every object every import has ever written. It is a
//! constant of this crate for the same reason a canonical template's uuid is a
//! constant of the workspace.

use uuid::Uuid;

/// The namespace every derived uuid hangs from.
///
/// A uuid v5 of the DNS namespace over `import.geojson.energyml-rust`, computed
/// once and written down. **Never change it**: it would rename every object of
/// every package this crate has produced.
pub const NAMESPACE: Uuid = Uuid::from_bytes([
    0x9f, 0x2a, 0x4b, 0x1c, 0x7d, 0x6e, 0x5f, 0x40, 0x8a, 0x3b, 0x2c, 0x1d, 0x0e, 0x9f, 0x8a, 0x7b,
]);

/// A uuid derived from a name, inside this crate's namespace.
fn derive(name: &str) -> String {
    Uuid::new_v5(&NAMESPACE, name.as_bytes()).to_string()
}

/// The collection that groups the objects of one **role** of a run.
///
/// Derived from the recipe, the run's seed and the role: two runs of the same
/// file write the same collection, as they write the same objects.
pub fn collection(recipe: &str, seed: &str, role: &str) -> String {
    derive(&format!("collection:{recipe}:{seed}:{role}"))
}

/// The association set that says **who is in which collection**.
///
/// One per run and not one per role: `CollectionsToDataobjectsAssociationSet`
/// carries `SingleCollectionAssociation` as `1..∞`, so it holds all the
/// collections of one run — one per role would be as many more objects in a
/// package as this change exists to lighten.
pub fn associations(recipe: &str, seed: &str) -> String {
    derive(&format!("associations:{recipe}:{seed}"))
}

/// The `RockVolumeFeature` for a lithostratigraphic unit.
///
/// `key` is the unit's referential uuid when it has one, its title otherwise —
/// which is what [`crate::sigeol::Unit::key`] already answers.
pub fn feature(key: &str) -> String {
    derive(&format!("feature:{key}"))
}

/// The `StratigraphicUnitInterpretation` of that unit.
pub fn interpretation(key: &str) -> String {
    derive(&format!("interpretation:{key}"))
}

/// The `PointSetRepresentation` of one surface.
///
/// The triple is the identity of the group: the same unit, on the same stage,
/// on the same side, is the same point set — whichever file it came out of.
pub fn point_set(key: &str, stage: &str, side: &str) -> String {
    derive(&format!("pointset:{key}:{stage}:{side}"))
}

/// The `ColumnBasedTable` that holds what the features say about themselves.
pub fn table(name: &str) -> String {
    derive(&format!("table:{name}"))
}

/// The two CRS objects, from their codes alone.
///
/// **Not from the file**: two imports of two different files into two packages
/// must produce *the same* 4326 object, or a consumer comparing them sees two
/// systems where there is one.
pub fn crs(epsg: i64, vertical: &str) -> (String, String) {
    (
        derive(&format!("crs:geographic2d:{epsg}")),
        derive(&format!("crs:vertical:{vertical}")),
    )
}

/// The `Activity` recording one run.
///
/// Carries the timestamp, because **two imports of one file are two events**.
/// This is the one identity that is meant to differ between runs, and it is
/// derived rather than drawn so that a caller replaying a run with the same
/// stated time reproduces the package exactly.
pub fn activity(source: &str, stage: &str, when: &str) -> String {
    derive(&format!("activity:{source}:{stage}:{when}"))
}

/// The activity of any operation: its recipe, what seeded the run, and the
/// instant — the one place a fresh identity is right, because two runs of one
/// file are two events and a package should record both.
pub fn activity_seeded(recipe: &str, seed: &str, when: &str) -> String {
    derive(&format!("activity:{recipe}:{seed}:{when}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_input_names_the_same_object_every_time() {
        // The whole point: a second import writes a package that can be diffed
        // against the first.
        assert_eq!(
            feature("006fd552-1f63-47b3-bae9-e528f169083c"),
            feature("006fd552-1f63-47b3-bae9-e528f169083c")
        );
        assert_eq!(
            point_set("u", "Lutétien", "Top"),
            point_set("u", "Lutétien", "Top")
        );
    }

    #[test]
    fn the_roles_do_not_collide() {
        // A feature and its interpretation are two objects. Deriving both from
        // the bare unit key would give one uuid to two objects, and a package
        // that cannot address either.
        let unit = "006fd552-1f63-47b3-bae9-e528f169083c";
        assert_ne!(feature(unit), interpretation(unit));
        assert_ne!(feature(unit), table(unit));
    }

    #[test]
    fn a_surface_is_identified_by_its_whole_triple() {
        // Same unit, other stage, and other side: three different objects. A
        // key that dropped any of the three would merge two point sets into one
        // and lose half the points without a word.
        assert_ne!(
            point_set("u", "Lutétien", "Top"),
            point_set("u", "Bartonien", "Top")
        );
        assert_ne!(
            point_set("u", "Lutétien", "Top"),
            point_set("u", "Lutétien", "Bottom")
        );
        assert_ne!(
            point_set("u", "Lutétien", "Top"),
            point_set("v", "Lutétien", "Top")
        );
    }

    #[test]
    fn the_crs_objects_do_not_depend_on_the_file_they_came_from() {
        // Two imports of two files must land on the same 4326 object, or a
        // consumer sees two coordinate systems where there is one.
        assert_eq!(crs(4326, "5720"), crs(4326, "5720"));
        // And a different vertical datum leaves the horizontal half alone: two
        // packages disagreeing about the datum still agree about 4326.
        let (h1, v1) = crs(4326, "5720");
        let (h2, v2) = crs(4326, "unknown");
        assert_eq!(h1, h2);
        assert_ne!(v1, v2);
    }

    #[test]
    fn an_activity_is_one_event_and_two_runs_are_two() {
        // The one identity meant to differ. Derived rather than drawn, so a run
        // replayed with the same stated time reproduces the package exactly.
        let a = activity(
            "GeologicUnit_Priabonien.json",
            "Priabonien",
            "2026-09-07T10:00:00Z",
        );
        let b = activity(
            "GeologicUnit_Priabonien.json",
            "Priabonien",
            "2026-09-07T11:00:00Z",
        );
        assert_ne!(a, b);
        assert_eq!(
            a,
            activity(
                "GeologicUnit_Priabonien.json",
                "Priabonien",
                "2026-09-07T10:00:00Z"
            )
        );
    }

    #[test]
    fn every_derived_name_is_a_well_formed_v5_uuid() {
        // The schema pins the shape with a pattern; a name that came out
        // malformed would fail at write time, far from here.
        for made in [
            feature("x"),
            interpretation("x"),
            table("x"),
            point_set("x", "y", "Top"),
        ] {
            let parsed = Uuid::parse_str(&made).expect("a uuid");
            assert_eq!(parsed.get_version_num(), 5, "{made}");
        }
    }
}
