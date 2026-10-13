//! Code generation for `#[derive(EnergymlEnum)]` - XSD `simpleType`
//! enumerations, including the open `...Ext` ones that also accept free text.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DataEnum, DeriveInput, Fields, Ident};

use crate::attrs::{Meta, doc_of};
use crate::common::Container;

const VARIANT_ATTRS: &[&str] = &["value", "other"];

struct Variant {
    ident: Ident,
    value: String,
    doc: String,
    /// The catch-all variant of an open enumeration, holding the raw string.
    is_other: bool,
}

pub fn expand(input: &DeriveInput, data: &DataEnum) -> syn::Result<TokenStream> {
    let container = Container::parse(input)?;
    let name = &input.ident;
    let name_str = name.to_string();

    let mut variants = Vec::new();
    for v in &data.variants {
        let meta = Meta::parse(&v.attrs)?;
        meta.deny_unknown(VARIANT_ATTRS, "variant")?;
        let is_other = meta.flag("other");

        match (&v.fields, is_other) {
            (Fields::Unit, false) => {}
            (Fields::Unnamed(f), true) if f.unnamed.len() == 1 => {}
            (_, true) => {
                return Err(syn::Error::new_spanned(
                    v,
                    "an `other` variant must hold exactly one String",
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    v,
                    "EnergymlEnum variants must be unit variants, except the `other` variant",
                ));
            }
        }

        variants.push(Variant {
            value: meta.string("value")?.unwrap_or_else(|| v.ident.to_string()),
            doc: doc_of(&v.attrs),
            ident: v.ident.clone(),
            is_other,
        });
    }

    if variants.iter().filter(|v| v.is_other).count() > 1 {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "at most one `other` variant is allowed",
        ));
    }
    let other = variants.iter().find(|v| v.is_other);
    if other.is_some() && !container.is_open {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "an `other` variant requires `#[energyml(open)]` on the enum",
        ));
    }

    // **What an open enumeration's free text must still be.** Without this,
    // `open` means *anything*, where the schema says *anything matching the
    // widening member's pattern* - in practice `.*:.*`, an authority and a name
    // around a mandatory colon. A `uom="unknown"` went unreported here for
    // exactly that reason while three other validators refused it (07/09).
    //
    // Only the `Other` arm is checked: every named variant is a value the schema
    // lists, so it is valid by construction and testing it would be work done on
    // every read of every unit in a package.
    let other_check = match (&container.other_pattern, other) {
        (Some(pattern), Some(v)) => {
            let ident = &v.ident;
            quote! {
                if let Self::#ident(__free) = self {
                    _ctx.check_facets(
                        &::energyml_core::reflect::Facets {
                            min_inclusive: None,
                            max_inclusive: None,
                            min_exclusive: None,
                            max_exclusive: None,
                            min_length: None,
                            max_length: None,
                            pattern: Some(#pattern),
                            fixed: None,
                            lexical: None,
                        },
                        ::energyml_core::value::Scalar::Str(__free.as_str()),
                    );
                }
            }
        }
        _ => quote!(),
    };

    let as_str_arms = variants.iter().map(|v| {
        let ident = &v.ident;
        let value = &v.value;
        if v.is_other {
            quote!(Self::#ident(__s) => __s.as_str())
        } else {
            quote!(Self::#ident => #value)
        }
    });

    let parse_arms = variants.iter().filter(|v| !v.is_other).map(|v| {
        let ident = &v.ident;
        let value = &v.value;
        quote!(#value => ::core::result::Result::Ok(Self::#ident))
    });

    let fallback = match other {
        Some(v) => {
            let ident = &v.ident;
            quote!(__other => ::core::result::Result::Ok(Self::#ident(__other.to_string())))
        }
        None => quote! {
            __other => ::core::result::Result::Err(::energyml_core::error::Error::parse(
                __other,
                #name_str,
                "not one of the values declared by the schema",
            ))
        },
    };

    let variant_entries = variants.iter().map(|v| {
        let variant_name = v.ident.to_string();
        let value = &v.value;
        let doc = &v.doc;
        let is_other = v.is_other;
        quote! {
            ::energyml_core::reflect::VariantInfo {
                name: #variant_name,
                value: #value,
                xsi_type: None,
                doc: #doc,
                ty: None,
                is_other: #is_other,
            }
        }
    });

    let type_info_body = container.type_info_body(
        quote!(::energyml_core::reflect::TypeKind::Enumeration),
        quote!(&[]),
        quote!(&[#(#variant_entries),*]),
        quote!(::energyml_core::reflect::Roles::EMPTY),
        quote!(::core::option::Option::Some(|| {
            ::std::boxed::Box::new(<#name as ::core::default::Default>::default())
                as ::std::boxed::Box<dyn ::energyml_core::value::Reflect>
        })),
    );

    Ok(quote! {
        impl #name {
            /// Lexical value used in XML and JSON documents.
            pub fn as_xml_str(&self) -> &str {
                match self { #(#as_str_arms),* }
            }
        }

        impl ::energyml_core::reflect::EnergymlType for #name {
            fn type_info() -> &'static ::energyml_core::reflect::TypeInfo {
                static INFO: ::energyml_core::reflect::TypeInfo = #type_info_body;
                &INFO
            }
        }

        impl ::energyml_core::xml::XmlText for #name {
            fn to_xml_text(&self) -> ::std::borrow::Cow<'_, str> {
                ::std::borrow::Cow::Borrowed(self.as_xml_str())
            }
            fn from_xml_text(s: &str) -> ::energyml_core::error::Result<Self> {
                match s.trim() {
                    #(#parse_arms,)*
                    #fallback,
                }
            }
        }

        impl ::energyml_core::xml::XmlSerialize for #name {
            fn xml_text(&self) -> ::core::option::Option<::std::borrow::Cow<'_, str>> {
                ::core::option::Option::Some(::std::borrow::Cow::Borrowed(self.as_xml_str()))
            }
        }

        impl ::energyml_core::xml::XmlDeserialize for #name {
            fn xml_read(
                r: &mut ::energyml_core::xml::XmlReader<'_>,
                _start: &::energyml_core::quick_xml::events::BytesStart<'_>,
            ) -> ::energyml_core::error::Result<Self> {
                let __text = r.read_text()?;
                // Through the reader rather than straight to `from_xml_text`,
                // so a closed enumeration outside its list can be repaired in
                // lenient mode - with the string that was there recorded,
                // since the variant cannot hold it.
                r.value::<Self>(&__text, "enumeration", #name_str)
            }
        }

        // An enumeration reads as a scalar, but still implements `Reflect` so
        // that struct fields holding one are uniform with object fields.
        impl ::energyml_core::value::Reflect for #name {
            fn type_info(&self) -> &'static ::energyml_core::reflect::TypeInfo {
                <Self as ::energyml_core::reflect::EnergymlType>::type_info()
            }
            fn field(&self, _name: &str) -> ::core::option::Option<::energyml_core::value::FieldValue<'_>> {
                ::core::option::Option::None
            }
            fn set_field(
                &mut self,
                name: &str,
                _value: ::energyml_core::value::OwnedValue,
            ) -> ::energyml_core::error::Result<()> {
                ::core::result::Result::Err(::energyml_core::error::Error::UnknownField {
                    ty: #name_str,
                    field: name.to_string(),
                })
            }
            fn field_object_mut(
                &mut self,
                _name: &str,
                _index: ::core::option::Option<usize>,
            ) -> ::core::option::Option<&mut dyn ::energyml_core::value::Reflect> {
                ::core::option::Option::None
            }
            fn as_any(&self) -> &dyn ::core::any::Any { self }
            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any { self }
            fn into_any(self: ::std::boxed::Box<Self>) -> ::std::boxed::Box<dyn ::core::any::Any> { self }
        }

        impl ::energyml_core::value::AsFieldValue for #name {
            fn as_field_value(&self) -> ::energyml_core::value::FieldValue<'_> {
                ::energyml_core::value::FieldValue::Scalar(
                    ::energyml_core::value::Scalar::Str(self.as_xml_str())
                )
            }
        }

        impl ::energyml_core::value::FromOwnedValue for #name {
            fn from_owned_value(
                value: ::energyml_core::value::OwnedValue,
            ) -> ::energyml_core::error::Result<Self> {
                match value {
                    ::energyml_core::value::OwnedValue::Str(s) => {
                        <Self as ::energyml_core::xml::XmlText>::from_xml_text(&s)
                    }
                    other => ::core::result::Result::Err(
                        ::energyml_core::error::Error::TypeMismatch {
                            path: ::std::string::String::new(),
                            expected: #name_str.to_string(),
                            found: other.kind_name().to_string(),
                        }
                    ),
                }
            }
        }

        impl ::energyml_core::validate::Validate for #name {
            fn validate_into(&self, _ctx: &mut ::energyml_core::validate::ValidationCtx<'_>) {
                #other_check
            }
        }

    })
}
