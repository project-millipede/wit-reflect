//! Declaring a WIT package: interfaces and the types they define.

use std::any::type_name;

use crate::error::SchemaErrors;
use crate::graph::TypeGraph;
use crate::package_name::PackageName;
use crate::reflect::WitType;
use crate::resolve::{self, ResolvedPackage};
use crate::types::Type;

/// The declaration of one WIT package.
///
/// A schema names interfaces and states which Rust types each one defines. It
/// does not restate fields: those are reflected from the types themselves.
///
/// Declarations are recorded as given. [`Schema::resolve`] is the single place
/// that validates them, so every problem is reported together.
#[derive(Debug)]
pub struct Schema {
    package: PackageName,
    graph: TypeGraph,
    interfaces: Vec<InterfaceDeclaration>,
}

#[derive(Debug)]
pub(crate) struct InterfaceDeclaration {
    pub(crate) name: String,
    pub(crate) declared: Vec<DeclaredType>,
}

/// One `define` or `define_as` call.
#[derive(Debug)]
pub(crate) struct DeclaredType {
    pub(crate) rust_name: &'static str,
    pub(crate) ty: Type,
    /// The declared WIT name, when it differs from the type's default.
    pub(crate) name: Option<String>,
}

impl Schema {
    /// Start an empty package declaration.
    #[must_use]
    pub fn new(package: PackageName) -> Self {
        Self {
            package,
            graph: TypeGraph::default(),
            interfaces: Vec::new(),
        }
    }

    /// Declare an interface, or continue one declared earlier under `name`.
    ///
    /// Interfaces are rendered in the order they are first declared.
    pub fn interface(&mut self, name: &str) -> InterfaceBuilder<'_> {
        let existing_position = self
            .interfaces
            .iter()
            .position(|interface| interface.name == name);
        let position = match existing_position {
            Some(position) => position,
            None => {
                self.interfaces.push(InterfaceDeclaration {
                    name: name.to_owned(),
                    declared: Vec::new(),
                });
                self.interfaces.len() - 1
            }
        };

        InterfaceBuilder {
            graph: &mut self.graph,
            interface: &mut self.interfaces[position],
        }
    }

    /// Validate the declarations and assign every type its name and interface.
    ///
    /// Each named type belongs to exactly one interface:
    ///
    /// 1. A type passed to `define` or `define_as` belongs to that interface.
    /// 2. An undeclared type belongs to the only interface that reaches it.
    /// 3. A type owned by another interface is imported with `use`.
    ///
    /// # Errors
    ///
    /// Returns every [`SchemaError`](crate::SchemaError) found: invalid or
    /// clashing names, a type reached from several interfaces without an
    /// owner, a type declared twice, recursive types, empty definitions, and
    /// unit values outside a `result`.
    pub fn resolve(&self) -> Result<ResolvedPackage, SchemaErrors> {
        resolve::resolve(self)
    }

    pub(crate) fn package(&self) -> &PackageName {
        &self.package
    }

    pub(crate) fn graph(&self) -> &TypeGraph {
        &self.graph
    }

    pub(crate) fn interfaces(&self) -> &[InterfaceDeclaration] {
        &self.interfaces
    }
}

/// Adds type definitions to one interface of a [`Schema`].
#[derive(Debug)]
pub struct InterfaceBuilder<'schema> {
    graph: &'schema mut TypeGraph,
    interface: &'schema mut InterfaceDeclaration,
}

impl InterfaceBuilder<'_> {
    /// Define `T` in this interface under its default name.
    ///
    /// The default is the derive's `#[wit(name = "…")]`, or the Rust name in
    /// kebab case. Types that `T` refers to are discovered automatically.
    pub fn define<T: WitType>(&mut self) -> &mut Self {
        self.declare::<T>(None)
    }

    /// Define `T` in this interface under `name`.
    ///
    /// Use this for instantiations of a generic type, which otherwise share
    /// one default name:
    ///
    /// ```text
    /// interface.define_as::<Outcome<InspectionStep>>("inspection-outcome");
    /// interface.define_as::<Outcome<SelectionStep>>("selection-outcome");
    /// ```
    ///
    /// A Rust type alias names the same instantiation as its expansion.
    pub fn define_as<T: WitType>(&mut self, name: &str) -> &mut Self {
        self.declare::<T>(Some(name.to_owned()))
    }

    fn declare<T: WitType>(&mut self, name: Option<String>) -> &mut Self {
        let ty = T::wit_type(self.graph);
        self.interface.declared.push(DeclaredType {
            rust_name: type_name::<T>(),
            ty,
            name,
        });
        self
    }
}
