//! Confronting the readers with each other over the reference corpus.
//!
//! # What this suite is looking for
//!
//! Not "does it crash" - the rest of the suite covers that. This one hunts one
//! specific shape of bug, the shape found on 02/08 twice in one afternoon:
//!
//! > **two readers agreeing on a total and disagreeing on what it means.**
//!
//! A faulted parametric grid had exactly the right number of points, and every
//! reader that counted them was satisfied; the one that indexed them was
//! looking at a different array. A 2D point array had exactly the number of
//! doubles it declared, and produced two thirds as many points, each built from
//! parts of two. Neither failed. Neither was caught by a test that asked a
//! single reader whether it was happy.
//!
//! The defence is **triangulation**: never ask one component to check itself.
//! Every check here confronts at least two independent sources of the same
//! fact, at least one of which is not code this workspace wrote - the bytes in
//! the HDF5 file, the part names in the zip, the counts the XML declares.
//!
//! # The confrontations
//!
//! | | first source | second source |
//! |---|---|---|
//! | [`array_shapes`] | the array's declared type | the dataset's real shape in HDF5 |
//! | [`point_counts`] | `points::count`, without evaluating | the points actually evaluated |
//! | [`tolerance_ladder`] | what strict refuses | what tolerant and lenient refuse |
//! | [`export_completeness`] | representations the scene supports | written + skipped + refused |
//! | [`identity_triangle`] | the uuid in the part name | the uuid in the XML, and in the inventory |
//! | [`borrowed_geometry_sits_on_its_lender`] | where the geometry reader put the points | the system the CRS lookup resolved |

#[cfg(feature = "hdf5")]
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_core::registry::Registry;
#[cfg(feature = "hdf5")]
use energyml_core::value::Reflect;
#[cfg(feature = "hdf5")]
use energyml_domain::source::ArraySource;
#[cfg(feature = "hdf5")]
use energyml_geometry::parametric::Vertical;
#[cfg(feature = "hdf5")]
use energyml_geometry::{geometry, points};
use energyml_storage::EpcStore;

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The public reference packages; see `fixtures.rs`.
fn corpus() -> Vec<PathBuf> {
    crate::fixtures::public_corpus()
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Everything one confrontation found, reported together rather than on the
/// first failure.
///
/// A cross-check that stops at the first disagreement tells you one thing about
/// one object. The same check run to the end tells you whether it is one file's
/// quirk or a reader that is wrong everywhere, and those call for different
/// fixes.
#[derive(Default)]
struct Findings {
    checked: usize,
    problems: Vec<String>,
}

impl Findings {
    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.checked += 1;
        if !ok {
            self.problems.push(what());
        }
    }

    fn finish(self, what: &str, least: usize) {
        eprintln!("{what}: {} confrontation(s)", self.checked);
        assert!(
            self.checked >= least,
            "{what} only made {} confrontation(s); it is not covering the corpus and \
             would pass over a broken reader",
            self.checked
        );
        assert!(
            self.problems.is_empty(),
            "{what}: {} disagreement(s)\n  {}",
            self.problems.len(),
            self.problems.join("\n  ")
        );
    }
}

