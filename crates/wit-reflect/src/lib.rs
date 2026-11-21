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
//! # Representation rules
//!
//! Some fields cannot cross a boundary as stored: a dependency's type, a
//! `usize`, an error cause. A rule on the field names its wire type and how
//! the value is obtained, without restating any field of the owner type.
//!
#![cfg_attr(feature = "derive", doc = "```")]
#![cfg_attr(not(feature = "derive"), doc = "```ignore")]
//! use std::fmt;
//! use wit_reflect::{Schema, WitType};
//!
//! #[derive(WitType)]
//! #[wit(field(name = "message", into = String, with = ToString::to_string))]
//! struct ReadFailure {
//!     #[wit(widen)]
//!     requested: usize,
//!     #[wit(display)]
//!     cause: std::io::Error,
//!     #[wit(skip)]
//!     attempts: u8,
//! }
//!
//! impl fmt::Display for ReadFailure {
//!     fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
//!         write!(formatter, "cannot read {} bytes: {}", self.requested, self.cause)
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut schema = Schema::new("example:files@0.1.0".parse()?);
//! schema.interface("reading").define::<ReadFailure>();
//!
//! let package = schema.resolve()?;
//! let expected_wit = "\
//! package example:files@0.1.0;
//!
//! interface reading {
//!   record read-failure {
//!     message: string,
//!     requested: u64,
//!     cause: string,
//!   }
//! }
//! ";
//! assert_eq!(package.to_string(), expected_wit);
//! # Ok(())
//! # }
//! ```
//!
//! The rules are listed with the `WitType` derive.
//!
//! # Building a component
//!
//! The derive also implements [`lower::Lower`], which hands a value to the
//! bindings with its rules applied. From the same schema that renders the WIT,
//! two Rust files are rendered:
//!
//! - [`ResolvedPackage::bindings`]: the schema's part of the
//!   `wit_bindgen::generate!` call, as the macro `generate_bindings!`;
//! - [`ResolvedPackage::forwarding`]: the implementation of the traits
//!   wit-bindgen generates, which calls the listed operations and assembles
//!   their results.
//!
//! A component crate then holds no handwritten boundary code:
//!
//! ```text
//! include!("bindings.rs");
//! generate_bindings! { path: "wit" }
//!
//! #[rustfmt::skip]
//! mod forwarding;
//! use forwarding::Component;
//!
//! export!(Component);
//! ```
//!
//! The WIT, the bindings file and the forwarding file are generated artifacts.
//! Commit them, and keep a check that fails when they differ from what the
//! schema generates; the `fixtures/session` crates in the repository show the
//! arrangement.
//!
//! # Types that cross unchanged
//!
//! Most results are converted: taken apart in the owner crate and assembled
//! into the type wit-bindgen generates. A type needs no conversion when the
//! bindings can use it as it is. The bindings file then maps its WIT type to
//! the owner type with wit-bindgen's `with` option, and its values are moved.
//!
//! A struct or enum crosses unchanged when all of this holds:
//!
//! - it is `pub`, has no type parameters and is `Clone` and `Debug`;
//! - no `#[wit(...)]` attribute changes its shape, a value or a member's name;
//! - a struct has only `pub` fields; an enum has only unit cases without
//!   explicit discriminants;
//! - every type inside it crosses unchanged too.
//!
//! Nothing has to be declared. The derive checks what it can see on the item,
//! and the compiler decides the rest from the field types, so a type that
//! does not qualify is simply converted. `#[wit(convert)]` forces conversion.
//!
//! The mapping names the type at the root of its crate, `owner::Type`. A type
//! exported elsewhere declares `#[wit(path = owner::module::Type)]`; a path
//! that does not resolve is a compile error in the component crate.
//!
//! # Keeping ordinary builds independent
//!
//! Gate the derive behind a default-off feature of the owning crate, so its
//! ordinary build does not compile this crate:
//!
//! ```text
//! #[cfg_attr(feature = "wit", derive(wit_reflect::WitType))]
//! pub struct BinaryReport { /* fields */ }
//! ```
//!
//! # Current scope
//!
//! This crate generates the WIT contract and the code that connects it to the
//! Rust implementation, for wit-bindgen 0.63. Parameters are limited to plain
//! values and borrowed resources, and operations are synchronous. `docs/phases.md`
//! in the repository records the evidence and the limits of each phase.

