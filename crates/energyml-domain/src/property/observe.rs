//! What a property's file says of it, as typed observations, beside the
//! `stats::describe::Observation`s that describe its values.
//!
//! The question this module answers: what does the **file** declare about a
//! property that a view should show next to its statistics (the shape and how
//! the store regroups it, the facets, the unit against the kind's quantity
//! class, the `synthetic` mark)? No value is read.
//!
//! The observations are typed and carry no sentence: a face says them in its
//! own words. **Nothing is said without proof**: an absence (no mark, no unit
//! to check for lack of a class) is either a named observation that says why,
//! or silence, never a supposed "all is well". A contradiction is a finding,
//! not a refusal (detect and signal, rather than forbid).

use energyml_core::value::Reflect;

use super::shape::{CountFit, PropertyShape, ShapeRefusal, StoredLayout};
use super::{Property, flatten, text};
use crate::property_kind::{self, KindIssue, Resolver};
use crate::source::ObjectSource;

/// Which object carries a mark.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkedObject {
    /// The property itself.
    Property,
    /// The representation the property hangs on.
    SupportingRepresentation,
}

/// An ADR-0019 mark: an `ExtensionNameValue` named `synthetic`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SyntheticMark {
    /// Value `true`; its `Description`, when written.
    Marked {
        /// The description of the entry.
        description: Option<String>,
    },
    /// A `synthetic` entry whose value is neither `true` nor `false`, as written.
    Unclear {
        /// The value as written.
        value: String,
    },
}

/// Why a unit could not be checked against its kind.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnitNotCheckable {
    /// The property states no unit (a discrete or boolean property).
    NoUnit,
    /// The property names no kind.
    NoKind,
    /// The kind does not resolve; the library's words, kept (the one text field).
    KindUnresolved {
        /// Why, as the resolver says it.
        why: String,
    },
    /// The kind resolves and declares no quantity class.
    NoQuantityClass {
        /// The title of the kind.
        kind: String,
    },
    /// The kind's class is one this build does not convert
    /// (`KindIssue::QuantityClassNotCheckable`).
    ClassNotInThisBuild {
        /// The class.
        class: String,
        /// The unit.
        uom: String,
    },
}

/// The unit against the kind's quantity class, as `property_kind::check` says it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnitCheck {
    /// Checked: the unit is of the class.
    Checked {
        /// The unit.
        uom: String,
        /// The class.
        class: String,
    },
    /// Checked: it is not (`KindIssue::UomOutsideQuantityClass`, carried as is).
    Outside(KindIssue),
    /// Not checked, and why.
    NotCheckable(UnitNotCheckable),
}

/// One thing a property's file says of it.
#[derive(Clone, PartialEq, Debug)]
#[non_exhaustive]
pub enum PropertyObservation {
    /// The declared shape, and how the store regroups it (`None`: not read, or
    /// not a regrouping of the declared one).
    Shape {
        /// The declared shape.
        shape: PropertyShape,
        /// How the stored dimensions regroup it.
        layout: Option<StoredLayout>,
    },
    /// The stored dimensions contradict the declared ones, by name.
    StoredDisagrees(ShapeRefusal),
    /// Several values per element declared, one per element held.
    DeclaredCountWrong {
        /// The count the document declares.
        declared: u64,
    },
    /// A facet value that is a producer's extension (EML TRG 2.3 §11.2 p. 284).
    ExtensionFacet {
        /// The facet kind, as written.
        kind: String,
        /// The value, as written.
        value: String,
    },
    /// A value of the schema's list under another facet kind than the one the
    /// schema documents for it.
    FacetValueOfOtherKind {
        /// The facet kind, as written.
        kind: String,
        /// The value, as written.
        value: String,
        /// The kind the schema documents for this value.
        schema_kind: &'static str,
    },
    /// Several facets of one kind (EML TRG 2.3 §11.11 p. 294).
    FacetKindRepeated {
        /// The kind, as first written.
        kind: String,
        /// How many facets carry it.
        count: usize,
    },
    /// The unit against the quantity class of the kind.
    Unit(UnitCheck),
    /// Another issue of the kind (`AbstractKindUsed`, `Deprecated`), as
    /// `property_kind::check` says it.
    Kind(KindIssue),
    /// An ADR-0019 mark on the property or on its supporting representation.
    Synthetic {
        /// Which object carries it.
        on: MarkedObject,
        /// The uri of that object.
        uri: String,
        /// The mark.
        mark: SyntheticMark,
    },
}

