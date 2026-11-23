//! Reflection of representation rules.
//!
//! A rule replaces a field's own type with a wire type and states how the
//! value is obtained. The derive calls one function of this module per rule.
//! Each function's bounds are the rule's requirements, so a rule that cannot
//! be satisfied is a compile error at the field that declares it.

use std::fmt;
use std::marker::PhantomData;

use crate::graph::TypeGraph;
use crate::lower::{Converted, Is, Lower, Passage};
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

/// A value's [`Display`](fmt::Display) text, shown in the contract as `string`.
///
/// Any type that implements `Display` converts into it, so it serves wherever
/// a rule or a map entry names the type a value is shown as:
/// `#[wit(into = DisplayText)]` on a field, or `Foreign => DisplayText` in a
/// crate's [`representations!`](crate::representations) map.
///
/// It does not implement `Display` itself. That keeps the conversion from
/// every `Display` type unambiguous.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayText(String);

impl DisplayText {
    /// The text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl<Shown: fmt::Display> From<Shown> for DisplayText {
    fn from(value: Shown) -> Self {
        Self(value.to_string())
    }
}

impl WitType for DisplayText {
    fn wit_type(_graph: &mut TypeGraph) -> Type {
        Type::String
    }
}

impl Lower for DisplayText {
    // The owner holds the original value, not this text.
    type Passage = Converted;
    type Parts = String;

    fn lower(self) -> String {
        self.0
    }
}

/// A crate's map declares how a type it does not own is shown in the contract.
///
/// [`representations!`](crate::representations) implements it for the map
/// type, once per entry. The map is a type of the declaring crate, which is
/// what lets that crate state a fact about another crate's type.
pub trait Represents<Foreign> {
    /// The type the contract shows.
    type As: WitType + Lower;

    /// Convert a value into the shown type.
    fn represent(value: Foreign) -> Self::As;
}

// A map that represents `T` also represents its containers.
impl<Map: Represents<T>, T> Represents<Option<T>> for Map {
    type As = Option<Map::As>;

    fn represent(value: Option<T>) -> Self::As {
        value.map(Map::represent)
    }
}

impl<Map: Represents<T>, T> Represents<Vec<T>> for Map {
    type As = Vec<Map::As>;

    fn represent(value: Vec<T>) -> Self::As {
        value.into_iter().map(Map::represent).collect()
    }
}

/// Whether `Map` has an entry for `Field`.
///
/// The answer is the constant `BY_MAP`. With an entry it resolves to the
/// inherent constant below; otherwise only the default of [`NotDeclared`]
/// exists. The derive reads it for one concrete field type, where the compiler
/// can tell the two apart. [`Usable`](crate::lower::Usable) works the same way.
#[doc(hidden)]
#[derive(Debug)]
pub struct Declared<Map, Field>(PhantomData<(Map, Field)>);

impl<Map: Represents<Field>, Field> Declared<Map, Field> {
    /// The map has an entry.
    pub const BY_MAP: bool = true;
}

/// The answer for a field type the map has no entry for.
#[doc(hidden)]
pub trait NotDeclared {
    /// The map has no entry.
    const BY_MAP: bool = false;
}

impl<Map, Field> NotDeclared for Declared<Map, Field> {}

/// The wire form of a field in an item with a map: the map's entry for the
/// field's type, or the type itself.
#[doc(hidden)]
pub trait Choose<Map, Field> {
    /// The type the contract shows.
    type Wire: WitType + Lower;

    /// How the field crosses inside its parent.
    type Passage: Passage;

    /// The value in its wire form.
    fn choose(value: Field) -> Self::Wire;
}

impl<Map: Represents<Field>, Field> Choose<Map, Field> for Is<true> {
    type Wire = Map::As;
    // The owner holds `Field` where the contract shows `Wire`, so the parent
    // cannot be used by the bindings as it is.
    type Passage = Converted;

    fn choose(value: Field) -> Map::As {
        Map::represent(value)
    }
}

impl<Map, Field: WitType + Lower> Choose<Map, Field> for Is<false> {
    type Wire = Field;
    type Passage = Field::Passage;

    fn choose(value: Field) -> Field {
        value
    }
}
