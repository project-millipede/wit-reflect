//! WIT source text for a resolved package.
//!
//! Layout only: names, placement and validity were settled during resolution.

use std::fmt;

use crate::identifier::Escaped;
use crate::resolve::{Body, Definition, Interface, ResolvedCase, ResolvedPackage, Use};

const MEMBER_INDENT: &str = "    ";
const ITEM_INDENT: &str = "  ";

impl fmt::Display for ResolvedPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "package {};", self.name)?;
        for interface in &self.interfaces {
            writeln!(formatter)?;
            write_interface(formatter, interface)?;
        }
        Ok(())
    }
}

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

    writeln!(formatter, "}}")
}

fn write_use(formatter: &mut fmt::Formatter<'_>, import: &Use) -> fmt::Result {
    write!(
        formatter,
        "{ITEM_INDENT}use {}.{{",
        Escaped(&import.interface)
    )?;
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
