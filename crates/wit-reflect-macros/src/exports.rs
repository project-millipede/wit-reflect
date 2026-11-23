//! Expansion of `#[exports]` on an `impl` block.
//!
//! `#[export]` on a method cannot register the method. A registration is an
//! item of its own, and an attribute on a method can only add associated
//! items to its block. The attribute on the block sees the type and every
//! function, so it emits one registration for all exported ones.
//!
//! A function is exported when the attribute lists its name or the function
//! carries `#[export]` itself. A listed function is given that mark here, so
//! both forms are reflected and checked by the same code.

use proc_macro2::{TokenStream, TokenTree};
use quote::{ToTokens, quote};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Attribute, Ident, ImplItem, ImplItemFn, ItemImpl, Meta, Path, Token, Type, parse_quote};

use crate::attributes::Errors;
use crate::names::{companion_ident, public_path, registration, runtime_path};

/// `#[exports(...)]` arguments.
#[derive(Default)]
struct ExportsOptions {
    /// The functions named in the attribute, in the order written.
    listed: Vec<Ident>,
    /// `path = …`: where the component crate can name the type.
    public_path: Option<Path>,
    crate_path: Option<Path>,
}

impl ExportsOptions {
    fn parse(arguments: TokenStream) -> syn::Result<Self> {
        let mut options = Self::default();
        let parser = syn::meta::parser(|meta| {
            // An option has a value. A bare name is a function of the block,
            // also when the function is called `path`.
            let is_option = meta.input.peek(Token![=]);
            if !is_option {
                let function = meta.path.require_ident()?.clone();
                options.listed.push(function);
                Ok(())
            } else if meta.path.is_ident("path") {
                let public_path = meta.value()?.parse()?;
                options.public_path = Some(public_path);
                Ok(())
            } else if meta.path.is_ident("crate") {
                let crate_path = meta.value()?.parse()?;
                options.crate_path = Some(crate_path);
                Ok(())
            } else {
                Err(meta.error("expected `path = …` or `crate = …`"))
            }
        });
        parser.parse2(arguments)?;
        Ok(options)
    }
}

/// A function of the block that is exported.
struct MarkedFunction<'block> {
    ident: &'block Ident,
    /// The `cfg` predicates under which the function is compiled and marked.
    conditions: Vec<TokenStream>,
}

pub(crate) fn expand(arguments: TokenStream, item: &TokenStream) -> syn::Result<TokenStream> {
    let options = ExportsOptions::parse(arguments)?;
    let mut block: ItemImpl = syn::parse2(item.clone())?;
    let runtime = runtime_path(options.crate_path.as_ref());

    let mut errors = Errors::default();
    mark_listed_functions(&mut block, &options, &runtime, &mut errors);
    if let Some((trait_path, _)) = &block.trait_ {
        errors.report(
            trait_path,
            "`exports` applies to an inherent `impl` block; \
             a trait's functions are not exported",
        );
    }
    if !block.generics.params.is_empty() {
        errors.report(
            &block.generics,
            "a generic `impl` block has no single resource to export from",
        );
    }
    let resource = resource_ident(&block.self_ty, &mut errors);
    let marked_functions = marked_functions(&block, &mut errors);
    if marked_functions.is_empty() {
        errors.report(
            &block.self_ty,
            "this block exports nothing; list its exported functions in `exports(…)` \
             or mark them with `#[export]`",
        );
    }
    errors.finish()?;
    let Some(resource) = resource else {
        // `resource_ident` reports an error whenever it yields no name.
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "this type cannot export functions",
        ));
    };

    let resource_type = &block.self_ty;
    let declarations = marked_functions.iter().map(|function| {
        let companion = companion_ident(function.ident);
        let rust_path = public_path(options.public_path.as_ref(), resource, Some(function.ident));
        let conditions = &function.conditions;
        let condition = if conditions.is_empty() {
            TokenStream::new()
        } else {
            quote! { #[cfg(all(#(#conditions),*))] }
        };
        quote! {
            #condition
            schema.resource_method::<#resource_type>(
                #runtime::Operation::new(<#resource_type>::#companion, #rust_path),
            );
        }
    });
    let registration = registration(&runtime, &quote! { #(#declarations)* });
    Ok(quote! {
        #block

        #registration
    })
}

/// Give every function the attribute lists the `#[export]` mark.
///
/// From here on a listed function is handled like one marked by hand: the
/// mark reflects its signature and reports a signature that cannot be
/// exported at the function itself.
fn mark_listed_functions(
    block: &mut ItemImpl,
    options: &ExportsOptions,
    runtime: &Path,
    errors: &mut Errors,
) {
    let export_mark: Attribute = match &options.crate_path {
        Some(crate_path) => parse_quote!(#[#runtime::export(crate = #crate_path)]),
        None => parse_quote!(#[#runtime::export]),
    };
    for listed in &options.listed {
        let function = block.items.iter_mut().find_map(|item| match item {
            ImplItem::Fn(function) if function.sig.ident == *listed => Some(function),
            _ => None,
        });
        let Some(function) = function else {
            errors.report(listed, "this block has no function of that name");
            continue;
        };
        let is_marked = function.attrs.iter().any(|attribute| {
            names_export(attribute.path()) || export_in_cfg_attr(attribute).is_some()
        });
        if is_marked {
            errors.report(
                listed,
                "this function already carries a mark of its own; \
                 list it or mark it, not both",
            );
            continue;
        }
        function.attrs.insert(0, export_mark.clone());
    }
}

/// The name of the type the block implements, when it is a plain path.
fn resource_ident<'block>(self_type: &'block Type, errors: &mut Errors) -> Option<&'block Ident> {
    let plain_segment = match self_type {
        Type::Path(type_path) if type_path.qself.is_none() => type_path
            .path
            .segments
            .last()
            .filter(|segment| segment.arguments.is_none()),
        _ => None,
    };
    match plain_segment {
        Some(segment) => Some(&segment.ident),
        None => {
            errors.report(
                self_type,
                "`exports` needs the resource's own name here, without type arguments",
            );
            None
        }
    }
}

fn marked_functions<'block>(
    block: &'block ItemImpl,
    errors: &mut Errors,
) -> Vec<MarkedFunction<'block>> {
    let mut marked_functions = Vec::new();
    for item in &block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let Some(conditions) = export_conditions(function, errors) else {
            continue;
        };
        marked_functions.push(MarkedFunction {
            ident: &function.sig.ident,
            conditions,
        });
    }
    marked_functions
}

