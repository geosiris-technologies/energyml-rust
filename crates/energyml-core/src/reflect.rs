//! Static description of generated types.
//!
//! Every generated type carries a `&'static TypeInfo` describing its fields,
//! their XSD cardinality and facets, the accepted types for polymorphic fields,
//! and the original `xsd:documentation`. This is what lets a UI build and
//! complete instances without compile-time knowledge of the model, and what a
//! version translator reads to declare mappings.
//!
//! References between `TypeInfo`s go through [`TypeInfoFn`] (a plain function
//! pointer) rather than `&'static TypeInfo`, because the Energistics model is
//! deeply cyclic and `static` initializers cannot be.

use crate::ns::Namespace;

/// Indirection used for every type-to-type link, so cyclic models can still be
/// described with `static` data.
pub type TypeInfoFn = fn() -> &'static TypeInfo;

/// Implemented by every generated type.
pub trait EnergymlType {
    /// Static description of this type.
    fn type_info() -> &'static TypeInfo
    where
        Self: Sized;
}

/// What flavour of Rust item a [`TypeInfo`] describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeKind {
    /// An XSD `complexType` mapped to a Rust struct.
    Struct,
    /// An XSD `simpleType` enumeration mapped to a Rust enum.
    Enumeration,
    /// An abstract type / substitution group mapped to a Rust enum whose
    /// variants are the concrete subtypes.
    Choice,
}

/// How a field appears in XML.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Binding {
    /// XML attribute on the owning element.
    Attribute,
    /// Child element.
    Element,
    /// Text content of the owning element (`xsd:simpleContent` extension).
    Text,
    /// An `xsd:any` wildcard: every child element no other field claims.
    ///
    /// The field has no XML name of its own — it *is* the leftovers — so it is
    /// typed [`AnyXml`] and holds the unmatched children as source text.
    ///
    /// [`AnyXml`]: crate::any_xml::AnyXml
    Any,
}

/// Upper bound of an XSD `maxOccurs`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaxOccurs {
    /// A finite bound.
    Bounded(u32),
    /// `maxOccurs="unbounded"`.
    Unbounded,
}

impl MaxOccurs {
    /// Whether `n` occurrences are allowed.
    pub fn allows(&self, n: usize) -> bool {
        match self {
            MaxOccurs::Unbounded => true,
            MaxOccurs::Bounded(max) => n <= *max as usize,
        }
    }
}

/// XSD cardinality of a field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Occurs {
    /// `minOccurs`.
    pub min: u32,
    /// `maxOccurs`.
    pub max: MaxOccurs,
}

impl Occurs {
    /// `minOccurs="1" maxOccurs="1"`.
    pub const REQUIRED: Occurs = Occurs {
        min: 1,
        max: MaxOccurs::Bounded(1),
    };
    /// `minOccurs="0" maxOccurs="1"`.
    pub const OPTIONAL: Occurs = Occurs {
        min: 0,
        max: MaxOccurs::Bounded(1),
    };
    /// `minOccurs="0" maxOccurs="unbounded"`.
    pub const ANY: Occurs = Occurs {
        min: 0,
        max: MaxOccurs::Unbounded,
    };

    /// Whether the field maps to a `Vec`.
    pub fn is_collection(&self) -> bool {
        !matches!(self.max, MaxOccurs::Bounded(0 | 1))
    }

    /// Whether the field may be absent.
    pub fn is_optional(&self) -> bool {
        self.min == 0
    }
}

/// Primitive XSD types after mapping to Rust.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalarKind {
    /// `xsd:boolean`
    Bool,
    /// Signed integers.
    Int,
    /// Unsigned integers.
    UInt,
    /// `xsd:double` / `xsd:float` / `xsd:decimal`
    Double,
    /// `xsd:string` and its restrictions.
    String,
    /// `xsd:dateTime`, `xsd:date`, `xsd:time` — kept lexical.
    DateTime,
    /// `xsd:base64Binary` / `xsd:hexBinary`.
    Binary,
}

impl ScalarKind {
    /// Human-readable name, used in error messages.
    pub fn name(&self) -> &'static str {
        match self {
            ScalarKind::Bool => "boolean",
            ScalarKind::Int => "integer",
            ScalarKind::UInt => "unsigned integer",
            ScalarKind::Double => "double",
            ScalarKind::String => "string",
            ScalarKind::DateTime => "dateTime",
            ScalarKind::Binary => "binary",
        }
    }
}

