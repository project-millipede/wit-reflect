//! Rendering of the forwarding file.
//!
//! The forwarding file implements the traits that wit-bindgen generates for a
//! world's exported interfaces. Each implementation calls the Rust operation
//! the schema lists and assembles the binding value from its result.
//!
//! A trait implementation must spell out its signatures, so this module
//! reproduces wit-bindgen's naming of modules, traits, types, functions and
//! parameters. All of that knowledge lives here. It follows wit-bindgen 0.60;
//! a difference in another version is a compile error in the component crate.

use std::collections::HashMap;
use std::fmt;

use heck::{ToSnakeCase, ToUpperCamelCase};

use crate::resolve::{
    Body, Definition, FunctionKind, Interface, ResolvedFunction, ResolvedPackage, ResolvedParam,
    ResolvedWorldItem,
};
use crate::schema::Direction;
use crate::types::{Type, TypeKey};

const RESOURCE_CELL: &str = "::wit_reflect::component::ResourceCell";
const ASSEMBLE: &str = "::wit_reflect::lower::assemble";
const INDENT: &str = "    ";

/// Rust keywords that wit-bindgen escapes with a trailing underscore.
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in",
    "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// One reason the forwarding file cannot be generated.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ForwardingError {
    /// The schema declares no world of that name.
    #[error("the schema declares no world named `{world}`")]
    UnknownWorld {
        /// The requested world.
        world: String,
    },

    /// A parameter is a record, variant, enum or owned handle.
    ///
    /// Generated forwarding passes numbers, booleans, strings, lists, options
    /// and tuples of those, and borrowed resources.
    #[error("{function} takes `{parameter}`, whose type cannot be passed to Rust yet")]
    UnsupportedParameter {
        /// The function, described with its interface or resource.
        function: String,
        /// The parameter's WIT name.
        parameter: &'static str,
    },

    /// A resource handle is nested inside the result.
    ///
    /// A function may return a resource directly, but not inside a list,
    /// option, tuple or `result`.
    #[error("{function} returns a resource handle inside another type")]
    NestedHandle {
        /// The function, described with its interface or resource.
        function: String,
    },

    /// An operation's path would not resolve in the component crate.
    #[error(
        "{function} is declared as `{path}`; write a path that starts with a crate name or \
         `crate`, because the forwarding file is compiled in another crate"
    )]
    RelativePath {
        /// The function, described with its interface or resource.
        function: String,
        /// The path given to `operation!`.
        path: &'static str,
    },

    /// A resource exports no function, so its Rust type is not known.
    #[error("resource `{resource}` exports no function, so its Rust type cannot be named")]
    UnknownResourceType {
        /// The resource's WIT name.
        resource: String,
    },

    /// The functions of one resource belong to different Rust types.
    #[error("the functions of resource `{resource}` belong to different Rust types: {}", .types.join(", "))]
    MixedResourceTypes {
        /// The resource's WIT name.
        resource: String,
        /// The Rust types named by the function paths.
        types: Vec<String>,
    },
}

/// Every problem found while generating a forwarding file. Never empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardingErrors {
    errors: Vec<ForwardingError>,
}

impl ForwardingErrors {
    fn check(errors: Vec<ForwardingError>) -> Result<(), Self> {
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Self { errors })
        }
    }

    /// The individual problems in the order they were found.
    pub fn iter(&self) -> std::slice::Iter<'_, ForwardingError> {
        self.errors.iter()
    }
}

