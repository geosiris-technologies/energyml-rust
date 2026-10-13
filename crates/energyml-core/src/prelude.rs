//! What generated code imports.
//!
//! Generated modules start with `use energyml_core::prelude::*;`, so adding a
//! capability here reaches every generated crate without regenerating them.
//!
//! The flip side: every name exported here is a name the schemas cannot use.
//! The glob sits next to `use crate::*`, so a collision shadows the schema type
//! and the generated crate stops compiling - `IdentityKind`, which resqml
//! declares as an enumeration, is why that sentence is here. Before adding a
//! name, check it against the ~2500 generated type names.

pub use crate::any_xml::AnyXml;
pub use crate::error::{Error, Result};
// `IdentityKind` is deliberately absent: resqml declares a schema enum of the
// same name, and this glob would shadow it inside the generated crate. The
// derive spells it by full path, so no generated code needs it here.
pub use crate::identity::{
    AsObjectRef, DeclaredType, EpcLayout, ObjectId, ObjectIdentity, ObjectIdentityMut, ObjectKey, ObjectRef,
    VersionRef,
};
pub use crate::naming;
pub use crate::ns::{self, Namespace};
pub use crate::path;
pub use crate::reflect::{
    Binding, EnergymlType, Facets, FieldInfo, FieldType, MaxOccurs, Occurs, Roles, ScalarKind, TypeInfo,
    TypeInfoFn, TypeKind, VariantInfo,
};
pub use crate::registry::{DynObject, Registry, TypeEntry};
pub use crate::validate::{Severity, Validate, ValidationCtx, ValidationReport};
pub use crate::value::{
    AsFieldValue, FieldValue, FromOwnedValue, OwnedValue, Reflect, Scalar, downcast_owned,
};
pub use crate::xml::{
    XmlDeserialize, XmlEvent, XmlReader, XmlSerialize, XmlText, XmlWriteOptions, XmlWriter, attr_value,
    from_xml_bytes, from_xml_str, local_name, local_part, optional_attr, push_attr, push_attr_required,
    push_xsi_type, required_attr, to_xml_string, xsi_type,
};

pub use quick_xml::events::BytesStart;

#[cfg(feature = "derive")]
pub use energyml_derive::{EnergymlChoice, EnergymlEnum, EnergymlObject};
