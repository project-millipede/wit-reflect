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
    /// `interface = "…"`: the interface that defines the type.
    pub(crate) interface: Option<LitStr>,
    pub(crate) transparent: Option<Path>,
    pub(crate) crate_path: Option<Path>,
    /// `field(name = "…", into = W, with = function)`, in declaration order.
    pub(crate) computed_fields: Vec<ComputedField>,
    /// The record field that holds an enum's cases when it has computed fields.
    pub(crate) cases: Option<LitStr>,
    /// The foreign type this declaration describes.
    pub(crate) remote: Option<Path>,
    /// `convert`: never let the type cross unchanged.
    pub(crate) convert: Option<Path>,
    /// `path = …`: where the component crate can name the type.
    pub(crate) public_path: Option<Path>,
}

/// A field of the wire record computed from the whole value.
pub(crate) struct ComputedField {
    pub(crate) name: LitStr,
    pub(crate) wire_type: Type,
    pub(crate) function: ExprPath,
}

impl ComputedField {
    fn parse(meta: &ParseNestedMeta<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut wire_type = None;
        let mut function = None;
        meta.parse_nested_meta(|entry| {
            if entry.path.is_ident("name") {
                set_string(&mut name, &entry)
            } else if entry.path.is_ident("into") {
                let parsed_type = entry.value()?.parse()?;
                wire_type = Some(parsed_type);
                Ok(())
            } else if entry.path.is_ident("with") {
                let parsed_function = entry.value()?.parse()?;
                function = Some(parsed_function);
                Ok(())
            } else {
                Err(entry.error("expected `name`, `into` or `with`"))
            }
        })?;
        match (name, wire_type, function) {
            (Some(name), Some(wire_type), Some(function)) => Ok(Self {
                name,
                wire_type,
                function,
            }),
            _ => Err(meta.error(
                "a computed field needs `name = \"…\"`, `into = WireType` and `with = function`",
            )),
        }
    }
}

impl ContainerAttributes {
    pub(crate) fn parse(attributes: &[Attribute], errors: &mut Errors) -> Self {
        let mut parsed = Self::default();
        parse_wit_attributes(attributes, errors, |meta| {
            if meta.path.is_ident("name") {
                set_string(&mut parsed.name, &meta)
            } else if meta.path.is_ident("interface") {
                set_string(&mut parsed.interface, &meta)
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
            } else if meta.path.is_ident("convert") {
                parsed.convert = Some(meta.path.clone());
                Ok(())
            } else if meta.path.is_ident("path") {
                if parsed.public_path.is_some() {
                    return Err(meta.error("`path` is given more than once"));
                }
                let public_path = meta.value()?.parse()?;
                parsed.public_path = Some(public_path);
                Ok(())
            } else {
                Err(meta.error(
                    "expected `name`, `interface`, `transparent`, `field`, `cases`, `remote`, \
                     `convert`, `path` or `crate`",
                ))
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

/// How a field's wire value is obtained instead of using the field as stored.
pub(crate) enum FieldRule {
    /// `display`: its `Display` text.
    Display,
    /// `widen`: `usize` or `isize` at a fixed, lossless width.
    Widen,
    /// `into = W`: the wire type `W`.
    Into {
        wire_type: Box<Type>,
        /// `with = function`: computed by `function(&parent)` instead of
        /// `W::from(field)`.
        function: Option<ExprPath>,
    },
    /// `skip`: not exported.
    Skip,
}

impl FieldAttributes {
    pub(crate) fn parse(attributes: &[Attribute], errors: &mut Errors) -> Self {
        let mut parsed = Self::default();
        // `with` modifies `into`, in either order, so it is combined afterwards.
        let mut with_function: Option<(ExprPath, Path)> = None;

        parse_wit_attributes(attributes, errors, |meta| {
            if meta.path.is_ident("name") {
                return set_string(&mut parsed.name, &meta);
            }
            if meta.path.is_ident("with") {
                if with_function.is_some() {
                    return Err(meta.error("`with` is given more than once"));
                }
                let function = meta.value()?.parse()?;
                with_function = Some((function, meta.path.clone()));
                return Ok(());
            }

            let rule = if meta.path.is_ident("display") {
                FieldRule::Display
            } else if meta.path.is_ident("widen") {
                FieldRule::Widen
            } else if meta.path.is_ident("skip") {
                FieldRule::Skip
            } else if meta.path.is_ident("into") {
                let wire_type = meta.value()?.parse()?;
                FieldRule::Into {
                    wire_type,
                    function: None,
                }
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

        if let Some((with, declared_by)) = with_function {
            match &mut parsed.rule {
                Some(DeclaredRule {
                    rule: FieldRule::Into { function, .. },
                    ..
                }) => *function = Some(with),
                _ => errors.report(
                    declared_by,
                    "`with` supplies the value of an `into` rule; \
                     write `into = WireType, with = function`",
                ),
            }
        }
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
