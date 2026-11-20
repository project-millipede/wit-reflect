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
//! # Operations
//!
//! Functions and methods are marked where they are declared and selected where
//! the contract is declared. Marking reflects the real signature; nothing is
//! exported until a schema lists it.
//!
#![cfg_attr(feature = "derive", doc = "```")]
#![cfg_attr(not(feature = "derive"), doc = "```ignore")]
//! use wit_reflect::{Schema, WitResource, export, operation};
//!
//! #[derive(WitResource)]
//! struct Session {
//!     selection: Option<String>,
//! }
//!
//! impl Session {
//!     #[export]
//!     fn select(&mut self, resource: &str) {
//!         self.selection = Some(resource.to_owned());
//!     }
//! }
//!
//! #[export]
//! fn create_session() -> Session {
//!     Session { selection: None }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut schema = Schema::new("example:files@0.1.0".parse()?);
//! let mut native = schema.interface("native");
//! native
//!     .resource::<Session>()
//!     .method(operation!(Session::select));
//! native.function(operation!(create_session));
//! schema.world("files").export("native");
//!
//! let package = schema.resolve()?;
//! let expected_wit = "\
//! package example:files@0.1.0;
//!
//! interface native {
//!   resource session {
//!     select: func(%resource: string);
//!   }
//!
//!   create-session: func() -> own<session>;
//! }
//!
//! world files {
//!   export native;
//! }
//! ";
//! assert_eq!(package.to_string(), expected_wit);
//! # Ok(())
//! # }
//! ```
//!
//! [`Schema::external_interface`] places types that a dependency package
//! already defines, so they are imported instead of redefined.
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
//! This crate generates the WIT contract: types, functions, resources and
//! worlds. Representation rules and the generated adapters that connect the
//! contract to the Rust implementation are later phases; see `docs/phases.md`
//! in the repository.

mod error;
mod function;
mod graph;
mod identifier;
mod package_name;
mod reflect;
mod render;
mod resolve;
mod schema;
mod types;

pub use error::{SchemaError, SchemaErrors};
pub use function::{Function, Method, Param};
pub use graph::TypeGraph;
pub use identifier::IdentifierError;
pub use package_name::{PackageName, PackageNameError};
pub use reflect::{Resource, WitType};
pub use resolve::ResolvedPackage;
pub use schema::{
    ExternalInterfaceBuilder, InterfaceBuilder, ResourceBuilder, Schema, WorldBuilder,
};
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

/// Derives [`WitType`](trait@WitType) and [`Resource`] for a type exported as
/// a WIT `resource`.
///
/// The type's fields are not reflected: a resource crosses the boundary as a
/// handle. Its exported functions are marked with [`macro@export`] and selected
/// by the schema.
///
/// `#[wit(name = "…")]` sets the resource's name; the default is the Rust name
/// in kebab case. `#[wit(crate = path)]` names this crate when it is
/// re-exported elsewhere.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::WitResource;

/// Makes the signature of a function or method available to a [`Schema`].
///
/// The function itself is unchanged. Its name, parameter names, types and
/// documentation are reflected from the declaration, and it becomes part of a
/// contract only when a schema lists it with [`operation!`](macro@operation).
///
/// | Rust | WIT |
/// | --- | --- |
/// | `&self` or `&mut self` | a method of the resource |
/// | no receiver, `#[export(static)]` | `static func` |
/// | no receiver, `#[export(constructor)]` | `constructor` |
/// | no receiver, no marker | a function of an interface |
/// | parameter `&str`, `&[T]`, `&T` | `string`, `list<T>`, `T` |
/// | parameter `&R` where `R` is a resource | `borrow<r>` |
/// | resource `R` by value | `own<r>` |
/// | no return type | no result |
///
/// `#[export(name = "…")]` sets the WIT name; the default is the Rust name in
/// kebab case. `#[export(crate = path)]` names this crate when it is
/// re-exported elsewhere.
///
/// Not exported: `async fn`, `unsafe fn`, generic functions, `impl Trait`,
/// methods that consume `self`, `&mut` parameters, pattern parameters and
/// reference results. Each is a compile error at the function.
///
/// To keep the owning crate's ordinary build independent of this crate, apply
/// it as `#[cfg_attr(feature = "wit", wit_reflect::export)]`.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::export;

/// Refers to the reflected signature of a function marked with
/// [`macro@export`], for [`InterfaceBuilder::function`] and
/// [`ResourceBuilder::method`].
///
/// ```text
/// interface.function(operation!(create_session));
/// interface.resource::<Session>().method(operation!(Session::inspect));
/// ```
#[cfg(feature = "derive")]
pub use wit_reflect_macros::operation;
