//! Resolution: validate a declared schema and give every type one owner and name.
//!
//! This module owns the rules the reflected model cannot express on its own:
//! which interface defines a type, which name it gets, and which definitions
//! are not valid WIT. Rendering consumes its result without further checks.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::error::{SchemaError, SchemaErrors};
use crate::graph::TypeGraph;
use crate::identifier::{self, Escaped};
use crate::package_name::PackageName;
use crate::schema::{InterfaceDeclaration, Schema};
use crate::types::{Case, Field, Payload, Type, TypeDef, TypeDefKind, TypeKey};

/// A validated WIT package.
///
/// Its [`Display`](fmt::Display) implementation writes the package as WIT
/// source text. Equal declarations always produce identical text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub(crate) name: PackageName,
    pub(crate) interfaces: Vec<Interface>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Interface {
    pub(crate) name: String,
    pub(crate) uses: Vec<Use>,
    pub(crate) definitions: Vec<Definition>,
}

/// `use <interface>.{<names>};`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Use {
    pub(crate) interface: String,
    pub(crate) names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Definition {
    pub(crate) name: String,
    pub(crate) docs: Option<&'static str>,
    pub(crate) body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Body {
    Record(Vec<ResolvedField>),
    Variant(Vec<ResolvedCase>),
    /// Cases never carry a payload.
    Enum(Vec<ResolvedCase>),
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

pub(crate) fn resolve(schema: &Schema) -> Result<ResolvedPackage, SchemaErrors> {
    let graph = schema.graph();
    let declarations = schema.interfaces();

    // 1. Check everything that does not depend on placement, and place types.
    //    These problems are independent, so they are reported together.
    let mut declaration_errors = Vec::new();
    report_cycles(graph, &mut declaration_errors);
    check_interface_names(declarations, &mut declaration_errors);
    check_definitions(graph, &mut declaration_errors);
    let placement = Placement::assign(graph, declarations, &mut declaration_errors);
    SchemaErrors::check(declaration_errors)?;

    // 2. Resolve names. A namespace is meaningful only once every type has
    //    exactly one owner, so this runs after placement succeeded.
    let names = Names { graph, placement };
    let mut naming_errors = Vec::new();
    let mut interfaces = Vec::with_capacity(declarations.len());
    for position in 0..declarations.len() {
        let interface = resolve_interface(position, declarations, &names, &mut naming_errors);
        interfaces.push(interface);
    }
    SchemaErrors::check(naming_errors)?;

    Ok(ResolvedPackage {
        name: schema.package().clone(),
        interfaces,
    })
}

fn report_cycles(graph: &TypeGraph, errors: &mut Vec<SchemaError>) {
    for cycle in graph.cycles() {
        errors.push(SchemaError::RecursiveType {
            cycle: cycle.clone(),
        });
    }
}

fn check_interface_names(declarations: &[InterfaceDeclaration], errors: &mut Vec<SchemaError>) {
    for declaration in declarations {
        check_name(&declaration.name, || "an interface".to_owned(), errors);
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

/// Check the shape of every reflected definition.
fn check_definitions(graph: &TypeGraph, errors: &mut Vec<SchemaError>) {
    for (key, definition) in graph.definitions() {
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
            rust_type: owner.to_owned(),
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
        | Type::Named(_) => false,
    }
}

/// Which interface owns each named type, and what each interface imports.
struct Placement<'schema> {
    owners: HashMap<TypeKey, Owner<'schema>>,
    /// Per interface: referenced types that another interface owns.
    imports: Vec<Vec<TypeKey>>,
}

#[derive(Clone, Copy)]
struct Owner<'schema> {
    /// Position of the owning interface in declaration order.
    interface: usize,
    declared_name: Option<&'schema str>,
}

impl<'schema> Placement<'schema> {
    fn assign(
        graph: &TypeGraph,
        declarations: &'schema [InterfaceDeclaration],
        errors: &mut Vec<SchemaError>,
    ) -> Self {
        // 1. A declared type belongs to the interface that declares it.
        let mut owners: HashMap<TypeKey, Owner<'schema>> = HashMap::new();
        for (interface, declaration) in declarations.iter().enumerate() {
            for declared in &declaration.declared {
                let Type::Named(key) = &declared.ty else {
                    errors.push(SchemaError::NotNamedType {
                        interface: declaration.name.clone(),
                        rust_type: declared.rust_name,
                    });
                    continue;
                };
                match owners.entry(*key) {
                    Entry::Occupied(first_declaration) => {
                        let first_interface = first_declaration.get().interface;
                        errors.push(SchemaError::DuplicateDeclaration {
                            rust_type: key.rust_name(),
                            first: declarations[first_interface].name.clone(),
                            second: declaration.name.clone(),
                        });
                    }
                    Entry::Vacant(unclaimed) => {
                        unclaimed.insert(Owner {
                            interface,
                            declared_name: declared.name.as_deref(),
                        });
                    }
                }
            }
        }

        // 2. Follow references from each interface's declared types. The walk
        //    stops at a type another interface declares: that is an import,
        //    and its own references belong to its owner.
        let mut imports = Vec::with_capacity(declarations.len());
        let mut reached_by: HashMap<TypeKey, Vec<usize>> = HashMap::new();
        for (interface, declaration) in declarations.iter().enumerate() {
            let mut interface_imports = Vec::new();
            let mut visited = HashSet::new();
            let mut pending: Vec<TypeKey> = declaration
                .declared
                .iter()
                .filter_map(|declared| match &declared.ty {
                    Type::Named(key) => Some(*key),
                    _ => None,
                })
                .collect();

            while let Some(key) = pending.pop() {
                let is_first_visit = visited.insert(key);
                if !is_first_visit {
                    continue;
                }
                match owners.get(&key) {
                    Some(owner) if owner.interface != interface => {
                        interface_imports.push(key);
                        continue;
                    }
                    Some(_) => {}
                    None => reached_by.entry(key).or_default().push(interface),
                }
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
                Some([interface]) => {
                    owners.insert(
                        *key,
                        Owner {
                            interface: *interface,
                            declared_name: None,
                        },
                    );
                }
                Some(interfaces) => {
                    let interface_names = interfaces
                        .iter()
                        .map(|interface| format!("`{}`", declarations[*interface].name))
                        .collect();
                    errors.push(SchemaError::AmbiguousPlacement {
                        rust_type: key.rust_name(),
                        interfaces: interface_names,
                    });
                }
                // Reached only through a declaration already reported as
                // `NotNamedType`, such as `Vec<Self>`.
                None => {}
            }
        }

        Self { owners, imports }
    }

    fn owner(&self, key: TypeKey) -> Owner<'schema> {
        *self
            .owners
            .get(&key)
            .expect("placement succeeded, so every reflected type has an owning interface")
    }
}

/// Queue every named type that `definition` refers to directly.
fn collect_references(definition: &TypeDef, pending: &mut Vec<TypeKey>) {
    match definition.kind() {
        TypeDefKind::Record(fields) => collect_field_references(fields, pending),
        TypeDefKind::Variant(cases) => {
            for case in cases {
                match &case.payload {
                    Payload::None => {}
                    Payload::Type(ty) => collect_type_references(ty, pending),
                    Payload::Record { fields, .. } => collect_field_references(fields, pending),
                }
            }
        }
        TypeDefKind::Enum(_) => {}
    }
}

fn collect_field_references(fields: &[Field], pending: &mut Vec<TypeKey>) {
    for field in fields {
        collect_type_references(&field.ty, pending);
    }
}

fn collect_type_references(ty: &Type, pending: &mut Vec<TypeKey>) {
    match ty {
        Type::Named(key) => pending.push(*key),
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

/// Resolved type names, available once placement has succeeded.
struct Names<'schema> {
    graph: &'schema TypeGraph,
    placement: Placement<'schema>,
}

impl<'schema> Names<'schema> {
    /// The declared name, or the default from the type's own definition.
    fn name_of(&self, key: TypeKey) -> &'schema str {
        let owner = self.placement.owner(key);
        match owner.declared_name {
            Some(declared_name) => declared_name,
            None => self.graph.definition(key).name(),
        }
    }

    /// `ty` as WIT source text.
    fn expression(&self, ty: &Type) -> String {
        let expression = Expression { ty, names: self };
        expression.to_string()
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
            Type::Named(key) => {
                let name = self.names.name_of(*key);
                write!(formatter, "{}", Escaped(name))
            }
        }
    }
}

