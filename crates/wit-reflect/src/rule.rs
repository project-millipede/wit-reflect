//! Reflection of representation rules.
//!
//! A rule replaces a field's own type with a wire type and states how the
//! value is obtained. The derive calls one function of this module per rule.
//! Each function's bounds are the rule's requirements, so a rule that cannot
//! be satisfied is a compile error at the field that declares it.

use std::fmt;

use crate::graph::TypeGraph;
use crate::reflect::WitType;
use crate::types::Type;

/// An integer whose width depends on the target, with its lossless wire type.
///
/// `usize` and `isize` are memory quantities, not wire types. They are
/// exported as `u64` and `s64`, with or without `#[wit(widen)]`. The
/// implementations exist only for targets whose pointers are at most 64 bits
/// wide, so the conversion never loses bits.
pub trait Widen {
    /// The fixed-width type that holds every value of `Self`.
    type Wire: WitType;

    /// Convert without loss.
    fn widen(self) -> Self::Wire;
}

#[cfg(any(
    target_pointer_width = "16",
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl Widen for usize {
    type Wire = u64;

    fn widen(self) -> u64 {
        // `u64: From<usize>` does not exist. Under this `cfg` the cast is
        // lossless, which neither truncation nor saturation would express.
        self as u64
    }
}

#[cfg(any(
    target_pointer_width = "16",
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl Widen for isize {
    type Wire = i64;

    fn widen(self) -> i64 {
        // Lossless for the same reason as `usize`.
        self as i64
    }
}

/// `#[wit(display)]`: a value exported as its `Display` text.
#[must_use]
pub fn display<S: fmt::Display + ?Sized>(_graph: &mut TypeGraph) -> Type {
    Type::String
}

/// `#[wit(widen)]`: a target-width integer exported at its lossless width.
pub fn widened<S: Widen>(graph: &mut TypeGraph) -> Type {
    S::Wire::wit_type(graph)
}

/// `#[wit(into = W)]`: a value exported as `W`, obtained with `W::from`.
pub fn converted<S, W: From<S> + WitType>(graph: &mut TypeGraph) -> Type {
    W::wit_type(graph)
}

/// `#[wit(with = function)]`: a value computed from the whole parent `P`.
///
/// The wire type is the function's return type. A sibling-dependent
/// projection and a computed field such as a `Display` message both use this.
pub fn computed<P: ?Sized, W: WitType>(_function: fn(&P) -> W, graph: &mut TypeGraph) -> Type {
    W::wit_type(graph)
}
