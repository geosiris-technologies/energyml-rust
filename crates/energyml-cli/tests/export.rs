//! One object failing is not the run failing.
//!
//! A directory export is a **batch**. Stopping at the first refusal leaves a
//! folder that looks like a finished export and holds a fraction of one - which
//! is the exact failure this workspace is arranged against, at the level where
//! it is hardest to notice, because the caller sees files.
//!
//! It happened on 02/08 and it is worth remembering how: the Grid2d
//! `030a82f6` used to be *refused by the geometry* - an unevaluated
//! `Point3dFromRepresentationLatticeArray` - and refusals there are caught and
//! counted. Then that array became readable, so the object evaluated, reached
//! the metre conversion, and was refused **there** instead - by a path that
//! propagated. Making one thing work moved a failure from a branch that handled
//! it to one that did not, and thirty-two representations stopped being written
//! without anyone touching the code that writes them.
//!
//! So the test is on the CLI binary rather than on a library call: the bug was
//! a `?` in a loop, and no unit test of an exporter would ever have seen it.

use std::path::PathBuf;
use std::process::Command;

mod fixtures;

fn corpus(name: &str) -> Option<PathBuf> {
    fixtures::find(name)
}

/// The package that carries the case: thirty-three representations, one of
/// which sits in a **time** CRS and cannot be converted to metres.
const PACKAGE: &str = "testingPackageCpp22.epc";

#[test]
#[cfg_attr(not(feature = "hdf5"), ignore = "the arrays live in HDF5")]
fn one_representation_that_cannot_be_written_does_not_stop_the_others() {
    let Some(package) = corpus(PACKAGE) else {
        eprintln!("skipped: the reference package is not in the fixtures");
        return;
    };

    for format in ["vtk", "vtu", "vtp", "obj"] {
        let out = std::env::temp_dir().join(format!("energyml-export-{format}"));
        let _ = std::fs::remove_dir_all(&out);

        let run = Command::new(env!("CARGO_BIN_EXE_energyml"))
            .arg("export")
            .arg(&package)
            .args(["--format", format])
            .arg("-o")
            .arg(&out)
            .output()
            .expect("the binary runs");
        let said = String::from_utf8_lossy(&run.stderr).into_owned();

        let files: Vec<_> = std::fs::read_dir(&out)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();

        assert!(
            files.len() > 25,
            "{format}: {} file(s) written out of a package of 33 representations - \
             one refusal took the batch with it\n{said}",
            files.len()
        );

        // Skipped, and **named**. A batch that silently drops one is the other
        // half of the same failure. Since 05/08 the time-axis object is not a
        // refusal of the exporter but a skip the command declares up front, with
        // the way to write it anyway - and it still has to say **which**.
        assert!(
            said.contains("skipped"),
            "{format}: the object that was skipped is not named anywhere\n{said}"
        );
        assert!(
            said.contains("030a82f6"),
            "{format}: the refusal does not say which object\n{said}"
        );

        // And no stub left behind. A zero-byte file in the output reads as a
        // representation with nothing in it, which is worse than no file.
        for file in &files {
            let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
            assert!(
                size > 0,
                "{format}: {} is empty - a failed write left its file behind",
                file.display()
            );
        }
    }
}