/// **The file against the type that claims to describe it.**
///
/// The check that would have caught the 2D point array from a direction no
/// amount of internal consistency could: HDF5 stores the dataset's real shape,
/// and `(6, 2)` says two columns whatever the reader divides by.
///
/// The rule is narrow on purpose. A trailing dimension is only meaningful when
/// the schema fixes it - `Point3d…Array` is 3 wide, `Point2d…Array` is 2 -
/// and the check says nothing about the arrays whose width is a property count
/// or a patch length, because there it has no independent expectation to
/// compare with and an assertion would just be the reader agreeing with itself.
#[test]
#[cfg(feature = "hdf5")]
fn array_shapes() {
    let mut findings = Findings::default();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();

    for path in corpus() {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for (at, array) in point_arrays(object.as_reflect()) {
                let name = array.type_info().name;
                let Some(width) = declared_width(name) else {
                    continue;
                };
                *seen.entry(name.to_string()).or_default() += 1;

                for location in store.locate(array) {
                    let Ok(meta) = store.metadata(&location) else {
                        continue;
                    };
                    let Some(&last) = meta.dimensions.last() else {
                        continue;
                    };
                    // A flat dataset says nothing about the width - the file is
                    // entitled to store 3n doubles in one dimension - so only a
                    // *shaped* one is evidence.
                    if meta.dimensions.len() < 2 {
                        continue;
                    }
                    findings.check(last == width as i64, || {
                        format!(
                            "{file}: {} {at} is a `{name}` ({width} coordinates per point) \
                             over a dataset shaped {:?}",
                            summary.uuid, meta.dimensions
                        )
                    });

                    // And the reader, against the same shape. The check above
                    // confronts the file with the type that names it, which is
                    // worth having but says nothing about what the reader did
                    // with either - reverting the divisor bug of 02/08 would
                    // leave it green. This is the half that goes red: the
                    // dataset holds `product(dimensions)` numbers, the type
                    // fixes how many make a point, so the answer is arithmetic
                    // and the reader has no say in it.
                    let numbers: i64 = meta.dimensions.iter().product();
                    let expected = (numbers / width as i64).max(0) as usize;
                    let where_ = format!("{}.{at}", summary.uuid);
                    if let Ok(evaluated) = points::read(array, &store, &where_, Vertical::Down) {
                        findings.check(evaluated.len() == expected, || {
                            format!(
                                "{file}: {where_} is a `{name}` over a dataset shaped {:?} \
                                 - {expected} point(s) - and read {} of them",
                                meta.dimensions,
                                evaluated.len()
                            )
                        });
                    }
                }
            }
        }
    }

    eprintln!("point array types met: {seen:?}");
    assert!(
        seen.keys().any(|name| name.starts_with("Point2d")),
        "no 2D point array was reached; the check that found the bug would not have"
    );
    findings.finish("array shapes", 20);
}

/// How many coordinates the schema fixes for a point array type, if it fixes
/// one.
#[cfg(feature = "hdf5")]
fn declared_width(name: &str) -> Option<usize> {
    if !name.ends_with("ExternalArray") && !name.ends_with("Hdf5Array") {
        return None;
    }
    if name.starts_with("Point3d") {
        Some(3)
    } else if name.starts_with("Point2d") {
        Some(2)
    } else {
        None
    }
}

/// Every `Point…Array` under an object, with the path that reached it.
#[cfg(feature = "hdf5")]
fn point_arrays(object: &dyn Reflect) -> Vec<(String, &dyn Reflect)> {
    let mut found = Vec::new();
    walk_arrays(object, String::new(), &mut found);
    found
}