/// Type of a field's item (ignoring cardinality, which lives in [`Occurs`]).
#[derive(Clone, Copy, Debug)]
pub enum FieldType {
    /// A primitive value.
    Scalar(ScalarKind),
    /// Another generated type.
    Object(TypeInfoFn),
    /// A generated enumeration. Kept distinct from [`FieldType::Object`] so a UI
    /// can offer the declared values as a closed list.
    Enumeration(TypeInfoFn),
}

impl FieldType {
    /// Resolve to a [`TypeInfo`] when the field holds a generated type.
    pub fn type_info(&self) -> Option<&'static TypeInfo> {
        match self {
            FieldType::Object(f) | FieldType::Enumeration(f) => Some(f()),
            FieldType::Scalar(_) => None,
        }
    }

    /// The values an enumeration field accepts, empty for other field types.
    ///
    /// An open (`...Ext`) enumeration also accepts any other string; check
    /// [`TypeInfo::is_open`] before treating this as exhaustive.
    pub fn allowed_values(&self) -> &'static [VariantInfo] {
        match self.type_info() {
            Some(info) if info.kind == TypeKind::Enumeration => info.variants,
            _ => &[],
        }
    }
}

/// XSD facets carried over from the schema so validation and UIs can enforce
/// the same constraints the schema does.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facets {
    /// `xsd:minInclusive`
    pub min_inclusive: Option<f64>,
    /// `xsd:maxInclusive`
    pub max_inclusive: Option<f64>,
    /// `xsd:minExclusive`
    pub min_exclusive: Option<f64>,
    /// `xsd:maxExclusive`
    pub max_exclusive: Option<f64>,
    /// `xsd:minLength`
    pub min_length: Option<u32>,
    /// `xsd:maxLength`
    pub max_length: Option<u32>,
    /// `xsd:pattern`, as written in the schema.
    pub pattern: Option<&'static str>,
    /// `fixed="…"` of the element or attribute declaration: the only value the
    /// document may carry.
    pub fixed: Option<&'static str>,
    /// The XSD built-in a string-kept value is written in (`dateTime`, `date`,
    /// `time`, `gYear`, `gMonth`, `gDay`, `duration`): checked against that
    /// type's grammar, which the schema's own pattern (`.+T.+[Z+\-].*`) only
    /// approximates.
    pub lexical: Option<&'static str>,
}

impl Facets {
    /// A facet set that constrains nothing.
    pub const NONE: Facets = Facets {
        min_inclusive: None,
        max_inclusive: None,
        min_exclusive: None,
        max_exclusive: None,
        min_length: None,
        max_length: None,
        pattern: None,
        fixed: None,
        lexical: None,
    };

    /// Whether any facet is set.
    pub fn is_empty(&self) -> bool {
        self.min_inclusive.is_none()
            && self.max_inclusive.is_none()
            && self.min_exclusive.is_none()
            && self.max_exclusive.is_none()
            && self.min_length.is_none()
            && self.max_length.is_none()
            && self.pattern.is_none()
            && self.fixed.is_none()
            && self.lexical.is_none()
    }
}

/// Description of one field of a generated struct.
#[derive(Clone, Copy, Debug)]
pub struct FieldInfo {
    /// Rust field name (snake_case).
    pub name: &'static str,
    /// Name in XML documents.
    pub xml_name: &'static str,
    /// Namespace of the XML name. Inherited fields keep the namespace of the
    /// schema that declared them, which is often not the owner's namespace.
    pub xml_ns: Namespace,
    /// Name in JSON documents.
    pub json_name: &'static str,
    /// `xsd:documentation` for this field.
    pub doc: &'static str,
    /// Whether the field is an attribute, an element or the text content.
    pub binding: Binding,
    /// XSD cardinality.
    pub occurs: Occurs,
    /// Item type.
    pub ty: FieldType,
    /// XSD facets applying to the item.
    pub facets: Facets,
    /// Whether the field is an `xsd:list`: **one** element or attribute whose
    /// text holds every value, separated by whitespace.
    ///
    /// The distinction matters to anything that maps fields onto a document.
    /// `Vec<f64>` with `is_list == false` is a repeated child element; the same
    /// type with `is_list == true` is a single `<Values>1.0 2.0 3.0</Values>`.
    /// [`FieldInfo::occurs`] counts values in both cases, and the facets apply
    /// to each value, not to the joined text.
    pub is_list: bool,
}

