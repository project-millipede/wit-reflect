//! Expansion of `#[derive(WitResource)]`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

use crate::attributes::{ContainerAttributes, Errors};
use crate::derive::{reject_lifetimes, with_bounds};
use crate::names::{kebab_name, runtime_path, with_docs};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let mut errors = Errors::default();

    let container = ContainerAttributes::parse(&input.attrs, &mut errors);
    if let Some(transparent) = &container.transparent {
        errors.report(transparent, "`transparent` does not apply to a resource");
    }
    let runtime = runtime_path(container.crate_path.as_ref());
    // No field is reflected, but the type itself must be `'static`.
    let static_bound = quote! { 'static };
    reject_lifetimes(&input.generics, &mut errors);
    let generics = with_bounds(&input.generics, &static_bound);
    errors.finish()?;

    let ident = &input.ident;
    let name = kebab_name(ident, container.name.as_ref());
    let docs = with_docs(&input.attrs);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #runtime::WitType for #ident #type_generics #where_clause {
            fn wit_type(graph: &mut #runtime::TypeGraph) -> #runtime::Type {
                graph.define::<Self>(|_| #runtime::TypeDef::resource(#name) #docs)
            }
        }

        #[automatically_derived]
        impl #impl_generics #runtime::Resource for #ident #type_generics #where_clause {}
    })
}
