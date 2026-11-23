//! Declaring a WIT package: interfaces, their types and operations, and worlds.

use std::any::type_name;
use std::marker::PhantomData;

use crate::error::SchemaErrors;
use crate::function::{Function, Method, MethodKind, Operation};
use crate::graph::TypeGraph;
use crate::package_name::PackageName;
use crate::reflect::{Resource, WitType};
use crate::resolve::{self, ResolvedPackage};
use crate::types::Type;
#[cfg(all(feature = "tasks", not(target_family = "wasm")))]
use crate::types::TypeKey;

/// The declaration of one WIT package.
///
/// A schema names interfaces and states which Rust types and operations each
/// one exports. It does not restate fields or signatures: those are reflected
/// from the Rust declarations themselves.
///
/// Declarations are recorded as given. [`Schema::resolve`] is the single place
/// that validates them, so every problem is reported together.
///
/// Two declarations do not depend on the order they are given in:
/// [`Schema::name_instantiation`] and [`Schema::resource_method`]. Items that
/// register themselves use them, because each one knows only its own part of
/// the contract.
#[derive(Debug)]
pub struct Schema {
    package: PackageName,
    graph: TypeGraph,
    interfaces: Vec<InterfaceDeclaration>,
    external_interfaces: Vec<ExternalInterfaceDeclaration>,
    worlds: Vec<WorldDeclaration>,
    names: Vec<DeclaredName>,
    resource_methods: Vec<DeclaredResourceMethod>,
}

/// One `name_instantiation` call: a name that follows a type to its interface.
#[derive(Debug)]
pub(crate) struct DeclaredName {
    pub(crate) rust_name: &'static str,
    pub(crate) ty: Type,
    pub(crate) name: String,
}

/// One `resource_method` call: a function of a resource declared elsewhere.
#[derive(Debug)]
pub(crate) struct DeclaredResourceMethod {
    pub(crate) rust_name: &'static str,
    pub(crate) resource: Type,
    pub(crate) method: DeclaredMethod,
}

#[derive(Clone, Debug)]
pub(crate) struct InterfaceDeclaration {
    pub(crate) name: String,
    pub(crate) declared: Vec<DeclaredType>,
    pub(crate) functions: Vec<DeclaredFunction>,
}

/// One `function` call: a free function of an interface.
#[derive(Clone, Debug)]
pub(crate) struct DeclaredFunction {
    pub(crate) function: Function,
    /// The path that names the Rust function in generated forwarding code.
    pub(crate) rust_path: &'static str,
}

/// An interface of another WIT package, such as a pinned dependency.
#[derive(Debug)]
pub(crate) struct ExternalInterfaceDeclaration {
    /// `namespace:package/interface@version`
    pub(crate) path: String,
    pub(crate) declared: Vec<DeclaredType>,
}

/// One `define`, `define_as`, `resource` or `resource_as` call.
#[derive(Clone, Debug)]
pub(crate) struct DeclaredType {
    pub(crate) rust_name: &'static str,
    pub(crate) ty: Type,
    /// The declared WIT name, when it differs from the type's default.
    pub(crate) name: Option<String>,
    /// The exported functions of a resource. Empty for other types.
    pub(crate) methods: Vec<DeclaredMethod>,
}

#[derive(Clone, Debug)]
pub(crate) struct DeclaredMethod {
    pub(crate) kind: MethodKind,
    pub(crate) function: Function,
    /// The path that names the Rust function in generated forwarding code.
    pub(crate) rust_path: &'static str,
}

#[derive(Debug)]
pub(crate) struct WorldDeclaration {
    pub(crate) name: String,
    pub(crate) items: Vec<WorldItem>,
}

