//! Expansion of `#[derive(WitType)]`.
//!
//! The expansion reflects the item exactly as declared, with a field's wire
//! type replaced where a representation rule says so. Unsupported shapes are
//! rejected here, at the declaration, instead of producing a schema that fails
//! later.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Field, Fields, FieldsNamed, Generics, Ident, LitStr,
    Path, Token, Variant, parse_quote,
};

use crate::attributes::{
    ContainerAttributes, Errors, FieldAttributes, FieldRule, VariantAttributes,
};
use crate::names::{kebab_name, runtime_path, with_docs};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let mut errors = Errors::default();

    let container = ContainerAttributes::parse(&input.attrs, &mut errors);
    let runtime = runtime_path(container.crate_path.as_ref());
    let reflection_bound = quote! { #runtime::WitType };
    let generics = bounded_generics(&input.generics, &reflection_bound, &mut errors);

    // After a reported error the partial expansion is discarded with `finish`.
    let body = match &input.data {
        Data::Struct(data) => struct_body(input, data, &container, &runtime, &mut errors),
        Data::Enum(data) => enum_body(input, data, &container, &runtime, &mut errors),
        Data::Union(data) => {
            errors.report(data.union_token, "a union has no WIT representation");
            TokenStream::new()
        }
    };
    let remote_conversion = match &container.remote {
        Some(remote) => remote_conversion(input, remote, &mut errors),
        None => TokenStream::new(),
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

        #remote_conversion
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

/// Where a field is declared, which decides the rules it accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldSite {
    /// A named field of a struct: every rule applies.
    StructField,
    /// A named field of a struct-like enum case.
    CaseField,
    /// An unnamed field: a tuple case's payload or a transparent struct's field.
    Unnamed,
}

fn struct_body(
    input: &DeriveInput,
    data: &DataStruct,
    container: &ContainerAttributes,
    runtime: &Path,
    errors: &mut Errors,
) -> TokenStream {
    if let Some(cases) = &container.cases {
        errors.report(cases, "`cases` applies only to enums");
    }
    if let Some(transparent) = &container.transparent {
        return transparent_body(data, transparent, container, runtime, errors);
    }

    match &data.fields {
        Fields::Named(fields) => {
            let name = container_name(input, container);
            let mut field_definitions = computed_fields(container, runtime);
            let declared_fields = record_fields(fields, FieldSite::StructField, runtime, errors);
            field_definitions.extend(declared_fields);
            if field_definitions.is_empty() {
                errors.report(&input.ident, "a WIT record needs at least one field");
            }
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

/// A single-field struct represented by its field's wire type.
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
    if let Some(computed_field) = container.computed_fields.first() {
        errors.report(
            &computed_field.name,
            "a transparent struct defines no record, so it has no computed fields",
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
    let field_attributes = FieldAttributes::parse(&field.attrs, errors);
    reflect(
        field,
        &field_attributes,
        FieldSite::Unnamed,
        runtime,
        errors,
    )
    .unwrap_or_default()
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

    // An enum with computed fields is a record that holds its cases.
    let leading_fields = computed_fields(container, runtime);
    match (leading_fields.is_empty(), &container.cases) {
        (true, None) => {}
        (true, Some(cases)) => {
            errors.report(
                cases,
                "`cases` names the field that holds the cases of an enum with computed \
                 fields; this enum has none",
            );
        }
        (false, None) => {
            errors.report(
                &input.ident,
                "an enum with computed fields is exported as a record; \
                 name the field that holds its cases with `cases = \"…\"`",
            );
        }
        (false, Some(cases_field)) => {
            let cases = variant_cases(&data.variants, runtime, errors);
            return quote! {
                graph.define::<Self>(|graph| {
                    #runtime::TypeDef::record_of_cases(
                        #name,
                        ::std::vec![#(#leading_fields),*],
                        #cases_field,
                        ::std::vec![#(#cases),*],
                    ) #docs
                })
            };
        }
    }

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
        reject_payload_name(&variant_attributes, errors);
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
                let field_definitions =
                    record_fields(fields, FieldSite::CaseField, runtime, errors);
                if field_definitions.is_empty() {
                    errors.report(fields, "a WIT record needs at least one field");
                }
                quote! {
                    #runtime::Payload::Record {
                        name: #record_name,
                        fields: ::std::vec![#(#field_definitions),*],
                    }
                }
            }
            Fields::Unnamed(fields) => {
                reject_payload_name(&variant_attributes, errors);
                let mut element_types = Vec::with_capacity(fields.unnamed.len());
                for field in &fields.unnamed {
                    let field_attributes = FieldAttributes::parse(&field.attrs, errors);
                    let reflected = reflect(
                        field,
                        &field_attributes,
                        FieldSite::Unnamed,
                        runtime,
                        errors,
                    );
                    element_types.extend(reflected);
                }
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

/// `Field::new(..)` expressions for the exported named fields, in order.
fn record_fields(
    fields: &FieldsNamed,
    site: FieldSite,
    runtime: &Path,
    errors: &mut Errors,
) -> Vec<TokenStream> {
    let mut field_definitions = Vec::with_capacity(fields.named.len());
    for field in &fields.named {
        let field_attributes = FieldAttributes::parse(&field.attrs, errors);
        let Some(ident) = &field.ident else {
            // `FieldsNamed` always carries identifiers.
            continue;
        };
        let Some(wire_type) = reflect(field, &field_attributes, site, runtime, errors) else {
            // `skip`: the field is not part of the contract.
            continue;
        };
        let name = kebab_name(ident, field_attributes.name.as_ref());
        let docs = with_docs(&field.attrs);
        field_definitions.push(quote! { #runtime::Field::new(#name, #wire_type) #docs });
    }
    field_definitions
}

/// `Field::new(..)` expressions for the container's computed fields.
fn computed_fields(container: &ContainerAttributes, runtime: &Path) -> Vec<TokenStream> {
    container
        .computed_fields
        .iter()
        .map(|computed_field| {
            let name = &computed_field.name;
            let function = &computed_field.function;
            quote! {
                #runtime::Field::new(#name, #runtime::rule::computed::<Self, _>(#function, graph))
            }
        })
        .collect()
}

/// The expression reflecting a field's wire type through the enclosing graph.
///
/// Returns `None` for a skipped field. Each rule calls the runtime function
/// whose bounds state the rule's requirements, so an unsatisfiable rule fails
/// at the field's type.
fn reflect(
    field: &Field,
    field_attributes: &FieldAttributes,
    site: FieldSite,
    runtime: &Path,
    errors: &mut Errors,
) -> Option<TokenStream> {
    let field_type = &field.ty;
    if site == FieldSite::Unnamed {
        if let Some(name) = &field_attributes.name {
            errors.report(name, "an unnamed field has no WIT name");
        }
    }

    let Some(declared) = &field_attributes.rule else {
        return Some(quote! { <#field_type as #runtime::WitType>::wit_type(graph) });
    };
    match &declared.rule {
        FieldRule::Display => Some(quote! { #runtime::rule::display::<#field_type>(graph) }),
        FieldRule::Widen => Some(quote! { #runtime::rule::widened::<#field_type>(graph) }),
        FieldRule::Into(wire_type) => Some(quote! {
            #runtime::rule::converted::<#field_type, #wire_type>(graph)
        }),
        FieldRule::With(function) => {
            if site != FieldSite::StructField {
                errors.report(
                    &declared.declared_by,
                    "`with` applies to struct fields; inside an enum case use `into` or `display`",
                );
            }
            Some(quote! { #runtime::rule::computed::<Self, _>(#function, graph) })
        }
        FieldRule::Skip => {
            if site == FieldSite::Unnamed {
                errors.report(&declared.declared_by, "`skip` applies only to named fields");
                // Keep the field's position so this is the only error reported.
                return Some(TokenStream::new());
            }
            None
        }
    }
}

/// `impl From<Foreign> for Local`, written so the compiler checks the local
/// declaration against the foreign type.
///
/// An enum is matched exhaustively and a struct is destructured completely. A
/// case or field that the foreign type adds, removes or retypes fails to
/// compile here.
fn remote_conversion(input: &DeriveInput, remote: &Path, errors: &mut Errors) -> TokenStream {
    if !input.generics.params.is_empty() {
        errors.report(
            &input.generics,
            "a remote declaration describes one concrete foreign type; remove the parameters",
        );
    }

    let conversion = match &input.data {
        Data::Enum(data) => {
            let arms = data.variants.iter().map(|variant| {
                let case = &variant.ident;
                let shape = moved_fields(&variant.fields);
                quote! { #remote::#case #shape => Self::#case #shape }
            });
            quote! {
                match foreign {
                    #(#arms,)*
                }
            }
        }
        Data::Struct(data) => {
            let shape = moved_fields(&data.fields);
            quote! {
                let #remote #shape = foreign;
                Self #shape
            }
        }
        // Already reported: a union has no WIT representation.
        Data::Union(_) => TokenStream::new(),
    };

    let ident = &input.ident;
    quote! {
        #[automatically_derived]
        impl ::core::convert::From<#remote> for #ident {
            fn from(foreign: #remote) -> Self {
                #conversion
            }
        }
    }
}

/// A pattern binding every field, which is also the expression moving them.
fn moved_fields(fields: &Fields) -> TokenStream {
    match fields {
        Fields::Named(fields) => {
            let names = fields.named.iter().map(|field| &field.ident);
            quote! { { #(#names),* } }
        }
        Fields::Unnamed(fields) => {
            let bindings =
                (0..fields.unnamed.len()).map(|position| format_ident!("field_{position}"));
            quote! { ( #(#bindings),* ) }
        }
        Fields::Unit => TokenStream::new(),
    }
}

fn container_name(input: &DeriveInput, container: &ContainerAttributes) -> LitStr {
    kebab_name(&input.ident, container.name.as_ref())
}
