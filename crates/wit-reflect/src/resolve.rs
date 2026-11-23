//! Resolution: validate a declared schema and give every type one owner and name.
//!
//! This module owns the rules the reflected model cannot express on its own:
//! which interface defines a type, which name it gets, and which declarations
//! are not valid WIT. Rendering consumes its result without further checks.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::error::{SchemaError, SchemaErrors};
use crate::function::{Function, MethodKind};
use crate::graph::TypeGraph;
use crate::identifier::{self, Escaped};
use crate::package_name::{self, PackageName};
use crate::schema::{
    DeclaredMethod, DeclaredName, DeclaredResourceMethod, DeclaredType, Direction,
    ExternalInterfaceDeclaration, InterfaceDeclaration, Schema, WorldDeclaration,
};
use crate::types::{Case, Field, Payload, Type, TypeDef, TypeDefKind, TypeKey};

/// A validated WIT package.
///
/// Its [`Display`](fmt::Display) implementation writes the package as WIT
/// source text. Equal declarations always produce identical text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub(crate) name: PackageName,
    pub(crate) interfaces: Vec<Interface>,
    pub(crate) worlds: Vec<World>,
    /// The resolved WIT name of every named type. Used for lookup only.
    pub(crate) type_names: HashMap<TypeKey, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Interface {
    pub(crate) name: String,
    pub(crate) uses: Vec<Use>,
    pub(crate) definitions: Vec<Definition>,
    pub(crate) functions: Vec<ResolvedFunction>,
}