impl FieldInfo {
    /// Types that may legally be stored in this field.
    ///
    /// For a field typed by an abstract type this expands to the known concrete
    /// subtypes, which is what a UI needs in order to offer a choice.
    ///
    /// # An enumeration is a value, not a class, and offering it as one was a
    /// dead end
    ///
    /// An enumeration answers `type_info()` like everything else, so it used to
    /// come back here as the single class the field accepts. Every consumer then
    /// treated the slot as structured: a front end offered "create a `ThrowKindExt`",
    /// wrote `{}` into `FaultThrow.Throw`, and the JSON reader — reaching a
    /// declared type with no `$type` — asked [`Registry::create`] for
    /// `resqml22.ThrowKindExt` and failed with
    ///
    /// > unknown type `resqml22.ThrowKindExt` (not registered)
    ///
    /// which names a real type and reads like a gap in the build. It is not one.
    /// Enumerations are deliberately absent from the registry: they are written
    /// as their lexical value, never as an object, so nothing ever needs to
    /// *construct* one from a qualified type. [`TypeInfo::make`] being `Some`
    /// does not contradict that — it builds the Rust value, and says nothing
    /// about whether the registry can find it by name.
    ///
    /// So the offer for an enumeration slot is its **values**
    /// ([`TypeInfo::variants`], plus [`TypeInfo::is_open`] for the `…Ext` types
    /// that accept anything), and there is no class to choose. Returning nothing
    /// here is what makes a caller ask the right question.
    ///
    /// [`Registry::create`]: crate::registry::Registry::create
    pub fn accepted_types(&self) -> Vec<&'static TypeInfo> {
        match self.ty.type_info() {
            None => Vec::new(),
            Some(info) if info.kind == TypeKind::Enumeration => Vec::new(),
            Some(info) => {
                if info.is_abstract || info.kind == TypeKind::Choice {
                    info.concrete_subtypes()
                } else {
                    vec![info]
                }
            }
        }
    }

    /// Whether the field must be present for the object to be schema-valid.
    pub fn is_required(&self) -> bool {
        self.occurs.min >= 1
    }
}

/// Description of one variant of a generated enum.
///
/// For an enumeration the variant carries its XSD lexical value; for a choice
/// (abstract type) it carries the `xsi:type` used to select it.
#[derive(Clone, Copy, Debug)]
pub struct VariantInfo {
    /// Rust variant name.
    pub name: &'static str,
    /// Lexical value in documents, or the substituted element's local name.
    pub value: &'static str,
    /// `xsi:type` selecting this variant, when the enum models an abstract type.
    pub xsi_type: Option<&'static str>,
    /// `xsd:documentation` for this variant.
    pub doc: &'static str,
    /// Payload type, for choice enums.
    pub ty: Option<TypeInfoFn>,
    /// The `other` hole of an open enumeration, not a value of its own.
    ///
    /// The Energistics `…Ext` pattern ends every open enumeration with a
    /// variant carrying whatever the schema has not seen. It has **no lexical
    /// form**: its `value` is only the Rust variant name, and a chooser that
    /// offered it would write the literal string `Other` into a document. Said
    /// here rather than guessed from the name, because `Other` is also a
    /// perfectly good value in some vocabularies.
    pub is_other: bool,
}

fn push_unique(out: &mut Vec<&'static TypeInfo>, info: &'static TypeInfo) {
    if !out.iter().any(|t| std::ptr::eq(*t, info)) {
        out.push(info);
    }
}

/// Which fields carry identity, by index into [`TypeInfo::fields`].
///
/// The derive works these out from the field names, so a version that spells
/// `UUID` where another spells `Uuid`, or `VersionString` where another says
/// `ObjectVersion`, needs no special case anywhere else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Roles {
    /// `uuid` / `UUID` / `uid` on an identifying type.
    pub uuid: Option<u16>,
    /// A direct `Title` field, as references have.
    pub title: Option<u16>,
    /// A `Citation`, whose own `Title` is the object's title.
    pub citation: Option<u16>,
    /// `objectVersion` / `VersionString`.
    pub object_version: Option<u16>,
    /// `ContentType`, on eml 2.0 / 2.1 / 2.2 references.
    pub content_type: Option<u16>,
    /// `QualifiedType`, on eml 2.3 references.
    pub qualified_type: Option<u16>,
    /// `Uri` / `EnergisticsUri`.
    pub uri: Option<u16>,
}