#[derive(Debug)]
pub(crate) struct WorldItem {
    pub(crate) direction: Direction,
    /// The name the world gives the item, when it differs from the target's.
    pub(crate) label: Option<String>,
    /// A declared interface's name, or the path of an external interface.
    pub(crate) target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Direction {
    Import,
    Export,
}

impl Schema {
    /// Start an empty package declaration.
    #[must_use]
    pub fn new(package: PackageName) -> Self {
        Self {
            package,
            graph: TypeGraph::default(),
            interfaces: Vec::new(),
            external_interfaces: Vec::new(),
            worlds: Vec::new(),
            names: Vec::new(),
            resource_methods: Vec::new(),
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
                    functions: Vec::new(),
                });
                self.interfaces.len() - 1
            }
        };

        InterfaceBuilder {
            graph: &mut self.graph,
            interface: &mut self.interfaces[position],
        }
    }

    /// Declare which types an interface of another package already defines.
    ///
    /// `path` names the interface as WIT does, for example
    /// `apple-code:filesystem/source@0.2.0`. Types placed here are imported
    /// with `use` wherever they are referenced and are never defined in this
    /// package. Calling it again with the same path continues the declaration.
    ///
    /// The Rust type's shape is not compared with the external definition.
    pub fn external_interface(&mut self, path: &str) -> ExternalInterfaceBuilder<'_> {
        let existing_position = self
            .external_interfaces
            .iter()
            .position(|interface| interface.path == path);
        let position = match existing_position {
            Some(position) => position,
            None => {
                self.external_interfaces.push(ExternalInterfaceDeclaration {
                    path: path.to_owned(),
                    declared: Vec::new(),
                });
                self.external_interfaces.len() - 1
            }
        };

        ExternalInterfaceBuilder {
            graph: &mut self.graph,
            declared: &mut self.external_interfaces[position].declared,
        }
    }

    /// Declare a world, or continue one declared earlier under `name`.
    ///
    /// Worlds are rendered after the interfaces, in declaration order.
    pub fn world(&mut self, name: &str) -> WorldBuilder<'_> {
        let existing_position = self.worlds.iter().position(|world| world.name == name);
        let position = match existing_position {
            Some(position) => position,
            None => {
                self.worlds.push(WorldDeclaration {
                    name: name.to_owned(),
                    items: Vec::new(),
                });
                self.worlds.len() - 1
            }
        };

        WorldBuilder {
            world: &mut self.worlds[position],
        }
    }

    /// Name `T` in whichever interface defines it.
    ///
    /// Use this for an instantiation of a generic type when the place that
    /// knows its name does not know its interface, such as a marked type
    /// alias:
    ///
    /// ```text
    /// schema.name_instantiation::<Outcome<InspectionStep>>("inspection-outcome");
    /// ```
    ///
    /// The type is placed by the usual rules. A name given with
    /// [`InterfaceBuilder::define_as`] takes precedence. A name for a type the
    /// package never reaches has no effect.
    pub fn name_instantiation<T: WitType>(&mut self, name: &str) -> &mut Self {
        let ty = T::wit_type(&mut self.graph);
        self.names.push(DeclaredName {
            rust_name: type_name::<T>(),
            ty,
            name: name.to_owned(),
        });
        self
    }

    /// Export a function of the resource `R` from the interface `R` belongs
    /// to: the one that declares it, or else the only one that reaches it.
    /// That interface may be stated before or after this call.
    ///
    /// Functions recorded here follow the ones listed with
    /// [`ResourceBuilder::method`], in the order of the calls.
    pub fn resource_method<R: Resource>(&mut self, operation: Operation<Method<R>>) -> &mut Self {
        let resource = R::wit_type(&mut self.graph);
        let method = (operation.reflect)(&mut self.graph);
        self.resource_methods.push(DeclaredResourceMethod {
            rust_name: type_name::<R>(),
            resource,
            method: DeclaredMethod {
                kind: method.kind,
                function: method.function,
                rust_path: operation.rust_path,
            },
        });
        self
    }

    /// Validate the declarations and assign every type its name and interface.
    ///
    /// Each named type belongs to exactly one interface:
    ///
    /// 1. A declared type belongs to the interface that declares it.
    /// 2. An undeclared type, a resource included, belongs to the only
    ///    interface that reaches it, through a field or an operation's
    ///    signature.
    /// 3. A type owned by another interface is imported with `use`.
    ///
    /// A type reachable only through a type of an external interface is not
    /// part of this package and is neither defined nor checked.
    ///
    /// # Errors
    ///
    /// Returns every [`SchemaError`](crate::SchemaError) found: invalid or
    /// clashing names, a type reached from several interfaces without an
    /// owner, a type declared twice or named twice, recursive types, empty
    /// definitions, unit values outside a `result`, invalid constructors,
    /// functions of a resource no interface declares or reaches, and world
    /// items that refer to nothing.
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

    pub(crate) fn external_interfaces(&self) -> &[ExternalInterfaceDeclaration] {
        &self.external_interfaces
    }

    pub(crate) fn worlds(&self) -> &[WorldDeclaration] {
        &self.worlds
    }

    pub(crate) fn names(&self) -> &[DeclaredName] {
        &self.names
    }

    /// Export from `interface` a free function whose signature is stated
    /// instead of reflected.
    #[cfg(all(feature = "tasks", not(target_family = "wasm")))]
    pub(crate) fn declare_function(
        &mut self,
        interface: &str,
        function: Function,
        rust_path: &'static str,
    ) {
        let builder = self.interface(interface);
        builder.interface.functions.push(DeclaredFunction {
            function,
            rust_path,
        });
    }

    /// A reference to every reflected type that `name` names: a name given
    /// with [`Schema::name_instantiation`] or `define_as`, or else the type's
    /// default name.
    #[cfg(all(feature = "tasks", not(target_family = "wasm")))]
    pub(crate) fn types_named(&self, name: &str) -> Vec<Type> {
        let declared_names = self
            .interfaces
            .iter()
            .flat_map(|interface| &interface.declared)
            .filter_map(|declared| Some((&declared.ty, declared.name.as_deref()?)));
        let given_names = self
            .names
            .iter()
            .map(|declared| (&declared.ty, declared.name.as_str()));
        let explicit_names: Vec<(TypeKey, &str)> = declared_names
            .chain(given_names)
            .filter_map(|(ty, given_name)| Some((ty.definition_key()?, given_name)))
            .collect();

        let mut types = Vec::new();
        for (key, definition) in self.graph.definitions() {
            let explicit_name = explicit_names
                .iter()
                .find(|(named_key, _)| named_key == key)
                .map(|(_, given_name)| *given_name);
            if explicit_name.unwrap_or(definition.name()) == name {
                types.push(self.graph.reference(*key));
            }
        }
        types
    }

    pub(crate) fn resource_methods(&self) -> &[DeclaredResourceMethod] {
        &self.resource_methods
    }
}

