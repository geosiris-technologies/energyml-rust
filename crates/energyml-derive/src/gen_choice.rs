//! Code generation for `#[derive(EnergymlChoice)]` — abstract XSD types and
//! substitution groups, mapped to a Rust enum whose variants are the concrete
//! subtypes and selected on the wire by `xsi:type`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DataEnum, DeriveInput, Fields, Ident, Type};

use crate::attrs::{Meta, doc_of};
use crate::common::Container;

const VARIANT_ATTRS: &[&str] = &["xsi_type", "xml_name", "implicit"];

struct Variant {
    ident: Ident,
    /// Payload as written, possibly `Box<T>`.
    payload: Type,
    /// Payload with `Box` stripped, used wherever the concrete type is needed.
    bare: Type,
    /// Whether the payload is boxed to break a cycle in the schema graph.
    boxed: bool,
    /// `xsi:type` as written in documents, e.g. `resqml:PointGeometry`.
    xsi_type: String,
    /// Local part used for matching, prefix and version stripped.
    key: String,
    /// Element local name when the subtype is selected by substitution rather
    /// than by `xsi:type`.
    xml_name: String,
    /// The declared type of the element: written without an `xsi:type`, and
    /// read when a document gives none.
    implicit: bool,
    doc: String,
}

/// `resqml:PointGeometry` and `resqml22.PointGeometry` both reduce to
/// `PointGeometry`, which is what documents actually disagree about.
fn local_part(value: &str) -> String {
    value.rsplit([':', '.']).next().unwrap_or(value).to_string()
}

