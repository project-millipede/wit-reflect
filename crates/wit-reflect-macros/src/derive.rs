//! Expansion of `#[derive(WitType)]`.
//!
//! The expansion reflects the item exactly as declared. Unsupported shapes are
//! rejected here, at the declaration, instead of producing a schema that fails
//! later.

use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Field, Fields, FieldsNamed, Generics, Ident, LitStr,
    Path, Token, Variant, parse_quote,
};

use crate::attributes::{ContainerAttributes, Errors, FieldAttributes, VariantAttributes};
use crate::names::{kebab_name, runtime_path, with_docs};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let mut errors = Errors::default();

    let container = ContainerAttributes::parse(&input.attrs, &mut errors);
    let runtime = runtime_path(container.crate_path.as_ref());
    let reflection_bound = quote! { #runtime::WitType };
    let generics = bounded_generics(&input.generics, &reflection_bound, &mut errors);

    // After a reported error the partial body is discarded with `finish`.
    let body = match &input.data {
        Data::Struct(data) => struct_body(input, data, &container, &runtime, &mut errors),
        Data::Enum(data) => enum_body(input, data, &container, &runtime, &mut errors),
        Data::Union(data) => {
            errors.report(data.union_token, "a union has no WIT representation");
            TokenStream::new()
        }
    };
    errors.finish()?;

    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #runtime::WitType for #ident #type_generics #where_clause {
            fn wit_type(graph: &mut #runtime::TypeGraph) -> #runtime::Type {
                #body
            }
        }
    })
}

