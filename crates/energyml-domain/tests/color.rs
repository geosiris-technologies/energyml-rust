//! Colour, against the public reference package that carries any (`testingPackageCpp22`).
//!
//! `testingPackageCpp22.epc` and `80wells` hold the same
//! `GraphicalInformationSet`: 15 entries, 12 of them constant colours on
//! features, interpretations and representations, and 3 `ColorInformation`
//! pointing at colour maps — including the kind-versus-property conflict that
//! made the property-over-kind rule necessary (entry 12 targets the `cellIndex` **kind**,
//! entry 13 targets a **property** of that kind).
//!
//! RESQML 2.0.1 has no colour at all, so ALWYN and Volve are expected to have
//! nothing here — and a test that finds colour in them would be finding a bug.

use std::path::PathBuf;
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::color::{self, Cascade, GraphicalSet, Information, Level, Paint, Request, Scope};
use energyml_domain::property;
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

/// Open the packages that carry a `GraphicalInformationSet`, with it read.
fn sets() -> Vec<(PathBuf, EpcStore, GraphicalSet)> {
    let registry = registry();
    let mut out = Vec::new();
    for path in corpus() {
        let Ok(store) = EpcStore::open(&path, registry.clone()) else {
            continue;
        };
        let found = store
            .resources()
            .iter()
            .find(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
            .and_then(|summary| store.get_object(&summary.uri).ok());
        if let Some(object) = found {
            let set = GraphicalSet::read(object.as_reflect());
            out.push((path, store, set));
        }
    }
    out
}

#[test]
fn every_entry_of_every_set_reads_and_keeps_its_position() {
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet under testdata/");
        return;
    }
    for (path, _, set) in &sets {
        assert!(!set.entries.is_empty(), "{}: an empty set", path.display());
        for (at, entry) in set.entries.iter().enumerate() {
            // Positions are the wiring: `Active*InformationIndex` is an index
            // into this list, so an entry that could not be read is kept as
            // opaque rather than dropped.
            assert_eq!(entry.position, at, "{}: positions must be the list order", path.display());
            assert!(
                !entry.targets.is_empty(),
                "{}: entry {at} decorates nothing",
                path.display()
            );
        }
        for note in &set.notes {
            assert!(note.len() > 20, "a note that says nothing: {note}");
        }
        eprintln!(
            "{}: {} entries, {} note(s)",
            path.file_name().unwrap().to_string_lossy(),
            set.entries.len(),
            set.notes.len()
        );
    }
}

#[test]
fn a_property_beats_its_kind() {
    // The property-over-kind rule, on the exact conflict the corpus contains: one entry
    // targets the `cellIndex` property kind, a later one targets a property
    // *of* that kind. The property must win.
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    let mut proved = false;

    for (path, store, set) in &sets {
        // Find a property that is targeted directly and whose kind is targeted too.
        for summary in store.resources() {
            if !summary.qualified_type.ends_with("Property") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Ok(read) = property::read(object.as_reflect()) else {
                continue;
            };
            let Some(energyml_domain::property::PropertyKindRef::Object { uuid: kind, .. }) = &read.kind
            else {
                continue;
            };
            let on_property = set
                .targeting(&read.uuid)
                .into_iter()
                .any(|e| matches!(e.information, Information::Color { .. }));
            let on_kind = set
                .targeting(kind)
                .into_iter()
                .any(|e| matches!(e.information, Information::Color { .. }));
            if !(on_property && on_kind) {
                continue;
            }

            let cascade = Cascade::new(set);
            let chain = vec![kind.clone()];
            let decision = cascade
                .resolve(
                    &Request {
                        representation: read.supporting_representation.as_deref().unwrap_or(""),
                        property: Some(&read.uuid),
                        kind_chain: &chain,
                        element: Scope::WholeObject,
                    },
                    store,
                )
                .expect("the cascade runs")
                .expect("something answers");
            assert_eq!(
                decision.level,
                Level::Property,
                "{}: {} is decorated directly and through its kind; the property must win",
                path.display(),
                read.uuid
            );
            proved = true;
        }
    }
    assert!(
        proved,
        "the kind-versus-property conflict is not in the corpus any more — this test no longer proves the kind-versus-property rule"
    );
}