/// The facet kind the schema documents for each value of EML 2.3 `FacetExt`
/// ("Applies to ... facet kind", `energyml-eml-2-3` `common_types.rs`).
///
/// Values the schema documents no kind for (`plus`, `minus`, `fracture`,
/// `matrix`) are absent. A table, not a `match`: the list is the schema's, and
/// a test confronts it with the documentation the generated code carries.
pub const FACET_VALUE_KINDS: &[(&str, &str)] = &[
    ("I", "direction"),
    ("J", "direction"),
    ("K", "direction"),
    ("X", "direction"),
    ("Y", "direction"),
    ("Z", "direction"),
    ("I+", "direction"),
    ("J+", "direction"),
    ("K+", "direction"),
    ("X+", "direction"),
    ("Y+", "direction"),
    ("Z+", "direction"),
    ("I-", "direction"),
    ("J-", "direction"),
    ("K-", "direction"),
    ("X-", "direction"),
    ("Y-", "direction"),
    ("Z-", "direction"),
    ("net", "netgross"),
    ("gross", "netgross"),
    ("average", "statistics"),
    ("maximum", "statistics"),
    ("minimum", "statistics"),
    ("cumulative", "statistics"),
    ("maximum threshold", "qualifier"),
    ("minimum threshold", "qualifier"),
    ("surface condition", "conditions"),
    ("reservoir condition", "conditions"),
    ("oil", "what"),
    ("water", "what"),
    ("gas", "what"),
    ("condensate", "what"),
];

/// The kind the schema documents for a facet value; `None` when it documents
/// none. The comparison is exact, to the byte.
pub fn schema_facet_kind(value: &str) -> Option<&'static str> {
    FACET_VALUE_KINDS
        .iter()
        .find(|(known, _)| *known == value)
        .map(|(_, kind)| *kind)
}

/// Every observation on one property, in a stable order: the shape, the
/// facets, the kind and the unit, the marks.
///
/// `object` is the object `property` was read from; `shape` and `stored` are
/// given when the caller has them (`PropertyChunks::shape` and `::stored`);
/// `resolver` resolves its kind. No value is read.
pub fn observations(
    uri: &str,
    property: &Property<'_>,
    object: &dyn Reflect,
    shape: Option<&PropertyShape>,
    stored: Option<&[u64]>,
    objects: &dyn ObjectSource,
    resolver: &Resolver<'_>,
) -> Vec<PropertyObservation> {
    let mut out = Vec::new();
    shape_observations(shape, stored, &mut out);
    facet_observations(property, &mut out);
    kind_observations(property, resolver, &mut out);
    synthetic_observations(uri, property, object, objects, &mut out);
    out
}

/// The text of a facet's value.
fn facet_text(facet: &super::Facet) -> &str {
    facet.value.as_str()
}

fn shape_observations(shape: Option<&PropertyShape>, stored: Option<&[u64]>, out: &mut Vec<PropertyObservation>) {
    // The shape is never guessed: without one, nothing is said.
    let Some(shape) = shape else {
        return;
    };
    let Some(stored) = stored else {
        out.push(PropertyObservation::Shape {
            shape: shape.clone(),
            layout: None,
        });
        return;
    };
    match shape.layout(stored) {
        Ok(layout) => out.push(PropertyObservation::Shape {
            shape: shape.clone(),
            layout: Some(layout),
        }),
        Err(refusal) => {
            out.push(PropertyObservation::Shape {
                shape: shape.clone(),
                layout: None,
            });
            let held = stored.iter().fold(1u64, |total, dim| total.saturating_mul(*dim));
            match shape.count_fit(held) {
                CountFit::OnePerElement { declared } => {
                    out.push(PropertyObservation::DeclaredCountWrong { declared });
                }
                _ => out.push(PropertyObservation::StoredDisagrees(refusal)),
            }
        }
    }
}

fn facet_observations(property: &Property<'_>, out: &mut Vec<PropertyObservation>) {
    // In 2.0.1 a facet value is free text (RESQML TRG 2.0.1 §6.36 p. 77): the
    // 2.3 list does not bind it.
    let bound_by_the_list = !property.qualified_type.starts_with("resqml20.");
    for facet in &property.facets {
        let value = facet_text(facet);
        if facet.value.is_extension() {
            out.push(PropertyObservation::ExtensionFacet {
                kind: facet.kind.clone(),
                value: value.to_string(),
            });
        } else if bound_by_the_list
            && let Some(schema_kind) = schema_facet_kind(value)
            && !schema_kind.eq_ignore_ascii_case(&facet.kind)
        {
            out.push(PropertyObservation::FacetValueOfOtherKind {
                kind: facet.kind.clone(),
                value: value.to_string(),
                schema_kind,
            });
        }
    }
    // Several facets of one kind, in the order the kinds first appear.
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for facet in &property.facets {
        match counts.iter_mut().find(|(kind, _)| kind.eq_ignore_ascii_case(&facet.kind)) {
            Some((_, count)) => *count += 1,
            None => counts.push((&facet.kind, 1)),
        }
    }
    for (kind, count) in counts {
        if count > 1 {
            out.push(PropertyObservation::FacetKindRepeated {
                kind: kind.to_string(),
                count,
            });
        }
    }
}