#[cfg(feature = "hdf5")]
fn walk_arrays<'a>(object: &'a dyn Reflect, at: String, out: &mut Vec<(String, &'a dyn Reflect)>) {
    if object.type_info().name.starts_with("Point") && object.type_info().name.contains("Array") {
        out.push((at.clone(), object));
    }
    for field in object.fields() {
        let path = if at.is_empty() {
            field.name.to_string()
        } else {
            format!("{at}.{}", field.name)
        };
        match object.field(field.name) {
            Some(energyml_core::value::FieldValue::Object(child)) => walk_arrays(child, path, out),
            Some(energyml_core::value::FieldValue::List(items)) => {
                for (index, item) in items.iter().enumerate() {
                    if let energyml_core::value::FieldValue::Object(child) = item {
                        walk_arrays(*child, format!("{path}[{index}]"), out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// **What an array says it holds, against what came out of it.**
///
/// `points::count` answers without evaluating - it is what a caller asks before
/// deciding whether to load a million points - and `points::read` answers by
/// doing the work. They are two code paths over the same array, and the whole
/// value of the cheap one is that it agrees with the expensive one.
///
/// This generalises a check that existed for IJK grids alone. The divisor bug
/// found on 02/08 lived in *both* paths at once, so this would not have caught
/// it - which is exactly why [`array_shapes`] asks the file instead. Kept
/// because the two drifting apart is a different bug, and a likelier one: they
/// are edited separately.
#[test]
#[cfg(feature = "hdf5")]
fn point_counts() {
    let mut findings = Findings::default();

    for path in corpus() {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for (at, array) in point_arrays(object.as_reflect()) {
                let where_ = format!("{}.{at}", summary.uuid);
                let (Ok(declared), Ok(evaluated)) = (
                    points::count(array, &store, &where_),
                    points::read(array, &store, &where_, Vertical::Down),
                ) else {
                    continue;
                };
                findings.check(declared == evaluated.len(), || {
                    format!(
                        "{file}: {where_} counts {declared} point(s) without reading and \
                         yields {} when read",
                        evaluated.len()
                    )
                });
            }
        }
    }

    findings.finish("point counts", 20);
}

/// **The three reading modes, as a ladder.**
///
/// Strict refuses the most, lenient the least, tolerant sits between them. That
/// ordering is the whole claim the modes make, and nothing was checking it: a
/// mode that refuses something a *stricter* mode accepted is a contradiction,
/// and the kind that hides for a long time because each mode is only ever
/// exercised on its own.
///
/// # Why this cannot be run on the corpus as it is
///
/// It was, first, and it passed while proving nothing: the reference packages
/// are conformant, so all three modes refuse **zero** parts and every subset
/// check holds trivially. A ladder whose three rungs are empty says nothing
/// about the order of its rungs.
///
/// So the package is **derived**: one copy of a real one with some `Originator`
/// elements removed - which strict refuses and tolerant reads past - and one
/// numeric value corrupted, which tolerant refuses and lenient repairs. Now the
/// three rungs are three different heights, the ordering is a real claim, and
/// the test asserts they are *distinct* as well as ordered. Equal sets would
/// mean the derivation stopped working and the check went quietly back to
/// proving nothing.
#[test]
#[cfg(feature = "hdf5")]
fn tolerance_ladder() {
    let Some(path) = damaged("testingPackageCpp22.epc") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };

    let refused = |strict: bool, lenient: bool| -> BTreeSet<String> {
        let mut store = EpcStore::open(&path, registry()).expect("the package opens");
        store.set_strict(strict);
        store.set_lenient(lenient);
        store
            .resources()
            .iter()
            .filter(|s| store.get_object(&s.uri).is_err())
            .map(|s| s.uuid.clone())
            .collect()
    };

    let strict = refused(true, false);
    let tolerant = refused(false, false);
    let lenient = refused(false, true);
    eprintln!(
        "damaged package: refused - strict {}, tolerant {}, lenient {}",
        strict.len(),
        tolerant.len(),
        lenient.len()
    );

    let mut findings = Findings::default();
    findings.check(tolerant.is_subset(&strict), || {
        format!(
            "tolerant refuses {:?} that strict accepted",
            tolerant.difference(&strict).take(3).collect::<Vec<_>>()
        )
    });
    findings.check(lenient.is_subset(&tolerant), || {
        format!(
            "lenient refuses {:?} that tolerant accepted",
            lenient.difference(&tolerant).take(3).collect::<Vec<_>>()
        )
    });
    // Ordered *and* distinct. Without this the check passes on three empty
    // sets, which is how it passed before the package was damaged on purpose.
    findings.check(strict.len() > tolerant.len(), || {
        format!(
            "strict and tolerant refuse the same {} part(s); the absent elements \
             are not reaching the reader and the ladder is untested",
            strict.len()
        )
    });
    findings.check(tolerant.len() > lenient.len(), || {
        format!(
            "tolerant and lenient refuse the same {} part(s); the corrupted value \
             is not reaching the reader and the ladder is untested",
            tolerant.len()
        )
    });
    findings.check(lenient.is_empty(), || {
        format!(
            "lenient still refuses {:?}",
            lenient.iter().take(3).collect::<Vec<_>>()
        )
    });

    // And the corpus itself, where all three should refuse nothing at all.
    for path in corpus() {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry()).expect("the package opens");
        let refused: Vec<String> = store
            .resources()
            .iter()
            .filter(|s| store.get_object(&s.uri).is_err())
            .map(|s| s.uuid.clone())
            .collect();
        findings.check(refused.is_empty(), || {
            format!(
                "{file}: {} part(s) refused by the default mode: {refused:?}",
                refused.len()
            )
        });
    }

    findings.finish("tolerance ladder", 8);
}

/// A copy of a corpus package with two deliberate injuries: some `Originator`
/// elements removed, and one dimension replaced by a word.
///
/// Both are things producers really do - the first is the client package that
/// prompted the 02/08 change, the second is what a hand-rolled writer emits
/// when a field is empty - and they are the two rungs the ladder is made of.
#[cfg(feature = "hdf5")]
fn damaged(name: &str) -> Option<PathBuf> {
    let source = crate::fixtures::find(name)?;
    let out = std::env::temp_dir().join(format!("energyml-audit-{name}"));

    let bytes = std::fs::read(&source).ok()?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let file = std::fs::File::create(&out).ok()?;
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let (mut absent, mut corrupted) = (0usize, 0usize);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).ok()?;
        let entry_name = entry.name().ok()?.into_owned();
        let mut content = Vec::new();
        std::io::copy(&mut entry, &mut content).ok()?;
        if entry_name.ends_with(".xml")
            && let Ok(text) = String::from_utf8(content.clone())
        {
            let mut text = text;
            if text.contains("<eml:Originator>") {
                let mut rebuilt = String::with_capacity(text.len());
                let mut rest = text.as_str();
                while let Some(at) = rest.find("<eml:Originator>") {
                    let Some(close) = rest[at..].find("</eml:Originator>") else {
                        break;
                    };
                    rebuilt.push_str(&rest[..at]);
                    rest = &rest[at + close + "</eml:Originator>".len()..];
                    absent += 1;
                }
                rebuilt.push_str(rest);
                text = rebuilt;
            }
            if text.contains("<resqml:Ni>2</resqml:Ni>") {
                text = text.replace("<resqml:Ni>2</resqml:Ni>", "<resqml:Ni>deux</resqml:Ni>");
                corrupted += 1;
            }
            content = text.into_bytes();
        }
        writer.start_file(entry_name, options).ok()?;
        std::io::Write::write_all(&mut writer, &content).ok()?;
    }
    writer.finish().ok()?;

    assert!(
        absent > 0 && corrupted > 0,
        "{name}: {absent} removed, {corrupted} corrupted"
    );
    Some(out)
}

/// **Every representation is accounted for.**
///
/// An export that writes 30 files out of 33 objects and says "30 written" has
/// told the truth and hidden the question. The three fates - written, refused
/// with a reason, skipped for a stated reason - have to add up to the number of
/// representations the geometry layer says it supports, or something is being
/// dropped between the two.
///
/// The failure this guards against is the one the whole workspace is arranged
/// against: a partial result that looks complete.
#[test]
#[cfg(feature = "hdf5")]
fn export_completeness() {
    let mut findings = Findings::default();

    for path in corpus() {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry()).expect("the package opens");

        let mut supported = 0usize;
        let mut evaluated = 0usize;
        let mut refused = 0usize;
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            if !geometry::is_supported(object.as_reflect()) {
                continue;
            }
            supported += 1;
            match geometry::of(object.as_reflect(), &store) {
                Ok(found) => {
                    evaluated += 1;
                    // A representation that evaluates to nothing at all is the
                    // silent case: no error, no geometry, and a viewer showing
                    // an empty scene with no reason given.
                    findings.check(found.point_count() > 0, || {
                        format!(
                            "{file}: {} evaluated without error and yielded no points",
                            summary.uuid
                        )
                    });
                }
                Err(_) => refused += 1,
            }
        }

        findings.check(evaluated + refused == supported, || {
            format!("{file}: {supported} supported, {evaluated} evaluated, {refused} refused")
        });
        eprintln!("{file}: {supported} supported - {evaluated} evaluated, {refused} refused");
    }

    findings.finish("export completeness", 10);
}