#[test]
fn a_representation_with_no_colour_of_its_own_inherits_and_says_so() {
    // Inheritance is on by default. The corpus paints all three levels, so to see inheritance
    // the test asks about a representation that is *not* itself decorated and
    // whose interpretation or feature is.
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    let mut inherited = 0usize;

    for (_, store, set) in &sets {
        let cascade = Cascade::new(set);
        for summary in store.resources() {
            if !summary.qualified_type.contains("Representation") {
                continue;
            }
            let decorated = set
                .targeting(&summary.uuid)
                .into_iter()
                .any(|e| matches!(e.information, Information::Default { .. }));
            if decorated {
                continue;
            }
            let Ok(Some(decision)) = cascade.resolve(
                &Request {
                    representation: &summary.uuid,
                    property: None,
                    kind_chain: &[],
                    element: Scope::WholeObject,
                },
                store,
            ) else {
                continue;
            };
            assert!(
                matches!(decision.level, Level::Interpretation | Level::Feature),
                "a representation that carries nothing can only have inherited: {decision:?}"
            );
            assert_ne!(
                decision.found_on.to_ascii_lowercase(),
                summary.uuid.to_ascii_lowercase(),
                "the colour must be recorded as coming from another object"
            );
            inherited += 1;
        }
    }
    assert!(
        inherited > 0,
        "no representation inherited a colour — colour inheritance is never exercised by this corpus"
    );
}

#[test]
fn switching_inheritance_off_gives_nothing_rather_than_someone_elses_colour() {
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    for (_, store, set) in &sets {
        let mut cascade = Cascade::new(set);
        cascade.inherit = false;
        for summary in store.resources() {
            if !summary.qualified_type.contains("Representation") {
                continue;
            }
            let decorated = set
                .targeting(&summary.uuid)
                .into_iter()
                .any(|e| matches!(e.information, Information::Default { .. }));
            if decorated {
                continue;
            }
            let decision = cascade
                .resolve(
                    &Request {
                        representation: &summary.uuid,
                        property: None,
                        kind_chain: &[],
                        element: Scope::WholeObject,
                    },
                    store,
                )
                .expect("the cascade runs");
            assert!(
                decision.is_none(),
                "{}: inheritance is off, so nothing may be borrowed from above",
                summary.uuid
            );
        }
    }
}

#[test]
fn every_colour_map_of_the_corpus_reads_and_samples() {
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    let mut maps = 0usize;

    for (path, store, _) in &sets {
        for summary in store.resources() {
            if !summary.qualified_type.contains("ColorMap") {
                continue;
            }
            let object = store.get_object(&summary.uri).expect("the map deserializes");
            let map = color::ColorMap::read(object.as_reflect())
                .unwrap_or_else(|e| panic!("{}: {} did not read: {e}", path.display(), summary.uuid));
            assert!(!map.stops.is_empty());
            // Sorted, so the "nearest lower entry" rule is a walk and not a search.
            for pair in map.stops.windows(2) {
                assert!(pair[0].index <= pair[1].index, "stops must come out sorted");
            }
            // Every stop is a colour that converts.
            let (low, high) = map.range();
            for value in [low, (low + high) / 2.0, high] {
                let sampled = map
                    .sample(Some(value), &color::Scaling::default())
                    .expect("a value inside the range always has a colour");
                for channel in [sampled.r, sampled.g, sampled.b, sampled.a] {
                    assert!((0.0..=1.0).contains(&channel), "{sampled:?} out of range");
                }
            }
            maps += 1;
        }
    }
    assert!(maps > 0, "the corpus carries colour maps; none was read");
    eprintln!("{maps} colour maps read");
}

#[test]
fn colouring_by_a_property_gives_a_map_and_the_component_to_use() {
    // The end of the chain: a continuous property, its `ColorInformation`, its
    // map, and `ValueVectorIndex` handed back as what it is — the component of
    // the value vector, not an entry of the map.
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    let mut mapped = 0usize;

    for (_, store, set) in &sets {
        let cascade = Cascade::new(set);
        for summary in store.resources() {
            if !summary.qualified_type.ends_with("Property") {
                continue;
            }
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Ok(read) = property::read(object.as_reflect()) else {
                continue;
            };
            let Ok(Some(decision)) = cascade.resolve(
                &Request {
                    representation: read.supporting_representation.as_deref().unwrap_or(""),
                    property: Some(&read.uuid),
                    kind_chain: &[],
                    element: Scope::WholeObject,
                },
                store,
            ) else {
                continue;
            };
            let Paint::Mapped {
                map,
                scaling,
                value_vector_index,
            } = &decision.paint
            else {
                continue;
            };
            mapped += 1;
            assert!(!map.stops.is_empty());
            // The corpus's continuous entry declares MinMax 0–1 and
            // ValueVectorIndex 1 — a component index, kept as such.
            if let Some(component) = value_vector_index {
                assert!(
                    *component < read.values_per_element().max(1),
                    "{}: component {component} of a property with {} value(s) per element — the \
                     index is into the data, so it has to fit the data",
                    read.uuid,
                    read.values_per_element()
                );
            }
            if let Some((min, max)) = scaling.min_max {
                assert!(min <= max, "an inverted MinMax would silently flip the ramp");
            }
        }
    }
    assert!(mapped > 0, "no property resolved to a colour map");
    eprintln!("{mapped} properties coloured by a map");
}

