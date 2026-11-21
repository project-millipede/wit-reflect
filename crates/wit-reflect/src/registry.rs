//! Declarations that marked items register themselves.
//!
//! An item states its own part of a contract where it is declared:
//!
//! | Mark | Registers |
//! | --- | --- |
//! | `#[wit(interface = "…")]` on a derived type or resource | the type in that interface |
//! | `#[export(interface = "…")]` on a free function | the function in that interface |
//! | `#[exports]` on an `impl` block | every function of the block marked `#[export]` |
//! | `#[named]` on a type alias | the name of the aliased instantiation |
//!
//! Each mark submits a [`Registration`]. The registrations of every crate
//! linked into a program are collected when it is linked, and
//! [`Schema::from_registry`] applies them. A generator program therefore
//! declares nothing itself.
//!
//! # Linking the declaring crates
//!
//! Rust links a dependency only if the program names it. A generator that
//! calls nothing from a crate with marked items writes
//!
//! ```text
//! use owner_crate as _;
//! ```
//!
//! once. The crates that crate depends on are linked with it.
//! [`Schema::from_registry`] fails when nothing at all is registered, so a
//! missing line cannot produce an empty contract.
//!
//! # Order
//!
//! The order in which a linker collects registrations is not defined. They are
//! applied in the order of [`Position`]: by package name, then by file, line
//! and column. Generated files are therefore reproducible, and moving an item
//! can move its definition.
//!
//! # Wasm
//!
//! This module does not exist on Wasm targets and the marks register nothing
//! there. A component is built from generated files and carries no registry.

use crate::package_name::PackageName;
use crate::schema::Schema;

// The marks expand to `inventory::submit!`, which must resolve in crates that
// depend only on this one.
#[doc(hidden)]
pub use inventory;

/// Where a registered item is declared. Registrations are applied in this
/// order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    package: &'static str,
    file: &'static str,
    line: u32,
    column: u32,
}

impl Position {
    /// The position of a mark: its package, and `file!()`, `line!()` and
    /// `column!()` where it is written.
    #[must_use]
    pub const fn new(package: &'static str, file: &'static str, line: u32, column: u32) -> Self {
        Self {
            package,
            file,
            line,
            column,
        }
    }
}

/// One marked item's part of a contract.
///
/// The marks create registrations; they are not written by hand.
#[derive(Debug)]
pub struct Registration {
    position: Position,
    declare: fn(&mut Schema),
}

impl Registration {
    /// A registration that adds an item to a schema with `declare`.
    #[must_use]
    pub const fn new(position: Position, declare: fn(&mut Schema)) -> Self {
        Self { position, declare }
    }
}

inventory::collect!(Registration);

/// The program links no crate with marked items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "no declaration is registered: the program links no crate with marked items; \
     name each such crate once, for example with `use owner_crate as _;`"
)]
pub struct NothingRegistered;

impl Schema {
    /// Declare everything the linked crates registered, as the package
    /// `package`.
    ///
    /// The result is an ordinary schema. Worlds, and anything no Rust item
    /// can state, are added to it as usual.
    ///
    /// # Errors
    ///
    /// Returns [`NothingRegistered`] if the program links no registration. A
    /// crate's registrations are linked only if the program names the crate;
    /// see the [module documentation](crate::registry).
    pub fn from_registry(package: PackageName) -> Result<Self, NothingRegistered> {
        let mut registrations: Vec<&Registration> =
            inventory::iter::<Registration>.into_iter().collect();
        if registrations.is_empty() {
            return Err(NothingRegistered);
        }
        registrations.sort_by_key(|registration| registration.position);

        let mut schema = Self::new(package);
        for registration in registrations {
            (registration.declare)(&mut schema);
        }
        Ok(schema)
    }
}
