//! Expansion of `#[derive(WitType)]`.
//!
//! The item is first turned into a plan: its exported fields and cases with
//! their representation rules. Two things are generated from that one plan,
//! the reflection (`WitType`) and the lowering (`Lower`), so the generated WIT
//! and the generated conversion cannot disagree about a field.
//!
//! Unsupported shapes are rejected while planning, at the declaration.

use proc_macro2::{TokenStream, TokenTree};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Field, Fields, FieldsNamed, Generics, Ident, LitStr,
    Path, Variant, parse_quote,
};

use crate::attributes::{
    ComputedField, ContainerAttributes, Errors, FieldAttributes, FieldRule, VariantAttributes,
};
use crate::names::{kebab_name, public_path, registration, runtime_path, with_docs};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let mut errors = Errors::default();

    let container = ContainerAttributes::parse(&input.attrs, &mut errors);
    let runtime = runtime_path(container.crate_path.as_ref());
    reject_lifetimes(&input.generics, &mut errors);
    reject_self_reference(input, &mut errors);
    let shape = Shape::plan(input, &container, &mut errors);
    let remote_conversion = match &container.remote {
        Some(remote) => remote_conversion(input, remote, &mut errors),
        None => TokenStream::new(),
    };
    let registration =
        interface_registration(input, &container, &runtime, &quote! { define }, &mut errors);
    errors.finish()?;
    let Some(shape) = shape else {
        // Planning reports an error whenever it yields no shape.
        return Err(syn::Error::new_spanned(
            &input.ident,
            "this item has no WIT representation",
        ));
    };

    let name = kebab_name(&input.ident, container.name.as_ref());
    let docs = with_docs(&input.attrs);
    let ident = &input.ident;
    let lower = quote! { #runtime::lower };
    let crossing = Crossing::of(input, &container, &shape);
    let details = crossing.reflected_details(&docs, ident, &container, &runtime);
    let reflection = shape.reflection(&name, &details, &container, &runtime);
    let CrossingCode {
        passage,
        parts,
        body,
        identity_assembly,
    } = crossing.code(&shape, ident, &container, &runtime);

    let reflected_generics = with_bounds(&input.generics, &quote! { #runtime::WitType });
    let (reflect_impl, type_generics, reflect_where) = reflected_generics.split_for_impl();
    let lowered_generics = with_bounds(&input.generics, &quote! { #lower::Lower });
    let (lower_impl, _, lower_where) = lowered_generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #reflect_impl #runtime::WitType for #ident #type_generics #reflect_where {
            fn wit_type(graph: &mut #runtime::TypeGraph) -> #runtime::Type {
                #reflection
            }
        }

        #[automatically_derived]
        impl #lower_impl #lower::Lower for #ident #type_generics #lower_where {
            type Passage = #passage;
            type Parts = #parts;

            fn lower(self) -> Self::Parts {
                #body
            }
        }

        #identity_assembly

        #remote_conversion

        #registration
    })
}