impl IntoIterator for ForwardingErrors {
    type Item = ForwardingError;
    type IntoIter = std::vec::IntoIter<ForwardingError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl<'errors> IntoIterator for &'errors ForwardingErrors {
    type Item = &'errors ForwardingError;
    type IntoIter = std::slice::Iter<'errors, ForwardingError>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// One line per problem, so a generator's output lists them all.
impl fmt::Display for ForwardingErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the forwarding file cannot be generated:")?;
        for error in &self.errors {
            write!(formatter, "\n- {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ForwardingErrors {}

impl ResolvedPackage {
    /// Render the Rust source that implements the interfaces `world` exports.
    ///
    /// The component crate includes the result as a module next to its
    /// bindings:
    ///
    /// ```text
    /// wit_bindgen::generate!({
    ///     path: "wit",
    ///     world: "example:session/session",
    ///     additional_derives: [wit_reflect::FromParts],
    /// });
    ///
    /// #[rustfmt::skip]
    /// mod forwarding;
    /// use forwarding::Component;
    ///
    /// export!(Component);
    /// ```
    ///
    /// The file defines `Component`, implements `Guest` and `Guest<Resource>`
    /// for every exported interface of this package that has functions, and
    /// registers each resource as a
    /// [`ResourceCell`](crate::component::ResourceCell) of its Rust type. It
    /// expects the bindings at the crate root and follows wit-bindgen 0.60.
    ///
    /// # Errors
    ///
    /// Returns every [`ForwardingError`] found: an unknown world, a parameter
    /// or result that generated forwarding cannot pass, an operation path
    /// that is not absolute, and a resource whose Rust type cannot be
    /// determined.
    pub fn forwarding(&self, world: &str) -> Result<String, ForwardingErrors> {
        let Some(exported_world) = self.worlds.iter().find(|declared| declared.name == world)
        else {
            let unknown_world = ForwardingError::UnknownWorld {
                world: world.to_owned(),
            };
            return Err(ForwardingErrors {
                errors: vec![unknown_world],
            });
        };

        let mut errors = Vec::new();
        let resource_types = resource_types(&self.interfaces, &mut errors);
        let mut renderer = Renderer {
            type_names: &self.type_names,
            resource_types,
            errors,
            text: String::new(),
        };
        renderer.line(
            0,
            &format!(
                "// Generated by wit-reflect from the schema of `{}`, world `{world}`.",
                self.name
            ),
        );
        renderer.line(
            0,
            "// Do not edit. Change the Rust declarations or the schema and regenerate.",
        );
        renderer.blank();
        renderer.line(
            0,
            "/// Implements the exported interfaces by calling the Rust operations.",
        );
        renderer.line(0, "pub struct Component;");

        for item in &exported_world.items {
            if item.direction != Direction::Export {
                continue;
            }
            // An external interface is implemented by its own package.
            let Some(position) = item.interface else {
                continue;
            };
            let interface = &self.interfaces[position];
            let module = self.binding_module(item, interface);
            renderer.interface(&module, interface);
        }

        ForwardingErrors::check(renderer.errors)?;
        Ok(renderer.text)
    }

    /// The module wit-bindgen generates for an exported interface.
    ///
    /// A named export is `exports::<label>`. Otherwise the module follows the
    /// interface's identity: `exports::<namespace>::<package>::<interface>`.
    fn binding_module(&self, item: &ResolvedWorldItem, interface: &Interface) -> String {
        match &item.label {
            Some(label) => format!("crate::exports::{}", rust_ident(label)),
            None => format!(
                "crate::exports::{}::{}::{}",
                rust_ident(self.name.namespace()),
                rust_ident(self.name.name()),
                rust_ident(&interface.name)
            ),
        }
    }
}

/// The Rust type of every resource, taken from the paths of its functions.
///
/// `owner::Session::inspect` names a function of `owner::Session`.
fn resource_types(
    interfaces: &[Interface],
    errors: &mut Vec<ForwardingError>,
) -> HashMap<TypeKey, String> {
    let mut resource_types = HashMap::new();
    for interface in interfaces {
        for definition in &interface.definitions {
            let (Some(key), Body::Resource(functions)) = (definition.key, &definition.body) else {
                continue;
            };
            let mut owning_types: Vec<&str> = Vec::new();
            for function in functions {
                let Some((owning_type, _)) = function.rust_path.rsplit_once("::") else {
                    // Reported as a relative path where the function is rendered.
                    continue;
                };
                if !owning_types.contains(&owning_type) {
                    owning_types.push(owning_type);
                }
            }
            match owning_types.as_slice() {
                // Reported where the type is needed: an unused resource needs none.
                [] => {}
                [owning_type] => {
                    resource_types.insert(key, (*owning_type).to_owned());
                }
                _ => errors.push(ForwardingError::MixedResourceTypes {
                    resource: definition.name.clone(),
                    types: owning_types.into_iter().map(str::to_owned).collect(),
                }),
            }
        }
    }
    resource_types
}

/// A WIT name as the Rust function, parameter or module name wit-bindgen uses.
fn rust_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_snake_case()
    }
}

/// A WIT name as the Rust type name wit-bindgen uses.
fn rust_type_name(name: &str) -> String {
    // `Guest` is the name of the trait wit-bindgen generates.
    if name == "guest" {
        "Guest_".to_owned()
    } else {
        name.to_upper_camel_case()
    }
}

struct Renderer<'package> {
    type_names: &'package HashMap<TypeKey, String>,
    resource_types: HashMap<TypeKey, String>,
    errors: Vec<ForwardingError>,
    text: String,
}

impl Renderer<'_> {
    fn line(&mut self, depth: usize, text: &str) {
        for _ in 0..depth {
            self.text.push_str(INDENT);
        }
        self.text.push_str(text);
        self.text.push('\n');
    }

