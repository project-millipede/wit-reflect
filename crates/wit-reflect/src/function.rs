//! Reflected operations: functions and the methods of resources.

use std::fmt;
use std::marker::PhantomData;

use crate::types::Type;

/// The signature of an exported Rust function.
///
/// `#[wit_reflect::export]` produces this from the function it is applied to,
/// so parameter names, types and documentation come from the declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
    pub(crate) params: Vec<Param>,
    pub(crate) result: Type,
}

impl Function {
    /// A function with its WIT name, parameters in order and result type.
    ///
    /// The unit type as `result` means the function returns nothing.
    #[must_use]
    pub fn new(name: &'static str, params: Vec<Param>, result: Type) -> Self {
        Self {
            name,
            docs: None,
            params,
            result,
        }
    }

    /// Attach documentation, rendered as WIT `///` comments.
    #[must_use]
    pub fn with_docs(mut self, docs: &'static str) -> Self {
        self.docs = Some(docs);
        self
    }
}

/// A function parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub(crate) name: &'static str,
    pub(crate) ty: Type,
}

impl Param {
    /// A parameter with its WIT name and type.
    #[must_use]
    pub fn new(name: &'static str, ty: Type) -> Self {
        Self { name, ty }
    }
}

/// A function that belongs to the resource `R`.
///
/// The type parameter ties the function to the Rust type whose `impl` block
/// declares it, so it cannot be attached to another resource by mistake.
pub struct Method<R: ?Sized> {
    pub(crate) function: Function,
    pub(crate) kind: MethodKind,
    // `fn() -> R` names the resource without owning or borrowing one.
    resource: PhantomData<fn() -> R>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MethodKind {
    /// Called on a borrowed resource.
    Method,
    /// Called without a resource.
    Static,
    /// Creates the resource.
    Constructor,
}

impl<R: ?Sized> Method<R> {
    /// A function called on a borrowed `R`, declared with `&self` or `&mut self`.
    #[must_use]
    pub fn borrowing(function: Function) -> Self {
        Self::new(function, MethodKind::Method)
    }

    /// An associated function of `R` without a receiver: a WIT `static func`.
    #[must_use]
    pub fn associated(function: Function) -> Self {
        Self::new(function, MethodKind::Static)
    }

    /// The WIT `constructor` of `R`. Its result must be `R` itself.
    #[must_use]
    pub fn constructor(function: Function) -> Self {
        Self::new(function, MethodKind::Constructor)
    }

    fn new(function: Function, kind: MethodKind) -> Self {
        Self {
            function,
            kind,
            resource: PhantomData,
        }
    }
}

// Derived `Debug` would require `R: Debug`, although no `R` is stored.
impl<R: ?Sized> fmt::Debug for Method<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Method")
            .field("function", &self.function)
            .field("kind", &self.kind)
            .finish()
    }
}
