//! Writing arrays, proved by reading them back with the reader that already
//! works.
//!
//! The whole suite needs `--features hdf5`; without it there is no backend and
//! nothing here would have anything to exercise.
//!
//! Why round-tripping is the test and not a nicety: an HDF5 file that is subtly
//! wrong - right values under a transposed shape, an integer written where a
//! double was meant, a uuid attribute of the wrong string type - opens
//! perfectly well in every tool that does not look closely. Comparing what came
//! back with what went in is the only check that catches those.

#![cfg(feature = "hdf5")]

use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use energyml_storage::backend::{ArrayBackend, Backends, ExternalFiles};
use energyml_storage::hdf5::Hdf5Backend;

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-write-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

fn data(dimensions: Vec<i64>, values: ArrayValues) -> ArrayData {
    ArrayData {
        metadata: ArrayMetadata {
            element_type: values.element_type().to_string(),
            dimensions,
        },
        values,
    }
}

#[test]
fn every_element_type_survives_the_trip() {
    let dir = folder("types");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;

    // One dataset per element type, all in the same file: writing the second
    // must not erase the first, which `File::create` would do and `open_rw`
    // does not.
    let cases: Vec<(&str, ArrayData)> = vec![
        (
            "/f64",
            data(vec![4], ArrayValues::F64(vec![1.5, -2.25, 0.0, 1e300])),
        ),
        ("/f32", data(vec![3], ArrayValues::F32(vec![1.5, -2.25, 3.0]))),
        (
            "/i64",
            data(vec![3], ArrayValues::I64(vec![i64::MIN, 0, i64::MAX])),
        ),
        (
            "/i32",
            data(vec![3], ArrayValues::I32(vec![i32::MIN, 0, i32::MAX])),
        ),
        (
            "/i16",
            data(vec![3], ArrayValues::I16(vec![i16::MIN, 0, i16::MAX])),
        ),
        ("/i8", data(vec![3], ArrayValues::I8(vec![i8::MIN, 0, i8::MAX]))),
        ("/u64", data(vec![2], ArrayValues::U64(vec![0, u64::MAX]))),
        ("/u32", data(vec![2], ArrayValues::U32(vec![0, u32::MAX]))),
        ("/u16", data(vec![2], ArrayValues::U16(vec![0, u16::MAX]))),
        ("/u8", data(vec![2], ArrayValues::U8(vec![0, u8::MAX]))),
    ];

    for (path, written) in &cases {
        backend.write(&file, path, written).expect("the write succeeds");
    }
    for (path, written) in &cases {
        let read = backend.read(&file, path, &[], &[]).expect("the read succeeds");
        assert_eq!(
            read.values, written.values,
            "`{path}` did not come back the way it went in"
        );
        assert_eq!(read.metadata.dimensions, written.metadata.dimensions, "{path}");
        assert_eq!(
            read.metadata.element_type, written.metadata.element_type,
            "`{path}` changed element type in the file"
        );
    }
}

#[test]
fn a_shape_is_kept_not_flattened() {
    let dir = folder("shape");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;

    // Points are the reason this matters: an N×3 array read back as 3N is the
    // same numbers in the same order, and every count still agrees. Only the
    // shape tells the two apart.
    let points = data(
        vec![4, 3],
        ArrayValues::F64(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0]),
    );
    backend.write(&file, "/resqml20/uuid/points", &points).unwrap();

    let metadata = backend.metadata(&file, "/resqml20/uuid/points").unwrap();
    assert_eq!(metadata.dimensions, vec![4, 3]);
    let read = backend.read(&file, "/resqml20/uuid/points", &[], &[]).unwrap();
    assert_eq!(read.metadata.dimensions, vec![4, 3]);
    assert_eq!(read.values, points.values);
}

#[test]
fn a_shape_that_does_not_describe_the_values_is_refused() {
    let dir = folder("mismatch");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;

    // 4×3 is twelve; six values is not twelve. Writing them anyway would
    // produce a file whose declared shape is a lie, and HDF5 would pad it.
    let wrong = data(vec![4, 3], ArrayValues::F64(vec![0.0; 6]));
    let error = backend.write(&file, "/points", &wrong).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("12") && message.contains('6'), "{message}");
    // And nothing was created under that name.
    assert!(backend.metadata(&file, "/points").is_err());
}

