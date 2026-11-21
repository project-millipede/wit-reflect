//! Expansion of `#[named]` on a type alias.
//!
//! The compiler resolves an alias away before any trait sees the type, so the
//! alias's name is available only here, on the item.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::{ItemType, LitStr, Path};

use crate::names::{kebab_name, registration, runtime_path};

/// `#[named(...)]` arguments.
#[derive(Default)]
struct NamedOptions {
    name: Option<LitStr>,
    crate_path: Option<Path>,
}

impl NamedOptions {
    fn parse(arguments: TokenStream) -> syn::Result<Self> {
        let mut options = Self::default();
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("name") {
                if options.name.is_some() {
                    return Err(meta.error("`name` is given more than once"));
                }
                let name = meta.value()?.parse()?;
                options.name = Some(name);
                Ok(())
            } else if meta.path.is_ident("crate") {
                let crate_path = meta.value()?.parse()?;
                options.crate_path = Some(crate_path);
                Ok(())
            } else {
                Err(meta.error("expected `name` or `crate`"))
            }
        });
        parser.parse2(arguments)?;
        Ok(options)
    }
}

pub(crate) fn expand(arguments: TokenStream, item: &TokenStream) -> syn::Result<TokenStream> {
    let options = NamedOptions::parse(arguments)?;
    let alias: ItemType = syn::parse2(item.clone())?;
    if !alias.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &alias.generics,
            "an alias with parameters names no single type; \
             mark an alias of one instantiation",
        ));
    }

    let runtime = runtime_path(options.crate_path.as_ref());
    let ident = &alias.ident;
    let name = kebab_name(ident, options.name.as_ref());
    let registration = registration(
        &runtime,
        &quote! { schema.name_instantiation::<#ident>(#name); },
    );
    Ok(quote! {
        #item

        #registration
    })
}
