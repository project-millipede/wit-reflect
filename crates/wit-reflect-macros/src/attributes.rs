//! Parsing of `#[wit(...)]` attributes and doc comments.

use quote::ToTokens;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Expr, ExprLit, ExprPath, Lit, LitStr, Meta, Path, Type};

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
    /// `field(name = "…", with = function)`, in declaration order.
    pub(crate) computed_fields: Vec<ComputedField>,
    /// The record field that holds an enum's cases when it has computed fields.
    pub(crate) cases: Option<LitStr>,
    /// The foreign type this declaration describes.
    pub(crate) remote: Option<Path>,
}

/// A field of the wire record computed from the whole value.
pub(crate) struct ComputedField {
    pub(crate) name: LitStr,
    pub(crate) function: ExprPath,
}

impl ComputedField {
    fn parse(meta: &ParseNestedMeta<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut function = None;
        meta.parse_nested_meta(|entry| {
            if entry.path.is_ident("name") {
                set_string(&mut name, &entry)
            } else if entry.path.is_ident("with") {
                let parsed_function = entry.value()?.parse()?;
                function = Some(parsed_function);
                Ok(())
            } else {
                Err(entry.error("expected `name` or `with`"))
            }
        })?;
        match (name, function) {
            (Some(name), Some(function)) => Ok(Self { name, function }),
            _ => Err(meta.error("a computed field needs `name = \"…\"` and `with = function`")),
        }
    }
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
            } else if meta.path.is_ident("field") {
                let computed_field = ComputedField::parse(&meta)?;
                parsed.computed_fields.push(computed_field);
                Ok(())
            } else if meta.path.is_ident("cases") {
                set_string(&mut parsed.cases, &meta)
            } else if meta.path.is_ident("remote") {
                if parsed.remote.is_some() {
                    return Err(meta.error("`remote` is given more than once"));
                }
                let remote = meta.value()?.parse()?;
                parsed.remote = Some(remote);
                Ok(())
            } else {
                Err(meta
                    .error("expected `name`, `transparent`, `field`, `cases`, `remote` or `crate`"))
            }
        });
        parsed
    }
}

/// `#[wit(...)]` on a field.
#[derive(Default)]
pub(crate) struct FieldAttributes {
    pub(crate) name: Option<LitStr>,
    pub(crate) rule: Option<DeclaredRule>,
}

/// A representation rule and the attribute tokens that declared it.
pub(crate) struct DeclaredRule {
    pub(crate) rule: FieldRule,
    pub(crate) declared_by: Path,
}

/// How a field's wire type is obtained instead of reflecting its own type.
pub(crate) enum FieldRule {
    /// `display`: its `Display` text.
    Display,
    /// `widen`: `usize` or `isize` at a fixed, lossless width.
    Widen,
    /// `into = W`: `W::from(field)`.
    Into(Box<Type>),
    /// `with = function`: `function(&parent)`.
    With(ExprPath),
    /// `skip`: not exported.
    Skip,
}

impl FieldAttributes {
    pub(crate) fn parse(attributes: &[Attribute], errors: &mut Errors) -> Self {
        let mut parsed = Self::default();
        parse_wit_attributes(attributes, errors, |meta| {
            if meta.path.is_ident("name") {
                return set_string(&mut parsed.name, &meta);
            }
            let rule = if meta.path.is_ident("display") {
                FieldRule::Display
            } else if meta.path.is_ident("widen") {
                FieldRule::Widen
            } else if meta.path.is_ident("skip") {
                FieldRule::Skip
            } else if meta.path.is_ident("into") {
                let wire_type = meta.value()?.parse()?;
                FieldRule::Into(wire_type)
            } else if meta.path.is_ident("with") {
                let function = meta.value()?.parse()?;
                FieldRule::With(function)
            } else {
                return Err(
                    meta.error("expected `name`, `display`, `widen`, `into`, `with` or `skip`")
                );
            };
            if parsed.rule.is_some() {
                return Err(meta.error("a field takes one representation rule"));
            }
            parsed.rule = Some(DeclaredRule {
                rule,
                declared_by: meta.path.clone(),
            });
            Ok(())
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