#[test]
fn the_groups_on_the_way_are_created() {
    let dir = folder("groups");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;

    // `/resqml20/<uuid>/points_patch0` is the shape fesapi writes, and HDF5
    // creates none of the intermediate groups on its own.
    let path = "/resqml20/0a327e2e-105c-4068-a325-a6c5e3de135f/points_patch0";
    backend
        .write(&file, path, &data(vec![2], ArrayValues::F64(vec![1.0, 2.0])))
        .expect("deep paths work");
    assert_eq!(backend.metadata(&file, path).unwrap().dimensions, vec![2]);
}

#[test]
fn writing_twice_replaces_rather_than_appends() {
    let dir = folder("replace");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;

    backend
        .write(
            &file,
            "/values",
            &data(vec![3], ArrayValues::F64(vec![1.0, 2.0, 3.0])),
        )
        .unwrap();
    // A different length, which HDF5 cannot do in place - the dataset has to be
    // unlinked and remade, and getting that wrong leaves the old one behind.
    backend
        .write(&file, "/values", &data(vec![2], ArrayValues::I32(vec![7, 8])))
        .unwrap();

    let read = backend.read(&file, "/values", &[], &[]).unwrap();
    assert_eq!(read.values, ArrayValues::I32(vec![7, 8]));
    assert_eq!(read.metadata.dimensions, vec![2]);
}

#[test]
fn the_uuid_attribute_is_written_the_way_the_reader_wants_it() {
    let dir = folder("uuid");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;
    let uuid = "0a327e2e-105c-4068-a325-a6c5e3de135f";

    backend
        .write(&file, "/values", &data(vec![1], ArrayValues::F64(vec![1.0])))
        .unwrap();
    assert_eq!(
        backend.file_uuid(&file).unwrap(),
        None,
        "no uuid until one is set"
    );

    backend.set_file_uuid(&file, uuid).unwrap();
    // Read back by the very function EPC §3.2.3 cross-validation uses. Writing
    // it as a variable-length string instead of fixed 36 ASCII would still
    // "work" here and fail against other implementations.
    assert_eq!(backend.file_uuid(&file).unwrap().as_deref(), Some(uuid));

    // Setting it again replaces rather than failing on the existing attribute.
    let other = "eb0b55a5-9562-4642-946b-d2ec57172daf";
    backend.set_file_uuid(&file, other).unwrap();
    assert_eq!(backend.file_uuid(&file).unwrap().as_deref(), Some(other));
}

#[test]
fn a_uuid_that_is_not_36_ascii_characters_is_refused() {
    let dir = folder("baduuid");
    let file = dir.join("model.h5");
    let backend = Hdf5Backend;
    backend
        .write(&file, "/values", &data(vec![1], ArrayValues::F64(vec![1.0])))
        .unwrap();
    // §3.2.3 is specific about the form. Writing something else produces a file
    // that opens fine and fails every conforming reader's check.
    assert!(backend.set_file_uuid(&file, "not-a-uuid").is_err());
    assert_eq!(backend.file_uuid(&file).unwrap(), None);
}

#[test]
fn a_backend_that_only_reads_says_so() {
    struct ReadOnly;
    impl ArrayBackend for ReadOnly {
        fn name(&self) -> &'static str {
            "read-only"
        }
        fn handles(&self, _mime: Option<&str>, _file: &std::path::Path) -> bool {
            true
        }
        fn metadata(&self, _f: &std::path::Path, _p: &str) -> energyml_storage::Result<ArrayMetadata> {
            unimplemented!()
        }
        fn read(
            &self,
            _f: &std::path::Path,
            _p: &str,
            _s: &[i64],
            _c: &[i64],
        ) -> energyml_storage::Result<ArrayData> {
            unimplemented!()
        }
        fn read_into(
            &self,
            _f: &std::path::Path,
            _p: &str,
            _s: &[i64],
            _c: &[i64],
            _o: &mut energyml_storage::array::ArrayBuffer<'_>,
        ) -> energyml_storage::Result<ArrayMetadata> {
            unimplemented!()
        }
    }

    // The default must refuse by name rather than appear to succeed: a backend
    // whose `write` silently did nothing would lose data with no error at all.
    let backend = ReadOnly;
    assert!(!backend.writes());
    let error = backend
        .write(
            std::path::Path::new("/tmp/x.h5"),
            "/values",
            &data(vec![1], ArrayValues::F64(vec![1.0])),
        )
        .unwrap_err();
    assert!(format!("{error}").contains("read-only"), "{error}");
    assert!(Hdf5Backend.writes());
}

