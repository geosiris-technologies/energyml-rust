//! Derive macros for `energyml-build`: an operation's parameters, choices and
//! inputs are **declared once**, as Rust types, and the form, the reading and
//! the activity template all come from that one declaration.
//!
//! ADR 0017 holds the why. The
//! short of it: a parameter used to be declared in `needs()` and then read
//! back **by its name** in `plan()`, its default written a second time beside
//! its suggestion — two places for one fact. Here the field is the
//! declaration, its type is the kind of answer, its doc comment is the label,
//! and `default = …` is written once.
//!
//! Three derives:
//!
//! - `Choice` — a unit enum becomes a closed list of words (`thin-plate`);
//! - `Params` — on a struct, one slot per field (or a nested block with
//!   `flatten`); on an enum with `#[params(tag = "…")]`, a **union**: one
//!   choice slot, then each case's slots, each acting only under its case;
//! - `Inputs` — one input slot per field, bound by the framework.
//!
//! The generated code names `::energyml_build`; the crate says
//! `extern crate self as energyml_build` so that it compiles inside it too.

use heck::{ToKebabCase, ToLowerCamelCase};
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, LitStr, parse_macro_input};

// ---------------------------------------------------------------- helpers

/// The **first paragraph** of an item's doc comment, joined into one line —
/// the label of a slot. What follows a blank line is a note for whoever reads
/// the code, not a label for whoever fills the form.
fn doc_of(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. }) = &nv.value {
                let line = text.value().trim().to_string();
                if line.is_empty() {
                    if !lines.is_empty() {
                        break;
                    }
                    continue;
                }
                lines.push(line);
            }
        }
    }
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// `["A", "B"]` as a list of strings.
fn strings_of(expr: &Expr) -> syn::Result<Vec<String>> {
    let Expr::Array(array) = expr else {
        return Err(syn::Error::new(expr.span(), "expected a list of strings: [\"a\", \"b\"]"));
    };
    array
        .elems
        .iter()
        .map(|one| match one {
            Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. }) => Ok(text.value()),
            other => Err(syn::Error::new(other.span(), "expected a string")),
        })
        .collect()
}

fn lits(list: &[String]) -> Vec<LitStr> {
    list.iter().map(|one| LitStr::new(one, Span::call_site())).collect()
}

// ---------------------------------------------------------------- Choice

/// A unit enum as a closed list of words — `#[choice(name = "…")]` renames a
/// variant, the default is its name in kebab-case.
#[proc_macro_derive(Choice, attributes(choice))]
pub fn derive_choice(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match choice(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn choice(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(input.span(), "`Choice` is for an enum of unit variants"));
    };
    let mut idents = Vec::new();
    let mut words = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(variant.span(), "a choice is a word: a unit variant, with no fields"));
        }
        let mut word = variant.ident.to_string().to_kebab_case();
        for attr in variant.attrs.iter().filter(|a| a.path().is_ident("choice")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    word = meta.value()?.parse::<LitStr>()?.value();
                    Ok(())
                } else {
                    Err(meta.error("expected `name = \"…\"`"))
                }
            })?;
        }
        idents.push(variant.ident.clone());
        words.push(LitStr::new(&word, Span::call_site()));
    }
    Ok(quote! {
        impl #name {
            /// Every word, in declaration order.
            pub const WORDS: &'static [&'static str] = &[#(#words),*];
            /// The word this value is written as.
            pub fn word(&self) -> &'static str {
                match self { #(#name::#idents => #words),* }
            }
        }
        impl ::energyml_build::declare::Scalar for #name {
            fn need() -> ::energyml_build::operation::Need {
                ::energyml_build::operation::Need::Choice(Self::WORDS)
            }
            fn from_json(value: &::energyml_build::declare::Json) -> ::std::result::Result<Self, ::std::string::String> {
                match value.as_str() {
                    #(Some(#words) => Ok(#name::#idents),)*
                    _ => Err(format!("expected one of {}, got {}", Self::WORDS.join(", "), value)),
                }
            }
            fn to_json(&self) -> ::energyml_build::declare::Json {
                ::energyml_build::declare::Json::from(self.word())
            }
        }
        ::energyml_build::scalar_param!(#name);
    })
}

