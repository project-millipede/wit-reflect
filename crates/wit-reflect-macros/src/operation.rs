//! Expansion of `operation!(path)`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::ExprPath;

use crate::names::companion_ident;

/// Rewrite the path of an exported function to the path of its companion.
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let mut function_path: ExprPath = syn::parse2(input)?;
    let Some(function_segment) = function_path.path.segments.last_mut() else {
        return Err(syn::Error::new_spanned(
            &function_path,
            "expected the path of a function marked with `#[export]`",
        ));
    };
    function_segment.ident = companion_ident(&function_segment.ident);
    Ok(quote! { #function_path })
}