mod bindings;
pub mod component;
mod error;
mod forwarding;
mod function;
mod graph;
mod identifier;
pub mod lower;
mod package_name;
mod reflect;
mod render;
mod resolve;
pub mod rule;
mod schema;
mod types;

pub use error::{SchemaError, SchemaErrors};
pub use forwarding::{ForwardingError, ForwardingErrors};
pub use function::{Function, Method, Operation, Param};
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
///
/// # Representation rules
///
/// A rule gives a field a wire type other than its own. A field takes at most
/// one. Each rule's requirement is checked by the compiler at the field.
///
/// | Rule | Wire type | Requirement |
/// | --- | --- | --- |
/// | `#[wit(display)]` | `string` | the field is `Display` |
/// | `#[wit(widen)]` | `u64` for `usize`, `s64` for `isize` | see [`rule::Widen`] |
/// | `#[wit(into = W)]` | `W`'s representation | `W: From<FieldType>` |
/// | `#[wit(into = W, with = function)]` | `W`'s representation | `function: fn(&Self) -> W` |
/// | `#[wit(skip)]` | none; the field is not exported | a named field |
///
/// `with` replaces the conversion of an `into` rule. Its function receives
/// the whole value, so the result may depend on other fields. It applies to
/// struct fields. `display`, `widen` and plain `into` also apply to the
/// payload of an enum case and to a transparent struct's field.
///
/// The derive implements [`lower::Lower`] from the same rules, so a value is
/// converted exactly as its WIT declares. A type parameter must therefore
/// implement both `WitType` and `Lower`. A type that contains itself is
/// rejected: WIT types cannot be recursive.
///
/// # Computed fields
///
/// `#[wit(field(name = "…", into = W, with = function))]` on the item adds a
/// leading field of wire type `W` computed from the whole value, for example
/// `into = String, with = ToString::to_string` for a `Display` message.
///
/// A WIT variant cannot hold a field next to its cases. An enum with computed
/// fields is therefore exported as a record, and `#[wit(cases = "…")]` names
/// the record field that holds the cases. They are defined as
/// `<record-name>-<cases-field>`.
///
/// # Types of other crates
///
/// No derive can be placed on a dependency's type. `#[wit(remote = path)]`
/// marks a local declaration of that type's shape. The derive reflects the
/// local declaration and generates `From<path>` for it with an exhaustive
/// match or a complete destructuring, so the declaration stops compiling when
/// the foreign type changes. Fields of the foreign type then use
/// `#[wit(into = Local)]`.
///
/// A foreign type that hides its fields, or an enum marked
/// `#[non_exhaustive]`, cannot be declared this way; convert it with `into`
/// or `with` instead.
///
/// # Crossing unchanged
///
/// The derive lets a type cross without conversion when generated bindings
/// can use it as it is; the crate documentation lists the conditions. Two
/// attributes on the item adjust that:
///
/// | Attribute | Effect |
/// | --- | --- |
/// | `#[wit(convert)]` | The type is always converted. |
/// | `#[wit(path = owner::module::Type)]` | Where the component crate names the type, if not at its crate's root. |
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

/// Derives [`lower::FromParts`] for a binding type generated by wit-bindgen.
///
/// It is not written by hand. The generated bindings file names it as an
/// `additional_derives` entry of `wit_bindgen::generate!`, so that every
/// generated record, variant and enum can be assembled from the wire parts of
/// an owner value; see [`ResolvedPackage::bindings`].
///
/// The expansion refers to this crate as `::wit_reflect`, so the component
/// crate depends on it under that name.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::FromParts;
