//! Reflect ordinary Rust types into validated WIT packages.
//!
//! The types an implementation already uses are the only definition of its
//! data. Deriving [`WitType`] on them lets a small generator program describe a
//! WIT package without restating a single field:
//!
#![cfg_attr(feature = "derive", doc = "```")]
#![cfg_attr(not(feature = "derive"), doc = "```ignore")]
//! use wit_reflect::{PackageName, Schema, WitType};
//!
//! /// Completed steps in execution order.
//! #[derive(WitType)]
//! struct Outcome<Step> {
//!     steps: Vec<Step>,
//! }
//!
//! #[derive(WitType)]
//! struct Report {
//!     byte_length: u64,
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let package_name: PackageName = "example:analysis@0.1.0".parse()?;
//! let mut schema = Schema::new(package_name);
//! schema
//!     .interface("inspection")
//!     .define_as::<Outcome<Report>>("report-outcome");
//!
//! let package = schema.resolve()?;
//! let expected_wit = "\
//! package example:analysis@0.1.0;
//!
//! interface inspection {
//!   record report {
//!     byte-length: u64,
//!   }
//!
//!   /// Completed steps in execution order.
//!   record report-outcome {
//!     steps: list<report>,
//!   }
//! }
//! ";
//! assert_eq!(package.to_string(), expected_wit);
//! # Ok(())
//! # }
//! ```
//!
//! # How types are found
//!
//! The derive runs in the crate that owns a type and reflects its fields as
//! declared. By then the Rust compiler has resolved aliases, re-exports,
//! dependency crates and generic arguments, so this crate never resolves a
//! Rust path itself. Each instantiation of a generic type is a separate
//! definition, identified by its Rust type rather than by its WIT name.
//!
//! # Declaring a package
//!
//! A [`Schema`] names the interfaces and the types each one defines. Everything
//! those types refer to is discovered automatically. [`Schema::resolve`]
//! assigns each type one owning interface and one name, validates the result,
//! and reports every problem it finds in one [`SchemaErrors`].
//!
//! # Keeping ordinary builds independent
//!
//! Gate the derive behind a default-off feature of the owning crate, so its
//! ordinary build does not compile this crate:
//!
//! ```text
//! #[cfg_attr(feature = "wit", derive(wit_reflect::WitType))]
//! #[cfg_attr(feature = "wit", wit(name = "report"))]
//! pub struct BinaryReport { /* fields */ }
//! ```
//!
//! # Current scope
//!
//! This crate generates type definitions. Functions, resources, worlds,
//! representation rules and generated adapters are later phases; see
//! `docs/phases.md` in the repository.

mod error;
mod graph;
mod identifier;
mod package_name;
mod reflect;
mod render;
mod resolve;
mod schema;
mod types;

pub use error::{SchemaError, SchemaErrors};
pub use graph::TypeGraph;
pub use identifier::IdentifierError;
pub use package_name::{PackageName, PackageNameError};
pub use reflect::WitType;
pub use resolve::ResolvedPackage;
pub use schema::{InterfaceBuilder, Schema};
pub use types::{Case, EnumCase, Field, Payload, Type, TypeDef, TypeKey};

/// Derives [`WitType`](trait@WitType) for a struct or enum.
///
/// | Rust item | WIT definition |
/// | --- | --- |
/// | Struct with named fields | `record` |
/// | Enum of unit variants | `enum` |
/// | Enum with data | `variant`; a struct-like case gets a payload record |
/// | `#[wit(transparent)]` single-field struct | the field's type, without a definition |
///
/// Names default to the Rust identifier in kebab case.
///
/// # Attributes
///
/// - `#[wit(name = "…")]` on the item, a field or a variant sets its WIT name.
/// - `#[wit(payload = "…")]` on a struct-like variant names its payload
///   record. The default is `<variant-name>-<case-name>`.
/// - `#[wit(transparent)]` on a single-field struct uses the field's
///   representation.
/// - `#[wit(crate = path)]` names this crate when it is re-exported elsewhere.
///
/// Every type parameter receives a `WitType` bound. Lifetime parameters are
/// rejected because boundary values are owned.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::WitType;
