//! WIT source text for a resolved package.
//!
//! Layout only: names, placement and validity were settled during resolution.

use std::fmt;

use crate::identifier::Escaped;
use crate::resolve::{
    Body, Definition, FunctionKind, Interface, ResolvedCase, ResolvedFunction, ResolvedPackage,
    Use, World,
};
use crate::schema::Direction;

const ITEM_INDENT: &str = "  ";
const MEMBER_INDENT: &str = "    ";

impl fmt::Display for ResolvedPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "package {};", self.name)?;
        for interface in &self.interfaces {
            writeln!(formatter)?;
            write_interface(formatter, interface)?;
        }
        for world in &self.worlds {
            writeln!(formatter)?;
            write_world(formatter, world)?;
        }
        Ok(())
    }
}

/// Imports first, then types, then functions, separated by blank lines.
fn write_interface(formatter: &mut fmt::Formatter<'_>, interface: &Interface) -> fmt::Result {
    writeln!(formatter, "interface {} {{", Escaped(&interface.name))?;

    for import in &interface.uses {
        write_use(formatter, import)?;
    }

    let mut needs_separator = !interface.uses.is_empty();
    for definition in &interface.definitions {
        if needs_separator {
            writeln!(formatter)?;
        }
        write_definition(formatter, definition)?;
        needs_separator = true;
    }
    for function in &interface.functions {
        if needs_separator {
            writeln!(formatter)?;
        }
        write_function(formatter, ITEM_INDENT, function)?;
        needs_separator = true;
    }

    writeln!(formatter, "}}")
}

fn write_use(formatter: &mut fmt::Formatter<'_>, import: &Use) -> fmt::Result {
    write!(formatter, "{ITEM_INDENT}use {}.{{", import.source)?;
    for (position, name) in import.names.iter().enumerate() {
        if position > 0 {
            formatter.write_str(", ")?;
        }
        write!(formatter, "{}", Escaped(name))?;
    }
    writeln!(formatter, "}};")
}

fn write_definition(formatter: &mut fmt::Formatter<'_>, definition: &Definition) -> fmt::Result {
    write_docs(formatter, ITEM_INDENT, definition.docs)?;
    let name = Escaped(&definition.name);
    match &definition.body {
        Body::Record(fields) => {
            writeln!(formatter, "{ITEM_INDENT}record {name} {{")?;
            for field in fields {
                write_docs(formatter, MEMBER_INDENT, field.docs)?;
                let field_name = Escaped(field.name);
                writeln!(formatter, "{MEMBER_INDENT}{field_name}: {},", field.ty)?;
            }
        }
        Body::Variant(cases) => {
            writeln!(formatter, "{ITEM_INDENT}variant {name} {{")?;
            write_cases(formatter, cases)?;
        }
        Body::Enum(cases) => {
            writeln!(formatter, "{ITEM_INDENT}enum {name} {{")?;
            write_cases(formatter, cases)?;
        }
        Body::Resource(functions) => {
            // A resource without exported functions has no body.
            if functions.is_empty() {
                return writeln!(formatter, "{ITEM_INDENT}resource {name};");
            }
            writeln!(formatter, "{ITEM_INDENT}resource {name} {{")?;
            for function in functions {
                write_function(formatter, MEMBER_INDENT, function)?;
            }
        }
    }
    writeln!(formatter, "{ITEM_INDENT}}}")
}

fn write_cases(formatter: &mut fmt::Formatter<'_>, cases: &[ResolvedCase]) -> fmt::Result {
    for case in cases {
        write_docs(formatter, MEMBER_INDENT, case.docs)?;
        let case_name = Escaped(case.name);
        match &case.payload {
            Some(payload) => writeln!(formatter, "{MEMBER_INDENT}{case_name}({payload}),")?,
            None => writeln!(formatter, "{MEMBER_INDENT}{case_name},")?,
        }
    }
    Ok(())
}

fn write_function(
    formatter: &mut fmt::Formatter<'_>,
    indent: &str,
    function: &ResolvedFunction,
) -> fmt::Result {
    write_docs(formatter, indent, function.docs)?;

    let name = Escaped(function.name);
    match function.kind {
        FunctionKind::Free | FunctionKind::Method { .. } => {
            write!(formatter, "{indent}{name}: func(")?;
        }
        FunctionKind::Static => write!(formatter, "{indent}{name}: static func(")?,
        // A constructor has no name and always returns its resource.
        FunctionKind::Constructor => write!(formatter, "{indent}constructor(")?,
    }
    for (position, param) in function.params.iter().enumerate() {
        if position > 0 {
            formatter.write_str(", ")?;
        }
        write!(formatter, "{}: {}", Escaped(param.name), param.ty)?;
    }
    formatter.write_str(")")?;

    if function.kind != FunctionKind::Constructor {
        if let Some(result) = &function.result {
            write!(formatter, " -> {result}")?;
        }
    }
    writeln!(formatter, ";")
}

fn write_world(formatter: &mut fmt::Formatter<'_>, world: &World) -> fmt::Result {
    writeln!(formatter, "world {} {{", Escaped(&world.name))?;
    for item in &world.items {
        let direction = match item.direction {
            Direction::Import => "import",
            Direction::Export => "export",
        };
        match &item.label {
            Some(label) => writeln!(
                formatter,
                "{ITEM_INDENT}{direction} {}: {};",
                Escaped(label),
                item.target
            )?,
            None => writeln!(formatter, "{ITEM_INDENT}{direction} {};", item.target)?,
        }
    }
    writeln!(formatter, "}}")
}

/// One `///` line per documentation line; blank lines stay blank comments.
fn write_docs(
    formatter: &mut fmt::Formatter<'_>,
    indent: &str,
    docs: Option<&'static str>,
) -> fmt::Result {
    let Some(docs) = docs else {
        return Ok(());
    };
    for line in docs.lines() {
        if line.is_empty() {
            writeln!(formatter, "{indent}///")?;
        } else {
            writeln!(formatter, "{indent}/// {line}")?;
        }
    }
    Ok(())
}