fn kind_observations(property: &Property<'_>, resolver: &Resolver<'_>, out: &mut Vec<PropertyObservation>) {
    let Some(kind) = &property.kind else {
        out.push(PropertyObservation::Unit(UnitCheck::NotCheckable(UnitNotCheckable::NoKind)));
        return;
    };
    let resolution = match resolver.resolve_at(kind, property.kind_uri.as_deref()) {
        Ok(resolution) => resolution,
        Err(why) => {
            out.push(PropertyObservation::Unit(UnitCheck::NotCheckable(
                UnitNotCheckable::KindUnresolved { why: why.to_string() },
            )));
            return;
        }
    };
    let issues = property_kind::check(&resolution, property.uom.as_deref());
    for issue in &issues {
        if matches!(issue, KindIssue::AbstractKindUsed { .. } | KindIssue::Deprecated { .. }) {
            out.push(PropertyObservation::Kind(issue.clone()));
        }
    }
    let Some(uom) = &property.uom else {
        out.push(PropertyObservation::Unit(UnitCheck::NotCheckable(UnitNotCheckable::NoUnit)));
        return;
    };
    let Some(class) = resolution.quantity_class() else {
        out.push(PropertyObservation::Unit(UnitCheck::NotCheckable(
            UnitNotCheckable::NoQuantityClass {
                kind: resolution.kind().title.clone(),
            },
        )));
        return;
    };
    // With a unit and a class, `check` pushes either a finding about the unit
    // or nothing at all, in which case the class was checked and the unit is of
    // it (property_kind::check, the `(Some(uom), Some(class))` arm).
    let outside = issues
        .iter()
        .find(|issue| matches!(issue, KindIssue::UomOutsideQuantityClass { .. }));
    let not_checkable = issues
        .iter()
        .find(|issue| matches!(issue, KindIssue::QuantityClassNotCheckable { .. }));
    let unit = match (outside, not_checkable) {
        (Some(issue), _) => UnitCheck::Outside(issue.clone()),
        (None, Some(KindIssue::QuantityClassNotCheckable { class, uom })) => {
            UnitCheck::NotCheckable(UnitNotCheckable::ClassNotInThisBuild {
                class: class.clone(),
                uom: uom.clone(),
            })
        }
        _ => UnitCheck::Checked {
            uom: uom.clone(),
            class: class.to_string(),
        },
    };
    out.push(PropertyObservation::Unit(unit));
}

fn synthetic_observations(
    uri: &str,
    property: &Property<'_>,
    object: &dyn Reflect,
    objects: &dyn ObjectSource,
    out: &mut Vec<PropertyObservation>,
) {
    if let Some(mark) = synthetic_mark_of(object) {
        out.push(PropertyObservation::Synthetic {
            on: MarkedObject::Property,
            uri: uri.to_string(),
            mark,
        });
    }
    // A representation that cannot be read is silence: nothing is said without
    // proof, and reading the support is the view's business.
    if let Some(support) = &property.supporting_representation_uri
        && let Ok(representation) = objects.read_object_at(support)
        && let Some(mark) = synthetic_mark_of(representation.as_reflect())
    {
        out.push(PropertyObservation::Synthetic {
            on: MarkedObject::SupportingRepresentation,
            uri: support.clone(),
            mark,
        });
    }
}

/// The `synthetic` mark of an object (ADR-0019: an `ExtensionNameValue` named
/// `synthetic` whose value is `true`), when it carries one.
///
/// The first entry named `synthetic`, compared to the byte as the writers write
/// it. `true` is a mark, `false` is none, anything else is unclear. An object
/// without the field (a 2.0.1 object) says nothing.
fn synthetic_mark_of(object: &dyn Reflect) -> Option<SyntheticMark> {
    let entries = flatten(object.field("ExtensionNameValue")?);
    for item in entries {
        let Some(entry) = item.as_object() else {
            continue;
        };
        if text(entry, "Name").as_deref() != Some("synthetic") {
            continue;
        }
        let value = entry_value(entry)?;
        return match value.as_str() {
            "true" => Some(SyntheticMark::Marked {
                description: text(entry, "Description"),
            }),
            "false" => None,
            _ => Some(SyntheticMark::Unclear { value }),
        };
    }
    None
}

/// The text of an `ExtensionNameValue`'s `Value` (a `StringMeasure`, whose text
/// content is its `value` field).
fn entry_value(entry: &dyn Reflect) -> Option<String> {
    let value = entry.field("Value")?;
    match value.as_object() {
        Some(measure) => text(measure, "value"),
        None => match value.as_scalar()? {
            energyml_core::value::Scalar::Str(s) => Some(s.to_string()),
            other => Some(other.to_string()),
        },
    }
}