#[test]
fn a_destination_is_known_before_the_file_exists() {
    let dir = folder("destination");
    let mut files = ExternalFiles::with_root(&dir);

    // `resolve` answers "where is it" and needs it to be there; `destination_for`
    // answers "where should it go" and must not.
    assert!(files.resolve("new.h5").is_err());
    assert_eq!(files.destination_for("new.h5").unwrap(), dir.join("new.h5"));

    // Once written, the two agree - which is what makes a file written through
    // one findable through the other.
    Hdf5Backend
        .write(
            &files.destination_for("new.h5").unwrap(),
            "/values",
            &data(vec![1], ArrayValues::F64(vec![1.0])),
        )
        .unwrap();
    assert_eq!(files.resolve("new.h5").unwrap(), dir.join("new.h5"));

    // A pinned name wins in both, still.
    files.set("renamed.h5", dir.join("new.h5"));
    assert_eq!(files.destination_for("renamed.h5").unwrap(), dir.join("new.h5"));

    // With no root and no override there is nowhere to put it, and guessing the
    // working directory would scatter data files wherever the process ran.
    assert!(ExternalFiles::default().destination_for("new.h5").is_err());
}

#[test]
fn a_store_writes_an_array_and_reads_its_own_file_back() {
    let dir = folder("store");
    // The smallest possible package: no objects, just somewhere to put arrays.
    // The point is the plumbing - backend choice, destination, round trip -
    // not the EnergyML on top.
    let mut backends = Backends::none();
    backends.push(Box::new(Hdf5Backend));

    let store = energyml_storage::EpcStore::from_reader(
        Box::new(std::io::Cursor::new(empty_epc())),
        "model.epc",
        Arc::new(Registry::new()),
    )
    .expect("an empty package opens");
    let mut store = store;
    store.external_files_mut().set_root(&dir);

    let written = data(vec![2, 3], ArrayValues::F64(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
    store
        .write_array_to("model.h5", "/resqml20/uuid/points", &written)
        .expect("the store writes");

    // And reads it back through the ordinary read path - the same one an
    // application uses, resolving the same name.
    let location = ArrayLocation {
        file: "model.h5".into(),
        path_in_file: "/resqml20/uuid/points".into(),
        ..Default::default()
    };
    let read = store.read_array(&location).expect("the store reads it back");
    assert_eq!(read.values, written.values);
    assert_eq!(read.metadata.dimensions, vec![2, 3]);

    // Stamping and verifying: the two halves of EPC §3.2.3, each written by one
    // side of this crate and checked by the other.
    let uuid = "0a327e2e-105c-4068-a325-a6c5e3de135f";
    store.stamp_external_file("model.h5", uuid).unwrap();
    let stamped = ArrayLocation {
        proxy_uuid: Some(uuid.to_string()),
        ..location.clone()
    };
    store.verify_external_file(&stamped).expect("the uuid matches");

    let wrong = ArrayLocation {
        proxy_uuid: Some("eb0b55a5-9562-4642-946b-d2ec57172daf".to_string()),
        ..location
    };
    assert!(
        store.verify_external_file(&wrong).is_err(),
        "a file stamped for another proxy must not pass"
    );
}

#[test]
fn a_declared_file_reaches_the_package_that_gets_written() {
    let dir = folder("declare");
    let mut store = energyml_storage::EpcStore::from_reader(
        Box::new(std::io::Cursor::new(empty_epc())),
        "model.epc",
        Arc::new(Registry::new()),
    )
    .unwrap();
    store.external_files_mut().set_root(&dir);

    // A proxy created in this session has no `.rels` to read the file name
    // from, so without this the saved package names no file at all and the
    // arrays become unreachable.
    let uuid = "0a327e2e-105c-4068-a325-a6c5e3de135f";
    assert!(store.external_file_map().is_empty());
    store.register_external_file(uuid, "model.h5");
    assert_eq!(
        store.external_file_map().get(uuid).map(String::as_str),
        Some("model.h5")
    );
}

/// The smallest EPC that opens: a content-type index and nothing else.
fn empty_epc() -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        use std::io::Write;
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
</Types>"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    buffer.into_inner()
}

// ---------------------------------------------------------------------------
// Against the real files
// ---------------------------------------------------------------------------

fn corpus_registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    energyml_eml_2_0::register(&mut registry);
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_0_1::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    Arc::new(registry)
}

