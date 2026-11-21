//! Naming shared by the derives and the `export` attribute.

use heck::ToKebabCase;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::ext::IdentExt;
use syn::{Attribute, Ident, LitStr, Path, parse_quote};

use crate::attributes;

/// The declared WIT name, or the Rust identifier in kebab case.
///
/// A raw identifier such as `r#type` names `type`; rendering escapes keywords.
pub(crate) fn kebab_name(ident: &Ident, declared_name: Option<&LitStr>) -> LitStr {
    match declared_name {
        Some(declared_name) => declared_name.clone(),
        None => {
            let rust_name = ident.unraw().to_string();
            let kebab_name = rust_name.to_kebab_case();
            LitStr::new(&kebab_name, ident.span())
        }
    }
}

/// The path of the runtime crate: the `crate = path` attribute or the default.
pub(crate) fn runtime_path(declared_path: Option<&Path>) -> Path {
    match declared_path {
        Some(declared_path) => declared_path.clone(),
        None => parse_quote!(::wit_reflect),
    }
}

/// The hidden function that reflects the signature of the function `ident`.
///
/// `#[export]` defines it and `operation!` refers to it; both use this rule.
pub(crate) fn companion_ident(ident: &Ident) -> Ident {
    format_ident!("__wit_{}", ident.unraw(), span = ident.span())
}

/// The path by which a component crate names an item, as a string expression.
///
/// `declared_path` is used as written. The default names the item at the root
/// of the crate that declares it. `member` is a function of the item.
pub(crate) fn public_path(
    declared_path: Option<&Path>,
    item: &Ident,
    member: Option<&Ident>,
) -> TokenStream {
    let member_suffix = match member {
        Some(member) => format!("::{member}"),
        None => String::new(),
    };
    match declared_path {
        Some(declared_path) => {
            // Token text separates tokens with spaces; a path has none.
            let written_path = declared_path.to_token_stream().to_string().replace(' ', "");
            let public_path = format!("{written_path}{member_suffix}");
            quote! { #public_path }
        }
        None => {
            let path_in_crate = format!("::{item}{member_suffix}");
            quote! { ::core::concat!(::core::env!("CARGO_CRATE_NAME"), #path_in_crate) }
        }
    }
}

/// An item that registers `declaration` for schemas built from the registry.
///
/// `declaration` is statements that add to a `&mut Schema` named `schema`.
/// The position is the place of the mark, so registrations are applied in
/// source order. Nothing is registered on Wasm targets.
pub(crate) fn registration(runtime: &Path, declaration: &TokenStream) -> TokenStream {
    quote! {
        #[cfg(not(target_family = "wasm"))]
        #runtime::registry::inventory::submit! {
            #runtime::registry::Registration::new(
                #runtime::registry::Position::new(
                    ::core::env!("CARGO_PKG_NAME"),
                    ::core::file!(),
                    ::core::line!(),
                    ::core::column!(),
                ),
                |schema: &mut #runtime::Schema| {
                    #declaration
                },
            )
        }
    }
}

/// `.with_docs("…")` when the item is documented, otherwise nothing.
pub(crate) fn with_docs(item_attributes: &[Attribute]) -> TokenStream {
    match attributes::docs(item_attributes) {
        Some(documentation) => quote! { .with_docs(#documentation) },
        None => TokenStream::new(),
    }
}
