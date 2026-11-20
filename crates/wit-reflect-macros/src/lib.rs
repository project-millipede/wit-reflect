//! Derive macro for `wit_reflect::WitType`.
//!
//! Use it through the `wit-reflect` crate, which re-exports and documents it.
//! This crate only translates a Rust item's declared shape into calls to the
//! runtime crate. Naming rules beyond the kebab-case default, placement and
//! validation all live there, so a manual implementation and a derived one are
//! checked by the same code.

mod attributes;
mod derive;

use proc_macro::TokenStream;

/// Derives `wit_reflect::WitType`. See the re-export in `wit-reflect` for the
/// mapping and the supported `#[wit(...)]` attributes.
#[proc_macro_derive(WitType, attributes(wit))]
pub fn derive_wit_type(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let expansion = derive::expand(&input);
    match expansion {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}
