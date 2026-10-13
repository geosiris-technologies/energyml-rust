//! Code generation for `#[derive(EnergymlObject)]` on structs.

use heck::ToUpperCamelCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{DataStruct, DeriveInput, Fields, Ident, Type};

use crate::attrs::{MaxOccurs, Meta, doc_of};
use crate::common::{Container, namespace_expr, occurs_expr};
use crate::ty::{self, Wrapper};

const FIELD_ATTRS: &[&str] = &[
    "attr",
    "elem",
    "text",
    "any",
    "list",
    "skip",
    "role",
    "xml_name",
    "json_name",
    "ns",
    "prefix",
    "uri",
    "min",
    "max",
    "scalar",
    "min_inclusive",
    "max_inclusive",
    "min_exclusive",
    "max_exclusive",
    "min_length",
    "max_length",
    "pattern",
    "fixed",
    "lexical",
];

/// How one field is bound to XML.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Attribute,
    Element,
    Text,
    Any,
}

struct Field {
    ident: Ident,
    doc: String,
    binding: Binding,
    xml_name: String,
    json_name: String,
    ns: TokenStream,
    occurs_min: u32,
    occurs_max: MaxOccurs,
    wrapper: Wrapper,
    inner: Type,
    bare: Type,
    boxed: bool,
    scalar: Option<TokenStream>,
    facets: TokenStream,
    /// `xsd:list`: one element whose text holds every value, whitespace
    /// separated, rather than one element per value.
    list: bool,
    /// `role = "uuid"`, when the generated code states one explicitly.
    role: Option<String>,
    /// Local binding used while deserializing.
    slot: Ident,
}

impl Field {
    fn is_object(&self) -> bool {
        self.scalar.is_none()
    }
}

pub fn expand(input: &DeriveInput, data: &DataStruct) -> syn::Result<TokenStream> {
    let container = Container::parse(input)?;
    let name = &input.ident;

    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "EnergymlObject requires a struct with named fields",
        ));
    };

    let mut fields = Vec::new();
    for f in &named.named {
        let meta = Meta::parse(&f.attrs)?;
        meta.deny_unknown(FIELD_ATTRS, "field")?;
        if meta.flag("skip") {
            continue;
        }
        let ident = f.ident.clone().expect("named field");
        let analyzed = ty::analyze(&f.ty);

        let binding = if meta.flag("attr") {
            Binding::Attribute
        } else if meta.flag("any") {
            Binding::Any
        } else if meta.flag("text") {
            Binding::Text
        } else {
            Binding::Element
        };

        let default_xml_name = match binding {
            // XSD attributes are lowerCamelCase, elements UpperCamelCase.
            Binding::Attribute => ident.to_string(),
            _ => ident.to_string().to_upper_camel_case(),
        };
        let xml_name = meta.string("xml_name")?.unwrap_or(default_xml_name);
        let json_name = meta.string("json_name")?.unwrap_or_else(|| xml_name.clone());

        let (occurs_min, occurs_max) = match analyzed.wrapper {
            Wrapper::Plain => (1, MaxOccurs::Bounded(1)),
            Wrapper::Optional => (0, MaxOccurs::Bounded(1)),
            Wrapper::Repeated => (0, MaxOccurs::Unbounded),
        };
        let occurs_min = meta.u32("min")?.unwrap_or(occurs_min);
        let occurs_max = meta.max_occurs()?.unwrap_or(occurs_max);

        let list = meta.flag("list");
        if list {
            if analyzed.wrapper != Wrapper::Repeated {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    "`list` describes an xsd:list, whose values all live in one \
                     element: the field has to be a `Vec<T>`",
                ));
            }
            if binding == Binding::Text {
                return Err(syn::Error::new_spanned(
                    &f.ty,
                    "`list` and `text` cannot be combined: an xsd:list is the text \
                     of its own element, not of the parent",
                ));
            }
        }

        let scalar = match meta.string("scalar")? {
            Some(n) => Some(
                ty::scalar_kind_from_name(&n)
                    .ok_or_else(|| syn::Error::new(meta.span(), format!("unknown scalar kind `{n}`")))?,
            ),
            None => ty::scalar_kind(&analyzed.bare),
        };

        fields.push(Field {
            slot: format_ident!("__f_{}", ident),
            doc: doc_of(&f.attrs),
            binding,
            xml_name,
            json_name,
            ns: namespace_expr(&meta, Some(&container))?,
            occurs_min,
            occurs_max,
            wrapper: analyzed.wrapper,
            boxed: analyzed.boxed,
            inner: analyzed.inner,
            bare: analyzed.bare,
            scalar,
            facets: facets_expr(&meta)?,
            list,
            role: meta.string("role")?,
            ident,
        });
    }

    let roles = Roles::detect(&container, &fields);
    let type_info = type_info_impl(name, &container, &fields, &roles);
    let reflect = reflect_impl(name, &fields);
    let serialize = xml_serialize_impl(name, &fields);
    let deserialize = xml_deserialize_impl(name, &fields);
    let validate = validate_impl(name, &fields);
    let identity = identity_impls(name, &container, &fields, &roles);

    Ok(quote! {
        #type_info
        #reflect
        #serialize
        #deserialize
        #validate
        #identity
    })
}