// ---------------------------------------------------------------- Params

/// One slot per field, or a union of cases — see the crate documentation.
#[proc_macro_derive(Params, attributes(param, params, case))]
pub fn derive_params(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = match &input.data {
        Data::Struct(_) => params_struct(&input),
        Data::Enum(_) => params_union(&input),
        Data::Union(_) => Err(syn::Error::new(input.span(), "`Params` is for a struct or an enum")),
    };
    match result {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// What `#[param(…)]` says of a field.
#[derive(Default)]
struct ParamAttrs {
    name: Option<String>,
    about: Option<String>,
    default: Option<Expr>,
    group: Option<String>,
    advanced: bool,
    min: Option<Expr>,
    max: Option<Expr>,
    uom: Option<String>,
    reads: Vec<String>,
    with: Vec<String>,
    without: Vec<String>,
    creates: Vec<String>,
    least: Option<Expr>,
    ordered: bool,
    need: Option<Expr>,
    flatten: bool,
    optional: bool,
    /// `"rotate: axis; scale: uniform, axes"` — applies when one of those holds.
    when_any: Option<String>,
    /// `"bands: true"` — applies when that one holds. For a controller that is
    /// no union (a flag): a union's case says it by the union's own tag.
    when: Option<String>,
}

fn param_attrs(attrs: &[Attribute]) -> syn::Result<ParamAttrs> {
    let mut out = ParamAttrs::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("param")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
            match key.as_str() {
                "name" => out.name = Some(meta.value()?.parse::<LitStr>()?.value()),
                "about" => out.about = Some(meta.value()?.parse::<LitStr>()?.value()),
                "default" => out.default = Some(meta.value()?.parse()?),
                "group" => out.group = Some(meta.value()?.parse::<LitStr>()?.value()),
                "uom" => out.uom = Some(meta.value()?.parse::<LitStr>()?.value()),
                "min" => out.min = Some(meta.value()?.parse()?),
                "max" => out.max = Some(meta.value()?.parse()?),
                "least" => out.least = Some(meta.value()?.parse()?),
                "need" => out.need = Some(meta.value()?.parse()?),
                "reads" => out.reads = strings_of(&meta.value()?.parse()?)?,
                "with" => out.with = strings_of(&meta.value()?.parse()?)?,
                "without" => out.without = strings_of(&meta.value()?.parse()?)?,
                "creates" => out.creates = strings_of(&meta.value()?.parse()?)?,
                "advanced" => out.advanced = true,
                "ordered" => out.ordered = true,
                "flatten" => out.flatten = true,
                // May be left out — and then takes its default: what
                // `Slot::optional(…).suggest(…)` said by hand.
                "optional" => out.optional = true,
                "when_any" => out.when_any = Some(meta.value()?.parse::<LitStr>()?.value()),
                "when" => out.when = Some(meta.value()?.parse::<LitStr>()?.value()),
                _ => return Err(meta.error("unknown `param` attribute")),
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// `"bands: true"` as one `Applies::When`.
fn when_one(text: &str, span: Span) -> syn::Result<Tokens> {
    let Some((slot, values)) = text.split_once(':') else {
        return Err(syn::Error::new(span, format!("`when`: `{text}` is not `slot: value, value`")));
    };
    if text.contains(';') {
        return Err(syn::Error::new(span, "`when` is one condition — two or more are `when_any`"));
    }
    let slot = LitStr::new(slot.trim(), Span::call_site());
    let values: Vec<LitStr> =
        values.split(',').map(str::trim).filter(|one| !one.is_empty()).map(|one| LitStr::new(one, Span::call_site())).collect();
    if values.is_empty() {
        return Err(syn::Error::new(span, format!("`when`: `{text}` names no value")));
    }
    Ok(quote! { ::energyml_build::operation::Applies::When { slot: #slot, is: vec![#(#values),*] } })
}

/// `"rotate: axis; scale: uniform, axes"` as an `Applies::Any` of `When`s.
fn when_any(text: &str, span: Span) -> syn::Result<Tokens> {
    let mut branches = Vec::new();
    for branch in text.split(';').map(str::trim).filter(|one| !one.is_empty()) {
        let Some((slot, values)) = branch.split_once(':') else {
            return Err(syn::Error::new(span, format!("`when_any`: `{branch}` is not `slot: value, value`")));
        };
        let slot = LitStr::new(slot.trim(), Span::call_site());
        let values: Vec<LitStr> =
            values.split(',').map(str::trim).filter(|one| !one.is_empty()).map(|one| LitStr::new(one, Span::call_site())).collect();
        if values.is_empty() {
            return Err(syn::Error::new(span, format!("`when_any`: `{branch}` names no value")));
        }
        branches.push(quote! { ::energyml_build::operation::Applies::When { slot: #slot, is: vec![#(#values),*] } });
    }
    if branches.len() < 2 {
        return Err(syn::Error::new(span, "`when_any` joins two conditions or more — one is `when`, by a union's case"));
    }
    Ok(quote! { ::energyml_build::operation::Applies::Any(vec![#(#branches),*]) })
}

fn params_struct(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else { unreachable!() };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(input.span(), "`Params` on a struct needs named fields"));
    };
    let mut declare = Vec::new();
    let mut read = Vec::new();
    let mut write = Vec::new();
    let mut idents = Vec::new();
    for field in &fields.named {
        let ident = field.ident.clone().expect("named");
        let ty = &field.ty;
        let attrs = param_attrs(&field.attrs)?;
        idents.push(ident.clone());
        let any = match (&attrs.when_any, &attrs.when) {
            (Some(_), Some(_)) => return Err(syn::Error::new(field.span(), "`when` or `when_any`, not both")),
            (Some(text), None) => Some(when_any(text, field.span())?),
            (None, Some(text)) => Some(when_one(text, field.span())?),
            (None, None) => None,
        };
        if attrs.flatten {
            match &any {
                // **The group applies under the condition**: the slots that
                // applied always now apply under it; those already under a
                // case of the group keep theirs, and nest through their tag.
                Some(any) => declare.push(quote! {
                    for mut slot in <#ty as ::energyml_build::declare::Params>::slots() {
                        if slot.applies == ::energyml_build::operation::Applies::Always {
                            slot.applies = #any;
                        }
                        out.push(slot);
                    }
                }),
                None => declare.push(quote! { out.extend(<#ty as ::energyml_build::declare::Params>::slots()); }),
            }
            read.push(quote! { let #ident = <#ty as ::energyml_build::declare::Params>::read(choices)?; });
            write.push(quote! { choices = ::energyml_build::declare::Params::write(&self.#ident, choices); });
            continue;
        }
        let slot = attrs.name.clone().unwrap_or_else(|| ident.to_string().to_lower_camel_case());
        let slot_lit = LitStr::new(&slot, Span::call_site());
        let about = attrs.about.clone().or_else(|| doc_of(&field.attrs)).ok_or_else(|| {
            syn::Error::new(field.span(), "a parameter says what it is: a doc comment, or `#[param(about = \"…\")]`")
        })?;
        let about = LitStr::new(&about, Span::call_site());
        let mut steps = Vec::new();
        if let Some(need) = &attrs.need {
            steps.push(quote! { slot.need = #need; });
        }
        if attrs.min.is_some() || attrs.max.is_some() || attrs.uom.is_some() {
            let min = attrs.min.as_ref().map(|e| quote! { Some((#e) as f64) }).unwrap_or(quote! { None });
            let max = attrs.max.as_ref().map(|e| quote! { Some((#e) as f64) }).unwrap_or(quote! { None });
            let uom = attrs.uom.as_ref().map(|u| quote! { Some(#u) }).unwrap_or(quote! { None });
            steps.push(quote! { slot.need = ::energyml_build::declare::bounded(slot.need, #min, #max, #uom); });
        }
        if let Some(default) = &attrs.default {
            steps.push(quote! {
                slot = slot.suggest(::energyml_build::declare::default_json::<#ty>(&(#default)));
            });
        }
        if let Some(group) = &attrs.group {
            steps.push(quote! { slot = slot.grouped(#group); });
        }
        if attrs.advanced {
            steps.push(quote! { slot = slot.advanced(); });
        }
        if !attrs.reads.is_empty() {
            let reads = lits(&attrs.reads);
            steps.push(quote! { slot = slot.reads(&[#(#reads),*]); });
        }
        if !attrs.with.is_empty() {
            let with = lits(&attrs.with);
            steps.push(quote! { slot = slot.when(::energyml_build::operation::Applies::With(&[#(#with),*])); });
        }
        if !attrs.without.is_empty() {
            let without = lits(&attrs.without);
            steps.push(quote! { slot = slot.when(::energyml_build::operation::Applies::Without(&[#(#without),*])); });
        }
        if let Some(any) = &any {
            steps.push(quote! { slot = slot.when(#any); });
        }
        if !attrs.creates.is_empty() {
            let creates = lits(&attrs.creates);
            steps.push(quote! {
                slot = slot.otherwise(::energyml_build::operation::Otherwise::Creates { types: vec![#(#creates),*] });
            });
        }
        if let Some(least) = &attrs.least {
            steps.push(quote! { slot.least = #least; });
        }
        if attrs.optional {
            if attrs.default.is_none() {
                return Err(syn::Error::new(
                    field.span(),
                    "`optional` is for a field with a default; a field that may have no value is an `Option`",
                ));
            }
            steps.push(quote! { slot.least = 0; });
        }
        if attrs.ordered {
            steps.push(quote! { slot = slot.in_order(); });
        }
        declare.push(quote! {
            {
                let mut slot = ::energyml_build::declare::slot_of::<#ty>(#slot_lit, #about);
                #(#steps)*
                out.push(slot);
            }
        });
        let fallback = match &attrs.default {
            // The default is the value the reading takes in its absence — the
            // same expression as the suggestion, so the two cannot drift.
            Some(default) => quote! { None => Ok(#default), },
            None => quote! {},
        };
        write.push(quote! {
            if let Some(value) = ::energyml_build::declare::Param::json(&self.#ident) {
                choices = choices.set(#slot_lit, value);
            }
        });
        read.push(quote! {
            let #ident: #ty = match choices.get(#slot_lit).filter(|value| !value.is_null()) {
                #fallback
                found => <#ty as ::energyml_build::declare::Param>::read(found),
            }
            .map_err(|why| ::energyml_build::operation::Refusal::on(#slot_lit, why))?;
        });
    }
    Ok(quote! {
        impl ::energyml_build::declare::Params for #name {
            fn slots() -> ::std::vec::Vec<::energyml_build::operation::Slot> {
                let mut out = ::std::vec::Vec::new();
                #(#declare)*
                out
            }
            fn read(choices: &::energyml_build::operation::Choices) -> ::std::result::Result<Self, ::energyml_build::operation::Refusal> {
                #(#read)*
                Ok(Self { #(#idents),* })
            }
            #[allow(unused_mut)]
            fn write(&self, mut choices: ::energyml_build::operation::Choices) -> ::energyml_build::operation::Choices {
                #(#write)*
                choices
            }
        }
    })
}

fn params_union(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else { unreachable!() };
    let mut tag = None;
    let mut about = doc_of(&input.attrs);
    let mut group = None;
    let mut advanced = false;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("params")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
            match key.as_str() {
                "tag" => tag = Some(meta.value()?.parse::<LitStr>()?.value()),
                "about" => about = Some(meta.value()?.parse::<LitStr>()?.value()),
                "group" => group = Some(meta.value()?.parse::<LitStr>()?.value()),
                "advanced" => advanced = true,
                _ => return Err(meta.error("unknown `params` attribute")),
            }
            Ok(())
        })?;
    }
    let tag = tag.ok_or_else(|| {
        syn::Error::new(input.span(), "a union of cases names its choice: `#[params(tag = \"method\")]`")
    })?;
    let about = about.ok_or_else(|| syn::Error::new(input.span(), "a union says what it chooses: a doc comment"))?;
    let tag_lit = LitStr::new(&tag, Span::call_site());
    let about = LitStr::new(&about, Span::call_site());

    let mut words = Vec::new();
    let mut cases_declare = Vec::new();
    let mut cases_read = Vec::new();
    let mut cases_write = Vec::new();
    let mut default = None;
    for variant in &data.variants {
        let ident = &variant.ident;
        let mut word = ident.to_string().to_kebab_case();
        let mut is_default = false;
        for attr in variant.attrs.iter().filter(|a| a.path().is_ident("case")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    word = meta.value()?.parse::<LitStr>()?.value();
                } else if meta.path.is_ident("default") {
                    is_default = true;
                } else {
                    return Err(meta.error("expected `name = \"…\"` or `default`"));
                }
                Ok(())
            })?;
        }
        let word_lit = LitStr::new(&word, Span::call_site());
        if is_default {
            if default.is_some() {
                return Err(syn::Error::new(variant.span(), "one default case at most"));
            }
            default = Some(word_lit.clone());
        }
        words.push(word_lit.clone());
        match &variant.fields {
            Fields::Unit => {
                cases_declare.push(quote! { (#word_lit, ::std::vec::Vec::new()) });
                cases_read.push(quote! { Some(#word_lit) => Ok(#name::#ident), });
                cases_write.push(quote! { #name::#ident => choices.set(#tag_lit, #word_lit), });
            }
            Fields::Unnamed(one) if one.unnamed.len() == 1 => {
                let ty = &one.unnamed[0].ty;
                cases_declare.push(quote! { (#word_lit, <#ty as ::energyml_build::declare::Params>::slots()) });
                cases_read.push(quote! {
                    Some(#word_lit) => Ok(#name::#ident(<#ty as ::energyml_build::declare::Params>::read(choices)?)),
                });
                cases_write.push(quote! {
                    #name::#ident(inner) => ::energyml_build::declare::Params::write(inner, choices.set(#tag_lit, #word_lit)),
                });
            }
            _ => {
                return Err(syn::Error::new(
                    variant.span(),
                    "a case is a word, or a word and one `Params` struct: `Kriging(KrigingParams)`",
                ));
            }
        }
    }
    let suggest = default.as_ref().map(|word| quote! { slot = slot.suggest(::energyml_build::declare::Json::from(#word)); });
    // The default case is also **the reading of an absent choice** — the rule
    // of a field's default: one expression, two readers.
    let absent = default.as_ref().map(|word| quote! { .or(Some(#word)) });
    let group = group.map(|g| quote! { slot = slot.grouped(#g); });
    let advanced = advanced.then(|| quote! { slot = slot.advanced(); });
    Ok(quote! {
        impl ::energyml_build::declare::Params for #name {
            fn slots() -> ::std::vec::Vec<::energyml_build::operation::Slot> {
                let mut slot = ::energyml_build::operation::Slot::required(
                    #tag_lit,
                    ::energyml_build::operation::Need::Choice(&[#(#words),*]),
                    #about,
                );
                #suggest
                #group
                #advanced
                ::energyml_build::declare::union_slots(slot, ::std::vec![#(#cases_declare),*])
            }
            fn read(choices: &::energyml_build::operation::Choices) -> ::std::result::Result<Self, ::energyml_build::operation::Refusal> {
                match choices.text(#tag_lit) #absent {
                    #(#cases_read)*
                    other => Err(::energyml_build::operation::Refusal::on(
                        #tag_lit,
                        format!("expected one of {}, got {:?}", [#(#words),*].join(", "), other),
                    )),
                }
            }
            fn write(&self, choices: ::energyml_build::operation::Choices) -> ::energyml_build::operation::Choices {
                match self { #(#cases_write)* }
            }
        }
    })
}

// ---------------------------------------------------------------- Inputs

/// One input slot per field, bound by the framework — see the crate
/// documentation.
#[proc_macro_derive(Inputs, attributes(input))]
pub fn derive_inputs(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match inputs(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn inputs(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(input.span(), "`Inputs` is for a struct"));
    };
    let fields: Vec<&syn::Field> = match &data.fields {
        Fields::Named(named) => named.named.iter().collect(),
        Fields::Unit => Vec::new(),
        Fields::Unnamed(_) => return Err(syn::Error::new(input.span(), "`Inputs` needs named fields")),
    };
    let mut declare = Vec::new();
    let mut bind = Vec::new();
    let mut idents = Vec::new();
    let mut names = Vec::new();
    for field in fields {
        let ident = field.ident.clone().expect("named");
        let ty = &field.ty;
        let mut slot = ident.to_string().to_lower_camel_case();
        let mut about = doc_of(&field.attrs);
        let mut topology: Option<String> = None;
        let mut types: Vec<String> = Vec::new();
        let mut least: Option<Expr> = None;
        let mut ordered = false;
        let mut every = false;
        let mut palette = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("input")) {
            attr.parse_nested_meta(|meta| {
                let key = meta.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
                match key.as_str() {
                    "name" => slot = meta.value()?.parse::<LitStr>()?.value(),
                    "about" => about = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "topology" => topology = Some(meta.value()?.parse::<LitStr>()?.value()),
                    // `types = "a, b"`: a string rather than an array, because
                    // a `[...]` in an attribute means parsing an
                    // expression to save a pair of brackets.
                    "types" => {
                        types = meta
                            .value()?
                            .parse::<LitStr>()?
                            .value()
                            .split(',')
                            .map(|one| one.trim().to_string())
                            .filter(|one| !one.is_empty())
                            .collect()
                    }
                    "least" => least = Some(meta.value()?.parse()?),
                    "ordered" => ordered = true,
                    // Left empty, every object of the target package of these
                    // `types` — `Otherwise::Every`, filled by `read::every`.
                    "every" => every = true,
                    // A ramp read as a palette — `Takes::palette`.
                    "palette" => palette = true,
                    _ => return Err(meta.error("unknown `input` attribute")),
                }
                Ok(())
            })?;
        }
        let about = about.ok_or_else(|| {
            syn::Error::new(field.span(), "an input says what it is: a doc comment, or `#[input(about = \"…\")]`")
        })?;
        let slot_lit = LitStr::new(&slot, Span::call_site());
        let about = LitStr::new(&about, Span::call_site());
        let topology = topology.map(|t| quote! { Some(#t) }).unwrap_or(quote! { None });
        let types = quote! { &[#(#types),*] };
        let least = least.map(|l| quote! { slot.least = #l; });
        let ordered = ordered.then(|| quote! { slot = slot.in_order(); });
        let every = every.then(|| quote! { slot = slot.otherwise(::energyml_build::operation::Otherwise::Every); });
        let palette = palette.then(|| {
            quote! {
                if let ::energyml_build::operation::Need::Input(takes) = &mut slot.need {
                    takes.palette = true;
                }
            }
        });
        declare.push(quote! {
            {
                let mut slot = ::energyml_build::declare::input_slot_of::<#ty>(#slot_lit, #about, #topology, #types);
                #least
                #ordered
                #every
                #palette
                out.push(slot);
            }
        });
        bind.push(quote! { let #ident: #ty = binder.take(#slot_lit, &slots)?; });
        idents.push(ident);
        names.push(slot_lit);
    }
    Ok(quote! {
        impl ::energyml_build::declare::Inputs for #name {
            fn slots() -> ::std::vec::Vec<::energyml_build::operation::Slot> {
                let mut out = ::std::vec::Vec::new();
                #(#declare)*
                out
            }
            #[allow(unused_variables)]
            fn bind(bag: &::energyml_build::pieces::Pieces) -> ::std::result::Result<Self, ::energyml_build::operation::Refusal> {
                let slots = <Self as ::energyml_build::declare::Inputs>::slots();
                let binder = ::energyml_build::declare::Binder::new(bag, &slots);
                #(#bind)*
                Ok(Self { #(#idents),* })
            }
        }
    })
}
