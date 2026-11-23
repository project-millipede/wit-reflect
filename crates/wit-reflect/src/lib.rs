//! Generate a WIT contract, and the code behind it, from ordinary Rust.
//!
//! The types and functions an implementation already has are the only
//! definition of its contract. Nothing restates a field or a signature: each
//! item carries a derive or a mark, and a generator writes the WIT file, the
//! bindings setup and the forwarding code from them.
//!
//! # In short
//!
//! In the crate that owns the types and operations:
//!
//! ```text
//! #[derive(WitType)]
//! #[wit(interface = "binary")]             // an interface has to own this type
//! pub struct Report {
//!     pub byte_length: u64,
//!     pub images: Vec<Image>,              // `Image` is found through this field
//! }
//!
//! #[wit_reflect::named]                    // names one instantiation of a generic type
//! pub type InspectionOutcome = Outcome<Report>;
//!
//! #[derive(WitResource)]
//! #[wit(interface = "native")]
//! pub struct Session { /* fields are not exported */ }
//!
//! #[wit_reflect::exports(inspect)]         // the exported functions of this block
//! impl Session {
//!     pub fn inspect(&mut self, resource: &str) -> InspectionOutcome { /* … */ }
//! }
//! ```
//!
//! In the `Cargo.toml` of the component crate:
//!
//! ```toml
//! [package.metadata.wit-reflect]
//! package = "example:session@0.1.0"
//! world = "session"
//! exports = ["native", "binary"]
//! ```
//!
//! The whole generator program:
//!
//! ```text
//! use owner_crate as _;   // links the crate whose items are marked
//!
//! fn main() -> std::process::ExitCode {
//!     wit_reflect::tasks::run("crates/component/Cargo.toml")
//! }
//! ```
//!
//! It writes `wit/session.wit`, `src/bindings.rs` and `src/forwarding.rs` into
//! the component crate; with `--check` it changes nothing and fails if one of
//! them is stale.
//!
//! # What you write
//!
//! | You write | On | It states |
//! | --- | --- | --- |
//! | [`#[derive(WitType)]`](macro@WitType) | a struct or enum | the type is part of the contract |
//! | [`#[derive(WitResource)]`](macro@WitResource) | a struct | the type is a resource |
//! | `#[wit(interface = "…")]` | a derived type or resource | the interface that owns it |
//! | [`#[exports(a, b)]`](macro@exports) | an `impl` block | the functions of the block that are exported |
//! | [`#[export(interface = "…")]`](macro@export) | a free function | the interface that exports it |
//! | [`#[named]`](macro@named) | a type alias | the name of that instantiation of a generic type |
//! | [`representations!`](macro@representations) | once in a crate | how each type of another crate is shown |
//! | a rule such as `#[wit(skip(…))]` | a type or a field | a representation that differs from what is stored |
//! | `[package.metadata.wit-reflect]` | the component crate's `Cargo.toml` | package, world, imports and exports |
//!
//! What is exported is always stated. It never follows from `pub`, from the
//! layout of an `impl` block or from the crate a type is in.
//!
//! # How types are found
//!
//! The derive runs in the crate that owns a type and reflects its fields as
//! declared. By then the Rust compiler has resolved aliases, re-exports,
//! dependency crates and generic arguments, so this crate never resolves a
//! Rust path itself. Each instantiation of a generic type is a separate
//! definition, identified by its Rust type rather than by its WIT name.
//!
//! Only a type an interface has to own names its interface. Every type it
//! refers to is found through it and belongs to the only interface that
//! reaches it.
//!
//! # How the marks become a contract
//!
//! Each mark registers its item when the program is linked.
//! [`Schema::from_registry`] collects the registrations of every linked crate
//! into a [`Schema`]; the [`registry`] module explains the linking. With the
//! `tasks` feature, the `tasks` module adds the settings from the component
//! crate's `Cargo.toml` and writes or checks the generated files.
//! [`Schema::resolve`] assigns each type one owning interface and one name,
//! validates the result, and reports every problem in one [`SchemaErrors`].
//!
//! # Types of other crates
//!
//! No derive can be placed on a dependency's type. A crate states once how
//! each such type is shown, and fields of those types then need no rule:
//!
//! ```text
//! wit_reflect::representations! {
//!     pub(crate) Representations {
//!         object {
//!             object::Endianness => Endianness,         // a local enum, checked by `remote`
//!             object::macho::FileType => MachoValue,    // a local record
//!             object::macho::Version => DisplayText,    // its `Display` text
//!         }
//!     }
//! }
//! ```
//!
//! The crate names the map in its own `Cargo.toml`
//! (`representations = "crate::Representations"`, with the `representations`
//! feature); see [`representations!`](macro@representations). `usize` and
//! `isize` need neither an entry nor a rule: they are shown as `u64` and
//! `s64`.
//!
//! # Rules
//!
//! A rule states a representation that differs from what is stored, for one
//! field or for the type. It never restates a field of the owner type.
//!
#![cfg_attr(feature = "derive", doc = "```")]
#![cfg_attr(not(feature = "derive"), doc = "```ignore")]
//! use std::fmt;
//! use wit_reflect::{Schema, WitType};
//!
//! #[derive(WitType)]
//! #[wit(display = "message", skip(attempts))]
//! struct ReadFailure {
//!     requested: usize,
//!     #[wit(display)]
//!     cause: std::io::Error,
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
//! `display = "message"` adds a field with the value's `Display` text, because
//! a method cannot cross a boundary. `skip(attempts)` keeps a field out of the
//! contract. `#[wit(display)]` shows one field as its text. The rules are
//! listed with the [`WitType`](macro@WitType) derive.
//!
//! # Building a component
//!
//! The derive also implements [`lower::Lower`], which hands a value to the
//! bindings with its rules applied. Next to the WIT file, the generator writes
//! two Rust files:
//!
//! - [`ResolvedPackage::bindings`]: the contract's part of the
//!   `wit_bindgen::generate!` call, as the macro `generate_bindings!`;
//! - [`ResolvedPackage::forwarding`]: the implementation of the traits
//!   wit-bindgen generates, which calls the exported operations and assembles
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
//! The three files are generated artifacts. Commit them and run the check in
//! your tests; the `fixtures/session` crates in the repository show the
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
//! - no field is a `usize`, an `isize` or a type the crate's map shows as
//!   another type;
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
//! # Declaring with a schema program
//!
//! The marks are a front end to [`Schema`]. A program can also declare a
//! contract call by call, for example to generate two contracts from the same
//! crates. Types need only the derive:
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
//! Operations are marked with [`macro@export`], which reflects the signature,
//! and listed with [`operation!`](macro@operation):
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
//! already defines, so they are imported instead of redefined. Only a schema
//! program can do that.
//!
//! # Features
//!
//! | Feature | Default | Enables |
//! | --- | --- | --- |
//! | `derive` | yes | the derives and the other macros |
//! | `tasks` | no | the `tasks` module: read the contract settings, write or check the generated files. For a generator program. |
//! | `representations` | no | reading the crate's map from its `Cargo.toml`. For a crate that names a map there. |
//!
//! The registry, the tasks and the map setting exist only on non-Wasm targets.
//! A component is built from the generated files and carries none of them.
//!
//! # Keeping ordinary builds independent
//!
//! Gate the macros behind a default-off feature of the owning crate, so its
//! ordinary build does not compile this crate. One `cfg_attr` takes several
//! attributes, and rules can be stated on the type, so a gated type needs one
//! block above it and nothing on its fields:
//!
//! ```text
//! #[cfg_attr(
//!     feature = "wit",
//!     derive(wit_reflect::WitType),
//!     wit(display = "message", skip(diagnostic, cause))
//! )]
//! pub struct Failure { /* plain fields */ }
//!
//! #[cfg_attr(feature = "wit", wit_reflect::exports(inspect))]
//! impl Session { /* plain functions */ }
//! ```
//!
//! # Current scope
//!
//! This crate generates the WIT contract and the code that connects it to the
//! Rust implementation, for wit-bindgen 0.63. Parameters are limited to plain
//! values and borrowed resources, and operations are synchronous. `docs/guide.md`
//! in the repository is the worked example; `docs/phases.md` records the
//! evidence and the limits of each phase.

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
#[cfg(not(target_family = "wasm"))]
pub mod registry;
mod render;
mod resolve;
pub mod rule;
mod schema;
#[cfg(all(feature = "tasks", not(target_family = "wasm")))]
pub mod tasks;
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
/// - `#[wit(interface = "…")]` on the item registers it as defined in that
///   interface, for [`Schema::from_registry`]. Only a type that an interface
///   has to own needs it; types found through it follow the placement rules
///   of [`Schema::resolve`]. A generic type cannot name an interface.
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
/// | `#[wit(widen)]` | `u64` for `usize`, `s64` for `isize` | see [`rule::Widen`]; optional, as these two types are widened without it |
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
/// # Rules stated on the item
///
/// `#[wit(skip(a, b))]` on a struct keeps the named fields out of the
/// contract, like `#[wit(skip)]` on each of them. Stating rules on the item
/// leaves the fields free of attributes, which matters where every attribute
/// is wrapped in `cfg_attr`. A name that is not a field, or a field with a
/// rule of its own, is a compile error.
///
/// # Computed fields
///
/// `#[wit(field(name = "…", into = W, with = function))]` on the item adds a
/// leading field of wire type `W` computed from the whole value.
/// `#[wit(display = "…")]` is the short form for the common case, a `string`
/// field holding the value's `Display` text: a method cannot cross the
/// boundary, so the text travels as data.
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
/// A foreign type that several fields hold needs the same rule on each of
/// them. [`representations!`](macro@representations) states it once for the
/// crate instead. `#[wit(map = path)]` on an item names the map its fields
/// without a rule are looked up in. A generic item cannot use a map.
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
/// handle. Its exported functions are named by [`macro@exports`] on their
/// `impl` blocks, or marked with [`macro@export`] and listed by a schema
/// program.
///
/// `#[wit(name = "…")]` sets the resource's name; the default is the Rust name
/// in kebab case. `#[wit(interface = "…")]` registers the resource as defined
/// in that interface, for [`Schema::from_registry`]; its functions are
/// registered by [`macro@exports`]. `#[wit(crate = path)]` names this crate
/// when it is re-exported elsewhere.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::WitResource;