/// The `cfg` predicates of a marked function, or `None` if it is not marked.
///
/// An attribute on the block receives the functions' attributes as written,
/// so `#[cfg_attr(predicate, export)]` is recognized here together with its
/// predicate.
fn export_conditions(function: &ImplItemFn, errors: &mut Errors) -> Option<Vec<TokenStream>> {
    let mut conditions = Vec::new();
    let mut is_marked = false;
    for attribute in &function.attrs {
        if attribute.path().is_ident("cfg") {
            if let Meta::List(condition) = &attribute.meta {
                conditions.push(condition.tokens.clone());
            }
        } else if names_export(attribute.path()) {
            is_marked = true;
            require_resource_function(function, &attribute.meta, errors);
        } else if let Some((condition, export)) = export_in_cfg_attr(attribute) {
            conditions.push(condition);
            is_marked = true;
            require_resource_function(function, &export, errors);
        }
    }
    is_marked.then_some(conditions)
}

fn names_export(path: &Path) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == "export")
}

/// The predicate and the mark of `#[cfg_attr(predicate, …, export, …)]`.
fn export_in_cfg_attr(attribute: &Attribute) -> Option<(TokenStream, Meta)> {
    if !attribute.path().is_ident("cfg_attr") {
        return None;
    }
    let entries = attribute
        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        .ok()?;
    let mut entries = entries.into_iter();
    let predicate = entries.next()?;
    let export = entries.find(|entry| names_export(entry.path()))?;
    Some((predicate.to_token_stream(), export))
}

/// A function without `self` belongs to the resource only as a `static`
/// function or a `constructor`. Unmarked, `#[export]` reflects it as a free
/// function, which no resource can export.
fn require_resource_function(function: &ImplItemFn, export: &Meta, errors: &mut Errors) {
    if function.sig.receiver().is_some() {
        return;
    }
    let states_association = match export {
        Meta::List(arguments) => arguments.tokens.clone().into_iter().any(|token| {
            matches!(&token, TokenTree::Ident(word) if word == "static" || word == "constructor")
        }),
        Meta::Path(_) | Meta::NameValue(_) => false,
    };
    if !states_association {
        errors.report(
            &function.sig.ident,
            "a function without `self` is exported from a resource as \
             `#[export(static)]` or `#[export(constructor)]`",
        );
    }
}