/// `use <source>.{<names>};`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Use {
    /// The owning interface as WIT source text.
    pub(crate) source: String,
    pub(crate) names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Definition {
    /// The Rust type this definition was reflected from. `None` for a payload
    /// record or the cases of a record, which resolution derives.
    pub(crate) key: Option<TypeKey>,
    pub(crate) name: String,
    pub(crate) docs: Option<&'static str>,
    /// Where a component crate names the owner type, when the binding uses
    /// that type itself instead of defining its own.
    pub(crate) owner_path: Option<&'static str>,
    pub(crate) body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Body {
    Record(Vec<ResolvedField>),
    Variant(Vec<ResolvedCase>),
    /// Cases never carry a payload.
    Enum(Vec<ResolvedCase>),
    Resource(Vec<ResolvedFunction>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedField {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
    /// The type as WIT source text.
    pub(crate) ty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedCase {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
    /// The payload type as WIT source text.
    pub(crate) payload: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedFunction {
    pub(crate) name: &'static str,
    pub(crate) docs: Option<&'static str>,
    pub(crate) kind: FunctionKind,
    pub(crate) params: Vec<ResolvedParam>,
    /// The result type as WIT source text. `None` when nothing is returned.
    pub(crate) result: Option<String>,
    /// The reflected result type, for generated forwarding code.
    pub(crate) result_type: Type,
    /// The path that names the Rust function in generated forwarding code.
    pub(crate) rust_path: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FunctionKind {
    /// A function of an interface.
    Free,
    /// A function called on a borrowed resource.
    Method {
        /// Whether the Rust method takes `&mut self`.
        exclusive: bool,
    },
    Static,
    Constructor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedParam {
    pub(crate) name: &'static str,
    /// The type as WIT source text.
    pub(crate) ty: String,
    /// The reflected type, for generated forwarding code.
    pub(crate) reflected: Type,
    /// Whether the Rust function takes the argument behind a reference.
    pub(crate) by_reference: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct World {
    pub(crate) name: String,
    pub(crate) items: Vec<ResolvedWorldItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedWorldItem {
    pub(crate) direction: Direction,
    pub(crate) label: Option<String>,
    /// The imported or exported interface as WIT source text.
    pub(crate) target: String,
    /// Position of the interface when it belongs to this package.
    pub(crate) interface: Option<usize>,
}

impl ResolvedPackage {
    pub(crate) fn world(&self, name: &str) -> Option<&World> {
        self.worlds.iter().find(|declared| declared.name == name)
    }

    /// The owner path of every type that generated bindings use unchanged.
    pub(crate) fn owner_paths(&self) -> HashMap<TypeKey, &'static str> {
        self.interfaces
            .iter()
            .flat_map(|interface| &interface.definitions)
            .filter_map(|definition| Some((definition.key?, definition.owner_path?)))
            .collect()
    }
}

pub(crate) fn resolve(schema: &Schema) -> Result<ResolvedPackage, SchemaErrors> {
    let graph = schema.graph();
    let externals = schema.external_interfaces();

    // 1. Check declarations and place types. These problems are independent
    //    of each other, so they are reported together.
    let mut declaration_errors = Vec::new();
    let declarations = &attach_resource_methods(
        graph,
        schema.interfaces(),
        externals,
        schema.resource_methods(),
        &mut declaration_errors,
    );
    report_cycles(graph, &mut declaration_errors);
    check_package_items(declarations, schema.worlds(), &mut declaration_errors);
    check_external_paths(externals, &mut declaration_errors);
    check_operations(graph, declarations, &mut declaration_errors);
    let mut placement = Placement::assign(graph, declarations, externals, &mut declaration_errors);
    placement.apply_names(schema.names(), &mut declaration_errors);
    check_definitions(graph, &placement, &mut declaration_errors);
    let worlds = resolve_worlds(schema.worlds(), declarations, &mut declaration_errors);
    SchemaErrors::check(declaration_errors)?;

    // 2. Resolve names. A namespace is meaningful only once every type has
    //    exactly one owner, so this runs after placement succeeded.
    let names = Names { graph, placement };
    let mut naming_errors = Vec::new();
    check_external_type_names(externals, &names, &mut naming_errors);
    let mut interfaces = Vec::with_capacity(declarations.len());
    for position in 0..declarations.len() {
        let scope = InterfaceScope {
            names: &names,
            namespace: Namespace::default(),
            definitions: Vec::new(),
            errors: &mut naming_errors,
        };
        let interface = scope.resolve(position, declarations, externals);
        interfaces.push(interface);
    }
    SchemaErrors::check(naming_errors)?;

    Ok(ResolvedPackage {
        name: schema.package().clone(),
        interfaces,
        worlds,
        type_names: names.type_names(),
    })
}

/// The interfaces with every separately recorded resource function added to
/// the declaration of its resource.
///
/// A resource that no interface declares is placed as any other type is: it
/// belongs to the only interface that reaches it, usually through the
/// function that returns it.
fn attach_resource_methods(
    graph: &TypeGraph,
    declarations: &[InterfaceDeclaration],
    externals: &[ExternalInterfaceDeclaration],
    resource_methods: &[DeclaredResourceMethod],
    errors: &mut Vec<SchemaError>,
) -> Vec<InterfaceDeclaration> {
    let mut declarations = declarations.to_vec();

    // 1. Functions of a declared resource join its declaration. The others
    //    are gathered per resource, in the order recorded.
    let mut undeclared: Vec<DeclaredType> = Vec::new();
    for recorded in resource_methods {
        let resource_key = recorded.resource.definition_key();
        let declaration = declarations
            .iter_mut()
            .flat_map(|declaration| &mut declaration.declared)
            .chain(&mut undeclared)
            .find(|declared| declared.ty.definition_key() == resource_key);
        match declaration {
            Some(declaration) => declaration.methods.push(recorded.method.clone()),
            None => undeclared.push(DeclaredType {
                rust_name: recorded.rust_name,
                ty: recorded.resource.clone(),
                name: None,
                methods: vec![recorded.method.clone()],
            }),
        }
    }

    // 2. An undeclared resource joins the only interface that reaches it. Its
    //    own functions can reach another resource, so this repeats until
    //    nothing more is placed.
    let mut is_placing = true;
    while is_placing {
        is_placing = false;
        let mut unplaced = Vec::new();
        for resource in undeclared {
            let reaching = reaching_interfaces(graph, &declarations, externals, &resource);
            if let [position] = reaching.as_slice() {
                declarations[*position].declared.push(resource);
                is_placing = true;
            } else {
                unplaced.push(resource);
            }
        }
        undeclared = unplaced;
    }

    // 3. A resource that nothing reaches is reported here. One that several
    //    interfaces reach is reported with the other types, by placement.
    for resource in &undeclared {
        if reaching_interfaces(graph, &declarations, externals, resource).is_empty() {
            errors.push(SchemaError::UndeclaredResource {
                resource: resource.rust_name,
            });
        }
    }
    declarations
}

/// The positions of the interfaces that reach `resource`: through the
/// signature of a function, or through a type such a signature refers to.
///
/// The walk is the one `Placement::assign` makes. It stops at a type another
/// interface declares, whose references belong to that interface.
fn reaching_interfaces(
    graph: &TypeGraph,
    declarations: &[InterfaceDeclaration],
    externals: &[ExternalInterfaceDeclaration],
    resource: &DeclaredType,
) -> Vec<usize> {
    let Some(target) = resource.ty.definition_key() else {
        return Vec::new();
    };
    let declares = |declared: &[DeclaredType], key: TypeKey| {
        declared
            .iter()
            .any(|declared| declared.ty.definition_key() == Some(key))
    };

    let mut reaching = Vec::new();
    for (position, declaration) in declarations.iter().enumerate() {
        let is_declared_elsewhere = |key: TypeKey| {
            let by_another_interface =
                declarations.iter().enumerate().any(|(other, declaration)| {
                    other != position && declares(&declaration.declared, key)
                });
            by_another_interface
                || externals
                    .iter()
                    .any(|external| declares(&external.declared, key))
        };
        let mut visited = HashSet::new();
        let mut pending = interface_roots(declaration);
        while let Some(key) = pending.pop() {
            if key == target {
                reaching.push(position);
                break;
            }
            if visited.insert(key) && !is_declared_elsewhere(key) {
                collect_references(graph.definition(key), &mut pending);
            }
        }
    }
    reaching
}

fn report_cycles(graph: &TypeGraph, errors: &mut Vec<SchemaError>) {
    for cycle in graph.cycles() {
        errors.push(SchemaError::RecursiveType {
            cycle: cycle.clone(),
        });
    }
}

/// Interfaces and worlds share the package's namespace.
fn check_package_items(
    declarations: &[InterfaceDeclaration],
    worlds: &[WorldDeclaration],
    errors: &mut Vec<SchemaError>,
) {
    for declaration in declarations {
        check_name(&declaration.name, || "an interface".to_owned(), errors);
    }
    for world in worlds {
        check_name(&world.name, || "a world".to_owned(), errors);
        let names_an_interface = declarations
            .iter()
            .any(|declaration| declaration.name == world.name);
        if names_an_interface {
            errors.push(SchemaError::DuplicatePackageItem {
                name: world.name.clone(),
            });
        }
    }
}

fn check_external_paths(externals: &[ExternalInterfaceDeclaration], errors: &mut Vec<SchemaError>) {
    for external in externals {
        check_interface_path(&external.path, errors);
    }
}

fn check_interface_path(path: &str, errors: &mut Vec<SchemaError>) {
    if let Err(reason) = package_name::validate_interface_path(path) {
        errors.push(SchemaError::InvalidInterfacePath {
            path: path.to_owned(),
            reason,
        });
    }
}

fn check_name(name: &str, subject: impl FnOnce() -> String, errors: &mut Vec<SchemaError>) {
    if let Err(reason) = identifier::validate(name) {
        errors.push(SchemaError::InvalidIdentifier {
            name: name.to_owned(),
            subject: subject(),
            reason,
        });
    }
}

/// Check the functions of every interface and the functions of its resources.
fn check_operations(
    graph: &TypeGraph,
    declarations: &[InterfaceDeclaration],
    errors: &mut Vec<SchemaError>,
) {
    for declaration in declarations {
        for declared in &declaration.functions {
            let function = &declared.function;
            let function_label = format!(
                "function `{}` of interface `{}`",
                function.name, declaration.name
            );
            check_function(&function_label, function, errors);
        }
        for declared in &declaration.declared {
            check_methods(graph, declared, errors);
        }
    }
}

fn check_methods(graph: &TypeGraph, declared: &DeclaredType, errors: &mut Vec<SchemaError>) {
    if declared.methods.is_empty() {
        return;
    }
    let is_resource = declared.ty.definition_key().is_some_and(|key| {
        let definition = graph.definition(key);
        matches!(definition.kind(), TypeDefKind::Resource)
    });
    if !is_resource {
        errors.push(SchemaError::MethodsOnNonResource {
            rust_type: declared.rust_name,
        });
        return;
    }

    let mut seen_names = HashSet::new();
    let mut constructor_count = 0_usize;
    for method in &declared.methods {
        let function = &method.function;
        let function_label = format!("function `{}` of `{}`", function.name, declared.rust_name);
        check_function(&function_label, function, errors);

        if method.kind == MethodKind::Constructor {
            // A constructor is written without a name, so it claims none.
            constructor_count += 1;
            if function.result != declared.ty {
                errors.push(SchemaError::ConstructorResult {
                    resource: declared.rust_name,
                });
            }
            continue;
        }
        let is_first_use = seen_names.insert(function.name);
        if !is_first_use {
            errors.push(SchemaError::DuplicateMember {
                rust_type: declared.rust_name.to_owned(),
                name: function.name,
            });
        }
    }
    if constructor_count > 1 {
        errors.push(SchemaError::MultipleConstructors {
            resource: declared.rust_name,
        });
    }
}

/// A function's own name, its parameter names and the values it exchanges.
fn check_function(function_label: &str, function: &Function, errors: &mut Vec<SchemaError>) {
    check_name(function.name, || function_label.to_owned(), errors);

    let mut seen_parameters = HashSet::new();
    let mut exchanges_misplaced_unit = false;
    for param in &function.params {
        check_name(
            param.name,
            || format!("a parameter of {function_label}"),
            errors,
        );
        let is_first_use = seen_parameters.insert(param.name);
        if !is_first_use {
            errors.push(SchemaError::DuplicateParameter {
                function: function_label.to_owned(),
                name: param.name,
            });
        }
        exchanges_misplaced_unit |= has_misplaced_unit(&param.ty);
    }

    // A unit result means "returns nothing"; unit nested in the result does not.
    if !function.result.is_unit() {
        exchanges_misplaced_unit |= has_misplaced_unit(&function.result);
    }
    if exchanges_misplaced_unit {
        errors.push(SchemaError::UnitOutsideResult {
            subject: function_label.to_owned(),
        });
    }
}

/// Check the shape of every definition this package would define.
fn check_definitions(graph: &TypeGraph, placement: &Placement<'_>, errors: &mut Vec<SchemaError>) {
    for (key, definition) in graph.definitions() {
        if !placement.reached_locally.contains(key) {
            continue;
        }
        let rust_type = key.rust_name();
        match definition.kind() {
            TypeDefKind::Record(fields) => check_fields(rust_type, fields, errors),
            TypeDefKind::Variant(cases) => {
                let case_names = cases.iter().map(|case| case.name);
                check_members(rust_type, "case", case_names, errors);
                for case in cases {
                    check_payload(rust_type, case, errors);
                }
            }
            TypeDefKind::Enum(cases) => {
                let case_names = cases.iter().map(|case| case.name);
                check_members(rust_type, "case", case_names, errors);
            }
            TypeDefKind::RecordOfCases {
                fields,
                cases_field,
                cases,
            } => {
                // The field holding the cases is a member of the record too.
                let field_names = fields.iter().map(|field| field.name).chain([*cases_field]);
                check_members(rust_type, "field", field_names, errors);
                for field in fields {
                    check_value_type(rust_type, &field.ty, errors);
                }
                let case_names = cases.iter().map(|case| case.name);
                check_members(rust_type, "case", case_names, errors);
                for case in cases {
                    check_payload(rust_type, case, errors);
                }
            }
            // A resource has no members; its functions are checked with the
            // interface that declares them.
            TypeDefKind::Resource => {}
        }
    }
}

fn check_payload(rust_type: &str, case: &Case, errors: &mut Vec<SchemaError>) {
    match &case.payload {
        Payload::None => {}
        Payload::Type(ty) => check_value_type(rust_type, ty, errors),
        Payload::Record { fields, .. } => {
            let case_owner = format!("{rust_type}::{}", case.name);
            check_fields(&case_owner, fields, errors);
        }
    }
}

fn check_fields(owner: &str, fields: &[Field], errors: &mut Vec<SchemaError>) {
    let field_names = fields.iter().map(|field| field.name);
    check_members(owner, "field", field_names, errors);
    for field in fields {
        check_value_type(owner, &field.ty, errors);
    }
}

/// A definition needs at least one member, and member names must be distinct
/// WIT identifiers. Distinct Rust identifiers can share one kebab-case name.
fn check_members(
    owner: &str,
    member_kind: &str,
    names: impl Iterator<Item = &'static str>,
    errors: &mut Vec<SchemaError>,
) {
    let mut seen_names = HashSet::new();
    for name in names {
        check_name(name, || format!("a {member_kind} of `{owner}`"), errors);
        let is_first_use = seen_names.insert(name);
        if !is_first_use {
            errors.push(SchemaError::DuplicateMember {
                rust_type: owner.to_owned(),
                name,
            });
        }
    }
    if seen_names.is_empty() {
        errors.push(SchemaError::EmptyDefinition {
            rust_type: owner.to_owned(),
        });
    }
}

fn check_value_type(owner: &str, ty: &Type, errors: &mut Vec<SchemaError>) {
    if has_misplaced_unit(ty) {
        errors.push(SchemaError::UnitOutsideResult {
            subject: format!("`{owner}`"),
        });
    }
}

/// Whether `ty` contains the unit type anywhere but directly as a `result` side.
fn has_misplaced_unit(ty: &Type) -> bool {
    match ty {
        Type::Tuple(elements) => elements.is_empty() || elements.iter().any(has_misplaced_unit),
        Type::List(inner) | Type::Option(inner) => has_misplaced_unit(inner),
        Type::Result { ok, err } => {
            let is_misplaced_in = |side: &Type| !side.is_unit() && has_misplaced_unit(side);
            is_misplaced_in(ok) || is_misplaced_in(err)
        }
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::Char
        | Type::String
        | Type::Named(_)
        | Type::Own(_)
        | Type::Borrow(_) => false,
    }
}

/// Where a named type is defined. Local interfaces sort before external ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Location {
    /// Position of an interface of this package, in declaration order.
    Local(usize),
    /// Position of an external interface, in declaration order.
    External(usize),
}

#[derive(Clone, Copy)]
struct Owner<'schema> {
    location: Location,
    declared_name: Option<&'schema str>,
}

/// Which interface owns each named type, and what each interface imports.
struct Placement<'schema> {
    owners: HashMap<TypeKey, Owner<'schema>>,
    /// Per local interface: referenced types that another interface owns.
    imports: Vec<Vec<(TypeKey, Location)>>,
    /// Types that some local interface defines or would define. Types reached
    /// only through an external type are not part of this package.
    reached_locally: HashSet<TypeKey>,
}

impl<'schema> Placement<'schema> {
    fn assign(
        graph: &TypeGraph,
        declarations: &'schema [InterfaceDeclaration],
        externals: &'schema [ExternalInterfaceDeclaration],
        errors: &mut Vec<SchemaError>,
    ) -> Self {
        let describe = |location: Location| match location {
            Location::Local(position) => declarations[position].name.clone(),
            Location::External(position) => externals[position].path.clone(),
        };

        // 1. A declared type belongs to the interface that declares it.
        let local_declarations = declarations
            .iter()
            .enumerate()
            .map(|(position, declaration)| (Location::Local(position), &declaration.declared));
        let external_declarations = externals
            .iter()
            .enumerate()
            .map(|(position, external)| (Location::External(position), &external.declared));
        let mut owners: HashMap<TypeKey, Owner<'schema>> = HashMap::new();
        for (location, declared_types) in local_declarations.chain(external_declarations) {
            for declared in declared_types {
                let Some(key) = declared.ty.definition_key() else {
                    errors.push(SchemaError::NotNamedType {
                        interface: describe(location),
                        rust_type: declared.rust_name,
                    });
                    continue;
                };
                match owners.entry(key) {
                    Entry::Occupied(first_declaration) => {
                        let first_location = first_declaration.get().location;
                        errors.push(SchemaError::DuplicateDeclaration {
                            rust_type: key.rust_name(),
                            first: describe(first_location),
                            second: describe(location),
                        });
                    }
                    Entry::Vacant(unclaimed) => {
                        unclaimed.insert(Owner {
                            location,
                            declared_name: declared.name.as_deref(),
                        });
                    }
                }
            }
        }

        // 2. Follow references from each local interface's declared types and
        //    operations. The walk stops at a type another interface declares:
        //    that is an import, and its own references belong to its owner.
        let mut imports = Vec::with_capacity(declarations.len());
        let mut reached_locally = HashSet::new();
        let mut reached_by: HashMap<TypeKey, Vec<usize>> = HashMap::new();
        for (position, declaration) in declarations.iter().enumerate() {
            let here = Location::Local(position);
            let mut interface_imports = Vec::new();
            let mut visited = HashSet::new();
            let mut pending = interface_roots(declaration);

            while let Some(key) = pending.pop() {
                let is_first_visit = visited.insert(key);
                if !is_first_visit {
                    continue;
                }
                match owners.get(&key) {
                    Some(owner) if owner.location != here => {
                        interface_imports.push((key, owner.location));
                        continue;
                    }
                    Some(_) => {}
                    None => reached_by.entry(key).or_default().push(position),
                }
                reached_locally.insert(key);
                let definition = graph.definition(key);
                collect_references(definition, &mut pending);
            }
            imports.push(interface_imports);
        }

        // 3. An undeclared type belongs to the only interface that reaches it.
        //    Iterating the graph keeps the report order reproducible.
        for (key, _) in graph.definitions() {
            if owners.contains_key(key) {
                continue;
            }
            match reached_by.get(key).map(Vec::as_slice) {
                Some([position]) => {
                    owners.insert(
                        *key,
                        Owner {
                            location: Location::Local(*position),
                            declared_name: None,
                        },
                    );
                }
                Some(positions) => {
                    let interface_names = positions
                        .iter()
                        .map(|position| format!("`{}`", declarations[*position].name))
                        .collect();
                    errors.push(SchemaError::AmbiguousPlacement {
                        rust_type: key.rust_name(),
                        interfaces: interface_names,
                    });
                }
                // Reached only through an external type, or through a
                // declaration already reported as `NotNamedType`. Such a type
                // is not part of this package.
                None => {}
            }
        }

        Self {
            owners,
            imports,
            reached_locally,
        }
    }

    fn location(&self, key: TypeKey) -> Option<Location> {
        let owner = self.owners.get(&key)?;
        Some(owner.location)
    }

    /// Give each placed type the name recorded for it apart from an interface.
    ///
    /// A name the owning interface declares itself is kept.
    fn apply_names(&mut self, names: &'schema [DeclaredName], errors: &mut Vec<SchemaError>) {
        // 1. Collect the distinct names of each type, in the order given.
        let mut names_by_type: Vec<(TypeKey, &'static str, Vec<&'schema str>)> = Vec::new();
        for declared in names {
            let Some(key) = declared.ty.definition_key() else {
                errors.push(SchemaError::NamedUndefinedType {
                    rust_type: declared.rust_name,
                    name: declared.name.clone(),
                });
                continue;
            };
            let name = declared.name.as_str();
            let known_type = names_by_type
                .iter_mut()
                .find(|(known_key, _, _)| *known_key == key);
            match known_type {
                Some((_, _, given_names)) if given_names.contains(&name) => {}
                Some((_, _, given_names)) => given_names.push(name),
                None => names_by_type.push((key, declared.rust_name, vec![name])),
            }
        }

        // 2. A type takes its one name. A type the package never reaches has
        //    no owner and takes none.
        for (key, rust_type, given_names) in names_by_type {
            let [name] = given_names.as_slice() else {
                errors.push(SchemaError::ConflictingNames {
                    rust_type,
                    names: given_names.iter().map(|name| format!("`{name}`")).collect(),
                });
                continue;
            };
            if let Some(owner) = self.owners.get_mut(&key) {
                owner.declared_name.get_or_insert(name);
            }
        }
    }
}

/// The named types an interface refers to directly: its declared types and
/// everything in the signatures of its functions and resource functions.
fn interface_roots(declaration: &InterfaceDeclaration) -> Vec<TypeKey> {
    let mut roots = Vec::new();
    for declared in &declaration.declared {
        if let Some(key) = declared.ty.definition_key() {
            roots.push(key);
        }
        for method in &declared.methods {
            collect_function_references(&method.function, &mut roots);
        }
    }
    for declared in &declaration.functions {
        collect_function_references(&declared.function, &mut roots);
    }
    roots
}

fn collect_function_references(function: &Function, pending: &mut Vec<TypeKey>) {
    for param in &function.params {
        collect_type_references(&param.ty, pending);
    }
    collect_type_references(&function.result, pending);
}

/// Queue every named type that `definition` refers to directly.
fn collect_references(definition: &TypeDef, pending: &mut Vec<TypeKey>) {
    match definition.kind() {
        TypeDefKind::Record(fields) => collect_field_references(fields, pending),
        TypeDefKind::Variant(cases) => collect_case_references(cases, pending),
        TypeDefKind::RecordOfCases { fields, cases, .. } => {
            collect_field_references(fields, pending);
            collect_case_references(cases, pending);
        }
        TypeDefKind::Enum(_) | TypeDefKind::Resource => {}
    }
}

fn collect_case_references(cases: &[Case], pending: &mut Vec<TypeKey>) {
    for case in cases {
        match &case.payload {
            Payload::None => {}
            Payload::Type(ty) => collect_type_references(ty, pending),
            Payload::Record { fields, .. } => collect_field_references(fields, pending),
        }
    }
}

fn collect_field_references(fields: &[Field], pending: &mut Vec<TypeKey>) {
    for field in fields {
        collect_type_references(&field.ty, pending);
    }
}

fn collect_type_references(ty: &Type, pending: &mut Vec<TypeKey>) {
    match ty {
        Type::Named(key) | Type::Own(key) | Type::Borrow(key) => pending.push(*key),
        Type::List(inner) | Type::Option(inner) => collect_type_references(inner, pending),
        Type::Result { ok, err } => {
            collect_type_references(ok, pending);
            collect_type_references(err, pending);
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_type_references(element, pending);
            }
        }
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::Char
        | Type::String => {}
    }
}

/// Worlds refer to interfaces by name or path; resolve and check each item.
fn resolve_worlds(
    worlds: &[WorldDeclaration],
    declarations: &[InterfaceDeclaration],
    errors: &mut Vec<SchemaError>,
) -> Vec<World> {
    let mut resolved_worlds = Vec::with_capacity(worlds.len());
    for world in worlds {
        let mut seen_items = HashSet::new();
        let mut items = Vec::with_capacity(world.items.len());
        for item in &world.items {
            // An external interface is named by a path, which contains `:`.
            let target = if item.target.contains(':') {
                check_interface_path(&item.target, errors);
                item.target.clone()
            } else {
                let is_declared = declarations
                    .iter()
                    .any(|declaration| declaration.name == item.target);
                if !is_declared {
                    errors.push(SchemaError::UnknownInterface {
                        world: world.name.clone(),
                        interface: item.target.clone(),
                    });
                }
                Escaped(&item.target).to_string()
            };
            if let Some(label) = &item.label {
                check_name(
                    label,
                    || format!("an item of world `{}`", world.name),
                    errors,
                );
            }

            // Imports and exports are separate namespaces of a world.
            let item_name = item.label.as_ref().unwrap_or(&item.target);
            let is_first_use = seen_items.insert((item.direction, item_name));
            if !is_first_use {
                errors.push(SchemaError::DuplicateWorldItem {
                    world: world.name.clone(),
                    item: item_name.clone(),
                });
            }
            let interface = declarations
                .iter()
                .position(|declaration| declaration.name == item.target);
            items.push(ResolvedWorldItem {
                direction: item.direction,
                label: item.label.clone(),
                target,
                interface,
            });
        }
        resolved_worlds.push(World {
            name: world.name.clone(),
            items,
        });
    }
    resolved_worlds
}

/// Resolved type names, available once placement has succeeded.
struct Names<'schema> {
    graph: &'schema TypeGraph,
    placement: Placement<'schema>,
}

impl<'schema> Names<'schema> {
    /// The declared name, or the default from the type's own definition.
    fn name_of(&self, key: TypeKey) -> &'schema str {
        let declared_name = self
            .placement
            .owners
            .get(&key)
            .and_then(|owner| owner.declared_name);
        match declared_name {
            Some(declared_name) => declared_name,
            None => self.graph.definition(key).name(),
        }
    }

    /// `ty` as WIT source text.
    fn expression(&self, ty: &Type) -> String {
        let expression = Expression { ty, names: self };
        expression.to_string()
    }

    fn function(
        &self,
        function: &Function,
        kind: FunctionKind,
        rust_path: &'static str,
    ) -> ResolvedFunction {
        let params = function
            .params
            .iter()
            .map(|param| ResolvedParam {
                name: param.name,
                ty: self.expression(&param.ty),
                reflected: param.ty.clone(),
                by_reference: param.by_reference,
            })
            .collect();
        let result = if function.result.is_unit() {
            None
        } else {
            Some(self.expression(&function.result))
        };
        ResolvedFunction {
            name: function.name,
            docs: function.docs,
            kind,
            params,
            result,
            result_type: function.result.clone(),
            rust_path,
        }
    }

    /// The name of every type that has an owning interface.
    fn type_names(&self) -> HashMap<TypeKey, String> {
        self.placement
            .owners
            .keys()
            .map(|key| (*key, self.name_of(*key).to_owned()))
            .collect()
    }
}

/// Writes a type expression with resolved, keyword-escaped names.
struct Expression<'names, 'schema> {
    ty: &'names Type,
    names: &'names Names<'schema>,
}

impl<'names> Expression<'names, '_> {
    fn nested(&self, ty: &'names Type) -> Self {
        Self {
            ty,
            names: self.names,
        }
    }

    fn name(&self, key: TypeKey) -> Escaped<'names> {
        let name = self.names.name_of(key);
        Escaped(name)
    }
}