/// The registration that declares the type in the interface it names, if it
/// names one. `declare` is the builder function that takes the type.
pub(crate) fn interface_registration(
    input: &DeriveInput,
    container: &ContainerAttributes,
    runtime: &Path,
    declare: &TokenStream,
    errors: &mut Errors,
) -> TokenStream {
    let Some(interface) = &container.interface else {
        return TokenStream::new();
    };
    if !input.generics.params.is_empty() {
        errors.report(
            interface,
            "a generic type has one definition for each instantiation, so it cannot \
             name an interface; each instantiation is defined where it is used",
        );
        return TokenStream::new();
    }
    let ident = &input.ident;
    registration(
        runtime,
        &quote! { schema.interface(#interface).#declare::<#ident>(); },
    )
}

/// How the derive lets a type cross, from what it can see on the item alone.
enum Crossing<'input> {
    /// Taken apart and assembled into the binding's own type.
    Converted,
    /// A record, or an enum of unit cases, that the binding can use itself
    /// if the type is `Clone` and `Debug` and every one of these field
    /// types crosses unchanged. The compiler decides both.
    Plain(Vec<&'input syn::Type>),
}

impl<'input> Crossing<'input> {
    /// A type may cross unchanged only if the binding could use it as it is:
    /// public, without parameters, with the shape and names its WIT definition
    /// has, and with no rule that changes a value.
    fn of(
        input: &'input DeriveInput,
        container: &ContainerAttributes,
        shape: &Shape<'input>,
    ) -> Self {
        let is_public = matches!(input.vis, syn::Visibility::Public(_));
        let changes_shape = container.convert.is_some()
            || container.remote.is_some()
            || !container.computed_fields.is_empty();
        if !is_public || changes_shape || !input.generics.params.is_empty() {
            return Self::Converted;
        }
        match shape {
            Shape::Record(fields) if fields.iter().all(FieldPlan::is_stored_as_declared) => {
                let field_types = fields.iter().map(|field| &field.field.ty).collect();
                Self::Plain(field_types)
            }
            Shape::Enum(cases) if cases.iter().all(CasePlan::is_plain_unit) => {
                Self::Plain(Vec::new())
            }
            Shape::Record(_) | Shape::Transparent(_) | Shape::Enum(_) => Self::Converted,
        }
    }

    /// The documentation of the reflected definition, followed by where the
    /// component crate names the type if it crosses unchanged.
    ///
    /// Whether it does is known only once its field types are resolved, so the
    /// generated code asks the type's own passage.
    fn reflected_details(
        &self,
        docs: &TokenStream,
        ident: &Ident,
        container: &ContainerAttributes,
        runtime: &Path,
    ) -> TokenStream {
        match self {
            Self::Converted => docs.clone(),
            Self::Plain(_) => {
                let lower = quote! { #runtime::lower };
                let owner_path = owner_path(ident, container);
                quote! {
                    #docs
                    .unchanged_at(
                        if <<Self as #lower::Lower>::Passage as #lower::Passage>::UNCHANGED {
                            ::core::option::Option::Some(#owner_path)
                        } else {
                            ::core::option::Option::None
                        },
                    )
                }
            }
        }
    }

    /// The members of the `Lower` implementation, and the assembly a plain
    /// type needs when a converted parent holds it.
    fn code(
        &self,
        shape: &Shape<'_>,
        ident: &Ident,
        container: &ContainerAttributes,
        runtime: &Path,
    ) -> CrossingCode {
        let lower = quote! { #runtime::lower };
        let field_types = match self {
            Self::Converted => {
                let Lowering { parts, body } = shape.lowering(container, runtime, &quote! { self });
                return CrossingCode {
                    passage: quote! { #lower::Converted },
                    parts,
                    body,
                    identity_assembly: TokenStream::new(),
                };
            }
            Self::Plain(field_types) => field_types,
        };

        let whole = format_ident!("__wit_whole");
        let Lowering { parts, body } = shape.lowering(container, runtime, &quote! { #whole });
        // The bindings must be able to use the type itself, which no macro can
        // see: `BY_BINDINGS` is the inherent constant if the type is `Clone`
        // and `Debug`, otherwise the default of the imported trait.
        let usable = quote! {
            <#lower::Is<{
                #[allow(unused_imports, reason = "used only when the inherent constant is absent")]
                use #lower::NotUsable as _;
                <#lower::Usable<#ident>>::BY_BINDINGS
            }> as #lower::Answer>::Passage
        };
        // Unchanged only if every field type is, too.
        let passage = field_types.iter().rev().fold(usable, |rest, field_type| {
            quote! {
                <<#field_type as #lower::Lower>::Passage as #lower::Passage>::And<#rest>
            }
        });
        CrossingCode {
            passage,
            parts: quote! { <Self::Passage as #lower::Passage>::Crossing<Self, #parts> },
            // The closure states its result, which the return type no longer
            // determines once it depends on the passage.
            body: quote! {
                <Self::Passage as #lower::Passage>::cross(self, |#whole: Self| -> #parts { #body })
            },
            // A converted parent assembles a field of this type from the value
            // itself.
            identity_assembly: quote! {
                #[automatically_derived]
                impl #lower::FromParts<#ident> for #ident {
                    fn from_parts(parts: #ident) -> Self {
                        parts
                    }
                }
            },
        }
    }
}

/// What [`Crossing::code`] contributes to the expansion.
struct CrossingCode {
    /// The associated `Passage` type.
    passage: TokenStream,
    /// The associated `Parts` type.
    parts: TokenStream,
    /// The body of `lower`.
    body: TokenStream,
    /// `impl FromParts<Self> for Self`, or nothing for a converted type.
    identity_assembly: TokenStream,
}

/// The path at which the component crate names the type: `path = …` if given,
/// otherwise the owner crate's root.
fn owner_path(ident: &Ident, container: &ContainerAttributes) -> TokenStream {
    public_path(container.public_path.as_ref(), ident, None)
}

/// A boundary value owns its data, so its type has no lifetime parameters.
pub(crate) fn reject_lifetimes(generics: &Generics, errors: &mut Errors) {
    for lifetime in generics.lifetimes() {
        errors.report(
            lifetime,
            "a WIT boundary value must own its data; remove the lifetime parameter",
        );
    }
}

/// Report a field whose type names the item itself.
///
/// WIT types cannot be recursive, and the wire parts of such a type would be
/// infinitely nested. Without this check the compiler reports only an
/// overflow while evaluating the parts. Recursion through another type is not
/// visible here and is still reported that way.
fn reject_self_reference(input: &DeriveInput, errors: &mut Errors) {
    let fields: Vec<&Field> = match &input.data {
        Data::Struct(data) => data.fields.iter().collect(),
        Data::Enum(data) => data
            .variants
            .iter()
            .flat_map(|variant| variant.fields.iter())
            .collect(),
        Data::Union(_) => Vec::new(),
    };
    for field in fields {
        let type_tokens = field.ty.to_token_stream();
        if names_item(type_tokens, &input.ident) {
            errors.report(
                &field.ty,
                "this field contains the type it belongs to; a WIT type cannot be recursive",
            );
        }
    }
}

/// Whether `tokens` contain `Self` or `item` as a path of its own.
///
/// `other::Item` and `Item::Associated` name something else, so an identifier
/// next to a path separator does not count.
fn names_item(tokens: TokenStream, item: &Ident) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let is_path_separator = |position: Option<usize>| {
        position
            .and_then(|position| trees.get(position))
            .is_some_and(|tree| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ':'))
    };

    for (position, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Group(group) => {
                if names_item(group.stream(), item) {
                    return true;
                }
            }
            TokenTree::Ident(ident) => {
                let is_qualified = is_path_separator(position.checked_sub(1))
                    || is_path_separator(position.checked_add(1));
                if !is_qualified && (ident == item || ident == "Self") {
                    return true;
                }
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
    false
}

/// `generics` with `bound` required of every type parameter.
pub(crate) fn with_bounds(generics: &Generics, bound: &TokenStream) -> Generics {
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

/// What the item exports, decided once for reflection and lowering.
enum Shape<'input> {
    /// A struct with named fields: a record.
    Record(Vec<FieldPlan<'input>>),
    /// A single-field struct represented by that field.
    Transparent(FieldPlan<'input>),
    /// An enum: a WIT enum, a variant, or a record of its cases.
    Enum(Vec<CasePlan<'input>>),
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

struct FieldPlan<'input> {
    field: &'input Field,
    /// The variable that holds the field once the value is taken apart.
    binding: Ident,
    /// The WIT field name. `None` for an unnamed field.
    name: Option<LitStr>,
    /// Whether `name = "…"` replaces the name derived from the identifier.
    is_renamed: bool,
    rule: Option<FieldRule>,
}

struct CasePlan<'input> {
    variant: &'input Variant,
    name: LitStr,
    /// Whether `name = "…"` replaces the name derived from the identifier.
    is_renamed: bool,
    payload: CasePayload<'input>,
}

enum CasePayload<'input> {
    Unit,
    /// One field is the payload itself; several form a tuple.
    Tuple(Vec<FieldPlan<'input>>),
    Record {
        /// The declared name of the payload record.
        name: Option<LitStr>,
        fields: Vec<FieldPlan<'input>>,
    },
}

/// The associated `Parts` type and the body of `lower`.
struct Lowering {
    parts: TokenStream,
    body: TokenStream,
}

impl<'input> Shape<'input> {
    /// Returns `None` after reporting why the item cannot be exported.
    fn plan(
        input: &'input DeriveInput,
        container: &ContainerAttributes,
        errors: &mut Errors,
    ) -> Option<Self> {
        let is_record = matches!(
            &input.data,
            Data::Struct(data) if matches!(data.fields, Fields::Named(_))
        );
        if !is_record || container.transparent.is_some() {
            reject_skip_list(container, errors);
        }
        match &input.data {
            Data::Struct(data) => Self::plan_struct(input, data, container, errors),
            Data::Enum(data) => Self::plan_enum(input, data, container, errors),
            Data::Union(data) => {
                errors.report(data.union_token, "a union has no WIT representation");
                None
            }
        }
    }

    fn plan_struct(
        input: &'input DeriveInput,
        data: &'input DataStruct,
        container: &ContainerAttributes,
        errors: &mut Errors,
    ) -> Option<Self> {
        if let Some(cases) = &container.cases {
            errors.report(cases, "`cases` applies only to enums");
        }
        if let Some(transparent) = &container.transparent {
            return Self::plan_transparent(data, transparent, container, errors);
        }

        match &data.fields {
            Fields::Named(fields) => {
                let mut field_plans = plan_named_fields(fields, FieldSite::StructField, errors);
                skip_listed_fields(&mut field_plans, &container.skipped_fields, errors);
                let has_exported_field = field_plans.iter().any(|field| !field.is_skipped());
                if !has_exported_field && container.computed_fields.is_empty() {
                    errors.report(&input.ident, "a WIT record needs at least one field");
                }
                Some(Self::Record(field_plans))
            }
            Fields::Unnamed(fields) => {
                errors.report(
                    fields,
                    "a WIT record needs field names; use named fields, or \
                     `#[wit(transparent)]` on a single-field struct",
                );
                None
            }
            Fields::Unit => {
                errors.report(&input.ident, "a WIT record needs at least one field");
                None
            }
        }
    }

    fn plan_transparent(
        data: &'input DataStruct,
        transparent: &Path,
        container: &ContainerAttributes,
        errors: &mut Errors,
    ) -> Option<Self> {
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
            return None;
        };
        // The single field is the whole value, so it takes the rules of a payload.
        let field_plan = FieldPlan::plan(field, 0, FieldSite::Unnamed, errors);
        Some(Self::Transparent(field_plan))
    }

    fn plan_enum(
        input: &'input DeriveInput,
        data: &'input DataEnum,
        container: &ContainerAttributes,
        errors: &mut Errors,
    ) -> Option<Self> {
        if let Some(transparent) = &container.transparent {
            errors.report(transparent, "`transparent` applies only to structs");
        }
        if data.variants.is_empty() {
            errors.report(
                &input.ident,
                "a WIT enum or variant needs at least one case",
            );
            return None;
        }
        match (container.computed_fields.is_empty(), &container.cases) {
            (true, Some(cases)) => errors.report(
                cases,
                "`cases` names the field that holds the cases of an enum with computed \
                 fields; this enum has none",
            ),
            (false, None) => errors.report(
                &input.ident,
                "an enum with computed fields is exported as a record; \
                 name the field that holds its cases with `cases = \"…\"`",
            ),
            (true, None) | (false, Some(_)) => {}
        }

        let case_plans = data
            .variants
            .iter()
            .map(|variant| CasePlan::plan(variant, errors))
            .collect();
        Some(Self::Enum(case_plans))
    }

    /// The body of `wit_type`.
    fn reflection(
        &self,
        name: &LitStr,
        docs: &TokenStream,
        container: &ContainerAttributes,
        runtime: &Path,
    ) -> TokenStream {
        match self {
            Self::Record(fields) => {
                let mut field_definitions = computed_field_definitions(container, runtime);
                field_definitions.extend(record_field_definitions(fields, runtime));
                quote! {
                    graph.define::<Self>(|graph| {
                        #runtime::TypeDef::record(#name, ::std::vec![#(#field_definitions),*]) #docs
                    })
                }
            }
            Self::Transparent(field) => field.reflection(runtime),
            Self::Enum(cases) => {
                if let Some(cases_field) = &container.cases {
                    let leading_fields = computed_field_definitions(container, runtime);
                    let case_definitions = cases.iter().map(|case| case.definition(runtime));
                    return quote! {
                        graph.define::<Self>(|graph| {
                            #runtime::TypeDef::record_of_cases(
                                #name,
                                ::std::vec![#(#leading_fields),*],
                                #cases_field,
                                ::std::vec![#(#case_definitions),*],
                            ) #docs
                        })
                    };
                }

                let has_only_unit_cases = cases
                    .iter()
                    .all(|case| matches!(case.payload, CasePayload::Unit));
                if has_only_unit_cases {
                    let case_definitions = cases.iter().map(|case| {
                        let case_name = &case.name;
                        let case_docs = with_docs(&case.variant.attrs);
                        quote! { #runtime::EnumCase::new(#case_name) #case_docs }
                    });
                    // No field type is reflected, so the closure's graph is unused.
                    quote! {
                        graph.define::<Self>(|_| {
                            #runtime::TypeDef::enumeration(
                                #name,
                                ::std::vec![#(#case_definitions),*],
                            ) #docs
                        })
                    }
                } else {
                    let case_definitions = cases.iter().map(|case| case.definition(runtime));
                    quote! {
                        graph.define::<Self>(|graph| {
                            #runtime::TypeDef::variant(
                                #name,
                                ::std::vec![#(#case_definitions),*],
                            ) #docs
                        })
                    }
                }
            }
        }
    }

    /// The `Parts` type and the body of `lower` for the value named `whole`.
    ///
    /// Functions of the whole value, for computed fields and `with` rules,
    /// run first because they borrow the value that is then taken apart.
    fn lowering(
        &self,
        container: &ContainerAttributes,
        runtime: &Path,
        whole: &TokenStream,
    ) -> Lowering {
        let lower = quote! { #runtime::lower };
        let computed = ComputedParts::plan(container, runtime, whole);

        match self {
            Self::Record(fields) => {
                let mut part_types = computed.types.clone();
                part_types.extend(exported(fields).map(|field| field.parts_type(runtime)));
                let mut part_values = computed.values.clone();
                part_values.extend(exported(fields).map(|field| field.lowered(runtime)));

                let computations = &computed.computations;
                let field_computations = fields.iter().filter_map(|field| field.computation(whole));
                let patterns = fields.iter().map(FieldPlan::named_pattern);
                let part_list_type = nested_list(&part_types);
                let part_list = nested_list(&part_values);
                Lowering {
                    parts: quote! { #lower::Fields<#part_list_type> },
                    body: quote! {
                        #(#computations)*
                        #(#field_computations)*
                        let Self { #(#patterns),* } = #whole;
                        #lower::Fields(#part_list)
                    },
                }
            }
            Self::Transparent(field) => {
                let pattern = if field.field.ident.is_some() {
                    let named_pattern = field.named_pattern();
                    quote! { Self { #named_pattern } }
                } else {
                    let binding = &field.binding;
                    quote! { Self(#binding) }
                };
                let value = field.lowered(runtime);
                Lowering {
                    parts: field.parts_type(runtime),
                    body: quote! {
                        let #pattern = #whole;
                        #value
                    },
                }
            }
            Self::Enum(cases) => {
                let case_parts: Vec<TokenStream> =
                    cases.iter().map(|case| case.parts_type(runtime)).collect();
                let choice_type = case_parts.iter().rev().fold(
                    quote! { #lower::Never },
                    |later_cases, case_part| quote! { #lower::Either<#case_part, #later_cases> },
                );
                let arms = cases
                    .iter()
                    .enumerate()
                    .map(|(position, case)| case.arm(position, runtime));
                let selected_case = quote! {
                    #lower::Cases(match #whole {
                        #(#arms,)*
                    })
                };
                if container.cases.is_none() {
                    return Lowering {
                        parts: quote! { #lower::Cases<#choice_type> },
                        body: selected_case,
                    };
                }

                // A record of the computed fields followed by the cases.
                let mut part_types = computed.types.clone();
                part_types.push(quote! { #lower::Cases<#choice_type> });
                let mut part_values = computed.values.clone();
                part_values.push(quote! { __wit_cases });
                let computations = &computed.computations;
                let part_list_type = nested_list(&part_types);
                let part_list = nested_list(&part_values);
                Lowering {
                    parts: quote! { #lower::Fields<#part_list_type> },
                    body: quote! {
                        #(#computations)*
                        let __wit_cases = #selected_case;
                        #lower::Fields(#part_list)
                    },
                }
            }
        }
    }
}

/// The parts of a container's computed fields, in declaration order.
struct ComputedParts {
    /// `let` statements evaluating each function on the whole value.
    computations: Vec<TokenStream>,
    types: Vec<TokenStream>,
    values: Vec<TokenStream>,
}

impl ComputedParts {
    fn plan(container: &ContainerAttributes, runtime: &Path, whole: &TokenStream) -> Self {
        let mut computations = Vec::new();
        let mut types = Vec::new();
        let mut values = Vec::new();
        for (position, computed_field) in container.computed_fields.iter().enumerate() {
            let ComputedField {
                wire_type,
                function,
                ..
            } = computed_field;
            let variable = format_ident!("__wit_computed_{position}");
            computations.push(quote! { let #variable: #wire_type = (#function)(&#whole); });
            types.push(quote! { <#wire_type as #runtime::lower::Lower>::Parts });
            values.push(quote! { #runtime::lower::Lower::lower(#variable) });
        }
        Self {
            computations,
            types,
            values,
        }
    }
}

fn exported<'plans, 'input>(
    fields: &'plans [FieldPlan<'input>],
) -> impl Iterator<Item = &'plans FieldPlan<'input>> {
    fields.iter().filter(|field| !field.is_skipped())
}

/// `(first, (second, ()))`: the same syntax for a list of types and of values.
fn nested_list(items: &[TokenStream]) -> TokenStream {
    items
        .iter()
        .rev()
        .fold(quote! { () }, |rest, item| quote! { (#item, #rest) })
}

/// Apply the item's `skip(…)` list: each field it names is not exported.
fn skip_listed_fields(fields: &mut [FieldPlan<'_>], listed: &[Ident], errors: &mut Errors) {
    for name in listed {
        let field = fields
            .iter_mut()
            .find(|field| field.field.ident.as_ref() == Some(name));
        match field {
            None => errors.report(name, "this struct has no field of that name"),
            Some(field) if field.rule.is_some() => errors.report(
                name,
                "this field already has a representation rule; a field takes one",
            ),
            Some(field) => field.rule = Some(FieldRule::Skip),
        }
    }
}

/// `skip(…)` names fields of a record, so it applies to nothing else.
fn reject_skip_list(container: &ContainerAttributes, errors: &mut Errors) {
    if let Some(first_field) = container.skipped_fields.first() {
        errors.report(
            first_field,
            "`skip(…)` on the item applies to a struct with named fields",
        );
    }
}

fn plan_named_fields<'input>(
    fields: &'input FieldsNamed,
    site: FieldSite,
    errors: &mut Errors,
) -> Vec<FieldPlan<'input>> {
    fields
        .named
        .iter()
        .enumerate()
        .map(|(position, field)| FieldPlan::plan(field, position, site, errors))
        .collect()
}

/// `Field::new(..)` expressions for the exported fields, in order.
fn record_field_definitions(fields: &[FieldPlan<'_>], runtime: &Path) -> Vec<TokenStream> {
    exported(fields)
        .map(|field| {
            let name = &field.name;
            let wire_type = field.reflection(runtime);
            let docs = with_docs(&field.field.attrs);
            quote! { #runtime::Field::new(#name, #wire_type) #docs }
        })
        .collect()
}

/// `Field::new(..)` expressions for the container's computed fields.
fn computed_field_definitions(container: &ContainerAttributes, runtime: &Path) -> Vec<TokenStream> {
    container
        .computed_fields
        .iter()
        .map(|computed_field| {
            let ComputedField {
                name,
                wire_type,
                function,
            } = computed_field;
            quote! {
                #runtime::Field::new(
                    #name,
                    #runtime::rule::computed::<Self, #wire_type>(#function, graph),
                )
            }
        })
        .collect()
}

impl<'input> FieldPlan<'input> {
    fn plan(field: &'input Field, position: usize, site: FieldSite, errors: &mut Errors) -> Self {
        let attributes = FieldAttributes::parse(&field.attrs, errors);

        let name = match (&field.ident, site) {
            (Some(ident), FieldSite::StructField | FieldSite::CaseField) => {
                Some(kebab_name(ident, attributes.name.as_ref()))
            }
            (_, _) => {
                if let Some(declared_name) = &attributes.name {
                    errors.report(declared_name, "this field has no WIT name of its own");
                }
                None
            }
        };

        if let Some(declared) = &attributes.rule {
            let takes_whole_value = matches!(
                declared.rule,
                FieldRule::Into {
                    function: Some(_),
                    ..
                }
            );
            if takes_whole_value && site != FieldSite::StructField {
                errors.report(
                    &declared.declared_by,
                    "`with` applies to struct fields; inside an enum case use `into` alone \
                     or `display`",
                );
            }
            if matches!(declared.rule, FieldRule::Skip) && site == FieldSite::Unnamed {
                errors.report(&declared.declared_by, "`skip` applies only to named fields");
            }
        }

        let binding = match &field.ident {
            Some(ident) => ident.clone(),
            None => format_ident!("field_{position}"),
        };
        Self {
            field,
            binding,
            name,
            is_renamed: attributes.name.is_some(),
            rule: attributes.rule.map(|declared| declared.rule),
        }
    }

    fn is_skipped(&self) -> bool {
        matches!(self.rule, Some(FieldRule::Skip))
    }

    /// Whether the binding could read this field from the owner type itself:
    /// public, under the name its WIT field has, and with no rule.
    fn is_stored_as_declared(&self) -> bool {
        let is_public = matches!(self.field.vis, syn::Visibility::Public(_));
        // A raw identifier is a keyword, which the binding spells differently.
        let is_keyword = self.binding.to_string().starts_with("r#");
        is_public && !is_keyword && !self.is_renamed && self.rule.is_none()
    }

    /// The expression reflecting the field's wire type.
    ///
    /// Each rule calls the runtime function whose bounds state the rule's
    /// requirements, so an unsatisfiable rule fails at the field's type.
    fn reflection(&self, runtime: &Path) -> TokenStream {
        let field_type = &self.field.ty;
        match &self.rule {
            None | Some(FieldRule::Skip) => {
                quote! { <#field_type as #runtime::WitType>::wit_type(graph) }
            }
            Some(FieldRule::Display) => quote! { #runtime::rule::display::<#field_type>(graph) },
            Some(FieldRule::Widen) => quote! { #runtime::rule::widened::<#field_type>(graph) },
            Some(FieldRule::Into {
                wire_type,
                function: None,
            }) => quote! { #runtime::rule::converted::<#field_type, #wire_type>(graph) },
            Some(FieldRule::Into {
                wire_type,
                function: Some(function),
            }) => quote! { #runtime::rule::computed::<Self, #wire_type>(#function, graph) },
        }
    }

    /// The type of the field's wire parts.
    fn parts_type(&self, runtime: &Path) -> TokenStream {
        let lower = quote! { #runtime::lower::Lower };
        let field_type = &self.field.ty;
        match &self.rule {
            None | Some(FieldRule::Skip) => quote! { <#field_type as #lower>::Parts },
            Some(FieldRule::Display) => quote! { ::std::string::String },
            Some(FieldRule::Widen) => quote! {
                <<#field_type as #runtime::rule::Widen>::Wire as #lower>::Parts
            },
            Some(FieldRule::Into { wire_type, .. }) => quote! { <#wire_type as #lower>::Parts },
        }
    }

    /// The variable holding a `with` rule's value, computed from the whole value.
    fn computed_variable(&self) -> Ident {
        format_ident!("__wit_with_{}", self.binding)
    }

    /// The statement evaluating a `with` rule before the value is taken apart.
    fn computation(&self, whole: &TokenStream) -> Option<TokenStream> {
        let Some(FieldRule::Into {
            wire_type,
            function: Some(function),
        }) = &self.rule
        else {
            return None;
        };
        let variable = self.computed_variable();
        Some(quote! { let #variable: #wire_type = (#function)(&#whole); })
    }

    /// Whether the stored field is read when lowering.
    fn is_read(&self) -> bool {
        !matches!(
            self.rule,
            Some(
                FieldRule::Skip
                    | FieldRule::Into {
                        function: Some(_),
                        ..
                    }
            )
        )
    }

    /// The field in a struct pattern: bound by name, or ignored when unused.
    fn named_pattern(&self) -> TokenStream {
        let binding = &self.binding;
        if self.is_read() {
            quote! { #binding }
        } else {
            quote! { #binding: _ }
        }
    }

    /// The field in a tuple pattern.
    fn unnamed_pattern(&self) -> TokenStream {
        let binding = &self.binding;
        if self.is_read() {
            quote! { #binding }
        } else {
            quote! { _ }
        }
    }

    /// The expression producing the field's wire parts from its binding.
    fn lowered(&self, runtime: &Path) -> TokenStream {
        let lower = quote! { #runtime::lower::Lower };
        let binding = &self.binding;
        let field_type = &self.field.ty;
        match &self.rule {
            None | Some(FieldRule::Skip) => quote! { #lower::lower(#binding) },
            Some(FieldRule::Display) => quote! { ::std::string::ToString::to_string(&#binding) },
            Some(FieldRule::Widen) => quote! {
                #lower::lower(<#field_type as #runtime::rule::Widen>::widen(#binding))
            },
            Some(FieldRule::Into {
                wire_type,
                function: None,
            }) => quote! {
                #lower::lower(<#wire_type as ::core::convert::From<#field_type>>::from(#binding))
            },
            Some(FieldRule::Into {
                function: Some(_), ..
            }) => {
                let variable = self.computed_variable();
                quote! { #lower::lower(#variable) }
            }
        }
    }
}

impl<'input> CasePlan<'input> {
    fn plan(variant: &'input Variant, errors: &mut Errors) -> Self {
        let attributes = VariantAttributes::parse(&variant.attrs, errors);
        let name = kebab_name(&variant.ident, attributes.name.as_ref());

        let payload = match &variant.fields {
            Fields::Named(fields) => {
                let field_plans = plan_named_fields(fields, FieldSite::CaseField, errors);
                let has_exported_field = field_plans.iter().any(|field| !field.is_skipped());
                if !has_exported_field {
                    errors.report(fields, "a WIT record needs at least one field");
                }
                CasePayload::Record {
                    name: attributes.payload,
                    fields: field_plans,
                }
            }
            Fields::Unnamed(fields) => {
                reject_payload_name(&attributes, errors);
                if fields.unnamed.is_empty() {
                    errors.report(
                        variant,
                        "an empty tuple variant carries nothing; write a unit variant",
                    );
                }
                let field_plans = fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(position, field)| {
                        FieldPlan::plan(field, position, FieldSite::Unnamed, errors)
                    })
                    .collect();
                CasePayload::Tuple(field_plans)
            }
            Fields::Unit => {
                reject_payload_name(&attributes, errors);
                CasePayload::Unit
            }
        };
        Self {
            variant,
            name,
            is_renamed: attributes.name.is_some(),
            payload,
        }
    }

    /// Whether the binding could use this case of the owner enum itself.
    ///
    /// The binding lowers an enum by its discriminant, so an explicit
    /// discriminant would select another case without any compile error.
    fn is_plain_unit(&self) -> bool {
        matches!(self.payload, CasePayload::Unit)
            && self.variant.discriminant.is_none()
            && !self.is_renamed
    }

    /// The `Case::new(..)` expression.
    fn definition(&self, runtime: &Path) -> TokenStream {
        let name = &self.name;
        let docs = with_docs(&self.variant.attrs);
        let payload = match &self.payload {
            CasePayload::Unit => quote! { #runtime::Payload::None },
            CasePayload::Tuple(fields) => {
                let element_types: Vec<TokenStream> = fields
                    .iter()
                    .map(|field| field.reflection(runtime))
                    .collect();
                match element_types.as_slice() {
                    [only_type] => quote! { #runtime::Payload::Type(#only_type) },
                    _ => quote! {
                        #runtime::Payload::Type(#runtime::Type::Tuple(
                            ::std::vec![#(#element_types),*]
                        ))
                    },
                }
            }
            CasePayload::Record {
                name: record_name,
                fields,
            } => {
                let record_name = match record_name {
                    Some(record_name) => quote! { ::core::option::Option::Some(#record_name) },
                    None => quote! { ::core::option::Option::None },
                };
                let field_definitions = record_field_definitions(fields, runtime);
                quote! {
                    #runtime::Payload::Record {
                        name: #record_name,
                        fields: ::std::vec![#(#field_definitions),*],
                    }
                }
            }
        };
        quote! { #runtime::Case::new(#name, #payload) #docs }
    }

    /// The type of the case's payload parts. A unit case has the part `()`.
    fn parts_type(&self, runtime: &Path) -> TokenStream {
        match &self.payload {
            CasePayload::Unit => quote! { () },
            CasePayload::Tuple(fields) => {
                let element_parts: Vec<TokenStream> = fields
                    .iter()
                    .map(|field| field.parts_type(runtime))
                    .collect();
                match element_parts.as_slice() {
                    [only_part] => only_part.clone(),
                    _ => quote! { (#(#element_parts,)*) },
                }
            }
            CasePayload::Record { fields, .. } => {
                let field_parts: Vec<TokenStream> = exported(fields)
                    .map(|field| field.parts_type(runtime))
                    .collect();
                let part_list_type = nested_list(&field_parts);
                quote! { #runtime::lower::Fields<#part_list_type> }
            }
        }
    }

    /// The `match` arm lowering this case, the `position`-th of its enum.
    fn arm(&self, position: usize, runtime: &Path) -> TokenStream {
        let lower = quote! { #runtime::lower };
        let case = &self.variant.ident;
        let (pattern, payload_parts) = match &self.payload {
            CasePayload::Unit => (quote! { Self::#case }, quote! { () }),
            CasePayload::Tuple(fields) => {
                let patterns = fields.iter().map(FieldPlan::unnamed_pattern);
                let element_values: Vec<TokenStream> =
                    fields.iter().map(|field| field.lowered(runtime)).collect();
                let payload_parts = match element_values.as_slice() {
                    [only_value] => only_value.clone(),
                    _ => quote! { (#(#element_values,)*) },
                };
                (quote! { Self::#case(#(#patterns),*) }, payload_parts)
            }
            CasePayload::Record { fields, .. } => {
                let patterns = fields.iter().map(FieldPlan::named_pattern);
                let field_values: Vec<TokenStream> = exported(fields)
                    .map(|field| field.lowered(runtime))
                    .collect();
                let part_list = nested_list(&field_values);
                (
                    quote! { Self::#case { #(#patterns),* } },
                    quote! { #lower::Fields(#part_list) },
                )
            }
        };

        // `Next(Next(This(parts)))` selects the case at `position`.
        let mut choice = quote! { #lower::Either::This(#payload_parts) };
        for _ in 0..position {
            choice = quote! { #lower::Either::Next(#choice) };
        }
        quote! { #pattern => #choice }
    }
}

fn reject_payload_name(variant_attributes: &VariantAttributes, errors: &mut Errors) {
    if let Some(payload) = &variant_attributes.payload {
        errors.report(payload, "`payload` applies only to struct-like variants");
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