/// Reflect `T` and record it as declared by one interface.
fn declare<'declared, T: WitType>(
    graph: &mut TypeGraph,
    declared: &'declared mut Vec<DeclaredType>,
    name: Option<&str>,
) -> &'declared mut DeclaredType {
    let ty = T::wit_type(graph);
    declared.push(DeclaredType {
        rust_name: type_name::<T>(),
        ty,
        name: name.map(str::to_owned),
        methods: Vec::new(),
    });
    let position = declared.len() - 1;
    &mut declared[position]
}

/// Adds types and operations to one interface of a [`Schema`].
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
        declare::<T>(self.graph, &mut self.interface.declared, None);
        self
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
        declare::<T>(self.graph, &mut self.interface.declared, Some(name));
        self
    }

    /// Export a free function from this interface.
    ///
    /// Pass the function with `operation!`, which refers
    /// to the signature that `#[wit_reflect::export]` reflected:
    ///
    /// ```text
    /// interface.function(operation!(create_session));
    /// ```
    ///
    /// Functions are rendered after the interface's types, in this order.
    pub fn function(&mut self, operation: Operation<Function>) -> &mut Self {
        let function = (operation.reflect)(self.graph);
        self.interface.functions.push(DeclaredFunction {
            function,
            rust_path: operation.rust_path,
        });
        self
    }

    /// Define the resource `R` in this interface under its default name.
    ///
    /// The returned builder selects which of its functions are exported. A
    /// function that is not listed is not part of the contract.
    pub fn resource<R: Resource>(&mut self) -> ResourceBuilder<'_, R> {
        self.declare_resource(None)
    }

    /// Define the resource `R` in this interface under `name`.
    pub fn resource_as<R: Resource>(&mut self, name: &str) -> ResourceBuilder<'_, R> {
        self.declare_resource(Some(name))
    }

    fn declare_resource<R: Resource>(&mut self, name: Option<&str>) -> ResourceBuilder<'_, R> {
        let resource = declare::<R>(self.graph, &mut self.interface.declared, name);
        ResourceBuilder {
            graph: self.graph,
            resource,
            rust_type: PhantomData,
        }
    }
}