impl fmt::Display for Expression<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ty {
            Type::Bool => formatter.write_str("bool"),
            Type::U8 => formatter.write_str("u8"),
            Type::U16 => formatter.write_str("u16"),
            Type::U32 => formatter.write_str("u32"),
            Type::U64 => formatter.write_str("u64"),
            Type::S8 => formatter.write_str("s8"),
            Type::S16 => formatter.write_str("s16"),
            Type::S32 => formatter.write_str("s32"),
            Type::S64 => formatter.write_str("s64"),
            Type::F32 => formatter.write_str("f32"),
            Type::F64 => formatter.write_str("f64"),
            Type::Char => formatter.write_str("char"),
            Type::String => formatter.write_str("string"),
            Type::List(element) => write!(formatter, "list<{}>", self.nested(element)),
            Type::Option(value) => write!(formatter, "option<{}>", self.nested(value)),
            Type::Result { ok, err } => match (ok.is_unit(), err.is_unit()) {
                (true, true) => formatter.write_str("result"),
                (false, true) => write!(formatter, "result<{}>", self.nested(ok)),
                (true, false) => write!(formatter, "result<_, {}>", self.nested(err)),
                (false, false) => write!(
                    formatter,
                    "result<{}, {}>",
                    self.nested(ok),
                    self.nested(err)
                ),
            },
            Type::Tuple(elements) => {
                formatter.write_str("tuple<")?;
                for (position, element) in elements.iter().enumerate() {
                    if position > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{}", self.nested(element))?;
                }
                formatter.write_str(">")
            }
            Type::Named(key) => write!(formatter, "{}", self.name(*key)),
            Type::Own(key) => write!(formatter, "own<{}>", self.name(*key)),
            Type::Borrow(key) => write!(formatter, "borrow<{}>", self.name(*key)),
        }
    }
}

