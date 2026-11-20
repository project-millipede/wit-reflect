//! Schema diagnostics.
//!
//! Resolution reports every problem it can establish instead of stopping at the
//! first one, so a generator run shows the complete list. Checks that depend on
//! an earlier failed step are skipped rather than reported from invalid state.

use std::fmt;

use crate::identifier::IdentifierError;

/// One reason a schema cannot be rendered as WIT.
///
/// Rust type names come from [`std::any::type_name`] and include generic
/// arguments, which identifies the exact instantiation involved.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// A derived or declared name is not a WIT identifier.
    #[error("`{name}` is not a valid WIT name for {subject} because {reason}")]
    InvalidIdentifier {
        /// The rejected name.
        name: String,
        /// What the name was meant for, such as a field of a Rust type.
        subject: String,
        /// The grammar rule it violates.
        reason: IdentifierError,
    },

    /// Several definitions in one interface resolve to the same name.
    ///
    /// Distinct instantiations of a generic type share a default name. Give
    /// each one its own with
    /// [`InterfaceBuilder::define_as`](crate::InterfaceBuilder::define_as).
    #[error(
        "interface `{interface}` would define `{name}` more than once, for {}",
        .origins.join(" and ")
    )]
    DuplicateTypeName {
        /// The interface whose namespace contains the clash.
        interface: String,
        /// The shared WIT name.
        name: String,
        /// The Rust types, payloads or imports that resolve to it.
        origins: Vec<String>,
    },

    /// One Rust type is declared by more than one `define` call.
    #[error("`{rust_type}` is declared in interface `{first}` and again in interface `{second}`")]
    DuplicateDeclaration {
        /// The Rust type declared twice.
        rust_type: &'static str,
        /// The interface of the first declaration.
        first: String,
        /// The interface of the repeated declaration.
        second: String,
    },

    /// An undeclared type is reached from several interfaces.
    ///
    /// Declare it in the interface that should own it; the others `use` it.
    #[error(
        "`{rust_type}` is used by interfaces {} but declared in none of them",
        .interfaces.join(", ")
    )]
    AmbiguousPlacement {
        /// The Rust type without an owning interface.
        rust_type: &'static str,
        /// The interfaces that reach it, in declaration order.
        interfaces: Vec<String>,
    },

    /// A declared type has no named definition, such as `Vec<u8>` or `u32`.
    #[error("`{rust_type}` cannot be defined in interface `{interface}`: it is not a named type")]
    NotNamedType {
        /// The interface of the declaration.
        interface: String,
        /// The declared Rust type.
        rust_type: &'static str,
    },

    /// A type contains itself. WIT types cannot be recursive.
    #[error("recursive type: {}", .cycle.join(" -> "))]
    RecursiveType {
        /// The Rust types from the outer occurrence back to itself.
        cycle: Vec<&'static str>,
    },

    /// A record without fields, or an enum or variant without cases.
    #[error("`{rust_type}` has no fields or cases to define")]
    EmptyDefinition {
        /// The Rust type, with the case name for a struct-like case.
        rust_type: String,
    },

    /// Two fields or cases of one definition share a WIT name.
    #[error("`{rust_type}` has more than one member named `{name}`")]
    DuplicateMember {
        /// The Rust type, with the case name for a struct-like case.
        rust_type: String,
        /// The shared WIT name.
        name: &'static str,
    },

    /// The unit type appears where WIT needs a value.
    #[error("`{rust_type}` uses `()` outside of a `Result`; WIT has no unit value")]
    UnitOutsideResult {
        /// The Rust type, with the case name for a struct-like case.
        rust_type: String,
    },
}

/// Every problem found while resolving a schema. Never empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaErrors {
    errors: Vec<SchemaError>,
}

impl SchemaErrors {
    /// Succeed when nothing was reported; otherwise return all reports.
    pub(crate) fn check(errors: Vec<SchemaError>) -> Result<(), Self> {
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Self { errors })
        }
    }

    /// The individual problems in the order they were found.
    pub fn iter(&self) -> std::slice::Iter<'_, SchemaError> {
        self.errors.iter()
    }
}

impl IntoIterator for SchemaErrors {
    type Item = SchemaError;
    type IntoIter = std::vec::IntoIter<SchemaError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl<'errors> IntoIterator for &'errors SchemaErrors {
    type Item = &'errors SchemaError;
    type IntoIter = std::slice::Iter<'errors, SchemaError>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// One line per problem, so a generator's output lists them all.
impl fmt::Display for SchemaErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the WIT schema is invalid:")?;
        for error in &self.errors {
            write!(formatter, "\n- {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SchemaErrors {}
