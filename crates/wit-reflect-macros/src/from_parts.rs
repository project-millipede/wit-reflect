//! Expansion of `#[derive(FromParts)]` on generated binding types.
//!
//! wit-bindgen applies the derive named in `additional_derives` to every
//! record, variant and enum it generates, including those of imported
//! interfaces. The expansion therefore never reports an error: for a shape it
//! does not handle it emits nothing.
//!
//! Each implementation is generic over the parts of its fields, so it carries
//! no knowledge of owner types. A binding record is built from a field list
//! and a binding enum from a nested choice, both by position.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DataEnum, DataStruct, DeriveInput, Fields, Ident};

pub(crate) fn expand(input: &DeriveInput) -> TokenStream {
    // Generated bindings that own their data have no parameters.
    if !input.generics.params.is_empty() {
        return TokenStream::new();
    }
    let assembly = match &input.data {
        Data::Struct(data) => record_assembly(&input.ident, data),
        Data::Enum(data) => cases_assembly(&input.ident, data),
        Data::Union(_) => None,
    };
    assembly.unwrap_or_default()
}

/// `impl<P0, P1> FromParts<Fields<(P0, (P1, ()))>> for Record`.
fn record_assembly(ident: &Ident, data: &DataStruct) -> Option<TokenStream> {
    let Fields::Named(fields) = &data.fields else {
        return None;
    };
    let lower = quote! { ::wit_reflect::lower };

    let field_names: Vec<&Option<Ident>> = fields.named.iter().map(|field| &field.ident).collect();
    let field_types = fields.named.iter().map(|field| &field.ty);
    let part_parameters: Vec<Ident> = (0..field_names.len())
        .map(|position| format_ident!("Parts{position}"))
        .collect();
    let part_variables: Vec<Ident> = (0..field_names.len())
        .map(|position| format_ident!("parts_{position}"))
        .collect();
    let part_list_type = part_parameters.iter().rev().fold(
        quote! { () },
        |rest, parameter| quote! { (#parameter, #rest) },
    );
    let part_list_pattern = part_variables.iter().rev().fold(
        quote! { () },
        |rest, variable| quote! { (#variable, #rest) },
    );

    Some(quote! {
        #[automatically_derived]
        impl<#(#part_parameters),*> #lower::FromParts<#lower::Fields<#part_list_type>> for #ident
        where
            #(#field_types: #lower::FromParts<#part_parameters>,)*
        {
            fn from_parts(parts: #lower::Fields<#part_list_type>) -> Self {
                let #lower::Fields(#part_list_pattern) = parts;
                Self {
                    #(#field_names: #lower::FromParts::from_parts(#part_variables),)*
                }
            }
        }
    })
}

/// `impl<P0, P1> FromParts<Cases<Either<P0, Either<P1, Never>>>> for Enum`.
fn cases_assembly(ident: &Ident, data: &DataEnum) -> Option<TokenStream> {
    let lower = quote! { ::wit_reflect::lower };

    let part_parameters: Vec<Ident> = (0..data.variants.len())
        .map(|position| format_ident!("Parts{position}"))
        .collect();
    let choice_type = part_parameters.iter().rev().fold(
        quote! { #lower::Never },
        |later_cases, parameter| quote! { #lower::Either<#parameter, #later_cases> },
    );

    let mut bounds = Vec::with_capacity(data.variants.len());
    let mut arms = Vec::with_capacity(data.variants.len());
    for (position, variant) in data.variants.iter().enumerate() {
        let case = &variant.ident;
        let parameter = &part_parameters[position];
        // `Next(Next(This(parts)))` selects the case at `position`.
        let mut choice = quote! { #lower::Either::This(parts) };
        for _ in 0..position {
            choice = quote! { #lower::Either::Next(#choice) };
        }

        match &variant.fields {
            // A unit case has the part `()`.
            Fields::Unit => {
                bounds.push(quote! { (): #lower::FromParts<#parameter> });
                arms.push(quote! {
                    #choice => {
                        let (): () = #lower::FromParts::from_parts(parts);
                        Self::#case
                    }
                });
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let payload_type = &fields.unnamed[0].ty;
                bounds.push(quote! { #payload_type: #lower::FromParts<#parameter> });
                arms.push(quote! {
                    #choice => Self::#case(#lower::FromParts::from_parts(parts))
                });
            }
            // wit-bindgen generates no other case shape.
            _ => return None,
        }
    }

    // After every case only the uninhabited end of the choice remains.
    let mut exhausted = quote! { never };
    for _ in 0..data.variants.len() {
        exhausted = quote! { #lower::Either::Next(#exhausted) };
    }

    Some(quote! {
        #[automatically_derived]
        impl<#(#part_parameters),*> #lower::FromParts<#lower::Cases<#choice_type>> for #ident
        where
            #(#bounds,)*
        {
            fn from_parts(parts: #lower::Cases<#choice_type>) -> Self {
                match parts.0 {
                    #(#arms,)*
                    #exhausted => match never {},
                }
            }
        }
    })
}