#[test]
fn a_2_0_1_package_has_no_colour_and_that_is_the_standard_not_a_gap() {
    // RESQML 2.0.1 defines nothing graphical whatsoever. If this ever finds a
    // set in ALWYN or Volve, something is reading a type that cannot exist.
    let registry = registry();
    for path in corpus() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        // The public packages that are 2.0.1 throughout: the four resqpy ones.
        // fesapi's `testingPackageCpp` is not one of them — it mixes in RESQML
        // 2.2 objects, colour maps included. (ALWYN and Volve are in
        // `..._private` below.)
        if name.starts_with("testingPackageCpp") {
            continue;
        }
        let store = EpcStore::open(&path, registry.clone()).expect("the package opens");
        let graphical = store
            .resources()
            .iter()
            .filter(|s| s.qualified_type.contains("Graphical") || s.qualified_type.contains("ColorMap"))
            .count();
        assert_eq!(graphical, 0, "{name} is RESQML 2.0.1, which has no colour at all");
    }
}

/// The same rule on two real 2.0.1 producers' files. Real-file campaign,
/// private, run on real files.
#[cfg(feature = "private-corpus")]
#[test]
fn a_2_0_1_package_has_no_colour_private() {
    let registry = registry();
    for path in crate::fixtures::private_corpus() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !(name.starts_with("ALWYN") || name.starts_with("Volve")) {
            continue;
        }
        let store = EpcStore::open(&path, registry.clone()).expect("the package opens");
        let graphical = store
            .resources()
            .iter()
            .filter(|s| s.qualified_type.contains("Graphical") || s.qualified_type.contains("ColorMap"))
            .count();
        assert_eq!(graphical, 0, "{name} is RESQML 2.0.1, which has no colour at all");
    }
}

// ---------------------------------------------------------------------------
// Writing a colour back — the reader is the specification of the writer