/// Require `bound` of every type parameter and reject borrowed data.
pub(crate) fn bounded_generics(
    generics: &Generics,
    bound: &TokenStream,
    errors: &mut Errors,
) -> Generics {
    for lifetime in generics.lifetimes() {
        errors.report(
            lifetime,
            "a WIT boundary value must own its data; remove the lifetime parameter",
        );
    }

    let mut bounded = generics.clone();
    let type_parameters: Vec<Ident> = generics
        .type_params()
        .map(|parameter| parameter.ident.clone())
        .collect();
    let where_clause = bounded.make_where_clause();
    for type_parameter in type_parameters {
        where_clause
            .predicates
            .push(parse_quote!(#type_parameter: #bound));
    }
    bounded
}

fn struct_body(
    input: &DeriveInput,
    data: &DataStruct,
    container: &ContainerAttributes,
    runtime: &Path,
    errors: &mut Errors,
) -> TokenStream {
    if let Some(transparent) = &container.transparent {
        return transparent_body(data, transparent, container, runtime, errors);
    }

    match &data.fields {
        Fields::Named(fields) => {
            let name = container_name(input, container);
            let field_definitions = record_fields(fields, runtime, errors);
            let docs = with_docs(&input.attrs);
            quote! {
                graph.define::<Self>(|graph| {
                    #runtime::TypeDef::record(#name, ::std::vec![#(#field_definitions),*]) #docs
                })
            }
        }
        Fields::Unnamed(fields) => {
            errors.report(
                fields,
                "a WIT record needs field names; use named fields, or \
                 `#[wit(transparent)]` on a single-field struct",
            );
            TokenStream::new()
        }
        Fields::Unit => {
            errors.report(&input.ident, "a WIT record needs at least one field");
            TokenStream::new()
        }
    }
}

/// A single-field struct represented by its field's type.
fn transparent_body(
    data: &DataStruct,
    transparent: &Path,
    container: &ContainerAttributes,
    runtime: &Path,
    errors: &mut Errors,
) -> TokenStream {
    if let Some(name) = &container.name {
        errors.report(
            name,
            "a transparent struct defines no WIT type, so it has no name",
        );
    }

    let mut fields = data.fields.iter();
    let (Some(field), None) = (fields.next(), fields.next()) else {
        errors.report(
            transparent,
            "`transparent` requires a struct with exactly one field",
        );
        return TokenStream::new();
    };
    let field_type = &field.ty;
    quote! {
        <#field_type as #runtime::WitType>::wit_type(graph)
    }
}

fn enum_body(
    input: &DeriveInput,
    data: &DataEnum,
    container: &ContainerAttributes,
    runtime: &Path,
    errors: &mut Errors,
) -> TokenStream {
    if let Some(transparent) = &container.transparent {
        errors.report(transparent, "`transparent` applies only to structs");
    }
    if data.variants.is_empty() {
        errors.report(
            &input.ident,
            "a WIT enum or variant needs at least one case",
        );
        return TokenStream::new();
    }

    let name = container_name(input, container);
    let docs = with_docs(&input.attrs);
    let has_only_unit_variants = data
        .variants
        .iter()
        .all(|variant| matches!(variant.fields, Fields::Unit));

    if has_only_unit_variants {
        let cases = enum_cases(&data.variants, runtime, errors);
        // No field type is reflected, so the closure's graph is unused.
        quote! {
            graph.define::<Self>(|_| {
                #runtime::TypeDef::enumeration(#name, ::std::vec![#(#cases),*]) #docs
            })
        }
    } else {
        let cases = variant_cases(&data.variants, runtime, errors);
        quote! {
            graph.define::<Self>(|graph| {
                #runtime::TypeDef::variant(#name, ::std::vec![#(#cases),*]) #docs
            })
        }
    }
}

fn enum_cases(
    variants: &Punctuated<Variant, Token![,]>,
    runtime: &Path,
    errors: &mut Errors,
) -> Vec<TokenStream> {
    let mut cases = Vec::with_capacity(variants.len());
    for variant in variants {
        let variant_attributes = VariantAttributes::parse(&variant.attrs, errors);
        if let Some(payload) = &variant_attributes.payload {
            errors.report(payload, "`payload` applies only to struct-like variants");
        }
        let name = kebab_name(&variant.ident, variant_attributes.name.as_ref());
        let docs = with_docs(&variant.attrs);
        cases.push(quote! { #runtime::EnumCase::new(#name) #docs });
    }
    cases
}

fn variant_cases(
    variants: &Punctuated<Variant, Token![,]>,
    runtime: &Path,
    errors: &mut Errors,
) -> Vec<TokenStream> {
    let mut cases = Vec::with_capacity(variants.len());
    for variant in variants {
        let variant_attributes = VariantAttributes::parse(&variant.attrs, errors);
        let name = kebab_name(&variant.ident, variant_attributes.name.as_ref());
        let docs = with_docs(&variant.attrs);

        let payload = match &variant.fields {
            Fields::Named(fields) => {
                let record_name = match &variant_attributes.payload {
                    Some(record_name) => quote! { ::core::option::Option::Some(#record_name) },
                    None => quote! { ::core::option::Option::None },
                };
                let field_definitions = record_fields(fields, runtime, errors);
                quote! {
                    #runtime::Payload::Record {
                        name: #record_name,
                        fields: ::std::vec![#(#field_definitions),*],
                    }
                }
            }
            Fields::Unnamed(fields) => {
                reject_payload_name(&variant_attributes, errors);
                let element_types: Vec<TokenStream> = fields
                    .unnamed
                    .iter()
                    .map(|field| reflect(field, runtime))
                    .collect();
                match element_types.as_slice() {
                    [] => {
                        errors.report(
                            variant,
                            "an empty tuple variant carries nothing; write a unit variant",
                        );
                        TokenStream::new()
                    }
                    [only_type] => quote! { #runtime::Payload::Type(#only_type) },
                    _ => quote! {
                        #runtime::Payload::Type(#runtime::Type::Tuple(
                            ::std::vec![#(#element_types),*]
                        ))
                    },
                }
            }
            Fields::Unit => {
                reject_payload_name(&variant_attributes, errors);
                quote! { #runtime::Payload::None }
            }
        };
        cases.push(quote! { #runtime::Case::new(#name, #payload) #docs });
    }
    cases
}

fn reject_payload_name(variant_attributes: &VariantAttributes, errors: &mut Errors) {
    if let Some(payload) = &variant_attributes.payload {
        errors.report(payload, "`payload` applies only to struct-like variants");
    }
}

/// `Field::new(..)` expressions for a struct's or struct-like variant's fields.
fn record_fields(fields: &FieldsNamed, runtime: &Path, errors: &mut Errors) -> Vec<TokenStream> {
    if fields.named.is_empty() {
        errors.report(fields, "a WIT record needs at least one field");
    }

    let mut field_definitions = Vec::with_capacity(fields.named.len());
    for field in &fields.named {
        let field_attributes = FieldAttributes::parse(&field.attrs, errors);
        let Some(ident) = &field.ident else {
            // `FieldsNamed` always carries identifiers.
            continue;
        };
        let name = kebab_name(ident, field_attributes.name.as_ref());
        let field_type = reflect(field, runtime);
        let docs = with_docs(&field.attrs);
        field_definitions.push(quote! { #runtime::Field::new(#name, #field_type) #docs });
    }
    field_definitions
}

/// The expression reflecting a field's type through the enclosing graph.
fn reflect(field: &Field, runtime: &Path) -> TokenStream {
    let field_type = &field.ty;
    quote! { <#field_type as #runtime::WitType>::wit_type(graph) }
}

fn container_name(input: &DeriveInput, container: &ContainerAttributes) -> LitStr {
    kebab_name(&input.ident, container.name.as_ref())
}