// ---------------------------------------------------------------------------
// Identity roles
// ---------------------------------------------------------------------------

/// Which field plays which identity role, by index.
///
/// Worked out here rather than in the generator, so a version that spells
/// `UUID` where another spells `Uuid`, or `VersionString` where another says
/// `ObjectVersion`, needs no special case in the pipeline. Generated code can
/// always override with `#[energyml(role = "...")]`.
#[derive(Default)]
struct Roles {
    uuid: Option<usize>,
    title: Option<usize>,
    citation: Option<usize>,
    object_version: Option<usize>,
    content_type: Option<usize>,
    qualified_type: Option<usize>,
    uri: Option<usize>,
}

/// Lowercase and drop separators, so `object_version`, `objectVersion` and
/// `ObjectVersion` compare equal.
fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Whether a field holds a plain `String` (possibly wrapped in `Option`).
fn is_string_field(f: &Field) -> bool {
    matches!(f.wrapper, Wrapper::Plain | Wrapper::Optional) && ty::is_string(&f.bare)
}

impl Roles {
    fn detect(c: &Container, fields: &[Field]) -> Roles {
        let mut roles = Roles::default();
        // A type that identifies something: only there does `uid` mean an
        // object identifier. Dozens of nested WITSML types carry a local `uid`
        // that must not be mistaken for one.
        let identifying = c.top_level || c.object_ref;

        for (i, f) in fields.iter().enumerate() {
            if let Some(role) = &f.role {
                match role.as_str() {
                    "uuid" => roles.uuid = Some(i),
                    "title" => roles.title = Some(i),
                    "citation" => roles.citation = Some(i),
                    "object_version" => roles.object_version = Some(i),
                    "content_type" => roles.content_type = Some(i),
                    "qualified_type" => roles.qualified_type = Some(i),
                    "uri" => roles.uri = Some(i),
                    _ => {}
                }
                continue;
            }

            let name = normalize_name(&f.xml_name);
            let string = is_string_field(f);
            match name.as_str() {
                "uuid" if string && roles.uuid.is_none() => roles.uuid = Some(i),
                "uid" if string && identifying && roles.uuid.is_none() => roles.uuid = Some(i),
                "title" if string && roles.title.is_none() => roles.title = Some(i),
                "objectversion" | "versionstring" if string && roles.object_version.is_none() => {
                    roles.object_version = Some(i)
                }
                "contenttype" if string && roles.content_type.is_none() => roles.content_type = Some(i),
                "qualifiedtype" if string && roles.qualified_type.is_none() => roles.qualified_type = Some(i),
                "uri" | "energisticsuri" if string && roles.uri.is_none() => roles.uri = Some(i),
                "citation" if !string && roles.citation.is_none() => roles.citation = Some(i),
                _ => {}
            }
        }
        roles
    }

    /// Whether this type is worth an `ObjectIdentity` implementation at all.
    fn carries_identity(&self, c: &Container) -> bool {
        self.uuid.is_some() && (c.top_level || c.object_ref)
    }