pub fn expand(input: &DeriveInput, data: &DataEnum) -> syn::Result<TokenStream> {
    let container = Container::parse(input)?;
    let name = &input.ident;
    let name_str = name.to_string();

    let mut variants = Vec::new();
    for v in &data.variants {
        let meta = Meta::parse(&v.attrs)?;
        meta.deny_unknown(VARIANT_ATTRS, "variant")?;

        let Fields::Unnamed(f) = &v.fields else {
            return Err(syn::Error::new_spanned(
                v,
                "EnergymlChoice variants must hold exactly one payload type",
            ));
        };
        if f.unnamed.len() != 1 {
            return Err(syn::Error::new_spanned(
                v,
                "EnergymlChoice variants must hold exactly one payload type",
            ));
        }
        let payload = f.unnamed[0].ty.clone();
        let analyzed = crate::ty::analyze(&payload);
        let boxed = analyzed.boxed;
        let xsi_type = meta.string("xsi_type")?.unwrap_or_else(|| v.ident.to_string());
        let xml_name = meta.string("xml_name")?.unwrap_or_else(|| v.ident.to_string());

        variants.push(Variant {
            key: local_part(&xsi_type),
            doc: doc_of(&v.attrs),
            ident: v.ident.clone(),
            bare: analyzed.bare,
            boxed,
            payload,
            xsi_type,
            xml_name,
            implicit: meta.flag("implicit"),
        });
    }

    if variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "EnergymlChoice needs at least one variant",
        ));
    }

    let attr_arms = variants.iter().map(|v| {
        let ident = &v.ident;
        let xsi_type = &v.xsi_type;
        // The declared type needs no `xsi:type`, and writing one where other
        // implementations write none is exactly the kind of gratuitous
        // difference that makes a file look wrong to them.
        if v.implicit {
            return quote! {
                Self::#ident(__inner) => {
                    ::energyml_core::xml::XmlSerialize::xml_attributes(__inner, tag)?;
                }
            };
        }
        quote! {
            Self::#ident(__inner) => {
                ::energyml_core::xml::push_xsi_type(tag, #xsi_type);
                ::energyml_core::xml::XmlSerialize::xml_attributes(__inner, tag)?;
            }
        }
    });

    let child_arms = variants.iter().map(|v| {
        let ident = &v.ident;
        quote!(Self::#ident(__inner) => ::energyml_core::xml::XmlSerialize::xml_children(__inner, w))
    });

    let text_arms = variants.iter().map(|v| {
        let ident = &v.ident;
        quote!(Self::#ident(__inner) => ::energyml_core::xml::XmlSerialize::xml_text(__inner))
    });

    // Match on the local part of xsi:type first, then on the element name, so
    // both the xsi:type form and the substitution-group form are readable.
    let read_arms = variants.iter().map(|v| {
        let ident = &v.ident;
        let payload = &v.payload;
        let key = &v.key;
        let xml_name = &v.xml_name;
        if key == xml_name {
            quote! {
                #key => ::core::result::Result::Ok(Self::#ident(
                    <#payload as ::energyml_core::xml::XmlDeserialize>::xml_read(r, start)?
                ))
            }
        } else {
            quote! {
                #key | #xml_name => ::core::result::Result::Ok(Self::#ident(
                    <#payload as ::energyml_core::xml::XmlDeserialize>::xml_read(r, start)?
                ))
            }
        }
    });

    // A boxed payload has to be dereferenced before it is passed on: `Box<T>`
    // does not itself implement `Reflect`.
    let delegate = |body: TokenStream| {
        let arms = variants.iter().map(|v| {
            let ident = &v.ident;
            let b = body.clone();
            if v.boxed {
                quote!(Self::#ident(__boxed) => { let __inner = &**__boxed; #b })
            } else {
                quote!(Self::#ident(__inner) => #b)
            }
        });
        quote!(match self { #(#arms),* })
    };
    let delegate_mut = |body: TokenStream| {
        let arms = variants.iter().map(|v| {
            let ident = &v.ident;
            let b = body.clone();
            if v.boxed {
                quote!(Self::#ident(__boxed) => { let __inner = &mut **__boxed; #b })
            } else {
                quote!(Self::#ident(__inner) => #b)
            }
        });
        quote!(match self { #(#arms),* })
    };

    let reflect_type_info = delegate(quote!(::energyml_core::value::Reflect::type_info(__inner)));
    let reflect_field = delegate(quote!(::energyml_core::value::Reflect::field(__inner, name)));
    let reflect_set = delegate_mut(quote!(::energyml_core::value::Reflect::set_field(
        __inner, name, value
    )));
    let reflect_obj_mut = delegate_mut(quote!(::energyml_core::value::Reflect::field_object_mut(
        __inner, name, index
    )));
    let reflect_any = delegate(quote!(::energyml_core::value::Reflect::as_any(__inner)));
    let reflect_any_mut = delegate_mut(quote!(::energyml_core::value::Reflect::as_any_mut(__inner)));
    let validate_delegate = delegate(quote!(::energyml_core::validate::Validate::validate_into(
        __inner, ctx
    )));

    let into_any_arms = variants.iter().map(|v| {
        let ident = &v.ident;
        if v.boxed {
            // Already boxed; coerce rather than box a second time.
            quote!(Self::#ident(__inner) => __inner as ::std::boxed::Box<dyn ::core::any::Any>)
        } else {
            quote!(Self::#ident(__inner) => ::std::boxed::Box::new(__inner) as ::std::boxed::Box<dyn ::core::any::Any>)
        }
    });

    let downcast_attempts = variants.iter().map(|v| {
        let ident = &v.ident;
        let bare = &v.bare;
        let rebuild = if v.boxed {
            quote!(Self::#ident(__v))
        } else {
            quote!(Self::#ident(*__v))
        };
        quote! {
            __any = match __any.downcast::<#bare>() {
                ::core::result::Result::Ok(__v) => {
                    return ::core::result::Result::Ok(#rebuild);
                }
                ::core::result::Result::Err(__a) => __a,
            };
        }
    });

    let variant_entries = variants.iter().map(|v| {
        let variant_name = v.ident.to_string();
        let xml_name = &v.xml_name;
        let xsi_type = &v.xsi_type;
        let doc = &v.doc;
        let bare = &v.bare;
        quote! {
            ::energyml_core::reflect::VariantInfo {
                name: #variant_name,
                value: #xml_name,
                xsi_type: Some(#xsi_type),
                doc: #doc,
                ty: Some(<#bare as ::energyml_core::reflect::EnergymlType>::type_info),
                // A choice has no `other` hole: an element it does not know is
                // a read error, not a value.
                is_other: false,
            }
        }
    });

    // Without an `xsi:type`, an element declared with a concrete base is that
    // base — whatever the element happens to be named. `Range` typed
    // `PlaneAngleOperatingRange` is the common shape, and matching on the
    // element name alone would reject it.
    let implicit_fallback = match variants.iter().find(|v| v.implicit) {
        Some(v) => {
            let ident = &v.ident;
            let payload = &v.payload;
            let payload_name = quote!(<#payload as ::energyml_core::reflect::EnergymlType>::type_info().name);
            quote! {
                if __xsi.is_none() {
                    return ::core::result::Result::Ok(Self::#ident(
                        <#payload as ::energyml_core::xml::XmlDeserialize>::xml_read(r, start)?
                    ));
                }
                // An `xsi:type` naming something this build does not know.
                // Strictly that is a refusal, and it stays one: the document
                // says the element is a type we cannot construct, and reading
                // it as the base would silently drop whatever the subtype
                // added. Leniently, the base is still the best available
                // reading of the fields it *does* share — and the substitution
                // is recorded, so nothing is lost quietly.
                if r.is_lenient()
                    && let ::core::option::Option::Some(__t) = &__xsi
                {
                    r.record_repair(::energyml_core::xml::Repaired {
                        kind: "xsi:type",
                        name: #name_str.to_string(),
                        found: __t.clone(),
                        used: #payload_name.to_string(),
                        reason: "no variant of this choice carries that type in this build"
                            .to_string(),
                    });
                    return ::core::result::Result::Ok(Self::#ident(
                        <#payload as ::energyml_core::xml::XmlDeserialize>::xml_read(r, start)?
                    ));
                }
            }
        }
        None => quote! {},
    };

    // `#[default]` is only allowed on unit variants, so the impl is written
    // here. The declared type is the natural default: it is what an element
    // with no `xsi:type` means.
    let default_impl = match variants.iter().find(|v| v.implicit) {
        Some(v) => {
            let ident = &v.ident;
            quote! {
                impl ::core::default::Default for #name {
                    fn default() -> Self {
                        Self::#ident(::core::default::Default::default())
                    }
                }
            }
        }
        None => quote! {},
    };

    // A choice builds only when the schema names a default variant: an element
    // with no `xsi:type` means the declared type, and that is the one instance
    // an abstract type can have of its own. Otherwise there is nothing to
    // build, and saying `None` is how a caller learns to pick a subtype.
    let make = if variants.iter().any(|v| v.implicit) {
        quote!(::core::option::Option::Some(|| {
            ::std::boxed::Box::new(<#name as ::core::default::Default>::default())
                as ::std::boxed::Box<dyn ::energyml_core::value::Reflect>
        }))
    } else {
        quote!(::core::option::Option::None)
    };

    let type_info_body = container.type_info_body(
        quote!(::energyml_core::reflect::TypeKind::Choice),
        quote!(&[]),
        quote!(&[#(#variant_entries),*]),
        quote!(::energyml_core::reflect::Roles::EMPTY),
        make,
    );

    Ok(quote! {
        #default_impl

        impl ::energyml_core::reflect::EnergymlType for #name {
            fn type_info() -> &'static ::energyml_core::reflect::TypeInfo {
                static INFO: ::energyml_core::reflect::TypeInfo = #type_info_body;
                &INFO
            }
        }

        impl ::energyml_core::xml::XmlSerialize for #name {
            fn xml_attributes(
                &self,
                tag: &mut ::energyml_core::quick_xml::events::BytesStart<'_>,
            ) -> ::energyml_core::error::Result<()> {
                match self { #(#attr_arms),* }
                ::core::result::Result::Ok(())
            }

            fn xml_children(
                &self,
                w: &mut ::energyml_core::xml::XmlWriter<'_>,
            ) -> ::energyml_core::error::Result<()> {
                match self { #(#child_arms),* }
            }

            fn xml_text(&self) -> ::core::option::Option<::std::borrow::Cow<'_, str>> {
                match self { #(#text_arms),* }
            }
        }

        impl ::energyml_core::xml::XmlDeserialize for #name {
            fn xml_read(
                r: &mut ::energyml_core::xml::XmlReader<'_>,
                start: &::energyml_core::quick_xml::events::BytesStart<'_>,
            ) -> ::energyml_core::error::Result<Self> {
                let __xsi = ::energyml_core::xml::xsi_type(start)?;
                let __key = match &__xsi {
                    ::core::option::Option::Some(__t) => ::energyml_core::xml::local_part(__t).to_string(),
                    ::core::option::Option::None => ::energyml_core::xml::local_name(start),
                };
                match __key.as_str() {
                    #(#read_arms,)*
                    _ => {
                        #implicit_fallback
                        ::core::result::Result::Err(::energyml_core::error::Error::UnknownXsiType {
                            value: __xsi.unwrap_or(__key),
                            ty: #name_str,
                        })
                    }
                }
            }
        }

        // Reflection is transparent: callers see the concrete subtype, which is
        // what a UI and a translator both want.
        impl ::energyml_core::value::Reflect for #name {
            fn type_info(&self) -> &'static ::energyml_core::reflect::TypeInfo { #reflect_type_info }
            fn field(&self, name: &str) -> ::core::option::Option<::energyml_core::value::FieldValue<'_>> {
                #reflect_field
            }
            fn set_field(
                &mut self,
                name: &str,
                value: ::energyml_core::value::OwnedValue,
            ) -> ::energyml_core::error::Result<()> { #reflect_set }
            fn field_object_mut(
                &mut self,
                name: &str,
                index: ::core::option::Option<usize>,
            ) -> ::core::option::Option<&mut dyn ::energyml_core::value::Reflect> { #reflect_obj_mut }
            fn as_any(&self) -> &dyn ::core::any::Any { #reflect_any }
            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any { #reflect_any_mut }
            fn into_any(self: ::std::boxed::Box<Self>) -> ::std::boxed::Box<dyn ::core::any::Any> {
                match *self { #(#into_any_arms),* }
            }
        }

        impl ::energyml_core::value::AsFieldValue for #name {
            fn as_field_value(&self) -> ::energyml_core::value::FieldValue<'_> {
                ::energyml_core::value::FieldValue::Object(self)
            }
        }

        impl ::energyml_core::value::FromOwnedValue for #name {
            fn from_owned_value(
                value: ::energyml_core::value::OwnedValue,
            ) -> ::energyml_core::error::Result<Self> {
                match value {
                    ::energyml_core::value::OwnedValue::Object(__o) => {
                        let __found = ::energyml_core::value::Reflect::type_info(__o.as_ref()).name;
                        let mut __any = __o.into_any();
                        #(#downcast_attempts)*
                        ::core::result::Result::Err(::energyml_core::error::Error::TypeMismatch {
                            path: ::std::string::String::new(),
                            expected: #name_str.to_string(),
                            found: __found.to_string(),
                        })
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
            fn validate_into(&self, ctx: &mut ::energyml_core::validate::ValidationCtx<'_>) {
                #validate_delegate
            }
        }
    })
}
