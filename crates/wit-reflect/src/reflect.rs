//! The reflection trait and its implementations for standard types.

use crate::graph::TypeGraph;
use crate::rule::Widen;
use crate::types::Type;

/// A Rust type with a WIT representation.
///
/// Derive it on the struct or enum that owns the data. The derive reflects the
/// fields exactly as declared, so the Rust definition stays the only place a
/// field is edited:
///
/// ```
/// # #[cfg(feature = "derive")] {
/// use wit_reflect::WitType;
///
/// #[derive(WitType)]
/// struct FileRange {
///     offset: u64,
///     size: u64,
/// }
/// # }
/// ```
///
/// The Rust compiler resolves aliases, re-exports, dependency crates and
/// generic arguments before this trait is consulted, so `Outcome<Report>` and
/// an alias for it reflect as the same instantiation.
///
/// Types must be owned (`'static`): a boundary value cannot borrow from its
/// producer.
///
/// # Implementing manually
///
/// A named type calls [`TypeGraph::define`] and reflects its field types
/// inside the closure. A type that only wraps another returns that type's
/// expression.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no WIT representation",
    label = "cannot cross a WIT boundary as written",
    note = "derive `wit_reflect::WitType` on the struct or enum that owns this data",
    note = "for a type of another crate, put a rule on the field or add the type to the \
            crate's `wit_reflect::representations!` map"
)]
pub trait WitType: 'static {
    /// Describe this type as a WIT type expression.
    ///
    /// Named definitions reached on the way are recorded in `graph`.
    fn wit_type(graph: &mut TypeGraph) -> Type;
}

/// A Rust type exported as a WIT `resource`.
///
/// Derive it with `#[derive(WitResource)]`. A resource crosses the boundary as
/// a handle: `own<R>` where the Rust signature passes the value, `borrow<R>`
/// where it passes a reference.
pub trait Resource: WitType {}

macro_rules! primitive {
    ($($rust:ty => $wit:ident),* $(,)?) => {
        $(
            impl WitType for $rust {
                fn wit_type(_graph: &mut TypeGraph) -> Type {
                    Type::$wit
                }
            }
        )*
    };
}

primitive! {
    bool => Bool,
    u8 => U8,
    u16 => U16,
    u32 => U32,
    u64 => U64,
    i8 => S8,
    i16 => S16,
    i32 => S32,
    i64 => S64,
    f32 => F32,
    f64 => F64,
    char => Char,
    String => String,
}

/// A platform-sized integer has no width of its own on the wire: `usize` is 64
/// bits natively and 32 bits in a Wasm component, while a contract is one text
/// for both. It is shown at the 64-bit width, which holds every value on any
/// target [`Widen`] is implemented for.
macro_rules! platform_sized {
    ($($rust:ty),* $(,)?) => {
        $(
            #[cfg(any(
                target_pointer_width = "16",
                target_pointer_width = "32",
                target_pointer_width = "64"
            ))]
            impl WitType for $rust {
                fn wit_type(graph: &mut TypeGraph) -> Type {
                    <$rust as Widen>::Wire::wit_type(graph)
                }
            }
        )*
    };
}

platform_sized!(usize, isize);

/// The referent of a `&str` parameter.
impl WitType for str {
    fn wit_type(_graph: &mut TypeGraph) -> Type {
        Type::String
    }
}

/// The referent of a `&[T]` parameter.
impl<T: WitType> WitType for [T] {
    fn wit_type(graph: &mut TypeGraph) -> Type {
        let element = T::wit_type(graph);
        Type::list(element)
    }
}

impl<T: WitType> WitType for Vec<T> {
    fn wit_type(graph: &mut TypeGraph) -> Type {
        let element = T::wit_type(graph);
        Type::list(element)
    }
}

impl<T: WitType> WitType for Option<T> {
    fn wit_type(graph: &mut TypeGraph) -> Type {
        let value = T::wit_type(graph);
        Type::option(value)
    }
}

impl<T: WitType, E: WitType> WitType for Result<T, E> {
    fn wit_type(graph: &mut TypeGraph) -> Type {
        let ok = T::wit_type(graph);
        let err = E::wit_type(graph);
        Type::result(ok, err)
    }
}

/// Owned indirection does not change the value's representation.
impl<T: WitType> WitType for Box<T> {
    fn wit_type(graph: &mut TypeGraph) -> Type {
        T::wit_type(graph)
    }
}

/// Rust's unit type, accepted only as a side of a `result`.
impl WitType for () {
    fn wit_type(_graph: &mut TypeGraph) -> Type {
        Type::Tuple(Vec::new())
    }
}

macro_rules! tuple {
    ($($element:ident),+) => {
        impl<$($element: WitType),+> WitType for ($($element,)+) {
            fn wit_type(graph: &mut TypeGraph) -> Type {
                let elements = vec![$($element::wit_type(graph)),+];
                Type::Tuple(elements)
            }
        }
    };
}

tuple!(A);
tuple!(A, B);
tuple!(A, B, C);
tuple!(A, B, C, D);
tuple!(A, B, C, D, E);
tuple!(A, B, C, D, E, F);
tuple!(A, B, C, D, E, F, G);
tuple!(A, B, C, D, E, F, G, H);
