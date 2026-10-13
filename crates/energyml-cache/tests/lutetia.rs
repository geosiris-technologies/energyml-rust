//! Lutetia, the reference model every test of this crate reads (the variant
//! **with its errors**: the colours and attachments audits want the whole
//! package, `[ERROR]` objects included).
//!
//! Lookup order:
//!
//! 1. `$ENERGYML_LUTETIA_DIR`, laid out like the public `energyml-lutetia`
//!    repository (contract in its README): `package-with-errors/` holds
//!    `Lutetia-with-errors.epc` or `Lutetia.epc` (+ the `.h5` it names). That
//!    is the variant with errors; `package/` is the clean one and is not read
//!    here;
//! 2. the monorepo, `web/public/examples/lutetia/Lutetia.epc` (the variant
//!    with errors — in the monorepo the clean one is `lutetia-sans-erreurs`).
//!
//! Nothing found: a panic when `$ENERGYML_FIXTURES_REQUIRED` is set, otherwise
//! a printed note and `None`, and the test returns.
//! Keep this file the same as `energyml-build/tests/lutetia_package.rs` as far
//! as the lookup goes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_storage::EpcStore;

fn locate() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("ENERGYML_LUTETIA_DIR") {
        let with_errors = PathBuf::from(dir).join("package-with-errors");
        candidates.push(with_errors.join("Lutetia-with-errors.epc"));
        candidates.push(with_errors.join("Lutetia.epc"));
    }
    candidates
        .push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/examples/lutetia/Lutetia.epc"));
    candidates.into_iter().find(|path| path.is_file())
}

pub fn bytes() -> Option<Vec<u8>> {
    let Some(path) = locate() else {
        if std::env::var_os("ENERGYML_FIXTURES_REQUIRED").is_some() {
            panic!(
                "Lutetia not found (looked in $ENERGYML_LUTETIA_DIR/package-with-errors and web/public/examples/lutetia): \
                 a missing reference package is a failure in CI, never a skip"
            );
        }
        eprintln!(
            "skipped: Lutetia is not available (set ENERGYML_LUTETIA_DIR to a checkout of energyml-lutetia, \
             or run `python scripts/fetch-lutetia.py --dest target/lutetia`)"
        );
        return None;
    };
    Some(std::fs::read(&path).unwrap_or_else(|why| panic!("reading {}: {why}", path.display())))
}

pub fn store() -> Option<EpcStore> {
    let bytes = bytes()?;
    Some(
        EpcStore::from_reader(
            Box::new(std::io::Cursor::new(bytes)),
            "Lutetia.epc",
            Arc::new(energyml_schemas::all()),
        )
        .expect("Lutetia opens"),
    )
}
