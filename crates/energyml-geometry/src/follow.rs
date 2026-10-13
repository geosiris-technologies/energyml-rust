//! Following a reference: by its **full uri**, never by its uuid alone.
//!
//! A package can hold one uuid in two `objectVersion`s, and a reader that asks
//! the store for the uuid a reference carries reads the wrong one of the two
//! (or refuses the pair as ambiguous). [`energyml_core::identity::uri_of_reflect`]
//! gives the address that tells them apart; every reader of this crate and of
//! the section view goes through here to reach it.
//!
//! Both are the library's one door, re-exported here for this crate's readers:
//! [`energyml_core::identity::address_of_reflect`] and
//! [`ObjectSource::object_of`]. One exception, and it is the reference's: a reference that
//! names **no type** (no `QualifiedType`, no `ContentType`) has no uri, since a
//! uri is built from the type. Its uuid is then the only address it has, and
//! the store is asked by that — which a store resolves when the uuid is unique
//! and refuses by name when it is not.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::ObjectSource;

/// The address a reference names — [`energyml_core::identity::address_of_reflect`].
pub fn address_of(reference: &dyn Reflect) -> Option<String> {
    energyml_core::identity::address_of_reflect(reference)
}

/// The object a reference names — [`ObjectSource::object_of`].
pub fn object_of(source: &dyn ObjectSource, reference: &dyn Reflect) -> Option<Box<dyn DynObject>> {
    source.object_of(reference)
}