/// **The uuid, from three places that are written independently.**
///
/// A part is named `obj_Type_<uuid>.xml` by the producer's packaging code, the
/// same uuid is written inside the XML by its serialisation code, and the
/// inventory the store builds is a third derivation. They are three chances to
/// disagree, and a disagreement means every lookup by uuid reaches a different
/// object than a lookup by part name.
///
/// The corpus is where this is worth asking: a synthetic package is written by
/// one code path and cannot disagree with itself.
#[test]
fn identity_triangle() {
    // The corpus is gitignored - it re-downloads, it does not version - so it is
    // simply absent on a runner. Skipping is not the same as passing: the
    // assertion below still refuses an empty run *when the files are there*,
    // which is the case it exists for. Without this the test fails on CI for a
    // reason that says nothing about the code.
    let files = corpus();
    if files.is_empty() {
        eprintln!("skipped: the corpus is not present");
        return;
    }
    let mut findings = Findings::default();

    for path in files {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry()).expect("the package opens");
        for summary in store.resources() {
            // The part name, when it carries a uuid at all - some producers
            // name parts without one, which is legal and simply says nothing.
            let Some(in_name) = uuid_in(&summary.locator) else {
                continue;
            };
            findings.check(in_name.eq_ignore_ascii_case(&summary.uuid), || {
                format!(
                    "{file}: part `{}` names {in_name}, inventoried as {}",
                    summary.locator, summary.uuid
                )
            });

            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let in_xml = energyml_core::identity::uuid_of_reflect(object.as_reflect())
                .unwrap_or_default()
                .to_string();
            findings.check(in_xml.eq_ignore_ascii_case(&summary.uuid), || {
                format!(
                    "{file}: part `{}` holds uuid {in_xml}, inventoried as {}",
                    summary.locator, summary.uuid
                )
            });
        }
    }

    findings.finish("identity triangle", 100);
}

