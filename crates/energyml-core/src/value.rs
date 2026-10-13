//! Dynamic access to generated objects.
//!
//! [`Reflect`] is object-safe: it is what a UI, a translator or a random-data
//! generator uses to walk an instance without knowing its concrete type. Values
//! are borrowed through [`FieldValue`] and written back through [`OwnedValue`].

use std::any::Any;
use std::fmt;

use crate::error::{Error, Result};
use crate::reflect::{FieldInfo, ScalarKind, TypeInfo};

/// A primitive value borrowed from an object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scalar<'a> {
    /// `xsd:boolean`
    Bool(bool),
    /// Any signed integer type.
    Int(i64),
    /// Any unsigned integer type.
    UInt(u64),
    /// `xsd:double` / `xsd:float`
    Double(f64),
    /// Any string-like value, including dateTime and binary in lexical form.
    Str(&'a str),
}

impl Scalar<'_> {
    /// Kind of this scalar, for error reporting and facet checks.
    pub fn kind(&self) -> ScalarKind {
        match self {
            Scalar::Bool(_) => ScalarKind::Bool,
            Scalar::Int(_) => ScalarKind::Int,
            Scalar::UInt(_) => ScalarKind::UInt,
            Scalar::Double(_) => ScalarKind::Double,
            Scalar::Str(_) => ScalarKind::String,
        }
    }

    /// Numeric view, for facet checks on ordered types.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Scalar::Int(v) => Some(v as f64),
            Scalar::UInt(v) => Some(v as f64),
            Scalar::Double(v) => Some(v),
            _ => None,
        }
    }

    /// String view, for length and pattern facets.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Scalar::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for Scalar<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scalar::Bool(v) => write!(f, "{v}"),
            Scalar::Int(v) => write!(f, "{v}"),
            Scalar::UInt(v) => write!(f, "{v}"),
            Scalar::Double(v) => write!(f, "{v}"),
            Scalar::Str(v) => write!(f, "{v}"),
        }
    }
}

/// A value borrowed out of an object through [`Reflect`].
pub enum FieldValue<'a> {
    /// An absent optional field.
    Null,
    /// A primitive.
    Scalar(Scalar<'a>),
    /// A nested generated object.
    Object(&'a dyn Reflect),
    /// A repeated field.
    List(Vec<FieldValue<'a>>),
}

impl<'a> FieldValue<'a> {
    /// Whether the field is absent or an empty collection.
    pub fn is_empty(&self) -> bool {
        match self {
            FieldValue::Null => true,
            FieldValue::List(items) => items.is_empty(),
            _ => false,
        }
    }

    /// Number of occurrences, for cardinality checks.
    pub fn occurrences(&self) -> usize {
        match self {
            FieldValue::Null => 0,
            FieldValue::List(items) => items.len(),
            _ => 1,
        }
    }

    /// Borrow as a scalar, if it is one.
    pub fn as_scalar(&self) -> Option<Scalar<'a>> {
        match self {
            FieldValue::Scalar(s) => Some(*s),
            _ => None,
        }
    }

    /// Borrow as an object, if it is one.
    pub fn as_object(&self) -> Option<&'a dyn Reflect> {
        match self {
            FieldValue::Object(o) => Some(*o),
            _ => None,
        }
    }

    /// Kind name, for error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            FieldValue::Null => "null",
            FieldValue::Scalar(_) => "scalar",
            FieldValue::Object(_) => "object",
            FieldValue::List(_) => "list",
        }
    }
}

impl fmt::Debug for FieldValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldValue::Null => write!(f, "Null"),
            FieldValue::Scalar(s) => write!(f, "Scalar({s})"),
            FieldValue::Object(o) => write!(f, "Object({})", o.type_info().name),
            FieldValue::List(items) => f.debug_list().entries(items.iter()).finish(),
        }
    }
}

/// An owned value handed to a reflective setter.
pub enum OwnedValue {
    /// Clear an optional field.
    Null,
    /// `xsd:boolean`
    Bool(bool),
    /// Signed integer.
    Int(i64),
    /// Unsigned integer.
    UInt(u64),
    /// Floating point.
    Double(f64),
    /// String, or the lexical form of a dateTime / binary / enumeration value.
    Str(String),
    /// A generated object, boxed so it can cross the dynamic boundary.
    Object(Box<dyn Reflect>),
    /// A repeated field's contents.
    List(Vec<OwnedValue>),
}

