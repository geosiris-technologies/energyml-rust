//! `energyml_core::reflect::fields`: the one door for reading a field of a
//! reflected object. Each test states the policy line it pins and how to break
//! it.

use energyml_core::error::Error;
use energyml_core::prelude::*;
use energyml_core::reflect::fields;
use energyml_core::value::Reflect;

#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "eml", version = "2.3", xml_name = "Leaf")]
pub struct Leaf {
    #[energyml(elem, xml_name = "Name")]
    pub name: String,
}

#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "eml", version = "2.3", xml_name = "Probe")]
pub struct Probe {
    #[energyml(elem, xml_name = "Text")]
    pub text: String,
    #[energyml(elem, xml_name = "MaybeText")]
    pub maybe_text: Option<String>,
    #[energyml(elem, xml_name = "Real")]
    pub real: f64,
    #[energyml(elem, xml_name = "MaybeReal")]
    pub maybe_real: Option<f64>,
    #[energyml(elem, xml_name = "Signed")]
    pub signed: i64,
    #[energyml(elem, xml_name = "Unsigned")]
    pub unsigned: u64,
    #[energyml(elem, xml_name = "On")]
    pub on: bool,
    #[energyml(elem, xml_name = "MaybeOn")]
    pub maybe_on: Option<bool>,
    #[energyml(elem, xml_name = "Words")]
    pub words: Vec<String>,
    #[energyml(elem, xml_name = "Reals")]
    pub reals: Vec<f64>,
    #[energyml(elem, xml_name = "One")]
    pub one: Option<Leaf>,
    #[energyml(elem, xml_name = "Many")]
    pub many: Vec<Leaf>,
}

fn leaf(name: &str) -> Leaf {
    Leaf { name: name.into() }
}

/// An absent optional field is `None` (or empty), never a default; so is a
/// field name the type does not declare.
///
/// Break it: make `text` return `Some(String::new())` for `Null`, or let
/// `flag` return `Some(false)` for a missing field.
#[test]
fn absent_is_none_never_a_default() {
    let probe = Probe::default();
    assert_eq!(fields::text(&probe, "MaybeText"), None);
    assert_eq!(fields::number(&probe, "MaybeReal"), None);
    assert_eq!(fields::flag(&probe, "MaybeOn"), None);
    assert!(fields::object(&probe, "One").is_none());
    assert!(fields::objects(&probe, "One").is_empty());
    assert!(fields::objects(&probe, "Many").is_empty());
    assert!(fields::items(&probe, "Many").is_empty());
    assert_eq!(fields::text(&probe, "NoSuchField"), None);
    assert!(fields::texts(&probe, "NoSuchField").is_empty());
}

/// A blank string (empty or whitespace) reads as absent; text is returned as
/// written, a boolean and a number by their wire text.
///
/// Break it: drop the `trim().is_empty()` guard in `scalar_text`.
#[test]
fn blank_text_is_absent_and_text_is_not_trimmed() {
    let mut probe = Probe {
        text: "  ".into(),
        ..Probe::default()
    };
    assert_eq!(fields::text(&probe, "Text"), None);
    probe.text = String::new();
    assert_eq!(fields::text(&probe, "Text"), None);
    probe.text = " North ".into();
    assert_eq!(fields::text(&probe, "Text").as_deref(), Some(" North "));
    probe.on = true;
    probe.signed = -3;
    assert_eq!(fields::text(&probe, "On").as_deref(), Some("true"));
    assert_eq!(fields::text(&probe, "Signed").as_deref(), Some("-3"));
}

/// A field of the wrong shape is `None`: a number asked of a string, a flag
/// asked of an integer, a scalar asked of an object, an object asked of a list.
///
/// Break it: let `number` parse `Scalar::Str`, or let `flag` accept `Int`.
#[test]
fn a_wrong_shape_is_none() {
    let probe = Probe {
        text: "12.5".into(),
        signed: 1,
        one: Some(leaf("a")),
        many: vec![leaf("b"), leaf("c")],
        reals: vec![1.0, 2.0],
        ..Probe::default()
    };
    assert_eq!(fields::number(&probe, "Text"), None, "a string is not a number");
    assert_eq!(fields::flag(&probe, "Signed"), None, "an integer is not a flag");
    assert_eq!(fields::text(&probe, "One"), None, "an object is not text");
    assert!(
        fields::object(&probe, "Many").is_none(),
        "a list is not one object"
    );
    assert_eq!(fields::number(&probe, "Reals"), None, "a list is not a scalar");
}

