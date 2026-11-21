//! A contract declared by marks on the items instead of a schema program.
//!
//! Every registering mark in this file is part of one contract: all of them
//! are linked into this test program, and `Schema::from_registry` applies
//! them together.

#![allow(dead_code, reason = "fixture items are reflected, never called")]

// Registered functions are named from the crate root, as a component crate
// names them. The same paths have to resolve in this file.
extern crate self as registry;

use std::error::Error;

use wit_reflect::{Schema, SchemaError, WitResource, WitType, export, exports, named, operation};

// The block comes before the resource it belongs to, so its functions are
// registered before any interface declares `Session`.
#[exports]
impl Session {
    /// Inspect the selected resource.
    #[export]
    fn inspect(&mut self, resource: &str) -> ReportOutcome {
        self.selection = Some(resource.to_owned());
        Outcome { steps: Vec::new() }
    }

    // Marked the way a crate that gates its marks behind a feature writes it.
    #[cfg_attr(all(), export)]
    fn reset(&mut self) {
        self.selection = None;
    }

    // The mark's condition never holds, so nothing is exported.
    #[cfg_attr(any(), export)]
    fn selection(&self) -> Option<String> {
        self.selection.clone()
    }

    // Not marked.
    fn is_idle(&self) -> bool {
        self.selection.is_none()
    }
}

#[derive(WitResource)]
#[wit(interface = "native")]
struct Session {
    selection: Option<String>,
}

#[derive(WitType)]
#[wit(interface = "binary")]
struct Report {
    byte_length: u64,
}

#[derive(WitType)]
struct Outcome<Step> {
    steps: Vec<Step>,
}

// The name is registered after the operation that uses the type.
#[named]
type ReportOutcome = Outcome<Report>;

/// Create a session without a selection.
#[export(interface = "native")]
fn create_session() -> Session {
    Session { selection: None }
}

// Neither is part of the registered contract: they carry no registering mark.
#[derive(WitResource)]
struct Orphan {
    is_open: bool,
}

impl Orphan {
    #[export]
    fn close(&mut self) {
        self.is_open = false;
    }
}

/// The registry is how a contract is declared without a schema program. It
/// must yield exactly the package the same declarations give when a schema
/// lists them: an operation it dropped would silently leave the contract.
#[test]
fn marked_items_declare_the_package_a_schema_program_would() -> Result<(), Box<dyn Error>> {
    let mut listed = Schema::new("example:session@0.1.0".parse()?);
    listed
        .interface("native")
        .resource::<Session>()
        .method(operation!(registry::Session::inspect))
        .method(operation!(registry::Session::reset));
    listed.interface("binary").define::<Report>();
    listed
        .interface("native")
        .define_as::<ReportOutcome>("report-outcome")
        .function(operation!(registry::create_session));

    let registered = Schema::from_registry("example:session@0.1.0".parse()?)?;

    assert_eq!(registered.resolve()?, listed.resolve()?);
    Ok(())
}

/// Declarations that arrive apart from their interface must not disappear
/// when they cannot be attached: a resource function without a declared
/// resource, and one type under two names.
#[test]
fn declarations_that_fit_nowhere_are_reported() -> Result<(), Box<dyn Error>> {
    let mut schema = Schema::new("example:session@0.1.0".parse()?);
    schema.interface("binary").define::<Report>();
    schema.resource_method(operation!(Orphan::close));
    schema
        .name_instantiation::<Report>("report")
        .name_instantiation::<Report>("summary");

    let errors: Vec<SchemaError> = schema
        .resolve()
        .expect_err("the declarations are invalid")
        .into_iter()
        .collect();

    assert_eq!(
        errors,
        [
            SchemaError::UndeclaredResource {
                resource: "registry::Orphan",
            },
            SchemaError::ConflictingNames {
                rust_type: "registry::Report",
                names: vec!["`report`".to_owned(), "`summary`".to_owned()],
            },
        ]
    );
    Ok(())
}
