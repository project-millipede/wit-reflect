//! Parsing of `#[wit(...)]` attributes and doc comments.

use quote::ToTokens;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Expr, ExprLit, Lit, LitStr, Meta, Path};

/// Collects every problem in an item so one compilation reports them all.
#[derive(Default)]
pub(crate) struct Errors {
    combined: Option<syn::Error>,
}

impl Errors {
    pub(crate) fn push(&mut self, error: syn::Error) {
        match &mut self.combined {
            Some(combined) => combined.combine(error),
            None => self.combined = Some(error),
        }
    }

    /// Report `message` at the source location of `tokens`.
    pub(crate) fn report(&mut self, tokens: impl ToTokens, message: &str) {
        let error = syn::Error::new_spanned(tokens, message);
        self.push(error);
    }

    pub(crate) fn finish(self) -> syn::Result<()> {
        match self.combined {
            Some(combined) => Err(combined),
            None => Ok(()),
        }
    }
}

/// `#[wit(...)]` on a struct or enum.
#[derive(Default)]
pub(crate) struct ContainerAttributes {
    pub(crate) name: Option<LitStr>,
    pub(crate) transparent: Option<Path>,
    pub(crate) crate_path: Option<Path>,
}

impl ContainerAttributes {
    pub(crate) fn parse(attributes: &[Attribute], errors: &mut Errors) -> Self {
        let mut parsed = Self::default();
        parse_wit_attributes(attributes, errors, |meta| {
            if meta.path.is_ident("name") {
                set_string(&mut parsed.name, &meta)
            } else if meta.path.is_ident("transparent") {
                parsed.transparent = Some(meta.path.clone());
                Ok(())
            } else if meta.path.is_ident("crate") {
                let crate_path = meta.value()?.parse()?;
                parsed.crate_path = Some(crate_path);
                Ok(())
            } else {
                Err(meta.error("expected `name`, `transparent` or `crate`"))
            }
        });
        parsed
    }
}

/// `#[wit(...)]` on a field.
#[derive(Default)]
pub(crate) struct FieldAttributes {
    pub(crate) name: Option<LitStr>,
}

impl FieldAttributes {
    pub(crate) fn parse(attributes: &[Attribute], errors: &mut Errors) -> Self {
        let mut parsed = Self::default();
        parse_wit_attributes(attributes, errors, |meta| {
            if meta.path.is_ident("name") {
                set_string(&mut parsed.name, &meta)
            } else {
                Err(meta.error("expected `name`"))
            }
        });
        parsed
    }
}

/// `#[wit(...)]` on an enum variant.
#[derive(Default)]
pub(crate) struct VariantAttributes {
    pub(crate) name: Option<LitStr>,
    pub(crate) payload: Option<LitStr>,
}

impl VariantAttributes {
    pub(crate) fn parse(attributes: &[Attribute], errors: &mut Errors) -> Self {
        let mut parsed = Self::default();
        parse_wit_attributes(attributes, errors, |meta| {
            if meta.path.is_ident("name") {
                set_string(&mut parsed.name, &meta)
            } else if meta.path.is_ident("payload") {
                set_string(&mut parsed.payload, &meta)
            } else {
                Err(meta.error("expected `name` or `payload`"))
            }
        });
        parsed
    }
}

fn parse_wit_attributes(
    attributes: &[Attribute],
    errors: &mut Errors,
    mut parse_entry: impl FnMut(ParseNestedMeta<'_>) -> syn::Result<()>,
) {
    for attribute in attributes {
        if !attribute.path().is_ident("wit") {
            continue;
        }
        let parse_result = attribute.parse_nested_meta(&mut parse_entry);
        if let Err(error) = parse_result {
            errors.push(error);
        }
    }
}

fn set_string(slot: &mut Option<LitStr>, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error("this attribute is given more than once"));
    }
    let literal = meta.value()?.parse()?;
    *slot = Some(literal);
    Ok(())
}

/// The item's `///` documentation, one line per comment line.
///
/// The single space that conventionally follows `///` is removed. Returns
/// `None` when the item has no documentation text.
pub(crate) fn docs(attributes: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attribute in attributes {
        let Meta::NameValue(name_value) = &attribute.meta else {
            continue;
        };
        if !name_value.path.is_ident("doc") {
            continue;
        }
        let Expr::Lit(ExprLit {
            lit: Lit::Str(text),
            ..
        }) = &name_value.value
        else {
            continue;
        };
        let comment = text.value();
        for comment_line in comment.lines() {
            let line = comment_line.strip_prefix(' ').unwrap_or(comment_line);
            lines.push(line.trim_end().to_owned());
        }
        if comment.is_empty() {
            lines.push(String::new());
        }
    }

    let documentation = lines.join("\n");
    let trimmed_documentation = documentation.trim_matches('\n');
    if trimmed_documentation.is_empty() {
        None
    } else {
        Some(trimmed_documentation.to_owned())
    }
}