impl Roles {
    /// No role at all — what an ordinary nested type carries.
    ///
    /// A `const` rather than `Default::default()`, because every `TypeInfo` is
    /// a `static` initializer and those cannot call trait methods.
    pub const EMPTY: Roles = Roles {
        uuid: None,
        title: None,
        citation: None,
        object_version: None,
        content_type: None,
        qualified_type: None,
        uri: None,
    };

    /// Whether the type carries any identity at all.
    pub fn is_empty(&self) -> bool {
        *self == Roles::EMPTY
    }
}

/// Static description of a generated type.
#[derive(Clone, Copy, Debug)]
pub struct TypeInfo {
    /// Rust type name.
    pub name: &'static str,
    /// Name in XML documents.
    pub xml_name: &'static str,
    /// Namespace this type's element belongs to.
    pub xml_ns: Namespace,
    /// Package short name: `eml`, `resqml`, `witsml` or `prodml`.
    pub package: &'static str,
    /// Package version, e.g. `2.2` — the URI alone cannot tell you this. This
    /// is the *directory* version: RESQML 2.0.1 reports `2.0.1` here.
    pub version: &'static str,
    /// Version as identifiers write it, e.g. `2.0` for RESQML 2.0.1. Content
    /// types use this form, qualified types its compact spelling.
    pub schema_version: &'static str,
    /// Complex type name, **verbatim**: `obj_Activity` keeps its prefix.
    ///
    /// This is what goes into a content type, a qualified type and an EPC part
    /// name. It differs from [`TypeInfo::xml_name`] — the *element* name —
    /// exactly where RESQML 2.0.1 and common 2.0 prefix their top-level types.
    pub object_type: &'static str,
    /// Canonical grouping key: [`TypeInfo::object_type`] without `obj_`, case
    /// preserved. Equal across versions of one concept, so a translator can key
    /// its rules on it.
    pub type_key: &'static str,
    /// EPC content type, for top-level objects only.
    pub content_type: Option<&'static str>,
    /// Energistics qualified type (e.g. `resqml22.TriangulatedSetRepresentation`),
    /// for top-level objects only.
    pub qualified_type: Option<&'static str>,
    /// The global element a part of this type is written under, when the
    /// schema gives the type none of its own — `FlowTestActivity` for PRODML
    /// 2.3's `InjectionFlowTest`, written
    /// `<prodml:FlowTestActivity xsi:type="prodml:InjectionFlowTest">`.
    /// `None` for every type that has an element. The part's identity stays
    /// the type's own ([`TypeInfo::content_type`], [`TypeInfo::qualified_type`])
    /// — see [`crate::registry::to_part`].
    pub root_element: Option<&'static str>,
    /// `xsd:documentation` for the type.
    pub doc: &'static str,
    /// Struct, enumeration or choice.
    pub kind: TypeKind,
    /// Whether the XSD type is abstract.
    pub is_abstract: bool,
    /// Whether the type is a top-level EnergyML object (derives from
    /// `eml:AbstractObject` and may be a part in an EPC).
    pub is_top_level: bool,
    /// Whether instances of this type may *also* appear nested inside another
    /// top-level object, instead of only as their own EPC part.
    ///
    /// A handful of WITSML, PRODML and RESQML 2.2 types derive from
    /// `AbstractObject` — so they are legal EPC parts — and are on top of that
    /// declared as children of another object. `witsml:Rig` and
    /// `witsml:Channel` are the usual examples. Listing the top-level objects
    /// of an EPC has to be able to include or exclude them without walking the
    /// tree, so it is decided at generation time.
    ///
    /// This is **not** "nested only": every type flagged here has its own
    /// global element and is written standalone in the wild. Filtering on it
    /// is the caller's decision, which is why [`Registry::top_level_types`]
    /// does not.
    ///
    /// [`Registry::top_level_types`]: crate::registry::Registry::top_level_types
    pub is_nestable: bool,
    /// Whether the type is a reference to an object rather than an object:
    /// `eml:DataObjectReference` and everything extending it.
    pub is_object_ref: bool,
    /// Where the identity-carrying fields are, as indices into
    /// [`TypeInfo::fields`]. Resolved at generation time, so dynamic access is
    /// an index rather than a search by name.
    pub roles: Roles,
    /// Base type in the XSD hierarchy. Generated structs flatten inherited
    /// fields, so this is metadata for translators rather than a layout detail.
    pub base: Option<TypeInfoFn>,
    /// Fields, for [`TypeKind::Struct`].
    pub fields: &'static [FieldInfo],
    /// Variants, for [`TypeKind::Enumeration`] and [`TypeKind::Choice`].
    pub variants: &'static [VariantInfo],
    /// Known direct subtypes, filled in by the generator.
    pub subtypes: &'static [TypeInfoFn],
    /// Whether an enumeration accepts values outside its declared set (the
    /// Energistics `...Ext` open enumerations).
    pub is_open: bool,
    /// What the free text of an open enumeration must match: the pattern of the
    /// union's widening member (`eml:EnumExtensionPattern`, `.*:.*`). `None`
    /// on a closed enumeration, and on an open one the schema leaves unshaped.
    pub other_pattern: Option<&'static str>,
    /// Build an empty instance of this type, without naming it.
    ///
    /// The piece reflection was missing to be able to *build* rather than only
    /// read. [`Registry::create`] does this for a registered **top-level**
    /// object, from a table keyed by qualified type; nothing could construct a
    /// nested one — a `Citation`, a `Point3dLatticeArray` — because a nested
    /// type is never in that table and has no qualified type to key it by.
    /// Anything walking the schema to instantiate it (a random-data generator,
    /// a "new object" form in a UI, a translator filling a target model)
    /// stopped at the first nested field.
    ///
    /// `None` where there is nothing to build: an abstract choice has no
    /// instance of its own, only of its variants. Read it through
    /// [`TypeInfo::create`], which says so by returning `None`.
    ///
    /// [`Registry::create`]: crate::registry::Registry::create
    pub make: Option<fn() -> Box<dyn crate::value::Reflect>>,
}