/// Build one interface: its definitions in graph order, then its imports.
fn resolve_interface(
    position: usize,
    declarations: &[InterfaceDeclaration],
    names: &Names<'_>,
    errors: &mut Vec<SchemaError>,
) -> Interface {
    let mut scope = InterfaceScope {
        names,
        namespace: Namespace::default(),
        definitions: Vec::new(),
        errors,
    };

    for (key, definition) in names.graph.definitions() {
        let owner = names.placement.owner(*key);
        if owner.interface == position {
            scope.define(*key, definition);
        }
    }
    let uses = scope.import(position, declarations);

    let interface_name = &declarations[position].name;
    scope.namespace.report_clashes(interface_name, scope.errors);
    Interface {
        name: interface_name.clone(),
        uses,
        definitions: scope.definitions,
    }
}

/// One interface while its definitions and imports are collected.
///
/// Definitions, payload records and imports all claim names in the same
/// namespace, so they are gathered by one owner.
struct InterfaceScope<'run, 'schema> {
    names: &'run Names<'schema>,
    namespace: Namespace,
    definitions: Vec<Definition>,
    errors: &'run mut Vec<SchemaError>,
}

impl InterfaceScope<'_, '_> {
    /// Add the definition of `key`, preceded by the payload records it needs.
    fn define(&mut self, key: TypeKey, definition: &TypeDef) {
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
                let mut resolved_cases = Vec::with_capacity(cases.len());
                for case in cases {
                    let payload = self.payload(key, name, case);
                    resolved_cases.push(ResolvedCase {
                        name: case.name,
                        docs: case.docs,
                        payload,
                    });
                }
                Body::Variant(resolved_cases)
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
        };
        self.definitions.push(Definition {
            name: name.to_owned(),
            docs: definition.docs(),
            body,
        });
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
                    name: payload_name,
                    docs: None,
                    body: Body::Record(resolved_fields),
                });
                Some(payload_reference)
            }
        }
    }

    /// Claim the names this interface imports and group them by owner.
    ///
    /// Owners follow declaration order and names are sorted, so the `use`
    /// lines do not depend on the order in which references were found.
    fn import(&mut self, position: usize, declarations: &[InterfaceDeclaration]) -> Vec<Use> {
        let mut imported_names: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
        for key in &self.names.placement.imports[position] {
            let owner = self.names.placement.owner(*key);
            let owner_name = &declarations[owner.interface].name;
            let name = self.names.name_of(*key);
            let origin = format!(
                "`{}` imported from interface `{owner_name}`",
                key.rust_name()
            );
            self.namespace.claim(name, origin);
            imported_names
                .entry(owner.interface)
                .or_default()
                .insert(name);
        }

        imported_names
            .into_iter()
            .map(|(owner_position, sorted_names)| Use {
                interface: declarations[owner_position].name.clone(),
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

/// The type names of one interface and what claims each of them.
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
                errors.push(SchemaError::DuplicateTypeName {
                    interface: interface.to_owned(),
                    name,
                    origins,
                });
            }
        }
    }
}
