//! Macros for `wit_reflect`.
//!
//! Use them through the `wit-reflect` crate, which re-exports and documents
//! them. This crate only translates a Rust item's declared shape into calls to
//! the runtime crate. Naming rules beyond the kebab-case default, placement
//! and validation all live there, so a manual implementation and a generated
//! one are checked by the same code.

mod attributes;
mod derive;
mod export;
mod exports;
mod from_parts;
mod named;
mod names;
mod operation;
mod representations;
mod resource;
mod setting;

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

/// Derives `wit_reflect::WitType` and `wit_reflect::Resource` for a type
/// exported as a WIT `resource`. See the re-export in `wit-reflect`.
#[proc_macro_derive(WitResource, attributes(wit))]
pub fn derive_wit_resource(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let expansion = resource::expand(&input);
    match expansion {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Derives `wit_reflect::lower::FromParts` for a generated binding type. See
/// the re-export in `wit-reflect`.
#[proc_macro_derive(FromParts)]
pub fn derive_from_parts(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    from_parts::expand(&input).into()
}

/// Makes a function's signature available to a schema. See the re-export in
/// `wit-reflect`.
#[proc_macro_attribute]
pub fn export(arguments: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let expansion = export::expand(arguments.into(), &item);
    match expansion {
        Ok(tokens) => tokens.into(),
        // Keep the function itself, so the only errors are the reported ones.
        Err(error) => {
            let compile_error = error.into_compile_error();
            quote::quote! { #item #compile_error }.into()
        }
    }
}

/// Registers the exported functions of an `impl` block or of a trait. See the
/// re-export in `wit-reflect`.
#[proc_macro_attribute]
pub fn exports(arguments: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let expansion = exports::expand(arguments.into(), &item);
    match expansion {
        Ok(tokens) => tokens.into(),
        // Keep the block itself, so the only errors are the reported ones.
        Err(error) => {
            let compile_error = error.into_compile_error();
            quote::quote! { #item #compile_error }.into()
        }
    }
}

/// Registers the name of the type a type alias stands for. See the re-export
/// in `wit-reflect`.
#[proc_macro_attribute]
pub fn named(arguments: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let expansion = named::expand(arguments.into(), &item);
    match expansion {
        Ok(tokens) => tokens.into(),
        // Keep the alias itself, so the only errors are the reported ones.
        Err(error) => {
            let compile_error = error.into_compile_error();
            quote::quote! { #item #compile_error }.into()
        }
    }
}

/// Declares a crate's map of foreign representations. See the re-export in
/// `wit-reflect`.
#[proc_macro]
pub fn representations(input: TokenStream) -> TokenStream {
    let expansion = representations::expand(input.into());
    match expansion {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Refers to the reflected signature of a function marked with `#[export]`.
/// See the re-export in `wit-reflect`.
#[proc_macro]
pub fn operation(input: TokenStream) -> TokenStream {
    let expansion = operation::expand(input.into());
    match expansion {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}