impl TypeInfo {
    /// An empty instance of this type, or `None` when it has none to give.
    ///
    /// Concrete structs and enumerations always build; an abstract choice does
    /// not, unless the schema names a default variant — an element with no
    /// `xsi:type` means the declared type, and that is the one thing a choice
    /// can be by itself. Callers that need an instance of an abstract type ask
    /// [`TypeInfo::concrete_subtypes`] first and build one of those.
    pub fn create(&self) -> Option<Box<dyn crate::value::Reflect>> {
        self.make.map(|make| make())
    }

    /// Look up a field by its Rust or XML name, case-sensitively.
    pub fn field(&self, name: &str) -> Option<&'static FieldInfo> {
        self.fields.iter().find(|f| f.name == name || f.xml_name == name)
    }

    /// Look up a field by its Rust or XML name, ignoring case.
    ///
    /// Lets callers write `citation.title` or `Citation.Title` interchangeably.
    pub fn field_ignore_case(&self, name: &str) -> Option<&'static FieldInfo> {
        self.fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name) || f.xml_name.eq_ignore_ascii_case(name))
    }

    /// Every concrete (non-abstract) type reachable from this one, including
    /// itself when it is already concrete.
    ///
    /// This is the list a UI offers when asking "what can I put here?".
    pub fn concrete_subtypes(&'static self) -> Vec<&'static TypeInfo> {
        let mut out = Vec::new();
        self.collect_concrete(&mut out);
        out
    }

    fn collect_concrete(&'static self, out: &mut Vec<&'static TypeInfo>) {
        if !self.is_abstract && self.kind != TypeKind::Choice {
            push_unique(out, self);
            return;
        }
        let linked = self
            .subtypes
            .iter()
            .map(|f| f())
            .chain(self.variants.iter().filter_map(|v| v.ty.map(|f| f())));
        for info in linked {
            if std::ptr::eq(info, self) {
                continue; // defensive: a self-link would recurse forever
            }
            info.collect_concrete(out);
        }
    }

    /// Whether this type derives, directly or transitively, from `other`.
    pub fn is_a(&self, other: &TypeInfo) -> bool {
        if self.name == other.name && self.version == other.version {
            return true;
        }
        let mut cur = self.base;
        while let Some(f) = cur {
            let info = f();
            if info.name == other.name && info.version == other.version {
                return true;
            }
            cur = info.base;
        }
        false
    }

    /// `package` and `version` joined, e.g. `resqml 2.2`. Handy for messages.
    pub fn package_version(&self) -> String {
        format!("{} {}", self.package, self.version)
    }
}
