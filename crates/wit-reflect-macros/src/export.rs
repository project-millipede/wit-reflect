//! Expansion of `#[export]` on a function or method.
//!
//! The attribute leaves the function untouched and adds a hidden companion
//! that reflects its signature. Signatures that cannot be exported faithfully
//! are rejected here, at the declaration.

use proc_macro2::TokenStream;
use quote::quote;
use syn::meta::ParseNestedMeta;
use syn::parse::Parser;
use syn::{
    Attribute, FnArg, GenericParam, ImplItemFn, LitStr, Pat, PatType, Path, ReceiverKind,
    ReturnType, Safety, Signature, Type,
};

use crate::attributes::Errors;
use crate::names::{
    companion_ident, kebab_name, public_path, registration, runtime_path, with_docs,
};

/// `#[export(...)]` arguments.
#[derive(Default)]
struct ExportOptions {
    name: Option<LitStr>,
    /// `interface = "…"`: the interface that exports a free function.
    interface: Option<LitStr>,
    /// `path = …`: where the component crate can name a free function.
    public_path: Option<Path>,
    /// `static` or `constructor`, with the tokens that requested it.
    association: Option<(Association, Path)>,
    crate_path: Option<Path>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Association {
    Static,
    Constructor,
}

impl ExportOptions {
    fn parse(arguments: TokenStream) -> syn::Result<Self> {
        let mut options = Self::default();
        let parser = syn::meta::parser(|meta| options.parse_entry(&meta));
        parser.parse2(arguments)?;
        Ok(options)
    }

    fn parse_entry(&mut self, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
        if meta.path.is_ident("name") {
            if self.name.is_some() {
                return Err(meta.error("`name` is given more than once"));
            }
            let name = meta.value()?.parse()?;
            self.name = Some(name);
            Ok(())
        } else if meta.path.is_ident("interface") {
            if self.interface.is_some() {
                return Err(meta.error("`interface` is given more than once"));
            }
            let interface = meta.value()?.parse()?;
            self.interface = Some(interface);
            Ok(())
        } else if meta.path.is_ident("path") {
            if self.public_path.is_some() {
                return Err(meta.error("`path` is given more than once"));
            }
            let public_path = meta.value()?.parse()?;
            self.public_path = Some(public_path);
            Ok(())
        } else if meta.path.is_ident("static") {
            self.associate(Association::Static, meta)
        } else if meta.path.is_ident("constructor") {
            self.associate(Association::Constructor, meta)
        } else if meta.path.is_ident("crate") {
            let crate_path = meta.value()?.parse()?;
            self.crate_path = Some(crate_path);
            Ok(())
        } else {
            Err(meta
                .error("expected `name`, `interface`, `path`, `static`, `constructor` or `crate`"))
        }
    }