impl OwnedValue {
    /// Kind name, for error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            OwnedValue::Null => "null",
            OwnedValue::Bool(_) => "boolean",
            OwnedValue::Int(_) => "integer",
            OwnedValue::UInt(_) => "unsigned integer",
            OwnedValue::Double(_) => "double",
            OwnedValue::Str(_) => "string",
            OwnedValue::Object(_) => "object",
            OwnedValue::List(_) => "list",
        }
    }
}

impl fmt::Debug for OwnedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OwnedValue::{}", self.kind_name())
    }
}

/// Dynamic access to a generated object.
///
/// Object-safe on purpose: `&dyn Reflect` is the currency of the UI, translator
/// and validation layers.
pub trait Reflect: Any {
    /// Static description of the concrete type.
    fn type_info(&self) -> &'static TypeInfo;

    /// Borrow a field by Rust or XML name.
    fn field(&self, name: &str) -> Option<FieldValue<'_>>;

    /// Replace a field's value.
    fn set_field(&mut self, name: &str, value: OwnedValue) -> Result<()>;

    /// Borrow a nested object mutably, so a path-based setter can descend.
    ///
    /// `index` selects an element of a repeated field.
    fn field_object_mut(&mut self, name: &str, index: Option<usize>) -> Option<&mut dyn Reflect>;

    /// Upcast for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Mutable upcast for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Consuming upcast, so a boxed dynamic object can be downcast by value.
    fn into_any(self: Box<Self>) -> Box<dyn Any>;

    /// Field descriptions, in schema order.
    fn fields(&self) -> &'static [FieldInfo] {
        self.type_info().fields
    }

    /// Borrow a field by name, ignoring case.
    fn field_ignore_case(&self, name: &str) -> Option<FieldValue<'_>> {
        let info = self.type_info().field_ignore_case(name)?;
        self.field(info.name)
    }
}

impl dyn Reflect {
    /// Downcast to a concrete generated type.
    pub fn downcast_ref<T: Reflect>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Mutably downcast to a concrete generated type.
    pub fn downcast_mut<T: Reflect>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

/// Borrow a Rust value as a [`FieldValue`].
///
/// Implemented in this crate for primitives, `Option` and `Vec`; the derive
/// macro emits an implementation for every generated type.
pub trait AsFieldValue {
    /// Borrow as a reflective value.
    fn as_field_value(&self) -> FieldValue<'_>;
}

/// Build a Rust value from an [`OwnedValue`] handed to a reflective setter.
pub trait FromOwnedValue: Sized {
    /// Convert, or explain why the value does not fit.
    fn from_owned_value(value: OwnedValue) -> Result<Self>;
}

fn mismatch<T>(expected: &str, value: &OwnedValue) -> Result<T> {
    Err(Error::TypeMismatch {
        path: String::new(),
        expected: expected.to_string(),
        found: value.kind_name().to_string(),
    })
}

impl AsFieldValue for bool {
    fn as_field_value(&self) -> FieldValue<'_> {
        FieldValue::Scalar(Scalar::Bool(*self))
    }
}

impl FromOwnedValue for bool {
    fn from_owned_value(value: OwnedValue) -> Result<Self> {
        match value {
            OwnedValue::Bool(v) => Ok(v),
            other => mismatch("boolean", &other),
        }
    }
}

macro_rules! impl_scalar_value {
    ($ty:ty, $variant:ident, $owned:ident, $name:literal, $conv:expr) => {
        impl AsFieldValue for $ty {
            fn as_field_value(&self) -> FieldValue<'_> {
                FieldValue::Scalar(Scalar::$variant((*self).into()))
            }
        }
        impl FromOwnedValue for $ty {
            fn from_owned_value(value: OwnedValue) -> Result<Self> {
                #[allow(clippy::redundant_closure_call)]
                match value {
                    OwnedValue::$owned(v) => ($conv)(v),
                    other => mismatch($name, &other),
                }
            }
        }
    };
}

