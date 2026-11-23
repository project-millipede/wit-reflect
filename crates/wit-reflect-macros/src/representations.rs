//! Expansion of `representations!`: a crate's map of foreign representations.
//!
//! The map is a unit struct of the declaring crate. Each entry implements
//! `Represents<Foreign>` for it, which the orphan rule allows because the
//! struct is local. The derive asks the map for every field without a rule.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Ident, Token, Type, Visibility, braced};

use crate::attributes::Errors;

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
struct Entry {
    foreign: Type,
    shown: Type,
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
        let foreign = input.parse()?;
        input.parse::<Token![=>]>()?;
        let shown = input.parse()?;
        Ok(Self { foreign, shown })
    }
}

pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let declaration: Declaration = syn::parse2(input)?;

    let mut errors = Errors::default();
    for group in &declaration.groups {
        for entry in &group.entries {
            require_source(entry, &group.source, &mut errors);
        }
    }
    errors.finish()?;

    let Declaration {
        attributes,
        visibility,
        name,
        groups,
    } = &declaration;
    let implementations = groups.iter().flat_map(|group| &group.entries).map(|entry| {
        let Entry { foreign, shown } = entry;
        // The conversion carries the shown type's location, so a missing
        // `From` is reported there. The implementation as a whole carries the
        // foreign type's, so two entries for one type are reported at the
        // entry.
        let conversion = quote_spanned! {shown.span()=>
            <#shown as ::core::convert::From<#foreign>>::from(value)
        };
        quote_spanned! {foreign.span()=>
            impl ::wit_reflect::rule::Represents<#foreign> for #name {
                type As = #shown;

                fn represent(value: #foreign) -> #shown {
                    #conversion
                }
            }
        }
    });
    Ok(quote! {
        #(#attributes)*
        #[derive(Clone, Copy, Debug)]
        #visibility struct #name;

        #(#implementations)*
    })
}

/// An entry stands in the group of the crate its type comes from: its path
/// begins with the group's name. The groups are only as useful as they are
/// true, so a misplaced entry is an error.
fn require_source(entry: &Entry, source: &Ident, errors: &mut Errors) {
    let first_segment = match &entry.foreign {
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
            &entry.foreign,
            &format!(
                "this type's path starts with `{first_segment}`, but the entry stands in \
                 the group `{source}`; write the full path and move it to its own group"
            ),
        ),
        None => errors.report(
            &entry.foreign,
            &format!("write this type as a path that starts with `{source}`"),
        ),
    }
}