    fn blank(&mut self) {
        self.text.push('\n');
    }

    /// `impl Guest` for the interface and `impl Guest<Resource>` for each of
    /// its resources. An interface of types only needs no implementation.
    fn interface(&mut self, module: &str, interface: &Interface) {
        let resources: Vec<(&Definition, &[ResolvedFunction])> = interface
            .definitions
            .iter()
            .filter_map(|definition| match &definition.body {
                Body::Resource(functions) => Some((definition, functions.as_slice())),
                _ => None,
            })
            .collect();
        if resources.is_empty() && interface.functions.is_empty() {
            return;
        }

        self.blank();
        self.line(0, &format!("impl {module}::Guest for Component {{"));
        let mut needs_separator = false;
        for (resource, _) in &resources {
            let Some(cell) = self.resource_cell(resource) else {
                continue;
            };
            let type_name = rust_type_name(&resource.name);
            self.line(1, &format!("type {type_name} = {cell};"));
            needs_separator = true;
        }
        for function in &interface.functions {
            if needs_separator {
                self.blank();
            }
            let label = format!(
                "function `{}` of interface `{}`",
                function.name, interface.name
            );
            self.function(module, &label, function);
            needs_separator = true;
        }
        self.line(0, "}");

        for (resource, functions) in resources {
            let Some(cell) = self.resource_cell(resource) else {
                continue;
            };
            let trait_name = format!("Guest{}", rust_type_name(&resource.name));
            self.blank();
            self.line(0, &format!("impl {module}::{trait_name} for {cell} {{"));
            for (position, function) in functions.iter().enumerate() {
                if position > 0 {
                    self.blank();
                }
                let label = format!(
                    "function `{}` of resource `{}`",
                    function.name, resource.name
                );
                self.function(module, &label, function);
            }
            self.line(0, "}");
        }
    }

    /// The cell type registered for a resource, or `None` after reporting
    /// that its Rust type is unknown.
    fn resource_cell(&mut self, resource: &Definition) -> Option<String> {
        let rust_type = resource.key.and_then(|key| self.resource_types.get(&key));
        match rust_type {
            Some(rust_type) => Some(format!("{RESOURCE_CELL}<{rust_type}>")),
            None => {
                self.report_unknown_resource(&resource.name);
                None
            }
        }
    }