/// Makes the signature of a function or method available to a [`Schema`].
///
/// The function itself is unchanged. Its name, parameter names, types and
/// documentation are reflected from the declaration. The mark alone exports
/// nothing: a function becomes part of a contract when it names an interface
/// (see "Registering" below), when [`macro@exports`] on its `impl` block
/// registers it, or when a schema program lists it with
/// [`operation!`](macro@operation).
///
/// A function of a resource usually needs no mark of its own:
/// `#[exports(a, b)]` on the block names the exported functions and applies
/// this mark to them.
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
/// # Registering
///
/// `#[export(interface = "…")]` on a free function also registers it as
/// exported from that interface, for [`Schema::from_registry`]. Generated
/// forwarding code calls it at the root of its crate, `owner::function`;
/// `path = owner::module::function` names another place. A function of a
/// resource is registered by [`macro@exports`] on its `impl` block.
///
/// Not exported: `async fn`, `unsafe fn`, generic functions, `impl Trait`,
/// methods that consume `self`, `&mut` parameters, `usize` and `isize`
/// parameters, pattern parameters and reference results. Each is a compile
/// error at the function. A `usize` result is exported as `u64`.
///
/// To keep the owning crate's ordinary build independent of this crate, apply
/// it as `#[cfg_attr(feature = "wit", wit_reflect::export)]`.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::export;