/// The names declared for types of external interfaces are WIT identifiers too.
fn check_external_type_names(
    externals: &[ExternalInterfaceDeclaration],
    names: &Names<'_>,
    errors: &mut Vec<SchemaError>,
) {
    for external in externals {
        for declared in &external.declared {
            let Some(key) = declared.ty.definition_key() else {
                continue;
            };
            let name = names.name_of(key);
            check_name(name, || format!("the type `{}`", key.rust_name()), errors);
        }
    }
}

/// One interface while its definitions, imports and functions are collected.
///
/// They all claim names in the same namespace, so one owner gathers them.
struct InterfaceScope<'run, 'schema> {
    names: &'run Names<'schema>,
    namespace: Namespace,
    definitions: Vec<Definition>,
    errors: &'run mut Vec<SchemaError>,
}

impl InterfaceScope<'_, '_> {
    /// Build the interface: definitions in graph order, imports, functions.
    fn resolve(
        mut self,
        position: usize,
        declarations: &[InterfaceDeclaration],
        externals: &[ExternalInterfaceDeclaration],
    ) -> Interface {
        let declaration = &declarations[position];
        let here = Location::Local(position);

        for (key, definition) in self.names.graph.definitions() {
            if self.names.placement.location(*key) != Some(here) {
                continue;
            }
            let declared_methods = declaration
                .declared
                .iter()
                .find(|declared| declared.ty.definition_key() == Some(*key))
                .map_or(&[][..], |declared| declared.methods.as_slice());
            self.define(*key, definition, declared_methods);
        }
        let uses = self.import(position, declarations, externals);

        let mut functions = Vec::with_capacity(declaration.functions.len());
        for declared in &declaration.functions {
            let function = &declared.function;
            self.namespace
                .claim(function.name, format!("the function `{}`", function.name));
            let resolved_function =
                self.names
                    .function(function, FunctionKind::Free, declared.rust_path);
            functions.push(resolved_function);
        }

        self.namespace
            .report_clashes(&declaration.name, self.errors);
        Interface {
            name: declaration.name.clone(),
            uses,
            definitions: self.definitions,
            functions,
        }
    }