/// Every array of the corpus, written out again and compared value by value.
///
/// The strongest test available for a writer: the input is not something this
/// crate invented, it is what two independent implementations actually produce -
/// 429 datasets across ten element types, shapes from a single scalar to a
/// 50×100 grid. A writer that handles the cases someone thought to make up is
/// not the same as one that handles the cases that exist.
#[test]
fn every_array_of_the_corpus_can_be_written_back() {
    let packages: Vec<std::path::PathBuf> = crate::fixtures::public_corpus();
    if packages.is_empty() {
        eprintln!("skipped: no reference package (scripts/fetch-fixtures.py)");
        return;
    }

    let out = folder("corpus");
    let mut copied = 0usize;
    let mut absent = 0usize;
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for package in &packages {
        let store = energyml_storage::EpcStore::open(package, corpus_registry()).expect("it opens");
        let mut target = energyml_storage::EpcStore::open(package, corpus_registry()).expect("it opens");
        let name = package.file_stem().unwrap().to_string_lossy().to_string();
        target.external_files_mut().set_root(&out);

        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for location in store.arrays_of(object.as_reflect()) {
                let original = match store.read_array(&location) {
                    Ok(data) => data,
                    // The resqpy packages ship without their `.h5`; counted so a
                    // pass is never mistaken for full coverage.
                    Err(_) => {
                        absent += 1;
                        continue;
                    }
                };
                let file = format!("{name}.h5");
                target
                    .write_array_to(&file, &location.path_in_file, &original)
                    .unwrap_or_else(|e| {
                        panic!("{}: writing {} - {e}", package.display(), location.path_in_file)
                    });

                let echo = energyml_storage::array::ArrayLocation {
                    file,
                    path_in_file: location.path_in_file.clone(),
                    ..Default::default()
                };
                let read = target
                    .read_array(&echo)
                    .unwrap_or_else(|e| panic!("{}: reading back - {e}", location.path_in_file));

                // Compared **bit for bit**, not with `==`. Two reasons, and the
                // corpus supplies both: `NaN != NaN`, so a float comparison
                // reports a difference where the bytes are identical; and a
                // writer that canonicalised a NaN payload, or rounded a double
                // through some intermediate, would pass `==` while corrupting
                // the file. The corpus uses NaN as the "no control point here"
                // sentinel in parametric arrays, so this is not hypothetical.
                if read.values.as_bytes() != original.values.as_bytes() {
                    panic!(
                        "{}: {} came back different\n  wrote {:?}\n  read  {:?}",
                        package.display(),
                        location.path_in_file,
                        original.values,
                        read.values
                    );
                }
                assert_eq!(
                    read.metadata.dimensions,
                    original.metadata.dimensions,
                    "{}: {} changed shape",
                    package.display(),
                    location.path_in_file
                );
                assert_eq!(
                    read.metadata.element_type,
                    original.metadata.element_type,
                    "{}: {} changed element type",
                    package.display(),
                    location.path_in_file
                );
                *kinds.entry(original.metadata.element_type.clone()).or_default() += 1;
                copied += 1;
            }
        }
    }

    eprintln!("{copied} array(s) written and read back, by element type: {kinds:?}");
    if absent > 0 {
        eprintln!("{absent} pointer(s) skipped: their package's `.h5` is not in testdata/");
    }
    assert!(copied > 0, "not one array was written: the corpus has no `.h5`");
    assert!(
        kinds.len() > 1,
        "only `{:?}` was exercised; a writer that handles one type is not tested",
        kinds.keys()
    );
}