    fn associate(
        &mut self,
        association: Association,
        meta: &ParseNestedMeta<'_>,
    ) -> syn::Result<()> {
        if self.association.is_some() {
            return Err(meta.error("give only one of `static` and `constructor`"));
        }
        self.association = Some((association, meta.path.clone()));
        Ok(())
    }
}

/// How the function is exported, decided from its receiver and the options.
enum Export {
    /// A function of an interface.
    Free,
    /// Called on a borrowed resource.
    Method {
        /// Whether the receiver is `&mut self`.
        exclusive: bool,
    },
    Static,
    Constructor,
}

pub(crate) fn expand(arguments: TokenStream, item: &TokenStream) -> syn::Result<TokenStream> {
    let options = ExportOptions::parse(arguments)?;
    let function: ImplItemFn = syn::parse2(item.clone())?;
    let signature = &function.sig;
    let runtime = runtime_path(options.crate_path.as_ref());

    let mut errors = Errors::default();
    reject_unsupported_qualifiers(signature, &mut errors);
    let export = classify(signature, &options, &mut errors);
    if let (Some(interface), false) = (&options.interface, matches!(export, Export::Free)) {
        errors.report(
            interface,
            "`interface` applies to a free function; a resource's functions are \
             exported from the resource's interface",
        );
    }
    let params = reflect_params(signature, &runtime, &mut errors);
    let result = reflect_result(signature, &runtime, &mut errors);
    errors.finish()?;

    let name = kebab_name(&signature.ident, options.name.as_ref());
    let docs = with_docs(&function.attrs);
    let reflected_function = quote! {
        #runtime::Function::new(#name, ::std::vec![#(#params),*], #result) #docs
    };
    let (companion_type, companion_value) = match export {
        Export::Free => (quote! { #runtime::Function }, reflected_function),
        Export::Method { exclusive: false } => (
            quote! { #runtime::Method<Self> },
            quote! { #runtime::Method::borrowing(#reflected_function) },
        ),
        Export::Method { exclusive: true } => (
            quote! { #runtime::Method<Self> },
            quote! { #runtime::Method::borrowing_mut(#reflected_function) },
        ),
        Export::Static => (
            quote! { #runtime::Method<Self> },
            quote! { #runtime::Method::associated(#reflected_function) },
        ),
        Export::Constructor => (
            quote! { #runtime::Method<Self> },
            quote! { #runtime::Method::constructor(#reflected_function) },
        ),
    };

    let visibility = &function.vis;
    let companion = companion_ident(&signature.ident);
    let registration = match &options.interface {
        Some(interface) => {
            let rust_path = public_path(options.public_path.as_ref(), &signature.ident, None);
            registration(
                &runtime,
                &quote! {
                    schema
                        .interface(#interface)
                        .function(#runtime::Operation::new(#companion, #rust_path));
                },
            )
        }
        None => TokenStream::new(),
    };
    // The companion is referenced only by a schema, which the owning crate
    // usually does not contain.
    Ok(quote! {
        #item

        #[doc(hidden)]
        #[allow(dead_code)]
        #[must_use]
        #visibility fn #companion(graph: &mut #runtime::TypeGraph) -> #companion_type {
            #companion_value
        }

        #registration
    })
}

fn reject_unsupported_qualifiers(signature: &Signature, errors: &mut Errors) {
    if let Some(asyncness) = &signature.asyncness {
        errors.report(asyncness, "exporting an `async fn` is not supported yet");
    }
    if let Safety::Unsafe(unsafety) = &signature.safety {
        errors.report(unsafety, "an `unsafe fn` cannot be exported");
    }
    if let Some(variadic) = &signature.variadic {
        errors.report(variadic, "a variadic function cannot be exported");
    }
    for parameter in &signature.generics.params {
        match parameter {
            // A lifetime does not change the exported signature.
            GenericParam::Lifetime(_) => {}
            GenericParam::Type(_) | GenericParam::Const(_) => errors.report(
                parameter,
                "a generic function has no single WIT signature; export a concrete function",
            ),
        }
    }
}

fn classify(signature: &Signature, options: &ExportOptions, errors: &mut Errors) -> Export {
    match (signature.receiver(), &options.association) {
        (Some(receiver), association) => {
            if let Some((_, requested_by)) = association {
                errors.report(
                    requested_by,
                    "`static` and `constructor` apply only to functions without `self`",
                );
            }
            let exclusive = match &receiver.kind {
                ReceiverKind::Reference(_, _, mutability) => mutability.is_some(),
                _ => {
                    errors.report(
                        receiver,
                        "an exported method must take `&self` or `&mut self`; \
                         a WIT method borrows its resource",
                    );
                    false
                }
            };
            Export::Method { exclusive }
        }
        (None, Some((Association::Static, _))) => Export::Static,
        (None, Some((Association::Constructor, _))) => Export::Constructor,
        (None, None) => Export::Free,
    }
}

/// `Param::new(..)` expressions for the parameters after the receiver.
fn reflect_params(signature: &Signature, runtime: &Path, errors: &mut Errors) -> Vec<TokenStream> {
    let mut params = Vec::new();
    for argument in &signature.inputs {
        let FnArg::Typed(parameter) = argument else {
            // The receiver is implicit in a WIT method.
            continue;
        };
        let Some(name) = parameter_name(parameter, errors) else {
            continue;
        };
        if let Some(platform_sized) = platform_sized_integer(&parameter.ty) {
            errors.report(
                platform_sized,
                "a `usize` or `isize` parameter cannot be exported: the contract carries \
                 64 bits, which may not fit; take a fixed-width integer",
            );
            continue;
        }
        let param = match &*parameter.ty {
            Type::Reference(reference) => {
                if let Some(mutability) = &reference.mutability {
                    errors.report(
                        mutability,
                        "a `&mut` parameter cannot be exported: changes to it would be lost",
                    );
                }
                // A reference passes data by value and a resource as a borrow.
                let referent = &reference.elem;
                quote! {
                    #runtime::Param::by_reference(
                        #name,
                        #runtime::Type::borrowed(
                            <#referent as #runtime::WitType>::wit_type(graph),
                        ),
                    )
                }
            }
            Type::ImplTrait(impl_trait) => {
                errors.report(impl_trait, "an `impl Trait` parameter has no WIT type");
                continue;
            }
            owned_type => quote! {
                #runtime::Param::new(#name, <#owned_type as #runtime::WitType>::wit_type(graph))
            },
        };
        params.push(param);
    }
    params
}

/// `usize` or `isize` as written, by value or behind a reference.
///
/// A result of that type is widened without loss. An argument would have to be
/// narrowed, which can fail, so it is rejected where it is declared. An alias
/// is not visible here and fails later, when the component crate compiles.
fn platform_sized_integer(parameter_type: &Type) -> Option<&Type> {
    let written = match parameter_type {
        Type::Reference(reference) => &*reference.elem,
        owned_type => owned_type,
    };
    let Type::Path(type_path) = written else {
        return None;
    };
    let is_platform_sized = type_path.path.is_ident("usize") || type_path.path.is_ident("isize");
    is_platform_sized.then_some(written)
}

/// The WIT name of a parameter bound to a plain identifier.
fn parameter_name(parameter: &PatType, errors: &mut Errors) -> Option<LitStr> {
    match &*parameter.pat {
        Pat::Ident(binding) if binding.subpat.is_none() => {
            let name = kebab_name(&binding.ident, None);
            Some(name)
        }
        pattern => {
            errors.report(
                pattern,
                "an exported parameter needs a name; bind it to an identifier",
            );
            None
        }
    }
}

/// The expression reflecting the return type. No return type reflects `()`.
fn reflect_result(signature: &Signature, runtime: &Path, errors: &mut Errors) -> TokenStream {
    let ReturnType::Type(_, result_type) = &signature.output else {
        return quote! { <() as #runtime::WitType>::wit_type(graph) };
    };
    match &**result_type {
        Type::Reference(reference) => {
            errors.report(
                reference,
                "an exported function must return an owned value, not a reference",
            );
            TokenStream::new()
        }
        Type::ImplTrait(impl_trait) => {
            errors.report(impl_trait, "an `impl Trait` result has no WIT type");
            TokenStream::new()
        }
        owned_type => quote! { <#owned_type as #runtime::WitType>::wit_type(graph) },
    }
}

/// The expression that reflects a function without a receiver: its WIT name,
/// parameters, result and documentation.
///
/// A signature that cannot be exported is reported at the function.
pub(crate) fn reflected_function(
    signature: &Signature,
    attributes: &[Attribute],
    runtime: &Path,
    errors: &mut Errors,
) -> TokenStream {
    reject_unsupported_qualifiers(signature, errors);
    let params = reflect_params(signature, runtime, errors);
    let result = reflect_result(signature, runtime, errors);
    let name = kebab_name(&signature.ident, None);
    let docs = with_docs(attributes);
    quote! {
        #runtime::Function::new(#name, ::std::vec![#(#params),*], #result) #docs
    }
}
