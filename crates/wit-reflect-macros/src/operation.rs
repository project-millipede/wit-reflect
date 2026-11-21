//! Expansion of `operation!(path)`.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::ExprPath;

use crate::names::companion_ident;

/// Pair the companion of an exported function with the path as written.
///
/// The path becomes part of generated forwarding code, which calls the
/// function by it.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let function_path: ExprPath = syn::parse2(input)?;
    // Token text separates tokens with spaces; a path has none.
    let written_path: String = function_path
        .to_token_stream()
        .to_string()
        .split_whitespace()
        .collect();

    let mut companion_path = function_path;
    let Some(function_segment) = companion_path.path.segments.last_mut() else {
        return Err(syn::Error::new_spanned(
            &companion_path,
            "expected the path of a function marked with `#[export]`",
        ));
    };
    function_segment.ident = companion_ident(&function_segment.ident);

    Ok(quote! {
        ::wit_reflect::Operation::new(#companion_path, #written_path)
    })
}