/// The first uuid-shaped run in a part name.
fn uuid_in(name: &str) -> Option<String> {
    let bytes: Vec<char> = name.chars().collect();
    for start in 0..bytes.len().saturating_sub(35) {
        let candidate: String = bytes[start..start + 36].iter().collect();
        let shaped = candidate.chars().enumerate().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
        if shaped {
            return Some(candidate);
        }
    }
    None
}

/// **The writer against the reader, value by value.**
///
/// The round-trip check that already exists compares **element counts**: it
/// reads a part, writes it, and confirms the same tags appear the same number
/// of times. That is a real check and it catches real losses - but it is
/// precisely the shape this suite distrusts. Two elements whose *values* have
/// swapped, a number written into the wrong sibling, a list reordered: every
/// one of those keeps the counts identical.
///
/// So this one walks the two object trees in parallel through reflection and
/// compares what is *in* them - every scalar, at every path, in order. Read,
/// write, read again, and the second object has to equal the first.
///
/// Read back **strictly**: an object that only round-trips because the second
/// read filled in what the writer dropped has not round-tripped.
#[test]
#[cfg(feature = "hdf5")]
fn written_objects_read_back_the_same() {
    let mut findings = Findings::default();
    let registry = registry();

    for path in corpus() {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry.clone()).expect("the package opens");
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let Ok(written) = energyml_core::registry::to_part(
                object.as_ref(),
                energyml_core::xml::XmlWriteOptions::COMPACT,
            ) else {
                continue;
            };
            let Some(entry) = registry.entry_of(object.as_reflect()) else {
                continue;
            };
            let Some(content_type) = entry.info.content_type else {
                continue;
            };

            match registry.read_xml_as(
                content_type,
                written.as_bytes(),
                energyml_core::registry::ReadMode::Strict,
            ) {
                Ok((again, _)) => {
                    let mut differences = Vec::new();
                    compare(object.as_reflect(), again.as_reflect(), "", &mut differences);
                    findings.check(differences.is_empty(), || {
                        format!(
                            "{file}: {} differs after write and read: {}",
                            summary.uuid,
                            differences.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
                        )
                    });
                }
                Err(e) => findings.check(false, || {
                    format!(
                        "{file}: {} was written and would not read back: {e}",
                        summary.uuid
                    )
                }),
            }
        }
    }

    // A comparison that reports nothing because it looks at nothing would pass
    // this test on every object in the corpus. So it is shown two objects that
    // really do differ, and has to say so.
    let mut proof = Vec::new();
    let one = energyml_resqml_2_2::structural::TriangulatedSetRepresentation {
        citation: energyml_eml_2_3::Citation {
            title: "one".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let other = energyml_resqml_2_2::structural::TriangulatedSetRepresentation {
        citation: energyml_eml_2_3::Citation {
            title: "other".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    compare(&one, &other, "", &mut proof);
    assert_eq!(
        proof,
        vec!["citation.title: `one` became `other`".to_string()],
        "the comparison does not see a difference it is shown; every green above means nothing"
    );

    findings.finish("write and read back", 500);
}

/// Walk two reflection trees together, collecting every path where they differ.
///
/// Depth-limited rather than trusting the data: a cyclic object graph is not
/// supposed to exist inside one part, and an audit that hangs is worse than one
/// that stops short and says so.
#[cfg(feature = "hdf5")]
fn compare(left: &dyn Reflect, right: &dyn Reflect, at: &str, out: &mut Vec<String>) {
    if at.matches('.').count() > 32 {
        out.push(format!("{at}: deeper than this audit walks"));
        return;
    }
    if left.type_info().name != right.type_info().name {
        out.push(format!(
            "{at}: `{}` became `{}`",
            left.type_info().name,
            right.type_info().name
        ));
        return;
    }
    for field in left.type_info().fields {
        let path = if at.is_empty() {
            field.name.to_string()
        } else {
            format!("{at}.{}", field.name)
        };
        compare_value(left.field(field.name), right.field(field.name), &path, out);
    }
}

#[cfg(feature = "hdf5")]
fn compare_value(
    left: Option<energyml_core::value::FieldValue<'_>>,
    right: Option<energyml_core::value::FieldValue<'_>>,
    at: &str,
    out: &mut Vec<String>,
) {
    use energyml_core::value::FieldValue as V;
    match (left, right) {
        (None | Some(V::Null), None | Some(V::Null)) => {}
        (Some(V::Scalar(a)), Some(V::Scalar(b))) => {
            let (a, b) = (a.to_string(), b.to_string());
            if a != b {
                out.push(format!("{at}: `{a}` became `{b}`"));
            }
        }
        (Some(V::Object(a)), Some(V::Object(b))) => compare(a, b, at, out),
        (Some(V::List(a)), Some(V::List(b))) => {
            if a.len() != b.len() {
                out.push(format!("{at}: {} item(s) became {}", a.len(), b.len()));
                return;
            }
            for (index, (a, b)) in a.into_iter().zip(b).enumerate() {
                compare_value(Some(a), Some(b), &format!("{at}[{index}]"), out);
            }
        }
        (a, b) => out.push(format!(
            "{at}: {} became {}",
            shape_of(a.as_ref()),
            shape_of(b.as_ref())
        )),
    }
}

#[cfg(feature = "hdf5")]
fn shape_of(value: Option<&energyml_core::value::FieldValue<'_>>) -> &'static str {
    use energyml_core::value::FieldValue as V;
    match value {
        None | Some(V::Null) => "absent",
        Some(V::Scalar(_)) => "a value",
        Some(V::Object(_)) => "an object",
        Some(V::List(_)) => "a list",
    }
}

/// **Borrowed geometry, borrowed system** - and the two are resolved by
/// different walks, so they are confronted here.
///
/// # The bug this was written for
///
/// `testingPackageCpp22` has a `WellboreFrameRepresentation` whose trajectory
/// carries a `Geometry`. The frame's nodes are that parametric line evaluated
/// at the frame's own measured depths, so they land in the trajectory's local
/// CRS - `EPSG:23031`, counting **downward**. The geometry reader followed the
/// `Trajectory` reference to compute them.
///
/// The CRS lookup did not. It looked for a system on the frame, found none, and
/// answered `None` - which is a legal answer, given plenty of objects name no
/// system. So `to_projected` ran on the trajectory and not on its own frame:
/// the well descended to −1000 and its log points climbed to +600. Nothing
/// errored, no count disagreed, and every individual number looked reasonable.
/// Only the picture was wrong, and only if you knew which way is down.
///
/// # Why a list drives it
///
/// The check iterates [`BORROWED_GEOMETRY_FIELDS`], the same constant the
/// resolver reads. A representation that starts borrowing geometry costs one
/// string there and is covered here for nothing - which is the point, because
/// the next such object will be added by someone who has never read this file.
#[test]
#[cfg(feature = "hdf5")]
fn borrowed_geometry_sits_on_its_lender() {
    use energyml_domain::crs::BORROWED_GEOMETRY_FIELDS;

    type Point = [f64; 3];

    fn box_of(points: &[Point]) -> Option<(Point, Point)> {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut seen = false;
        for point in points {
            for axis in 0..3 {
                if !point[axis].is_finite() {
                    continue;
                }
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
                seen = true;
            }
        }
        seen.then_some((min, max))
    }

    let mut findings = Findings::default();

    for path in corpus() {
        let file = name_of(&path);
        let store = EpcStore::open(&path, registry()).expect("the package opens");

        // Projected points and the system they came from, for one object.
        let placed = |uri: &str| -> Option<(Vec<Point>, Option<String>)> {
            let object = store.get_object(uri).ok()?;
            let found = geometry::of(object.as_reflect(), &store).ok()?;
            let crs = energyml_domain::crs::of(object.as_reflect(), &store).ok()?;
            let points = found
                .patches
                .iter()
                .flat_map(|patch| patch.points.iter().copied())
                .map(|p| crs.as_ref().map_or(p, |c| c.to_projected(p)))
                .collect();
            Some((points, crs.map(|c| c.uuid)))
        };

        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            let reflect = object.as_reflect();

            for field in BORROWED_GEOMETRY_FIELDS {
                let Some(energyml_core::value::FieldValue::Object(lender)) = reflect.field(field) else {
                    continue;
                };
                let Some(uuid) = energyml_core::identity::uuid_of_reflect(lender) else {
                    continue;
                };
                let Some(lent) = store.resources().iter().find(|s| s.uuid == uuid).cloned() else {
                    continue;
                };

                let (Some((mine, my_crs)), Some((theirs, their_crs))) =
                    (placed(&summary.uri), placed(&lent.uri))
                else {
                    continue;
                };

                // **The sharp one.** Points computed from another object's are
                // in that object's system, with no exception: the borrower
                // placed nothing itself. `None` against `Some` is the exact
                // shape of the defect above.
                findings.check(my_crs == their_crs, || {
                    format!(
                        "{file}: {} borrows geometry through `{field}` from {uuid} but resolves \
                         CRS {my_crs:?} against the lender's {their_crs:?}",
                        summary.uuid
                    )
                });

                // And the consequence, measured rather than inferred: once
                // placed, the borrower has to sit on the lender. A flip applied
                // once too often or once too few mirrors it about the datum,
                // which no count and no CRS comparison would notice if the two
                // ever came to agree on the wrong system.
                let (Some((low, high)), Some((their_low, their_high))) = (box_of(&mine), box_of(&theirs))
                else {
                    continue;
                };
                // A tenth of the lender's own extent, and never zero: a frame
                // may legitimately reach a metre past the last station of the
                // trajectory it samples.
                let slack: Vec<f64> = (0..3)
                    .map(|axis| ((their_high[axis] - their_low[axis]) * 0.1).max(1.0))
                    .collect();
                let inside = (0..3).all(|axis| {
                    low[axis] >= their_low[axis] - slack[axis] && high[axis] <= their_high[axis] + slack[axis]
                });
                findings.check(inside, || {
                    format!(
                        "{file}: {} spans {low:?}..{high:?} while the {field} it is computed \
                         from spans {their_low:?}..{their_high:?}",
                        summary.uuid
                    )
                });
            }
        }
    }

    // The corollary that bites: this passed over an empty set until the corpus
    // was checked for actually holding a borrower.
    findings.finish("borrowed geometry", 2);
}