/// The same arrays written twice make the same file, byte for byte.
///
/// HDF5 stamps a modification time into every group's header unless told not
/// to, and the time is in seconds: two runs a second apart differed in 946
/// bytes on Lutetia (28/09) and in nothing else. The front end's example is
/// versioned and checked by replaying its recipe, which only works if a replay
/// gives the same bytes. The pause is what makes this test able to fail - two
/// writes within the same second would agree on the clock too.
#[test]
fn the_same_arrays_make_the_same_file() {
    let dir = folder("replay");
    let write = |name: &str| {
        let file = dir.join(name);
        let backend = Hdf5Backend;
        backend
            .write(
                &file,
                "/resqml22/uuid-a/points",
                &data(vec![2, 3], ArrayValues::F64(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0])),
            )
            .expect("written");
        backend
            .write(
                &file,
                "/resqml22/uuid-b/indices",
                &data(vec![3], ArrayValues::I32(vec![0, 1, 2])),
            )
            .expect("written");
        std::fs::read(&file).expect("read back")
    };
    let first = write("first.h5");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = write("second.h5");
    assert_eq!(first.len(), second.len());
    let differing = first.iter().zip(&second).filter(|(a, b)| a != b).count();
    assert_eq!(
        differing, 0,
        "{differing} byte(s) differ between two writes of the same arrays"
    );
}

/// **Many arrays in one opening read back as one at a time would write them**
/// (`ArrayBackend::write_many`, 28/09). One opening and one flush per dataset
/// was 95 % of what writing a SIGEOL import took; the batch opens once - and
/// must not change a single value, a shape or a type on the way.
#[test]
fn many_arrays_in_one_opening_read_back_as_one_at_a_time_would() {
    let dir = folder("many");
    let backend = Hdf5Backend;
    let arrays: Vec<(String, ArrayData)> = (0..40)
        .map(|n| {
            let values = if n % 2 == 0 {
                ArrayValues::F64((0..n + 1).map(|v| v as f64 * 0.5).collect())
            } else {
                ArrayValues::I64((0..n + 1).map(|v| v - 7).collect())
            };
            (format!("/resqml22/uuid-{n}/values"), data(vec![n + 1], values))
        })
        .collect();
    let items: Vec<(&str, &ArrayData)> = arrays.iter().map(|(path, data)| (path.as_str(), data)).collect();

    let batched = dir.join("batched.h5");
    let results = backend.write_many(&batched, &items);
    assert_eq!(results.len(), items.len(), "one result per array");
    assert!(results.iter().all(Result::is_ok), "{results:?}");

    let single = dir.join("single.h5");
    for (path, data) in &items {
        backend.write(&single, path, data).expect("written alone");
    }
    for (path, written) in &items {
        let from_batch = backend
            .read(&batched, path, &[], &[])
            .expect("the batch holds it");
        let from_single = backend
            .read(&single, path, &[], &[])
            .expect("the single write holds it");
        assert_eq!(from_batch.values, written.values, "{path}: the batch's values");
        assert_eq!(
            from_batch.values, from_single.values,
            "{path}: the two writers disagree"
        );
        assert_eq!(
            from_batch.metadata.dimensions, written.metadata.dimensions,
            "{path}: the shape"
        );
    }
}

/// **A refused array is refused in its place, and the others are written**
/// (`EpcStore::write_arrays_to`): a shape that does not describe its values is
/// the caller's bug for that one array, not the batch's.
#[test]
fn a_refused_array_does_not_sink_the_others() {
    let dir = folder("many-refused");
    let mut backends = Backends::none();
    backends.push(Box::new(Hdf5Backend));
    let mut store = energyml_storage::EpcStore::from_reader(
        Box::new(std::io::Cursor::new(empty_epc())),
        "model.epc",
        Arc::new(Registry::new()),
    )
    .expect("an empty package opens");
    store.external_files_mut().set_root(&dir);

    let good = data(vec![2], ArrayValues::F64(vec![1.0, 2.0]));
    let wrong = data(vec![3], ArrayValues::F64(vec![1.0, 2.0]));
    let also = data(vec![1], ArrayValues::I64(vec![42]));
    let results = store.write_arrays_to("model.h5", &[("/a", &good), ("/b", &wrong), ("/c", &also)]);
    assert!(results[0].is_ok() && results[2].is_ok(), "{results:?}");
    let refused = results[1]
        .as_ref()
        .expect_err("the shape promises three values and holds two");
    assert!(
        refused.to_string().contains("/b"),
        "the refusal names its array: {refused}"
    );

    for (path, written) in [("/a", &good), ("/c", &also)] {
        let read = store
            .read_array(&ArrayLocation {
                file: "model.h5".into(),
                path_in_file: path.into(),
                ..Default::default()
            })
            .expect("written despite the refusal beside it");
        assert_eq!(read.values, written.values);
    }
}