impl_scalar_value!(f64, Double, Double, "double", |v: f64| Ok(v));
impl_scalar_value!(f32, Double, Double, "double", |v: f64| Ok(v as f32));
impl_scalar_value!(i64, Int, Int, "integer", |v: i64| Ok(v));
impl_scalar_value!(i32, Int, Int, "integer", |v: i64| i32::try_from(v)
    .map_err(|_| Error::parse(v.to_string(), "i32", "out of range")));
impl_scalar_value!(i16, Int, Int, "integer", |v: i64| i16::try_from(v)
    .map_err(|_| Error::parse(v.to_string(), "i16", "out of range")));
impl_scalar_value!(i8, Int, Int, "integer", |v: i64| i8::try_from(v)
    .map_err(|_| Error::parse(v.to_string(), "i8", "out of range")));
impl_scalar_value!(u64, UInt, UInt, "unsigned integer", |v: u64| Ok(v));
impl_scalar_value!(u32, UInt, UInt, "unsigned integer", |v: u64| u32::try_from(v)
    .map_err(|_| Error::parse(v.to_string(), "u32", "out of range")));
impl_scalar_value!(u16, UInt, UInt, "unsigned integer", |v: u64| u16::try_from(v)
    .map_err(|_| Error::parse(v.to_string(), "u16", "out of range")));
impl_scalar_value!(u8, UInt, UInt, "unsigned integer", |v: u64| u8::try_from(v)
    .map_err(|_| Error::parse(v.to_string(), "u8", "out of range")));

impl AsFieldValue for String {
    fn as_field_value(&self) -> FieldValue<'_> {
        FieldValue::Scalar(Scalar::Str(self.as_str()))
    }
}

impl FromOwnedValue for String {
    fn from_owned_value(value: OwnedValue) -> Result<Self> {
        match value {
            OwnedValue::Str(s) => Ok(s),
            OwnedValue::Bool(v) => Ok(v.to_string()),
            OwnedValue::Int(v) => Ok(v.to_string()),
            OwnedValue::UInt(v) => Ok(v.to_string()),
            OwnedValue::Double(v) => Ok(v.to_string()),
            other => mismatch("string", &other),
        }
    }
}

impl<T: AsFieldValue> AsFieldValue for Option<T> {
    fn as_field_value(&self) -> FieldValue<'_> {
        match self {
            None => FieldValue::Null,
            Some(v) => v.as_field_value(),
        }
    }
}

impl<T: FromOwnedValue> FromOwnedValue for Option<T> {
    fn from_owned_value(value: OwnedValue) -> Result<Self> {
        match value {
            OwnedValue::Null => Ok(None),
            other => Ok(Some(T::from_owned_value(other)?)),
        }
    }
}

impl<T: AsFieldValue> AsFieldValue for Vec<T> {
    fn as_field_value(&self) -> FieldValue<'_> {
        FieldValue::List(self.iter().map(|v| v.as_field_value()).collect())
    }
}

impl<T: FromOwnedValue> FromOwnedValue for Vec<T> {
    fn from_owned_value(value: OwnedValue) -> Result<Self> {
        match value {
            OwnedValue::Null => Ok(Vec::new()),
            OwnedValue::List(items) => items.into_iter().map(T::from_owned_value).collect(),
            // Setting a single value on a repeated field is a common UI action.
            other => Ok(vec![T::from_owned_value(other)?]),
        }
    }
}

impl<T: AsFieldValue> AsFieldValue for Box<T> {
    fn as_field_value(&self) -> FieldValue<'_> {
        (**self).as_field_value()
    }
}

impl<T: FromOwnedValue> FromOwnedValue for Box<T> {
    fn from_owned_value(value: OwnedValue) -> Result<Self> {
        Ok(Box::new(T::from_owned_value(value)?))
    }
}

/// Downcast a boxed dynamic object handed to a setter.
///
/// Used by derive-generated `set_field` implementations for object fields.
pub fn downcast_owned<T: Reflect>(value: OwnedValue, expected: &'static str) -> Result<T> {
    match value {
        OwnedValue::Object(obj) => {
            let found = obj.type_info().name;
            match obj.into_any().downcast::<T>() {
                Ok(v) => Ok(*v),
                Err(_) => Err(Error::TypeMismatch {
                    path: String::new(),
                    expected: expected.to_string(),
                    found: found.to_string(),
                }),
            }
        }
        other => mismatch(expected, &other),
    }
}
