//! The reflection trait and its implementations for standard types.

use crate::graph::TypeGraph;
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
    note = "`usize` and `isize` depend on the target; use a fixed-width integer for wire data"
)]
pub trait WitType: 'static {
    /// Describe this type as a WIT type expression.
    ///
    /// Named definitions reached on the way are recorded in `graph`.
    fn wit_type(graph: &mut TypeGraph) -> Type;
}

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