/// Registers the exported functions of an `impl` block, for
/// [`Schema::from_registry`].
///
/// ```text
/// #[wit_reflect::exports(select, inspect)]
/// impl Session {
///     pub fn select(&mut self, resource: &str) { /* … */ }
///
///     pub fn inspect(&mut self, resource: &str) -> InspectionOutcome { /* … */ }
///
///     pub fn read_at(&mut self, offset: u64) -> Vec<u8> { /* public, not exported */ }
/// }
/// ```
///
/// The attribute names the functions that are exported. Each one is reflected
/// as [`macro@export`] reflects it, so the same signatures are accepted and
/// rejected. What is exported never follows from `pub` or from the layout of
/// the block; a name that is not a function of the block is a compile error.
///
/// A function may carry [`macro@export`] itself instead of being listed. That
/// is the form for a function without `self`, which needs
/// `#[export(static)]` or `#[export(constructor)]`. A function marked
/// `#[cfg_attr(predicate, export)]` is registered under the same predicate.
/// Listing and marking the same function is an error.
///
/// The functions are exported from the interface that declares the resource,
/// in the order they are written in the block. A resource may have several
/// marked blocks.
///
/// The attribute belongs on the block because one on a function cannot
/// register anything: a registration is an item of its own, and an attribute
/// on a function can only add to its `impl` block.
///
/// Generated forwarding code names the resource at the root of its crate,
/// `owner::Session`; `#[exports(path = owner::module::Session)]` names another
/// place. `#[exports(crate = path)]` names this crate when it is re-exported
/// elsewhere.
///
/// A block that exports nothing, a trait implementation and a generic block
/// are compile errors.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::exports;

