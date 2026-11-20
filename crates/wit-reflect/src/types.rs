//! The reflected type model: type expressions and named definitions.
//!
//! These values describe Rust types as written by their owners. Names are the
//! derive's defaults; [`Schema::resolve`](crate::Schema::resolve) applies
//! declared names, placement and validation.

use std::any::{TypeId, type_name};
use std::hash::{Hash, Hasher};

/// The identity of a named definition: the Rust type it was reflected from.
///
/// Two instantiations of one generic type have different keys, so they are
/// never merged. Keys are created only by [`TypeGraph::define`](crate::TypeGraph::define).
#[derive(Clone, Copy, Debug)]
pub struct TypeKey {
    id: TypeId,
    rust_name: &'static str,
}

impl TypeKey {
    pub(crate) fn of<T: ?Sized + 'static>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            rust_name: type_name::<T>(),
        }
    }

    /// The Rust type's name with its generic arguments, for diagnostics.
    ///
    /// The exact text comes from [`std::any::type_name`] and is not a stable
    /// identifier.
    #[must_use]
    pub fn rust_name(&self) -> &'static str {
        self.rust_name
    }
}

// Identity is the `TypeId`; the name is derived from it and only describes it.
impl PartialEq for TypeKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for TypeKey {}

impl Hash for TypeKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// A WIT type expression, as used for a field, payload or list element.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Type {
    /// `bool`
    Bool,
    /// `u8`
    U8,
    /// `u16`
    U16,
    /// `u32`
    U32,
    /// `u64`
    U64,
    /// `s8`
    S8,
    /// `s16`
    S16,
    /// `s32`
    S32,
    /// `s64`
    S64,
    /// `f32`
    F32,
    /// `f64`
    F64,
    /// `char`
    Char,
    /// `string`
    String,
    /// `list<T>`
    List(Box<Type>),
    /// `option<T>`
    Option(Box<Type>),
    /// `result<T, E>`. A unit side is written `_` or omitted.
    Result {
        /// The success type.
        ok: Box<Type>,
        /// The failure type.
        err: Box<Type>,
    },
    /// `tuple<…>`.
    ///
    /// The empty tuple is Rust's unit type. WIT has no unit value, so it is
    /// accepted only as a side of [`Type::Result`].
    Tuple(Vec<Type>),
    /// A reference to a named definition in the type graph.
    Named(TypeKey),
}

impl Type {
    /// `list<element>`
    #[must_use]
    pub fn list(element: Type) -> Self {
        Self::List(Box::new(element))
    }

    /// `option<value>`
    #[must_use]
    pub fn option(value: Type) -> Self {
        Self::Option(Box::new(value))
    }

    /// `result<ok, err>`
    #[must_use]
    pub fn result(ok: Type, err: Type) -> Self {
        Self::Result {
            ok: Box::new(ok),
            err: Box::new(err),
        }
    }

    pub(crate) fn is_unit(&self) -> bool {
        matches!(self, Self::Tuple(elements) if elements.is_empty())
    }
}

/// A named WIT definition reflected from one Rust struct or enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDef {
    name: &'static str,
    docs: Option<&'static str>,
    kind: TypeDefKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TypeDefKind {
    Record(Vec<Field>),
    Variant(Vec<Case>),
    Enum(Vec<EnumCase>),
}

impl TypeDef {
    /// A `record` with the given default name and fields in declaration order.
    #[must_use]
    pub fn record(name: &'static str, fields: Vec<Field>) -> Self {
        Self::new(name, TypeDefKind::Record(fields))
    }

    /// A `variant` whose cases may carry payloads.
    #[must_use]
    pub fn variant(name: &'static str, cases: Vec<Case>) -> Self {
        Self::new(name, TypeDefKind::Variant(cases))
    }

    /// An `enum` of payload-free cases.
    #[must_use]
    pub fn enumeration(name: &'static str, cases: Vec<EnumCase>) -> Self {
        Self::new(name, TypeDefKind::Enum(cases))
    }

    fn new(name: &'static str, kind: TypeDefKind) -> Self {
        Self {
            name,
            docs: None,
            kind,
        }
    }

    /// Attach documentation, rendered as WIT `///` comments.
    #[must_use]
    pub fn with_docs(mut self, docs: &'static str) -> Self {
        self.docs = Some(docs);
        self
    }

    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    pub(crate) fn docs(&self) -> Option<&'static str> {
        self.docs
    }

    pub(crate) fn kind(&self) -> &TypeDefKind {
        &self.kind
    }
}

/// A record field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
    pub(crate) ty: Type,
}

impl Field {
    /// A field with its WIT name and type.
    #[must_use]
    pub fn new(name: &'static str, ty: Type) -> Self {
        Self {
            name,
            docs: None,
            ty,
        }
    }

    /// Attach documentation, rendered as WIT `///` comments.
    #[must_use]
    pub fn with_docs(mut self, docs: &'static str) -> Self {
        self.docs = Some(docs);
        self
    }
}

/// A variant case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
    pub(crate) payload: Payload,
}

impl Case {
    /// A case with its WIT name and payload.
    #[must_use]
    pub fn new(name: &'static str, payload: Payload) -> Self {
        Self {
            name,
            docs: None,
            payload,
        }
    }

    /// Attach documentation, rendered as WIT `///` comments.
    #[must_use]
    pub fn with_docs(mut self, docs: &'static str) -> Self {
        self.docs = Some(docs);
        self
    }
}

/// What a variant case carries.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Payload {
    /// A unit case.
    None,
    /// A case carrying one value.
    Type(Type),
    /// A struct-like case.
    ///
    /// WIT cases carry at most one value, so resolution defines a record for
    /// these fields next to the variant and uses it as the payload.
    Record {
        /// The record's declared name. `None` derives `<variant>-<case>` from
        /// the resolved variant name, which keeps instantiations distinct.
        name: Option<&'static str>,
        /// The fields in declaration order.
        fields: Vec<Field>,
    },
}

/// An enum case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumCase {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
}

impl EnumCase {
    /// A case with its WIT name.
    #[must_use]
    pub fn new(name: &'static str) -> Self {
        Self { name, docs: None }
    }

    /// Attach documentation, rendered as WIT `///` comments.
    #[must_use]
    pub fn with_docs(mut self, docs: &'static str) -> Self {
        self.docs = Some(docs);
        self
    }
}