/// The lexical readers are the named variant that does parse text.
///
/// Break it: make `number_lexical` ignore `Scalar::Str`.
#[test]
fn lexical_readers_parse_text_on_request() {
    let probe = Probe {
        text: " 12.5 ".into(),
        maybe_text: Some("true".into()),
        ..Probe::default()
    };
    assert_eq!(fields::number_lexical(&probe, "Text"), Some(12.5));
    assert_eq!(fields::integer_lexical(&probe, "Text"), None, "12.5 is not whole");
    assert_eq!(fields::flag_lexical(&probe, "MaybeText"), Some(true));
    assert_eq!(
        fields::number_lexical(&probe, "MaybeText"),
        None,
        "`true` is no number"
    );
    let nan = Probe {
        text: "NaN".into(),
        ..Probe::default()
    };
    assert_eq!(fields::number_lexical(&nan, "Text"), None, "not finite");
}

/// A double is an integer only when it is whole; an integer asked as unsigned
/// must not be negative.
///
/// Break it: replace `whole_f64_to_i64` by `Some(value as i64)`.
#[test]
fn integers_are_whole() {
    let whole = Probe {
        real: 2.0,
        signed: -4,
        unsigned: 7,
        ..Probe::default()
    };
    assert_eq!(fields::integer(&whole, "Real"), Some(2));
    assert_eq!(fields::integer(&whole, "Signed"), Some(-4));
    assert_eq!(fields::integer(&whole, "Unsigned"), Some(7));
    assert_eq!(fields::unsigned(&whole, "Real"), Some(2));
    assert_eq!(fields::unsigned(&whole, "Signed"), None, "negative");
    let fractional = Probe {
        real: 2.5,
        ..Probe::default()
    };
    assert_eq!(fields::integer(&fractional, "Real"), None);
    assert_eq!(fields::unsigned(&fractional, "Real"), None);
    assert_eq!(fields::number(&fractional, "Real"), Some(2.5));
    let huge = Probe {
        unsigned: u64::MAX,
        ..Probe::default()
    };
    assert_eq!(fields::integer(&huge, "Unsigned"), None, "does not fit an i64");
    assert_eq!(fields::unsigned(&huge, "Unsigned"), Some(u64::MAX));
}

/// A singular field and a repeated one read alike through `objects` and
/// `items`; scalars come out in order.
///
/// Break it: make `objects` return an empty `Vec` for `FieldValue::Object`.
#[test]
fn one_or_many_read_alike() {
    let probe = Probe {
        one: Some(leaf("a")),
        many: vec![leaf("b"), leaf("c")],
        words: vec!["x".into(), " ".into(), "y".into()],
        reals: vec![1.5, 2.5],
        ..Probe::default()
    };
    let one = fields::objects(&probe, "One");
    assert_eq!(one.len(), 1);
    assert_eq!(fields::text(one[0], "Name").as_deref(), Some("a"));
    let many = fields::objects(&probe, "Many");
    let names: Vec<_> = many.iter().filter_map(|o| fields::text(*o, "Name")).collect();
    assert_eq!(names, ["b", "c"]);
    assert_eq!(fields::items(&probe, "One").len(), 1);
    assert_eq!(fields::items(&probe, "Many").len(), 2);
    assert_eq!(
        fields::texts(&probe, "Words"),
        ["x", " ", "y"],
        "a list keeps its positions"
    );
    assert_eq!(fields::numbers(&probe, "Reals"), [1.5, 2.5]);
}

/// The `require_*` readers refuse by name: missing when absent, a mismatch
/// when the field is there with another shape.
///
/// Break it: make `required` return `MissingRequired` for every failure.
#[test]
fn require_refuses_by_name() {
    let probe = Probe {
        text: "t".into(),
        one: Some(leaf("a")),
        ..Probe::default()
    };
    assert_eq!(fields::require_text(&probe, "Text").unwrap(), "t");
    assert!(matches!(
        fields::require_text(&probe, "MaybeText"),
        Err(Error::MissingRequired { .. })
    ));
    assert!(matches!(
        fields::require_number(&probe, "Text"),
        Err(Error::TypeMismatch { .. })
    ));
    assert!(matches!(
        fields::require_unsigned(&probe, "One"),
        Err(Error::TypeMismatch { .. })
    ));
    let blank = Probe {
        text: "  ".into(),
        ..Probe::default()
    };
    assert!(matches!(
        fields::require_text(&blank, "Text"),
        Err(Error::MissingRequired { .. })
    ));
    let dynamic: &dyn Reflect = &probe;
    assert_eq!(fields::require_unsigned(dynamic, "Unsigned").unwrap(), 0);
}