    /// Add the definition of `key`, preceded by the payload records it needs.
    fn define(&mut self, key: TypeKey, definition: &TypeDef, methods: &[DeclaredMethod]) {
        let name = self.names.name_of(key);
        check_name(
            name,
            || format!("the type `{}`", key.rust_name()),
            self.errors,
        );
        self.namespace.claim(name, format!("`{}`", key.rust_name()));

        let body = match definition.kind() {
            TypeDefKind::Record(fields) => {
                let resolved_fields = resolve_fields(fields, self.names);
                Body::Record(resolved_fields)
            }
            TypeDefKind::Variant(cases) => {
                let resolved_cases = self.cases(key, name, cases);
                Body::Variant(resolved_cases)
            }
            TypeDefKind::RecordOfCases {
                fields,
                cases_field,
                cases,
            } => {
                let cases_name = self.define_cases(key, name, cases_field, cases);
                let mut resolved_fields = resolve_fields(fields, self.names);
                resolved_fields.push(ResolvedField {
                    name: cases_field,
                    docs: None,
                    ty: Escaped(&cases_name).to_string(),
                });
                Body::Record(resolved_fields)
            }
            TypeDefKind::Enum(cases) => {
                let resolved_cases = cases
                    .iter()
                    .map(|case| ResolvedCase {
                        name: case.name,
                        docs: case.docs,
                        payload: None,
                    })
                    .collect();
                Body::Enum(resolved_cases)
            }
            TypeDefKind::Resource => {
                let resolved_functions = methods
                    .iter()
                    .map(|method| {
                        let kind = match method.kind {
                            MethodKind::Method { exclusive } => FunctionKind::Method { exclusive },
                            MethodKind::Static => FunctionKind::Static,
                            MethodKind::Constructor => FunctionKind::Constructor,
                        };
                        self.names
                            .function(&method.function, kind, method.rust_path)
                    })
                    .collect();
                Body::Resource(resolved_functions)
            }
        };
        self.definitions.push(Definition {
            key: Some(key),
            name: name.to_owned(),
            docs: definition.docs(),
            owner_path: definition.owner_path(),
            body,
        });
    }

