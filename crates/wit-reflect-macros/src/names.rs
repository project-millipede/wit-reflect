//! Naming shared by the derives and the `export` attribute.

use heck::ToKebabCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
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

/// `.with_docs("…")` when the item is documented, otherwise nothing.
pub(crate) fn with_docs(item_attributes: &[Attribute]) -> TokenStream {
    match attributes::docs(item_attributes) {
        Some(documentation) => quote! { .with_docs(#documentation) },
        None => TokenStream::new(),
    }
}