/// Selects the exported functions of the resource `R`.
#[derive(Debug)]
pub struct ResourceBuilder<'schema, R: ?Sized> {
    graph: &'schema mut TypeGraph,
    resource: &'schema mut DeclaredType,
    rust_type: PhantomData<fn() -> R>,
}

impl<R: Resource> ResourceBuilder<'_, R> {
    /// Export a method, static function or constructor of `R`.
    ///
    /// Pass it with `operation!`:
    ///
    /// ```text
    /// interface.resource::<Session>().method(operation!(Session::inspect));
    /// ```
    ///
    /// Only functions declared in an `impl` block of `R` are accepted.
    /// Functions are rendered in this order.
    pub fn method(&mut self, operation: Operation<Method<R>>) -> &mut Self {
        let method = (operation.reflect)(self.graph);
        self.resource.methods.push(DeclaredMethod {
            kind: method.kind,
            function: method.function,
            rust_path: operation.rust_path,
        });
        self
    }
}

/// Places types in an interface that another package defines.
#[derive(Debug)]
pub struct ExternalInterfaceBuilder<'schema> {
    graph: &'schema mut TypeGraph,
    declared: &'schema mut Vec<DeclaredType>,
}

impl ExternalInterfaceBuilder<'_> {
    /// State that the external interface defines `T` under its default name.
    pub fn define<T: WitType>(&mut self) -> &mut Self {
        declare::<T>(self.graph, self.declared, None);
        self
    }

    /// State that the external interface defines `T` under `name`.
    pub fn define_as<T: WitType>(&mut self, name: &str) -> &mut Self {
        declare::<T>(self.graph, self.declared, Some(name));
        self
    }
}

/// Adds imports and exports to one world of a [`Schema`].
///
/// A target is the name of an interface declared in this schema, or the path
/// of an external interface such as `polymorph:websocket/connections@0.1.0`.
/// Items are rendered in the order they are added.
#[derive(Debug)]
pub struct WorldBuilder<'schema> {
    world: &'schema mut WorldDeclaration,
}

impl WorldBuilder<'_> {
    /// `import <target>;`
    pub fn import(&mut self, target: &str) -> &mut Self {
        self.add(Direction::Import, None, target)
    }

    /// `export <target>;`
    pub fn export(&mut self, target: &str) -> &mut Self {
        self.add(Direction::Export, None, target)
    }

    /// `export <label>: <target>;`
    ///
    /// The component export is named `label` instead of the interface's
    /// versioned identity.
    pub fn export_as(&mut self, label: &str, target: &str) -> &mut Self {
        self.add(Direction::Export, Some(label), target)
    }

    fn add(&mut self, direction: Direction, label: Option<&str>, target: &str) -> &mut Self {
        self.world.items.push(WorldItem {
            direction,
            label: label.map(str::to_owned),
            target: target.to_owned(),
        });
        self
    }
}