    fn cases(&mut self, key: TypeKey, variant_name: &str, cases: &[Case]) -> Vec<ResolvedCase> {
        let mut resolved_cases = Vec::with_capacity(cases.len());
        for case in cases {
            let payload = self.payload(key, variant_name, case);
            resolved_cases.push(ResolvedCase {
                name: case.name,
                docs: case.docs,
                payload,
            });
        }
        resolved_cases
    }

    /// Define the cases of a record-of-cases and return their definition's name.
    ///
    /// The name is derived from the resolved record name, so instantiations of
    /// a generic enum keep distinct case definitions. Cases without data are
    /// defined as an `enum`, like any other Rust enum of unit variants.
    fn define_cases(
        &mut self,
        key: TypeKey,
        record_name: &str,
        cases_field: &str,
        cases: &[Case],
    ) -> String {
        let cases_name = format!("{record_name}-{cases_field}");
        check_name(
            &cases_name,
            || format!("the cases of `{}`", key.rust_name()),
            self.errors,
        );
        self.namespace
            .claim(&cases_name, format!("the cases of `{}`", key.rust_name()));

        let resolved_cases = self.cases(key, &cases_name, cases);
        let carries_data = resolved_cases.iter().any(|case| case.payload.is_some());
        let body = if carries_data {
            Body::Variant(resolved_cases)
        } else {
            Body::Enum(resolved_cases)
        };
        self.definitions.push(Definition {
            key: None,
            name: cases_name.clone(),
            docs: None,
            owner_path: None,
            body,
        });
        cases_name
    }

