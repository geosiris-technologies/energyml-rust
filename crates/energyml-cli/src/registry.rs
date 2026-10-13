//! The registry this tool uses: every schema version the workspace generates.
//!
//! One line, because the list itself lives in `energyml-schemas` - see that
//! crate for what a new Energistics release actually costs and for the test
//! that fails when the list goes stale.
//!
//! It used to live here, and a second copy lived in `energyml-roundtrip`, and
//! eighteen more subsets lived in the test suites. Nothing detected an
//! omission.

use energyml_core::registry::Registry;

/// Every generated crate, registered.
pub fn all() -> Registry {
    energyml_schemas::all()
}