    /// Report once per resource that its Rust type cannot be named.
    fn report_unknown_resource(&mut self, resource: &str) {
        // A resource with functions of several types was reported as such.
        let is_reported = self.errors.iter().any(|error| match error {
            ForwardingError::UnknownResourceType { resource: reported }
            | ForwardingError::MixedResourceTypes {
                resource: reported, ..
            } => reported == resource,
            _ => false,
        });
        if !is_reported {
            self.errors.push(ForwardingError::UnknownResourceType {
                resource: resource.to_owned(),
            });
        }
    }

    /// One trait function: its binding signature, the call and the result.
    fn function(&mut self, module: &str, label: &str, function: &ResolvedFunction) {
        let is_absolute = function.rust_path.contains("::")
            && !function.rust_path.starts_with("self::")
            && !function.rust_path.starts_with("super::");
        if !is_absolute {
            self.errors.push(ForwardingError::RelativePath {
                function: label.to_owned(),
                path: function.rust_path,
            });
        }

        // 1. The signature, in the binding's names and types.
        let mut parameters = Vec::with_capacity(function.params.len() + 1);
        let mut arguments = Vec::with_capacity(function.params.len() + 1);
        match function.kind {
            FunctionKind::Method { exclusive } => {
                parameters.push("&self".to_owned());
                // The guard dereferences to the resource at the call.
                let receiver = if exclusive {
                    "&mut self.borrow_mut()"
                } else {
                    "&self.borrow()"
                };
                arguments.push(receiver.to_owned());
            }
            FunctionKind::Free | FunctionKind::Static | FunctionKind::Constructor => {}
        }
        let mut preparations = Vec::new();
        for param in &function.params {
            if !is_supported_parameter(&param.reflected) {
                self.errors.push(ForwardingError::UnsupportedParameter {
                    function: label.to_owned(),
                    parameter: param.name,
                });
            }
            let name = rust_ident(param.name);
            let binding_type = self.binding_type(&param.reflected, module);
            parameters.push(format!("{name}: {binding_type}"));
            arguments.push(self.argument(param, &name, &mut preparations));
        }
        let parameter_list = parameters.join(", ");

        let returns_handle = matches!(function.result_type, Type::Own(_));
        if !returns_handle && contains_handle(&function.result_type) {
            self.errors.push(ForwardingError::NestedHandle {
                function: label.to_owned(),
            });
        }
        let name = rust_ident(function.name);
        let signature = match function.kind {
            // wit-bindgen names the constructor `new`; it returns the cell.
            FunctionKind::Constructor => format!("fn new({parameter_list}) -> Self {{"),
            _ if function.result_type.is_unit() => format!("fn {name}({parameter_list}) {{"),
            _ => {
                let result_type = self.binding_type(&function.result_type, module);
                format!("fn {name}({parameter_list}) -> {result_type} {{")
            }
        };
        self.line(1, &signature);

        // 2. The call to the Rust operation.
        for preparation in preparations {
            self.line(2, &preparation);
        }
        let call = format!("{}({})", function.rust_path, arguments.join(", "));

        // 3. The result, in the binding's representation.
        match (&function.kind, &function.result_type) {
            (FunctionKind::Constructor, _) => {
                self.line(2, &format!("let constructed = {call};"));
                self.line(2, &format!("{RESOURCE_CELL}::new(constructed)"));
            }
            (_, result_type) if result_type.is_unit() => {
                self.line(2, &format!("{call};"));
            }
            (_, Type::Own(key)) => {
                let handle = self.binding_type(&Type::Own(*key), module);
                self.line(2, &format!("let created = {call};"));
                self.line(2, &format!("{handle}::new({RESOURCE_CELL}::new(created))"));
            }
            (_, _) => {
                self.line(2, &format!("let returned = {call};"));
                self.line(2, &format!("{ASSEMBLE}(returned)"));
            }
        }
        self.line(1, "}");
    }

