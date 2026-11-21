//! Reflected operations: functions and the methods of resources.

use std::fmt;
use std::marker::PhantomData;

use crate::graph::TypeGraph;
use crate::types::Type;

/// A reference to an exported Rust function: how to reflect it and where it is.
///
/// `operation!` produces this value. The path is written into generated
/// forwarding code exactly as given, so it must resolve from the crate that
/// includes that code: start it with a crate name or with `crate`.
pub struct Operation<Reflected> {
    pub(crate) reflect: fn(&mut TypeGraph) -> Reflected,
    pub(crate) rust_path: &'static str,
}

// Derives would require the traits of `Reflected`, of which no value is stored.
impl<Reflected> Clone for Operation<Reflected> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Reflected> Copy for Operation<Reflected> {}

impl<Reflected> fmt::Debug for Operation<Reflected> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Operation")
            .field("rust_path", &self.rust_path)
            .finish_non_exhaustive()
    }
}

impl<Reflected> Operation<Reflected> {
    /// Pair the reflection of a function with the path that names it.
    #[must_use]
    pub fn new(reflect: fn(&mut TypeGraph) -> Reflected, rust_path: &'static str) -> Self {
        Self { reflect, rust_path }
    }
}

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
    /// Whether the Rust function takes the argument behind a reference.
    pub(crate) by_reference: bool,
}

impl Param {
    /// A parameter the Rust function takes by value.
    #[must_use]
    pub fn new(name: &'static str, ty: Type) -> Self {
        Self {
            name,
            ty,
            by_reference: false,
        }
    }

    /// A parameter the Rust function takes behind a shared reference.
    ///
    /// `ty` is the referent's type as seen through the reference; see
    /// [`Type::borrowed`].
    #[must_use]
    pub fn by_reference(name: &'static str, ty: Type) -> Self {
        Self {
            name,
            ty,
            by_reference: true,
        }
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
    Method {
        /// Whether the Rust method takes `&mut self`.
        exclusive: bool,
    },
    /// Called without a resource.
    Static,
    /// Creates the resource.
    Constructor,
}

impl<R: ?Sized> Method<R> {
    /// A function called on a borrowed `R`, declared with `&self`.
    #[must_use]
    pub fn borrowing(function: Function) -> Self {
        Self::new(function, MethodKind::Method { exclusive: false })
    }

    /// A function called on a borrowed `R`, declared with `&mut self`.
    ///
    /// A WIT method still receives a shared borrow. The forwarding code
    /// obtains exclusive access through the resource's cell.
    #[must_use]
    pub fn borrowing_mut(function: Function) -> Self {
        Self::new(function, MethodKind::Method { exclusive: true })
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