/// A colour written into a set comes back out of the reader as the colour that
/// went in, and the entry names the object it decorates.
///
/// **The round trip is the whole test.** A writer checked against a string of
/// its own making proves that the string is what the writer writes; checked
/// against the reader, it proves that another tool reading this package finds
/// the colour — and the reader is the one this workspace ships, tested against
/// every set of the corpus.
#[test]
fn a_colour_written_into_a_set_is_read_back_as_itself() {
    let Some((path, _store, before)) = sets().into_iter().next() else {
        eprintln!("skipped: no GraphicalInformationSet in testdata (it is git-ignored)");
        return;
    };
    let registry = registry();
    let store = EpcStore::open(&path, registry).expect("the package opens");
    let uri = store
        .resources()
        .iter()
        .find(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .map(|summary| summary.uri.clone())
        .expect("the set is there");
    let mut object = store.get_object(&uri).expect("the set reads");

    let target = color::Decorated {
        uuid: "0f2e6b41-8c73-4a95-b1d0-5e8a7c34f962".to_string(),
        qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
        title: "a surface & <its> name".to_string(),
    };
    let chosen = color::Rgba { r: 0.2, g: 0.4, b: 0.8, a: 0.5 };

    let wrote = color::set_constant_colour(
        object.as_reflect_mut(),
        std::slice::from_ref(&target),
        color::Scope::WholeObject,
        chosen,
        Some("bleu choisi"),
    )
        .expect("the colour is written");
    assert_eq!(wrote, color::Wrote::Added, "nothing coloured that object before");

    let after = GraphicalSet::read(object.as_reflect());
    assert_eq!(after.entries.len(), before.entries.len() + 1, "one entry more, and only one");
    let entry = after.entries.last().expect("the entry just written");
    assert_eq!(entry.targets, std::slice::from_ref(&target.uuid), "it names the object it decorates");
    let Information::Default { elements, .. } = &entry.information else {
        panic!("{:?}", entry.information)
    };
    let element = elements.iter().find(|e| e.scope == Scope::WholeObject).expect("a whole-object colour");
    let read = element.constant_color.as_ref().expect("the colour").to_rgba();
    for (wrote, read) in [(chosen.r, read.r), (chosen.g, read.g), (chosen.b, read.b), (chosen.a, read.a)] {
        assert!((wrote - read).abs() < 1e-6, "wrote {wrote}, read {read}");
    }

    // Writing again for the same object replaces, it does not pile up: two
    // entries naming one target with one role is the ambiguity §7.2 leaves
    // undecided, and this tool must not be the one creating it.
    let again = color::Rgba { r: 0.9, g: 0.1, b: 0.1, a: 1.0 };
    let wrote = color::set_constant_colour(
        object.as_reflect_mut(),
        std::slice::from_ref(&target),
        color::Scope::WholeObject,
        again,
        None,
    )
        .expect("the colour is written again");
    assert_eq!(wrote, color::Wrote::Replaced(after.entries.len() - 1));
    let third = GraphicalSet::read(object.as_reflect());
    assert_eq!(third.entries.len(), after.entries.len(), "replaced, not appended");

    // And every entry that was there before is still there, unchanged: the
    // list is rebuilt to change one, so the others have to survive it.
    assert_eq!(third.entries[..before.entries.len()], before.entries[..]);
}

/// Two surfaces given one colour in one gesture, and a scope that is not the
/// object.
///
/// The first two extensions of §13.3, and they are the same write. What makes
/// them worth a test rather than an assertion about a signature: an entry
/// decorating several objects is **one statement** — `TargetObject` is
/// unbounded in the schema — and a writer that emitted one entry per object
/// would be saying something the file cannot tell apart from thirty unrelated
/// decisions. The scope is the other half: colouring the faces is not colouring
/// the object, and both live in one set at once.
#[test]
fn one_entry_can_colour_several_objects_and_a_scope_that_is_not_the_object() {
    let Some((path, _store, before)) = sets().into_iter().next() else {
        eprintln!("skipped: no GraphicalInformationSet in testdata (it is git-ignored)");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let uri = store
        .resources()
        .iter()
        .find(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .map(|summary| summary.uri.clone())
        .expect("the set is there");
    let mut object = store.get_object(&uri).expect("the set reads");

    let targets = [
        color::Decorated {
            uuid: "11111111-1111-4111-8111-111111111111".to_string(),
            qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
            title: "first".to_string(),
        },
        color::Decorated {
            uuid: "22222222-2222-4222-8222-222222222222".to_string(),
            qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
            title: "second".to_string(),
        },
    ];
    let chosen = color::Rgba { r: 0.1, g: 0.7, b: 0.3, a: 1.0 };

    let wrote = color::set_constant_colour(
        object.as_reflect_mut(),
        &targets,
        color::Scope::Faces,
        chosen,
        Some("vert"),
    )
    .expect("the colour is written");
    assert_eq!(wrote, color::Wrote::Added);

    let after = GraphicalSet::read(object.as_reflect());
    let entry = after.entries.last().expect("the entry just written");
    // **One** entry, naming both, in the order given.
    assert_eq!(after.entries.len(), before.entries.len() + 1, "one entry, not two");
    assert_eq!(entry.targets, vec![targets[0].uuid.clone(), targets[1].uuid.clone()]);

    let Information::Default { elements, .. } = &entry.information else {
        panic!("{:?}", entry.information)
    };
    // The scope the writer was given, read back by the reader that is its
    // specification — not by a string comparison on what was written.
    assert!(
        elements.iter().any(|element| element.scope == Scope::Faces),
        "the entry should colour the faces, and it holds {:?}",
        elements.iter().map(|element| element.scope).collect::<Vec<_>>()
    );
    assert!(
        !elements.iter().any(|element| element.scope == Scope::WholeObject),
        "colouring the faces must not also colour the object"
    );

    // A whole-object colour on the same objects is a **second** entry, because
    // it says something else. This is the assertion that fails if the scope is
    // ignored and every write lands on the whole object.
    let wrote = color::set_constant_colour(
        object.as_reflect_mut(),
        &targets,
        color::Scope::WholeObject,
        chosen,
        None,
    )
    .expect("the whole-object colour is written too");
    assert_eq!(wrote, color::Wrote::Added, "a different scope is a different entry");
    assert_eq!(GraphicalSet::read(object.as_reflect()).entries.len(), before.entries.len() + 2);
}

/// Recolouring one object of a shared entry is refused, and used to lose the
/// others.
///
/// Until 04/09 the writer replaced the entry it found with a single-target one:
/// an entry decorating four surfaces, recoloured through one of them, left the
/// other three with no colour at all — from a gesture that named one object, in
/// silence. Refusing loses nothing and says which entry stands in the way.
#[test]
fn recolouring_one_object_of_a_shared_entry_is_refused_rather_than_losing_the_others() {
    let Some((path, _store, _before)) = sets().into_iter().next() else {
        eprintln!("skipped: no GraphicalInformationSet in testdata (it is git-ignored)");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let uri = store
        .resources()
        .iter()
        .find(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .map(|summary| summary.uri.clone())
        .expect("the set is there");
    let mut object = store.get_object(&uri).expect("the set reads");

    let together = [
        color::Decorated {
            uuid: "33333333-3333-4333-8333-333333333333".to_string(),
            qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
            title: "kept".to_string(),
        },
        color::Decorated {
            uuid: "44444444-4444-4444-8444-444444444444".to_string(),
            qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
            title: "recoloured".to_string(),
        },
    ];
    color::set_constant_colour(
        object.as_reflect_mut(),
        &together,
        color::Scope::WholeObject,
        color::Rgba { r: 0.5, g: 0.5, b: 0.5, a: 1.0 },
        None,
    )
    .expect("both are coloured together");
    let shared = GraphicalSet::read(object.as_reflect());

    // Now recolour only the second. The old behaviour replaced the shared entry
    // and the first object silently lost its colour.
    let refusal = color::set_constant_colour(
        object.as_reflect_mut(),
        &together[1..],
        color::Scope::WholeObject,
        color::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
        None,
    )
    .expect_err("it should refuse rather than take the colour from the other object");
    let said = format!("{refusal}");
    assert!(said.contains("shared colour entry"), "{said}");

    // And nothing moved: a refusal that had already rewritten half the set
    // would be worse than the loss it prevents.
    assert_eq!(GraphicalSet::read(object.as_reflect()).entries, shared.entries);
}

/// A colour **map** written into a set, and read back as itself.
///
/// §13.2 point 6. The round trip is the whole test, for the same reason it is
/// for a constant colour: a writer checked against a string of its own making
/// proves that the string is what the writer writes. Checked against the reader
/// — the one this workspace ships against every set of the corpus — it proves
/// that another tool opening this package finds a colour map on these objects.
///
/// The element **order** is what a hand-written subtree gets wrong: the schema
/// declares an `xsd:sequence`, so `MinMax` before the two flags before
/// `ValueVectorIndex` before `ColorMap`, and a document in another order is one
/// no validator accepts. The reader is order-insensitive, so it cannot catch
/// that — which is said here rather than left as a false sense of cover.
#[test]
fn a_colour_map_written_into_a_set_is_read_back_as_itself() {
    let Some((path, _store, before)) = sets().into_iter().next() else {
        eprintln!("skipped: no GraphicalInformationSet in testdata (it is git-ignored)");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let uri = store
        .resources()
        .iter()
        .find(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .map(|summary| summary.uri.clone())
        .expect("the set is there");
    let mut object = store.get_object(&uri).expect("the set reads");

    let targets = [color::Decorated {
        uuid: "55555555-5555-4555-8555-555555555555".to_string(),
        qualified_type: "resqml22.ContinuousProperty".to_string(),
        title: "porosity".to_string(),
    }];
    let map = color::Decorated {
        uuid: "66666666-6666-4666-8666-666666666666".to_string(),
        qualified_type: "resqml22.ContinuousColorMap".to_string(),
        title: "viridis & <friends>".to_string(),
    };
    let scaling = color::Scaling {
        min_max: Some((0.05, 0.31)),
        logarithmic: true,
        reverse: false,
    };

    let wrote = color::set_colour_map(object.as_reflect_mut(), &targets, &map, &scaling, Some(2))
        .expect("the map is written");
    assert_eq!(wrote, color::Wrote::Added);

    let after = GraphicalSet::read(object.as_reflect());
    assert_eq!(after.entries.len(), before.entries.len() + 1, "one entry more, and only one");
    let entry = after.entries.last().expect("the entry just written");
    assert_eq!(entry.targets, vec![targets[0].uuid.clone()]);

    let Information::Color { scaling: read, map: read_map, value_vector_index } = &entry.information
    else {
        panic!("{:?}", entry.information)
    };
    // Every field, because each one is a separate chance to write the wrong
    // element name into a subtree nobody type-checks.
    assert_eq!(read_map.as_deref(), Some(map.uuid.as_str()), "the map it points at");
    assert_eq!(*value_vector_index, Some(2), "which component of the property");
    assert!(read.logarithmic, "the log flag crossed");
    assert!(!read.reverse, "and the reverse flag stayed false");
    let (low, high) = read.min_max.expect("the range");
    assert!((low - 0.05).abs() < 1e-12 && (high - 0.31).abs() < 1e-12, "{low}..{high}");

    // Writing again replaces: a colour map and a second colour map on one object
    // is the ambiguity §7.2 leaves undecided, and this tool must not create it.
    let wrote = color::set_colour_map(
        object.as_reflect_mut(),
        &targets,
        &map,
        &color::Scaling::default(),
        None,
    )
    .expect("written again");
    assert_eq!(wrote, color::Wrote::Replaced(after.entries.len() - 1));
    let third = GraphicalSet::read(object.as_reflect());
    assert_eq!(third.entries.len(), after.entries.len(), "replaced, not appended");
    // And the optionals really went away rather than keeping their old values.
    let Information::Color { scaling: read, value_vector_index, .. } =
        &third.entries.last().expect("the entry").information
    else {
        panic!()
    };
    assert_eq!(read.min_max, None, "an absent range is absent, not the previous one");
    assert_eq!(*value_vector_index, None);

    // Every entry that was there before is still there, unchanged.
    assert_eq!(third.entries[..before.entries.len()], before.entries[..]);
}

/// A constant colour and a colour map on one object are two entries.
///
/// They say different things — *this object is blue* and *this object is
/// painted by that map* — so neither replaces the other. The assertion that
/// would fail if the two writers shared their notion of *the same statement*,
/// which they nearly do: they share everything else.
#[test]
fn a_constant_colour_and_a_colour_map_do_not_replace_each_other() {
    let Some((path, _store, before)) = sets().into_iter().next() else {
        eprintln!("skipped: no GraphicalInformationSet in testdata (it is git-ignored)");
        return;
    };
    let store = EpcStore::open(&path, registry()).expect("the package opens");
    let uri = store
        .resources()
        .iter()
        .find(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .map(|summary| summary.uri.clone())
        .expect("the set is there");
    let mut object = store.get_object(&uri).expect("the set reads");

    let targets = [color::Decorated {
        uuid: "77777777-7777-4777-8777-777777777777".to_string(),
        qualified_type: "resqml22.ContinuousProperty".to_string(),
        title: "both".to_string(),
    }];
    let map = color::Decorated {
        uuid: "88888888-8888-4888-8888-888888888888".to_string(),
        qualified_type: "resqml22.ContinuousColorMap".to_string(),
        title: "a map".to_string(),
    };

    color::set_constant_colour(
        object.as_reflect_mut(),
        &targets,
        color::Scope::WholeObject,
        color::Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 },
        None,
    )
    .expect("the constant colour is written");
    color::set_colour_map(
        object.as_reflect_mut(),
        &targets,
        &map,
        &color::Scaling::default(),
        None,
    )
    .expect("the map is written too");

    let after = GraphicalSet::read(object.as_reflect());
    assert_eq!(after.entries.len(), before.entries.len() + 2, "two entries, not one replaced");
}

/// One step up from a representation is not always an interpretation.
///
/// RESQML 2.2's business rule on `RepresentedObject`, in the schema's own
/// words: *"The data object represented by the representation is either an
/// interpretation or a technical feature."* So the chain
/// representation → interpretation → feature has a short form, and naming the
/// level by counting steps calls a feature "the interpretation".
///
/// The colour is the same either way — which is exactly why this needed a test
/// rather than being noticed. What is wrong is the provenance, and provenance
/// is the half of the answer a user reads to decide where to change it.
#[test]
fn a_representation_that_names_a_feature_directly_says_feature() {
    use energyml_core::registry::DynObject;
    use energyml_domain::source::ObjectSource;

    const REPRESENTATION: &str = "aaaaaaaa-0000-0000-0000-000000000001";
    const PARENT: &str = "aaaaaaaa-0000-0000-0000-000000000002";

    /// A store holding one representation, whose `RepresentedObject` declares
    /// whatever type the case under test needs.
    struct One(&'static str);
    impl ObjectSource for One {
        fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
            if uuid != REPRESENTATION {
                return None;
            }
            Some(Box::new(energyml_resqml_2_2::TriangulatedSetRepresentation {
                uuid: REPRESENTATION.to_string(),
                represented_object: Some(energyml_eml_2_3::DataObjectReference {
                    uuid: PARENT.to_string(),
                    qualified_type: self.0.to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }))
        }
    }

    // An empty set with one colour written onto the parent, whatever it is.
    let mut set = energyml_eml_2_3::GraphicalInformationSet::default();
    color::set_constant_colour(
        set.as_reflect_mut(),
        &[color::Decorated {
            uuid: PARENT.to_string(),
            qualified_type: "resqml22.SeismicLatticeFeature".to_string(),
            title: "the parent".to_string(),
        }],
        Scope::WholeObject,
        color::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
        None,
    )
    .expect("the colour is written");
    let graphical = GraphicalSet::read(set.as_reflect());
    let cascade = Cascade::new(&graphical);

    let level_with = |declared: &'static str| {
        cascade
            .resolve(
                &Request {
                    representation: REPRESENTATION,
                    property: None,
                    kind_chain: &[],
                    element: Scope::WholeObject,
                },
                &One(declared),
            )
            .expect("the cascade runs")
            .expect("the parent carries a colour")
            .level
    };

    assert_eq!(
        level_with("resqml22.SeismicLatticeFeature"),
        Level::Feature,
        "a technical feature named directly is a feature, not an interpretation"
    );
    // The ordinary shape still reads as before, so the fix is a distinction and
    // not a relabelling of everything.
    assert_eq!(level_with("resqml22.FaultInterpretation"), Level::Interpretation);
}

/// The whole ladder, and the winner is its first carrying rung.
///
/// `Cascade::chain` exists so a person can **choose** a fallback instead of
/// having one inferred. It walks separately from `resolve`,
/// because `resolve` stops at the first answer and that early stop is what keeps
/// a scene from opening every parent of every object. Two walks that must agree
/// and nothing forcing them to is the shape this repository distrusts, so the
/// agreement is asserted rather than assumed — over the corpus, where the ladders
/// are real.
#[test]
fn the_chain_is_the_ladder_and_resolve_takes_its_first_carrying_rung() {
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    let mut with_a_silent_rung = 0usize;
    let mut compared = 0usize;

    for (_, store, set) in &sets {
        let cascade = Cascade::new(set);
        for summary in store.resources() {
            if !summary.qualified_type.contains("Representation") {
                continue;
            }
            let request = Request {
                representation: &summary.uuid,
                property: None,
                kind_chain: &[],
                element: Scope::WholeObject,
            };
            let chain = cascade.chain(&request, store).expect("the chain walks");
            let decided = cascade.resolve(&request, store).expect("the cascade runs");

            // The first rung that carries something is what `resolve` returns —
            // and when none carries, `resolve` says nothing.
            assert_eq!(
                chain.iter().find_map(|step| step.carries.clone()),
                decided,
                "{}: the two walks disagree", summary.uuid
            );
            // The object itself is always the first rung of the object part, so
            // a chain is never empty: an object with nothing above it still has
            // itself, and a caller must be able to show that.
            assert!(!chain.is_empty(), "{}: an empty ladder", summary.uuid);
            assert_eq!(chain[0].uuid.to_ascii_lowercase(), summary.uuid.to_ascii_lowercase());
            assert_eq!(chain[0].level, Level::Representation);

            if chain.iter().any(|step| step.carries.is_none()) {
                with_a_silent_rung += 1;
            }
            compared += 1;
        }
    }

    assert!(compared > 0, "no representation was compared");
    // The rung that carries nothing is the whole point of returning the list —
    // "this parent exists and says nothing" is what a person needs to see. If
    // the corpus never produced one, this test would be proving only the easy
    // half.
    assert!(
        with_a_silent_rung > 0,
        "no ladder has a silent rung, so nothing here exercises the case the list exists for"
    );
}

/// Inheritance off leaves exactly one rung, and it is the object itself.
///
/// The switch has to mean the same thing in both walks; a `chain` that kept
/// climbing would offer parents as choices that `resolve` would refuse to use.
#[test]
fn switching_inheritance_off_shortens_the_ladder_to_the_object() {
    let sets = sets();
    if sets.is_empty() {
        eprintln!("skipped: no package with a GraphicalInformationSet");
        return;
    }
    let (_, store, set) = &sets[0];
    let mut cascade = Cascade::new(set);
    cascade.inherit = false;

    let mut seen = 0usize;
    for summary in store.resources() {
        if !summary.qualified_type.contains("Representation") {
            continue;
        }
        let chain = cascade
            .chain(
                &Request {
                    representation: &summary.uuid,
                    property: None,
                    kind_chain: &[],
                    element: Scope::WholeObject,
                },
                store,
            )
            .expect("the chain walks");
        assert_eq!(chain.len(), 1, "{}: {chain:?}", summary.uuid);
        seen += 1;
    }
    assert!(seen > 0, "no representation in the package");
}

// -- Flat bands over a continuous map --------------------------------------

/// A colour named by its hue, so a test can tell two apart at a glance.
fn hue(degrees: f64) -> color::Hsv {
    color::Hsv {
        hue: degrees,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        title: None,
    }
}

/// The map `banded` is meant to produce, wrapped so `sample` can be asked.
fn banded_map(levels: &[f64], colours: &[color::Hsv]) -> color::ColorMap {
    color::ColorMap {
        uuid: "00000000-0000-4000-8000-000000000001".to_string(),
        title: Some("bands".to_string()),
        continuous: Some((color::Domain::Rgb, color::Interpolation::Linear)),
        stops: color::banded(levels, colours).expect("the levels and colours line up"),
        null_color: None,
        above_max: None,
        below_min: None,
    }
}

/// **The point of the whole thing, measured on the picture rather than on the
/// stops**: inside a band the colour does not move, and at the level it jumps.
///
/// A `ContinuousColorMap` interpolates, so this is exactly what a single stop
/// per level would fail to do — which is what makes this test able to fail.
#[test]
fn a_banded_map_paints_flat_colour_between_its_levels() {
    let levels = [0.0, 25.0, 50.0, 75.0];
    let colours = [hue(0.0), hue(90.0), hue(180.0), hue(270.0)];
    let map = banded_map(&levels, &colours);
    let plain = color::Scaling::default();

    // Right across the second band, 25 to 50, the colour is one colour.
    let at = |value: f64| map.sample(Some(value), &plain).expect("a colour");
    let start = at(25.0);
    for value in [25.5, 30.0, 37.5, 44.0, 49.0] {
        assert_eq!(at(value), start, "the band is not flat at {value}");
    }
    // And it is a different colour from its neighbours.
    assert_ne!(at(10.0), start, "the first band bleeds into the second");
    assert_ne!(at(60.0), start, "the third band bleeds into the second");
}

#[test]
fn the_jump_happens_at_the_level_and_nowhere_else() {
    let levels = [0.0, 25.0, 50.0];
    let colours = [hue(0.0), hue(120.0), hue(240.0)];
    let map = banded_map(&levels, &colours);
    let plain = color::Scaling::default();
    let at = |value: f64| map.sample(Some(value), &plain).expect("a colour");

    // Just under the level is still the low band; at the level, the high one.
    // The closing stop sits a millionth of the band short, so "just under"
    // has to be further than that to be inside the flat part.
    assert_eq!(at(24.9), at(1.0), "below the level is the first band");
    assert_eq!(at(25.0), at(40.0), "at the level is the second band");
    assert_ne!(at(24.9), at(25.0), "there is a jump, and it is at 25");
}

#[test]
fn a_band_is_written_as_a_pair_of_stops_and_the_last_one_is_open() {
    let levels = [0.0, 25.0, 50.0];
    let stops = color::banded(&levels, &[hue(0.0), hue(120.0), hue(240.0)]).expect("no refusal");
    // Two stops per band, except the last level, which has nothing to close.
    assert_eq!(stops.len(), 5, "{stops:?}");
    assert_eq!(stops[0].index, 0.0);
    assert!(stops[1].index < 25.0 && stops[1].index > 24.99, "{}", stops[1].index);
    assert_eq!(stops[1].color, stops[0].color, "a band's pair shares its colour");
    assert_eq!(stops[2].index, 25.0);
    assert_eq!(stops[4].index, 50.0, "the last level runs upwards on its own");
    // Strictly ascending: two stops at one index is not a map.
    for at in 1..stops.len() {
        assert!(stops[at].index > stops[at - 1].index, "{stops:?}");
    }
}

#[test]
fn the_last_band_closes_where_the_values_end() {
    // Reported: the last band had no width, so no legend showed it.
    let colours = [hue(0.0), hue(120.0), hue(240.0)];
    let stops = color::banded_until(&[0.0, 25.0, 50.0], &colours, 80.0).expect("no refusal");
    assert_eq!(stops.len(), 6, "every band a pair: {stops:?}");
    assert_eq!(stops[5].index, 80.0);
    assert_eq!(stops[5].color, stops[4].color, "the last band is one colour over its width");
    // A top at or below the last level closes nothing: the same as `banded`.
    assert_eq!(color::banded_until(&[0.0, 25.0, 50.0], &colours, 50.0).expect("no refusal").len(), 5);
}

#[test]
fn banding_refuses_by_name() {
    assert!(color::banded(&[0.0, 1.0], &[hue(0.0)]).is_err(), "one colour for two levels");
    assert!(color::banded(&[], &[]).is_err(), "no level at all");
    assert!(
        color::banded(&[0.0, f64::NAN], &[hue(0.0), hue(1.0)]).is_err(),
        "a level that is not a number"
    );
    assert!(
        color::banded(&[10.0, 5.0], &[hue(0.0), hue(1.0)]).is_err(),
        "levels that do not ascend"
    );
    assert!(
        color::banded(&[5.0, 5.0], &[hue(0.0), hue(1.0)]).is_err(),
        "two levels at the same place"
    );
}

/// **Several sets of a package read as one** (`GraphicalSet::merged`, 28/09):
/// in package order, positions renumbered end to end, and at one level the
/// **last** entry written answers — as within one set. Until then the front end
/// read the first set only, and on Lutetia the chart's colours hid behind the
/// strata table's set: an object colour in the second set only was simply not
/// found.
#[test]
fn several_sets_read_as_one_in_package_order() {
    let entry = |position: usize, target: &str, name: &str| color::Entry {
        position,
        targets: vec![target.to_string()],
        target_uris: Vec::new(),
        map_uri: None,
        information: Information::Other { type_name: name.to_string() },
    };
    let first = GraphicalSet { entries: vec![entry(0, "a", "first a"), entry(1, "b", "first b")], notes: vec!["first note".into()] };
    let second = GraphicalSet { entries: vec![entry(0, "b", "second b"), entry(1, "c", "second c")], notes: vec!["second note".into()] };
    let merged = GraphicalSet::merged([first, second]);

    let positions: Vec<usize> = merged.entries.iter().map(|one| one.position).collect();
    assert_eq!(positions, [0, 1, 2, 3], "one wiring, end to end");
    assert_eq!(merged.at(3).map(|one| one.targets.clone()), Some(vec!["c".to_string()]));
    // Only in the second set: found — the defect was that it was not.
    assert_eq!(merged.targeting("c").len(), 1);
    // In both: the later statement first, as `targeting` orders one set.
    let named = |one: &color::Entry| match &one.information {
        Information::Other { type_name } => type_name.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(merged.targeting("b").iter().map(|one| named(one)).collect::<Vec<_>>(), ["second b", "first b"]);
    assert_eq!(merged.notes, ["first note", "second note"]);
}