    /// The expression passed for a parameter, with any statements it needs.
    fn argument(
        &mut self,
        param: &ResolvedParam,
        name: &str,
        preparations: &mut Vec<String>,
    ) -> String {
        if let Type::Borrow(key) = &param.reflected {
            // The binding passes a borrowed handle; the operation takes a
            // reference to the resource inside its cell.
            let cell = match self.resource_types.get(key) {
                Some(rust_type) => format!("{RESOURCE_CELL}<{rust_type}>"),
                None => {
                    let resource = self.type_name(*key);
                    self.report_unknown_resource(&resource);
                    String::new()
                }
            };
            preparations.push(format!("let {name} = {name}.get::<{cell}>();"));
            preparations.push(format!("let {name} = {name}.borrow();"));
            return format!("&{name}");
        }
        if param.by_reference {
            format!("&{name}")
        } else {
            name.to_owned()
        }
    }

    fn type_name(&self, key: TypeKey) -> String {
        match self.type_names.get(&key) {
            Some(name) => name.clone(),
            // Not owned by any interface of this package.
            None => key.rust_name().to_owned(),
        }
    }

    /// The Rust type wit-bindgen uses for `ty` inside `module`.
    ///
    /// Every named type an interface refers to is nameable in the interface's
    /// own module, either defined there or aliased by its `use`.
    fn binding_type(&self, ty: &Type, module: &str) -> String {
        match ty {
            Type::Bool => "bool".to_owned(),
            Type::U8 => "u8".to_owned(),
            Type::U16 => "u16".to_owned(),
            Type::U32 => "u32".to_owned(),
            Type::U64 => "u64".to_owned(),
            Type::S8 => "i8".to_owned(),
            Type::S16 => "i16".to_owned(),
            Type::S32 => "i32".to_owned(),
            Type::S64 => "i64".to_owned(),
            Type::F32 => "f32".to_owned(),
            Type::F64 => "f64".to_owned(),
            Type::Char => "char".to_owned(),
            Type::String => "::std::string::String".to_owned(),
            Type::List(element) => {
                format!("::std::vec::Vec<{}>", self.binding_type(element, module))
            }
            Type::Option(value) => {
                format!(
                    "::core::option::Option<{}>",
                    self.binding_type(value, module)
                )
            }
            Type::Result { ok, err } => format!(
                "::core::result::Result<{}, {}>",
                self.binding_type(ok, module),
                self.binding_type(err, module)
            ),
            Type::Tuple(elements) => {
                let element_types: Vec<String> = elements
                    .iter()
                    .map(|element| self.binding_type(element, module))
                    .collect();
                match element_types.as_slice() {
                    [only_type] => format!("({only_type},)"),
                    _ => format!("({})", element_types.join(", ")),
                }
            }
            Type::Named(key) | Type::Own(key) => {
                format!("{module}::{}", rust_type_name(&self.type_name(*key)))
            }
            Type::Borrow(key) => {
                format!(
                    "{module}::{}Borrow<'_>",
                    rust_type_name(&self.type_name(*key))
                )
            }
        }
    }
}

/// Whether generated forwarding can pass a parameter of this type.
fn is_supported_parameter(ty: &Type) -> bool {
    match ty {
        // A borrowed resource is supported as the parameter itself.
        Type::Borrow(_) => true,
        other => is_plain_value(other),
    }
}

/// Numbers, booleans, text, and lists, options and tuples of those: values
/// whose binding type is also the type the Rust operation takes.
fn is_plain_value(ty: &Type) -> bool {
    match ty {
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
        | Type::String => true,
        Type::List(inner) | Type::Option(inner) => is_plain_value(inner),
        Type::Tuple(elements) => elements.iter().all(is_plain_value),
        Type::Result { .. } | Type::Named(_) | Type::Own(_) | Type::Borrow(_) => false,
    }
}

fn contains_handle(ty: &Type) -> bool {
    match ty {
        Type::Own(_) | Type::Borrow(_) => true,
        Type::List(inner) | Type::Option(inner) => contains_handle(inner),
        Type::Result { ok, err } => contains_handle(ok) || contains_handle(err),
        Type::Tuple(elements) => elements.iter().any(contains_handle),
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