/// Registers the name of the type a type alias stands for, for
/// [`Schema::from_registry`].
///
/// WIT has no generic types of its own, so each instantiation of a generic
/// Rust type is a separate definition and needs a name. A type alias is where
/// Rust code names one:
///
/// ```text
/// #[wit_reflect::named]
/// pub type InspectionOutcome = Outcome<InspectionStepOutcome>;
/// ```
///
/// defines `Outcome<InspectionStepOutcome>` as `inspection-outcome`, in the
/// interface the placement rules of [`Schema::resolve`] give it. The mark is
/// needed because the compiler resolves an alias away: no derive and no trait
/// can see its name.
///
/// `#[named(name = "…")]` sets another name. `#[named(crate = path)]` names
/// this crate when it is re-exported elsewhere. An alias with parameters is a
/// compile error.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::named;

/// Declares a crate's map of representations: how each type of another
/// crate is shown in the contract.
///
/// A field of a foreign type needs a rule, and the same type needs the same
/// rule wherever it appears. The map states that fact once:
///
/// ```text
/// wit_reflect::representations! {
///     /// The contract's form of each foreign type the reports hold.
///     pub(crate) Representations {
///         object {
///             object::Endianness => Endianness,
///             object::macho::FileType => MachoValue,
///             object::macho::Version => DisplayText,
///         }
///         std {
///             std::io::Error => DisplayText,
///         }
///     }
/// }
/// ```
///
/// It defines the unit struct `Representations`. Each entry `Foreign => Shown`
/// says that the contract shows `Foreign` as the type `Shown`, and needs
/// `Shown: From<Foreign>`; a missing conversion is a compile error at the
/// entry. [`rule::DisplayText`] is the shown type for a value's `Display`
/// text.
///
/// Entries stand in groups named after the crate their types come from, and
/// an entry's path must begin with its group's name. The groups do nothing
/// else; they keep a growing map ordered.
///
/// A crate names its map once, in its `Cargo.toml`, and every derive of its
/// library then uses it:
///
/// ```toml
/// [package.metadata.wit-reflect]
/// representations = "crate::wit::Representations"
/// ```
///
/// Reading the manifest needs the `representations` feature of this crate;
/// enable it in the crate that has the setting. Without the setting, or for
/// another map, an item names one with `#[wit(map = Representations)]`.
///
/// Each field without a rule is then shown as the map's entry for the field's
/// type, or as the type itself if the map has none:
///
/// - A rule on a field takes precedence over the map.
/// - An entry for `T` also covers `Option<T>` and `Vec<T>`.
/// - Case payloads and the fields of struct-like cases are covered like the
///   fields of a struct.
/// - A type without an entry and without a derive is a compile error, as it
///   is without a map. Nothing is covered that the map does not name.
/// - A type whose fields have no entry crosses as it would without a map.
///
/// The compiler makes the choice for each field. Nothing is looked up when a
/// value crosses.
///
/// The setting applies to the package's library target. A generic item uses no
/// map, because the choice needs a concrete field type; a foreign field there
/// takes a rule.
#[cfg(feature = "derive")]
pub use wit_reflect_macros::representations;

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
