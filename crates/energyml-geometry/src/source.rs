//! Where the arrays and the referenced objects come from.
//!
//! There is no trait defined here any more. Resolving a uuid and reading an
//! array are [`energyml_domain::ObjectSource`] and
//! [`energyml_domain::ArraySource`], one layer down, because the CRS reader and
//! the value-array reader need exactly the same two things - and two
//! definitions of "where the data comes from" is one too many for a store to
//! implement.
//!
//! An EPC file implements both; an ETP dataspace will implement them the same
//! way, and no line of [`crate::points`] or [`crate::geometry`] changes.

pub use energyml_domain::{ArraySource as ValueSource, ObjectSource};