    /// The payload of `case` as WIT source text.
    ///
    /// A WIT case carries at most one value. The fields of a struct-like case
    /// therefore become a record, defined immediately before its variant.
    fn payload(&mut self, variant_key: TypeKey, variant_name: &str, case: &Case) -> Option<String> {
        match &case.payload {
            Payload::None => None,
            Payload::Type(ty) => Some(self.names.expression(ty)),
            Payload::Record {
                name: declared_name,
                fields,
            } => {
                let payload_name = match declared_name {
                    Some(declared_name) => (*declared_name).to_owned(),
                    None => format!("{variant_name}-{}", case.name),
                };
                let case_owner = format!("{}::{}", variant_key.rust_name(), case.name);
                check_name(
                    &payload_name,
                    || format!("the payload record of `{case_owner}`"),
                    self.errors,
                );
                self.namespace
                    .claim(&payload_name, format!("the payload of `{case_owner}`"));

                let payload_reference = Escaped(&payload_name).to_string();
                let resolved_fields = resolve_fields(fields, self.names);
                self.definitions.push(Definition {
                    key: None,
                    name: payload_name,
                    docs: None,
                    owner_path: None,
                    body: Body::Record(resolved_fields),
                });
                Some(payload_reference)
            }
        }
    }

    /// Claim the names this interface imports and group them by owner.
    ///
    /// Local owners come first in declaration order, then external ones, and
    /// names are sorted. The `use` lines therefore do not depend on the order
    /// in which references were found.
    fn import(
        &mut self,
        position: usize,
        declarations: &[InterfaceDeclaration],
        externals: &[ExternalInterfaceDeclaration],
    ) -> Vec<Use> {
        let source_of = |location: Location| match location {
            Location::Local(owner_position) => {
                Escaped(&declarations[owner_position].name).to_string()
            }
            Location::External(owner_position) => externals[owner_position].path.clone(),
        };

        let mut imported_names: BTreeMap<Location, BTreeSet<&str>> = BTreeMap::new();
        for (key, location) in &self.names.placement.imports[position] {
            let name = self.names.name_of(*key);
            let origin = format!(
                "`{}` imported from interface `{}`",
                key.rust_name(),
                source_of(*location)
            );
            self.namespace.claim(name, origin);
            imported_names.entry(*location).or_default().insert(name);
        }

        imported_names
            .into_iter()
            .map(|(location, sorted_names)| Use {
                source: source_of(location),
                names: sorted_names.into_iter().map(str::to_owned).collect(),
            })
            .collect()
    }
}

fn resolve_fields(fields: &[Field], names: &Names<'_>) -> Vec<ResolvedField> {
    fields
        .iter()
        .map(|field| ResolvedField {
            name: field.name,
            docs: field.docs,
            ty: names.expression(&field.ty),
        })
        .collect()
}

/// The names of one interface and what claims each of them.
///
/// An ordered map keeps clash reports reproducible.
#[derive(Default)]
struct Namespace {
    claims: BTreeMap<String, Vec<String>>,
}

impl Namespace {
    fn claim(&mut self, name: &str, origin: String) {
        self.claims.entry(name.to_owned()).or_default().push(origin);
    }

    fn report_clashes(self, interface: &str, errors: &mut Vec<SchemaError>) {
        for (name, origins) in self.claims {
            if origins.len() > 1 {
                errors.push(SchemaError::DuplicateName {
                    interface: interface.to_owned(),
                    name,
                    origins,
                });
            }
        }
    }
}