    fn expr(&self) -> TokenStream {
        let entry = |v: Option<usize>| match v {
            Some(i) => {
                let i = i as u16;
                quote!(Some(#i))
            }
            None => quote!(None),
        };
        let uuid = entry(self.uuid);
        let title = entry(self.title);
        let citation = entry(self.citation);
        let object_version = entry(self.object_version);
        let content_type = entry(self.content_type);
        let qualified_type = entry(self.qualified_type);
        let uri = entry(self.uri);
        quote! {
            ::energyml_core::reflect::Roles {
                uuid: #uuid,
                title: #title,
                citation: #citation,
                object_version: #object_version,
                content_type: #content_type,
                qualified_type: #qualified_type,
                uri: #uri,
            }
        }
    }
}

/// Borrow a `String` / `Option<String>` field as `Option<&str>`.
fn borrow_str(f: &Field) -> TokenStream {
    let ident = &f.ident;
    match f.wrapper {
        Wrapper::Optional => quote!(self.#ident.as_deref()),
        _ => quote!(Some(self.#ident.as_str())),
    }
}

/// Assign a `String` / `Option<String>` field from a `&str`.
fn assign_str(f: &Field, value: TokenStream) -> TokenStream {
    let ident = &f.ident;
    match f.wrapper {
        Wrapper::Optional => quote!(self.#ident = Some(#value.to_string())),
        _ => quote!(self.#ident = #value.to_string()),
    }
}

/// Assign a field of a freshly defaulted value, used by `ObjectRef::from_key`.
fn init_str(f: &Field, value: TokenStream) -> TokenStream {
    let ident = &f.ident;
    match f.wrapper {
        Wrapper::Optional => quote!(out.#ident = Some(#value.to_string())),
        _ => quote!(out.#ident = #value.to_string()),
    }
}

/// `ObjectIdentity`, plus `ObjectIdentityMut` and `ObjectRef` where they apply.
///
/// Only direct field access is generated here: three or four one-line methods
/// that inline away. Everything derived from them - building a content type,
/// walking references, comparing identities - lives in `energyml_core::identity`
/// and is compiled once for the whole workspace rather than per type.
fn identity_impls(name: &Ident, c: &Container, fields: &[Field], roles: &Roles) -> TokenStream {
    if !roles.carries_identity(c) {
        return quote!();
    }

    let kind = if c.object_ref {
        quote!(::energyml_core::identity::IdentityKind::ObjectRef)
    } else {
        quote!(::energyml_core::identity::IdentityKind::TopLevel)
    };

    let uuid_field = &fields[roles.uuid.expect("checked by carries_identity")];
    let uuid_expr = borrow_str(uuid_field);

    // A reference states its title directly; a top-level object keeps it in its
    // Citation, whose own type differs per version - so that hop goes through
    // reflection, which is an index into a static array, not a name lookup.
    let title_expr = match (roles.title, roles.citation) {
        (Some(i), _) => borrow_str(&fields[i]),
        (None, Some(i)) => {
            let ident = &fields[i].ident;
            match fields[i].wrapper {
                Wrapper::Optional => quote! {
                    self.#ident.as_ref().and_then(|c| ::energyml_core::identity::title_of_reflect(c))
                },
                _ => quote!(::energyml_core::identity::title_of_reflect(&self.#ident)),
            }
        }
        (None, None) => quote!(None),
    };

    let version_expr = match roles.object_version {
        Some(i) => borrow_str(&fields[i]),
        None => quote!(None),
    };

    let declared_expr = match (roles.qualified_type, roles.content_type) {
        (Some(i), _) => {
            let get = borrow_str(&fields[i]);
            quote!(#get.map(::energyml_core::identity::DeclaredType::QualifiedType))
        }
        (None, Some(i)) => {
            let get = borrow_str(&fields[i]);
            quote!(#get.map(::energyml_core::identity::DeclaredType::ContentType))
        }
        (None, None) => quote!(None),
    };

    let identity = quote! {
        impl ::energyml_core::identity::ObjectIdentity for #name {
            #[inline]
            fn identity_kind(&self) -> ::energyml_core::identity::IdentityKind { #kind }

            #[inline]
            fn identity_type(&self) -> &'static ::energyml_core::reflect::TypeInfo {
                <Self as ::energyml_core::reflect::EnergymlType>::type_info()
            }

            #[inline]
            fn uuid(&self) -> Option<&str> { #uuid_expr }

            #[inline]
            fn title(&self) -> Option<&str> { #title_expr }

            #[inline]
            fn object_version(&self) -> Option<&str> { #version_expr }

            #[inline]
            fn declared_type(&self) -> Option<::energyml_core::identity::DeclaredType<'_>> {
                #declared_expr
            }
        }
    };

    // Writing back: the uuid is validated unless the field is a WITSML `uid`,
    // which is a local identifier and not a UUID.
    let checked_uuid = normalize_name(&uuid_field.xml_name) == "uuid";
    let set_uuid_body = {
        let assign = assign_str(uuid_field, quote!(uuid));
        if checked_uuid {
            quote! {
                if !::energyml_core::identifiers::is_uuid(uuid) {
                    return Err(::energyml_core::Error::parse(uuid, "uuid", "not a UUID"));
                }
                #assign;
                Ok(())
            }
        } else {
            quote! { #assign; Ok(()) }
        }
    };

    let set_title_body = match (roles.title, roles.citation) {
        (Some(i), _) => {
            let assign = assign_str(&fields[i], quote!(title));
            quote! { #assign; Ok(()) }
        }
        (None, Some(i)) => {
            let ident = &fields[i].ident;
            match fields[i].wrapper {
                Wrapper::Optional => quote! {
                    let citation = self.#ident.get_or_insert_with(Default::default);
                    ::energyml_core::identity::set_title_reflect(citation, title)
                },
                _ => quote! {
                    ::energyml_core::identity::set_title_reflect(&mut self.#ident, title)
                },
            }
        }
        (None, None) => quote! {
            Err(::energyml_core::Error::other(concat!(stringify!(#name), " has no title")))
        },
    };

    let set_version_body = match roles.object_version {
        Some(i) => {
            let ident = &fields[i].ident;
            match fields[i].wrapper {
                Wrapper::Optional => quote! {
                    self.#ident = version.map(str::to_string);
                    Ok(())
                },
                _ => quote! {
                    self.#ident = version.unwrap_or_default().to_string();
                    Ok(())
                },
            }
        }
        None => quote!(Ok(())),
    };

    let set_target_body = if c.object_ref {
        let mut writes = Vec::new();
        if let Some(i) = roles.qualified_type {
            writes.push(assign_str(&fields[i], quote!(key.qualified_type())));
        }
        if let Some(i) = roles.content_type {
            writes.push(assign_str(&fields[i], quote!(key.content_type())));
        }
        writes.push(assign_str(uuid_field, quote!(key.uuid)));
        if let Some(i) = roles.title {
            let f = &fields[i];
            writes.push(assign_str(f, quote!(key.title.unwrap_or_default())));
        }
        quote! {
            #(#writes;)*
            self.set_object_version(key.object_version)?;
            Ok(())
        }
    } else {
        quote! {
            let _ = key;
            Err(::energyml_core::Error::other(
                concat!(stringify!(#name), " is not a reference; nothing to point at a target")
            ))
        }
    };

    let identity_mut = quote! {
        impl ::energyml_core::identity::ObjectIdentityMut for #name {
            fn set_uuid(&mut self, uuid: &str) -> ::energyml_core::Result<()> { #set_uuid_body }
            fn set_title(&mut self, title: &str) -> ::energyml_core::Result<()> { #set_title_body }
            fn set_object_version(&mut self, version: Option<&str>) -> ::energyml_core::Result<()> {
                #set_version_body
            }
            fn set_target(
                &mut self,
                key: &::energyml_core::identity::ObjectKey<'_>,
            ) -> ::energyml_core::Result<()> {
                #set_target_body
            }
        }
    };

    // Only a reference can be built from a key.
    let object_ref = if c.object_ref {
        let mut writes = Vec::new();
        if let Some(i) = roles.qualified_type {
            writes.push(init_str(&fields[i], quote!(key.qualified_type())));
        }
        if let Some(i) = roles.content_type {
            writes.push(init_str(&fields[i], quote!(key.content_type())));
        }
        writes.push(init_str(uuid_field, quote!(key.uuid)));
        if let Some(i) = roles.title {
            writes.push(init_str(&fields[i], quote!(key.title.unwrap_or_default())));
        }
        if let Some(i) = roles.object_version {
            let f = &fields[i];
            let ident = &f.ident;
            writes.push(match f.wrapper {
                Wrapper::Optional => quote!(out.#ident = key.object_version.map(str::to_string)),
                _ => quote!(out.#ident = key.object_version.unwrap_or_default().to_string()),
            });
        }
        quote! {
            impl ::energyml_core::identity::ObjectRef for #name {
                fn from_key(key: &::energyml_core::identity::ObjectKey<'_>) -> Self {
                    let mut out = Self::default();
                    #(#writes;)*
                    out
                }
            }
        }
    } else {
        quote!()
    };

    quote! {
        #identity
        #identity_mut
        #object_ref
    }
}

fn facets_expr(meta: &Meta) -> syn::Result<TokenStream> {
    let min_inclusive = opt_f64(meta.f64("min_inclusive")?);
    let max_inclusive = opt_f64(meta.f64("max_inclusive")?);
    let min_exclusive = opt_f64(meta.f64("min_exclusive")?);
    let max_exclusive = opt_f64(meta.f64("max_exclusive")?);
    let min_length = opt_u32(meta.u32("min_length")?);
    let max_length = opt_u32(meta.u32("max_length")?);
    let pattern = match meta.string("pattern")? {
        Some(p) => quote!(Some(#p)),
        None => quote!(None),
    };
    let fixed = match meta.string("fixed")? {
        Some(p) => quote!(Some(#p)),
        None => quote!(None),
    };
    let lexical = match meta.string("lexical")? {
        Some(p) => quote!(Some(#p)),
        None => quote!(None),
    };
    Ok(quote! {
        ::energyml_core::reflect::Facets {
            fixed: #fixed,
            lexical: #lexical,
            min_inclusive: #min_inclusive,
            max_inclusive: #max_inclusive,
            min_exclusive: #min_exclusive,
            max_exclusive: #max_exclusive,
            min_length: #min_length,
            max_length: #max_length,
            pattern: #pattern,
        }
    })
}

fn opt_f64(v: Option<f64>) -> TokenStream {
    match v {
        Some(v) => quote!(Some(#v)),
        None => quote!(None),
    }
}

fn opt_u32(v: Option<u32>) -> TokenStream {
    match v {
        Some(v) => quote!(Some(#v)),
        None => quote!(None),
    }
}

fn field_type_expr(f: &Field) -> TokenStream {
    match &f.scalar {
        Some(kind) => quote!(::energyml_core::reflect::FieldType::Scalar(
            ::energyml_core::reflect::ScalarKind::#kind
        )),
        None => {
            let ty = &f.bare;
            // Enumerations are told apart from objects at runtime, from the
            // TypeInfo the type reports, so both spellings resolve the same way.
            quote!(::energyml_core::reflect::FieldType::Object(
                <#ty as ::energyml_core::reflect::EnergymlType>::type_info
            ))
        }
    }
}

fn type_info_impl(name: &Ident, c: &Container, fields: &[Field], roles: &Roles) -> TokenStream {
    let field_entries = fields.iter().map(|f| {
        let rust_name = f.ident.to_string();
        let xml_name = &f.xml_name;
        let json_name = &f.json_name;
        let doc = &f.doc;
        let ns = &f.ns;
        let binding = match f.binding {
            Binding::Attribute => quote!(Attribute),
            Binding::Element => quote!(Element),
            Binding::Text => quote!(Text),
            Binding::Any => quote!(Any),
        };
        let occurs = occurs_expr(f.occurs_min, f.occurs_max);
        let ty = field_type_expr(f);
        let facets = &f.facets;
        let is_list = f.list;
        quote! {
            ::energyml_core::reflect::FieldInfo {
                name: #rust_name,
                xml_name: #xml_name,
                xml_ns: #ns,
                json_name: #json_name,
                doc: #doc,
                binding: ::energyml_core::reflect::Binding::#binding,
                occurs: #occurs,
                ty: #ty,
                facets: #facets,
                is_list: #is_list,
            }
        }
    });

    let body = c.type_info_body(
        quote!(::energyml_core::reflect::TypeKind::Struct),
        quote!(&[#(#field_entries),*]),
        quote!(&[]),
        roles.expr(),
        // Every generated struct derives `Default`, so building an empty one
        // needs nothing but the type - which is exactly what reflection could
        // not name before this.
        quote!(::core::option::Option::Some(|| {
            ::std::boxed::Box::new(<#name as ::core::default::Default>::default())
                as ::std::boxed::Box<dyn ::energyml_core::value::Reflect>
        })),
    );

    quote! {
        impl ::energyml_core::reflect::EnergymlType for #name {
            fn type_info() -> &'static ::energyml_core::reflect::TypeInfo {
                static INFO: ::energyml_core::reflect::TypeInfo = #body;
                &INFO
            }
        }
    }
}

fn reflect_impl(name: &Ident, fields: &[Field]) -> TokenStream {
    let name_str = name.to_string();

    let get_arms = fields.iter().map(|f| {
        let ident = &f.ident;
        let pat = name_patterns(f);
        quote! {
            #pat => ::core::option::Option::Some(
                ::energyml_core::value::AsFieldValue::as_field_value(&self.#ident)
            ),
        }
    });

    let set_arms = fields.iter().map(|f| {
        let ident = &f.ident;
        let field_ty = full_field_type(f);
        let pat = name_patterns(f);
        quote! {
            #pat => {
                self.#ident = <#field_ty as ::energyml_core::value::FromOwnedValue>::from_owned_value(value)?;
                ::core::result::Result::Ok(())
            }
        }
    });

    let object_mut_arms = fields.iter().filter(|f| f.is_object()).map(|f| {
        let ident = &f.ident;
        let pat = name_patterns(f);
        // A boxed field must be dereferenced: `Box<T>` does not implement
        // `Reflect`, only the `T` inside it does.
        let body = match (f.wrapper, f.boxed) {
            (Wrapper::Plain, false) => quote! {
                ::core::option::Option::Some(&mut self.#ident as &mut dyn ::energyml_core::value::Reflect)
            },
            (Wrapper::Plain, true) => quote! {
                ::core::option::Option::Some(&mut *self.#ident as &mut dyn ::energyml_core::value::Reflect)
            },
            (Wrapper::Optional, false) => quote! {
                self.#ident.as_mut().map(|v| v as &mut dyn ::energyml_core::value::Reflect)
            },
            (Wrapper::Optional, true) => quote! {
                self.#ident.as_mut().map(|v| &mut **v as &mut dyn ::energyml_core::value::Reflect)
            },
            (Wrapper::Repeated, false) => quote! {
                self.#ident
                    .get_mut(index.unwrap_or(0))
                    .map(|v| v as &mut dyn ::energyml_core::value::Reflect)
            },
            (Wrapper::Repeated, true) => quote! {
                self.#ident
                    .get_mut(index.unwrap_or(0))
                    .map(|v| &mut **v as &mut dyn ::energyml_core::value::Reflect)
            },
        };
        quote! { #pat => #body, }
    });

    quote! {
        impl ::energyml_core::value::Reflect for #name {
            fn type_info(&self) -> &'static ::energyml_core::reflect::TypeInfo {
                <Self as ::energyml_core::reflect::EnergymlType>::type_info()
            }

            fn field(&self, name: &str) -> ::core::option::Option<::energyml_core::value::FieldValue<'_>> {
                match name {
                    #(#get_arms)*
                    _ => ::core::option::Option::None,
                }
            }

            fn set_field(
                &mut self,
                name: &str,
                value: ::energyml_core::value::OwnedValue,
            ) -> ::energyml_core::error::Result<()> {
                match name {
                    #(#set_arms)*
                    _ => ::core::result::Result::Err(::energyml_core::error::Error::UnknownField {
                        ty: #name_str,
                        field: name.to_string(),
                    }),
                }
            }

            fn field_object_mut(
                &mut self,
                name: &str,
                index: ::core::option::Option<usize>,
            ) -> ::core::option::Option<&mut dyn ::energyml_core::value::Reflect> {
                let _ = index;
                match name {
                    #(#object_mut_arms)*
                    _ => ::core::option::Option::None,
                }
            }

            fn as_any(&self) -> &dyn ::core::any::Any { self }
            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any { self }
            fn into_any(self: ::std::boxed::Box<Self>) -> ::std::boxed::Box<dyn ::core::any::Any> { self }
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
                ::energyml_core::value::downcast_owned::<Self>(value, #name_str)
            }
        }
    }
}

/// Match both the Rust and the XML spelling of a field name, without emitting a
/// duplicate (and therefore unreachable) pattern when they coincide.
fn name_patterns(f: &Field) -> TokenStream {
    let rust_name = f.ident.to_string();
    if rust_name == f.xml_name {
        quote!(#rust_name)
    } else {
        let xml_name = &f.xml_name;
        quote!(#rust_name | #xml_name)
    }
}

fn full_field_type(f: &Field) -> TokenStream {
    let inner = &f.inner;
    match f.wrapper {
        Wrapper::Plain => quote!(#inner),
        Wrapper::Optional => quote!(::core::option::Option<#inner>),
        Wrapper::Repeated => quote!(::std::vec::Vec<#inner>),
    }
}

fn xml_serialize_impl(name: &Ident, fields: &[Field]) -> TokenStream {
    let attrs = fields
        .iter()
        .filter(|f| f.binding == Binding::Attribute)
        .map(|f| {
            let ident = &f.ident;
            let xml_name = &f.xml_name;
            match f.wrapper {
                Wrapper::Optional => quote! {
                    ::energyml_core::xml::push_attr(tag, #xml_name, &self.#ident);
                },
                _ => quote! {
                    ::energyml_core::xml::push_attr_required(tag, #xml_name, &self.#ident);
                },
            }
        });

    // A wildcard's prefixes were declared on an ancestor of the document it
    // came from, so they are re-declared on the element that carries it - the
    // content is written verbatim and would otherwise bind them to nothing.
    let any_attrs = fields.iter().filter(|f| f.binding == Binding::Any).map(|f| {
        let ident = &f.ident;
        quote! {
            if let ::core::option::Option::Some(__any) = &self.#ident {
                ::energyml_core::xml::XmlSerialize::xml_attributes(__any, tag)?;
            }
        }
    });

    let any_children = fields.iter().filter(|f| f.binding == Binding::Any).map(|f| {
        let ident = &f.ident;
        quote! {
            if let ::core::option::Option::Some(__any) = &self.#ident {
                w.write_raw(&__any.content)?;
            }
        }
    });

    let children = fields.iter().filter(|f| f.binding == Binding::Element).map(|f| {
        let ident = &f.ident;
        let xml_name = &f.xml_name;
        let ns = &f.ns;
        // An xsd:list is one element holding every value, so it is written
        // once from the joined text - never one element per item.
        if f.list {
            return quote! {
                w.write_element(
                    #ns,
                    #xml_name,
                    &::energyml_core::xml::join_list(&self.#ident),
                )?;
            };
        }
        match f.wrapper {
            Wrapper::Plain => quote! { w.write_element(#ns, #xml_name, &self.#ident)?; },
            Wrapper::Optional => quote! { w.write_optional(#ns, #xml_name, &self.#ident)?; },
            Wrapper::Repeated => quote! { w.write_all(#ns, #xml_name, &self.#ident)?; },
        }
    });

    let text = fields.iter().find(|f| f.binding == Binding::Text).map(|f| {
        let ident = &f.ident;
        match f.wrapper {
            Wrapper::Optional => quote! {
                self.#ident.as_ref().map(::energyml_core::xml::XmlText::to_xml_text)
            },
            _ => quote! {
                ::core::option::Option::Some(::energyml_core::xml::XmlText::to_xml_text(&self.#ident))
            },
        }
    });

    let text_fn = match text {
        Some(body) => quote! {
            fn xml_text(&self) -> ::core::option::Option<::std::borrow::Cow<'_, str>> { #body }
        },
        None => quote! {},
    };

    quote! {
        impl ::energyml_core::xml::XmlSerialize for #name {
            fn xml_attributes(
                &self,
                tag: &mut ::energyml_core::quick_xml::events::BytesStart<'_>,
            ) -> ::energyml_core::error::Result<()> {
                let _ = &tag;
                #(#attrs)*
                #(#any_attrs)*
                ::core::result::Result::Ok(())
            }

            fn xml_children(
                &self,
                w: &mut ::energyml_core::xml::XmlWriter<'_>,
            ) -> ::energyml_core::error::Result<()> {
                let _ = &w;
                #(#children)*
                #(#any_children)*
                ::core::result::Result::Ok(())
            }

            #text_fn
        }
    }
}

fn xml_deserialize_impl(name: &Ident, fields: &[Field]) -> TokenStream {
    let name_str = name.to_string();

    let attr_reads = fields
        .iter()
        .filter(|f| f.binding == Binding::Attribute)
        .map(|f| {
            let slot = &f.slot;
            let inner = &f.inner;
            let xml_name = &f.xml_name;
            quote! {
                let #slot: ::core::option::Option<#inner> =
                    r.attr_optional::<#inner>(start, #xml_name)?;
            }
        });

    let decls = fields
        .iter()
        .filter(|f| f.binding != Binding::Attribute)
        .map(|f| {
            let slot = &f.slot;
            let inner = &f.inner;
            match f.wrapper {
                Wrapper::Repeated => {
                    quote! { let mut #slot: ::std::vec::Vec<#inner> = ::std::vec::Vec::new(); }
                }
                _ => quote! { let mut #slot: ::core::option::Option<#inner> = ::core::option::Option::None; },
            }
        });

    // Group element fields by XML local name. A struct can legitimately have two
    // children with the same local name in different namespaces - WITSML objects
    // declare their own `witsml:Citation` next to the inherited `eml:Citation` -
    // so those groups are disambiguated by the resolved namespace URI.
    let mut groups: Vec<(String, Vec<&Field>)> = Vec::new();
    for f in fields.iter().filter(|f| f.binding == Binding::Element) {
        match groups.iter_mut().find(|(name, _)| name == &f.xml_name) {
            Some((_, members)) => members.push(f),
            None => groups.push((f.xml_name.clone(), vec![f])),
        }
    }

    let assign = |f: &Field| {
        let slot = &f.slot;
        let inner = &f.inner;
        // The whole list arrives in this one element's text, so it replaces the
        // slot rather than pushing to it. A second element of the same name
        // would be schema-invalid; last one wins, as everywhere else here.
        if f.list {
            return quote! {
                #slot = ::energyml_core::xml::split_list::<#inner>(&r.read_text()?)?;
            };
        }
        match f.wrapper {
            Wrapper::Repeated => quote! {
                #slot.push(<#inner as ::energyml_core::xml::XmlDeserialize>::xml_read(r, &__e)?);
            },
            _ => quote! {
                #slot = ::core::option::Option::Some(
                    <#inner as ::energyml_core::xml::XmlDeserialize>::xml_read(r, &__e)?
                );
            },
        }
    };

    let child_arms = groups.iter().map(|(xml_name, members)| {
        if members.len() == 1 {
            let body = assign(members[0]);
            return quote! { #xml_name => { #body } };
        }
        // Namespace decides. An unprefixed or unresolvable element falls back to
        // the first declaration, which is the inherited one.
        let checks = members.iter().enumerate().map(|(i, f)| {
            let ns = &f.ns;
            let body = assign(f);
            if i == 0 {
                quote! { if __ns.as_deref() == ::core::option::Option::Some(#ns.uri) { #body } }
            } else {
                quote! { else if __ns.as_deref() == ::core::option::Option::Some(#ns.uri) { #body } }
            }
        });
        let fallback = assign(members[0]);
        quote! {
            #xml_name => {
                #(#checks)*
                else { #fallback }
            }
        }
    });

    let unmatched = match fields.iter().find(|f| f.binding == Binding::Any) {
        Some(f) => {
            let slot = &f.slot;
            quote! {
                let __any = #slot.get_or_insert_with(::core::default::Default::default);
                ::energyml_core::any_xml::capture_into(__any, r, &__e, __ns.as_deref())?;
            }
        }
        None => quote! { r.skip_element()?; },
    };

    let text_field = fields.iter().find(|f| f.binding == Binding::Text);
    let text_assign = match text_field {
        Some(f) => {
            let slot = &f.slot;
            let inner = &f.inner;
            quote! {
                if !__text.is_empty() {
                    #slot = ::core::option::Option::Some(
                        r.value::<#inner>(&__text, "text content", #name_str)?
                    );
                }
            }
        }
        None => quote! { let _ = &__text; },
    };

    let inits = fields.iter().map(|f| {
        let ident = &f.ident;
        let slot = &f.slot;
        let xml_name = &f.xml_name;
        let kind = match f.binding {
            Binding::Attribute => "attribute",
            Binding::Text => "text content",
            Binding::Element => "element",
            Binding::Any => "wildcard content",
        };
        match f.wrapper {
            Wrapper::Repeated => quote! { #ident: #slot },
            Wrapper::Optional => quote! { #ident: #slot },
            // The reader decides what an absent required item means: an error,
            // or its default plus a record of the invention. Keeping the choice
            // there rather than here is what lets one call site serve both
            // modes, and keeps ~1000 generated impls to one line per field.
            Wrapper::Plain => quote! {
                #ident: r.required(#slot, #kind, #xml_name, #name_str)?
            },
        }
    });

    quote! {
        impl ::energyml_core::xml::XmlDeserialize for #name {
            fn xml_read(
                r: &mut ::energyml_core::xml::XmlReader<'_>,
                start: &::energyml_core::quick_xml::events::BytesStart<'_>,
            ) -> ::energyml_core::error::Result<Self> {
                let _ = &start;
                #(#attr_reads)*
                #(#decls)*
                let mut __text = ::std::string::String::new();

                loop {
                    match r.next_event()? {
                        ::energyml_core::xml::XmlEvent::Start(__e, __ns) => {
                            let _ = &__ns;
                            match ::energyml_core::xml::local_name(&__e).as_str() {
                                #(#child_arms)*
                                // Unknown children are skipped rather than
                                // rejected: EPCs in the wild carry extensions,
                                // and a newer minor version must stay readable.
                                // Unless the schema declared `xsd:any` here, in
                                // which case they *are* the content: captured
                                // verbatim so they survive a round trip.
                                _ => { #unmatched }
                            }
                        }
                        ::energyml_core::xml::XmlEvent::Text(__t) => __text.push_str(&__t),
                        ::energyml_core::xml::XmlEvent::End(_) => break,
                        ::energyml_core::xml::XmlEvent::Eof => {
                            return ::core::result::Result::Err(::energyml_core::error::Error::Eof);
                        }
                    }
                }

                #text_assign

                ::core::result::Result::Ok(Self { #(#inits),* })
            }
        }
    }
}

fn validate_impl(name: &Ident, fields: &[Field]) -> TokenStream {
    let recursions = fields.iter().map(|f| {
        let ident = &f.ident;
        let xml_name = &f.xml_name;
        quote! {
            ctx.enter(#xml_name);
            ::energyml_core::validate::Validate::validate_into(&self.#ident, ctx);
            ctx.leave();
        }
    });

    quote! {
        impl ::energyml_core::validate::Validate for #name {
            fn validate_into(&self, ctx: &mut ::energyml_core::validate::ValidationCtx<'_>) {
                ctx.run_rules(self);
                let __info = <Self as ::energyml_core::reflect::EnergymlType>::type_info();
                for __f in __info.fields {
                    if let ::core::option::Option::Some(__v) =
                        ::energyml_core::value::Reflect::field(self, __f.name)
                    {
                        ctx.check_field(__f, &__v);
                    }
                }
                #(#recursions)*
            }
        }
    }
}
