//! Expansion of `representations!`: a crate's map of foreign representations.
//!
//! The map is a unit struct of the declaring crate. Each entry implements
//! `Represents<Foreign>` for it, which the orphan rule allows because the
//! struct is local. The derive asks the map for every field without a rule.
//!
//! An entry converts through `From` unless it states its conversion after
//! `with`. A stated conversion is expanded once for each foreign type of its
//! entry. It can therefore call functions that several foreign types have in
//! common without sharing a trait, and it can connect two types of other
//! crates, for which this crate could not implement `From`.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, ExprClosure, Ident, Pat, Token, Type, Visibility, braced};

use crate::attributes::Errors;

mod keyword {
    syn::custom_keyword!(with);
}

/// `#[doc] vis Name { group { Foreign => Shown, … } … }`
struct Declaration {
    attributes: Vec<Attribute>,
    visibility: Visibility,
    name: Ident,
    groups: Vec<Group>,
}

/// The entries for the types of one crate.
struct Group {
    /// The crate the types come from.
    source: Ident,
    entries: Punctuated<Entry, Token![,]>,
}

/// `Foreign => Shown`: the contract shows `Foreign` as `Shown`.
///
/// Several foreign types separated by `|` share the entry, and
/// `with conversion` states how a value becomes `Shown`.
struct Entry {
    foreign: Punctuated<Type, Token![|]>,
    shown: Type,
    conversion: Option<Expr>,
}

impl Parse for Declaration {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;
        let visibility = input.parse()?;
        let name = input.parse()?;
        let body;
        braced!(body in input);
        let mut groups = Vec::new();
        while !body.is_empty() {
            groups.push(body.parse()?);
        }
        Ok(Self {
            attributes,
            visibility,
            name,
            groups,
        })
    }
}

impl Parse for Group {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let source = input.parse()?;
        let body;
        braced!(body in input);
        let entries = body.parse_terminated(Entry::parse, Token![,])?;
        Ok(Self { source, entries })
    }
}

impl Parse for Entry {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let foreign = Punctuated::parse_separated_nonempty(input)?;
        input.parse::<Token![=>]>()?;
        let shown = input.parse()?;
        let conversion = if input.peek(keyword::with) {
            input.parse::<keyword::with>()?;
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self {
            foreign,
            shown,
            conversion,
        })
    }
}

pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let declaration: Declaration = syn::parse2(input)?;

    let mut errors = Errors::default();
    for group in &declaration.groups {
        for entry in &group.entries {
            for foreign in &entry.foreign {
                require_source(foreign, &group.source, &mut errors);
            }
            require_plain_parameter(entry, &mut errors);
        }
    }
    errors.finish()?;

    let Declaration {
        attributes,
        visibility,
        name,
        groups,
    } = &declaration;
    let implementations = groups
        .iter()
        .flat_map(|group| &group.entries)
        .flat_map(|entry| {
            entry
                .foreign
                .iter()
                .map(move |foreign| implementation(name, foreign, entry))
        });
    Ok(quote! {
        #(#attributes)*
        #[derive(Clone, Copy, Debug)]
        #visibility struct #name;

        #(#implementations)*
    })
}

/// `impl Represents<Foreign> for Map`, for one foreign type of an entry.
fn implementation(map: &Ident, foreign: &Type, entry: &Entry) -> TokenStream {
    let shown = &entry.shown;
    let represent = match &entry.conversion {
        // The conversion carries the shown type's location, so a missing
        // `From` is reported there.
        None => {
            let conversion = quote_spanned! {shown.span()=>
                <#shown as ::core::convert::From<#foreign>>::from(value)
            };
            quote! {
                fn represent(value: #foreign) -> #shown {
                    #conversion
                }
            }
        }
        // The stated expression becomes the function's body, with its
        // parameter typed as this foreign type. A closure would have to infer
        // that type, which fails for a method call on the value.
        Some(Expr::Closure(closure)) => {
            let parameter = parameter(closure);
            let body = &closure.body;
            quote! {
                fn represent(#parameter: #foreign) -> #shown {
                    #body
                }
            }
        }
        Some(function) => quote_spanned! {function.span()=>
            fn represent(value: #foreign) -> #shown {
                (#function)(value)
            }
        },
    };
    // The implementation as a whole carries the foreign type's location, so
    // two entries for one type are reported at the entry.
    quote_spanned! {foreign.span()=>
        impl ::wit_reflect::rule::Represents<#foreign> for #map {
            type As = #shown;

            #represent
        }
    }
}

/// The single, untyped name a stated conversion gives its value.
fn parameter(closure: &ExprClosure) -> Option<&Ident> {
    let mut inputs = closure.inputs.iter();
    match (inputs.next(), inputs.next()) {
        (Some(Pat::Ident(name)), None) if name.by_ref.is_none() && name.subpat.is_none() => {
            Some(&name.ident)
        }
        _ => None,
    }
}

/// A conversion written as a closure names its value and nothing else. The
/// value takes each foreign type of the entry, so it has no type of its own.
fn require_plain_parameter(entry: &Entry, errors: &mut Errors) {
    let Some(Expr::Closure(closure)) = &entry.conversion else {
        return;
    };
    if parameter(closure).is_none() {
        errors.report(
            &closure.inputs,
            "write the conversion as `|value| expression`, with one untyped name for the value",
        );
    }
}

/// An entry stands in the group of the crate its types come from: each path
/// begins with the group's name. The groups are only as useful as they are
/// true, so a misplaced entry is an error.
fn require_source(foreign: &Type, source: &Ident, errors: &mut Errors) {
    let first_segment = match foreign {
        Type::Path(type_path) if type_path.qself.is_none() => type_path
            .path
            .segments
            .first()
            .map(|segment| &segment.ident),
        _ => None,
    };
    match first_segment {
        Some(first_segment) if first_segment == source => {}
        Some(first_segment) => errors.report(
            foreign,
            &format!(
                "this type's path starts with `{first_segment}`, but the entry stands in \
                 the group `{source}`; write the full path and move it to its own group"
            ),
        ),
        None => errors.report(
            foreign,
            &format!("write this type as a path that starts with `{source}`"),
        ),
    }
}
