//! Convention B: a `qualifier` facet that is the uri of another object.
//!
//! A column of a table, or a property, names the table it points at by **one**
//! facet of kind `qualifier` whose value is the full ETP uri of that table. The
//! convention is this library's own (EML 2.3 has no field for a link between a
//! column or a property and a table); it rests on three statements of the
//! standard:
//!
//! - EML TRG 2.3 §11.11 `PropertyKindFacet`, p. 294: *"Don't use several facets
//!   of the same kind for the same property."* - so one `qualifier` facet can
//!   carry one link, and a second `qualifier` facet beside a uri is a refusal,
//!   not a choice between two;
//! - EML TRG 2.3 §11.3 `FacetKind`, p. 285: `qualifier` is *"any other context
//!   not covered by the other facet types"* - so an ordinary qualifier
//!   (`cumulative`, `measured`) is not a link and is left alone;
//! - EML TRG 2.3 §11.2 `FacetExt`, p. 284: the value list is open, so a uri is
//!   a legal extension value.
//!
//! This is **the only** reading of that convention. A property's axes
//! (`component_axes`) and a column's link to a table call it; neither reads a
//! facet list a second way.

use energyml_core::identifiers::EtpUri;

use super::Facet;

/// Why convention B cannot be read on a list of facets.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QualifierRefusal {
    /// A uri and another `qualifier` facet.
    #[error(
        "{count} `qualifier` facets, one of them an ETP uri: EML TRG 2.3 §11.11 p. 294 allows one facet of a kind"
    )]
    Several {
        /// How many `qualifier` facets the list holds.
        count: usize,
    },
    /// A value that starts like an ETP uri and is not the uri of an object.
    #[error(
        "the qualifier facet `{value}` starts like an ETP uri and does not parse as the uri of an object"
    )]
    MalformedUri {
        /// The value, trimmed.
        value: String,
    },
}

/// The one reading of convention B: the uri of the object a list of facets
/// names.
///
/// `Ok(None)`: no `qualifier` facet holds a uri (an ordinary qualifier such as
/// `cumulative` is not a link). The object named is **not** checked to be a
/// table: each caller does that against what it needs.
///
/// # Errors
///
/// [`QualifierRefusal::Several`] when a uri sits beside another `qualifier`
/// facet; [`QualifierRefusal::MalformedUri`] when a value starts with `eml:`
/// and is not the uri of an object (a dataspace uri names no object).
pub fn qualifier_link(facets: &[Facet]) -> Result<Option<String>, QualifierRefusal> {
    let qualifiers: Vec<&Facet> = facets
        .iter()
        .filter(|facet| facet.kind.eq_ignore_ascii_case("qualifier"))
        .collect();
    let Some(link) = qualifiers
        .iter()
        .find(|facet| facet.value.as_str().trim().starts_with("eml:"))
    else {
        return Ok(None);
    };
    if qualifiers.len() > 1 {
        return Err(QualifierRefusal::Several {
            count: qualifiers.len(),
        });
    }
    let value = link.value.as_str().trim();
    match EtpUri::parse(value) {
        Ok(uri) if uri.uuid.is_some() => Ok(Some(value.to_string())),
        _ => Err(QualifierRefusal::MalformedUri {
            value: value.to_string(),
        }),
    }
}
