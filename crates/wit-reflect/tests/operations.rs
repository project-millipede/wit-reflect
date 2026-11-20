//! Reflection of operations: a resource, a factory, external types and a world.
//!
//! The fixture mirrors the workbench's `native` interface: a session resource
//! whose methods live in two `impl` blocks, a free factory returning the
//! session, an error type owned by a pinned dependency and a world.

#![allow(dead_code, reason = "fixture items are reflected, never called")]

use std::error::Error;

use wit_reflect::{Schema, WitResource, WitType, export, operation};

#[derive(WitType)]
struct Report {
    byte_length: u64,
}

// Part of the dependency's `read-error`, so this package must not define it.
#[derive(WitType)]
struct IoFailure {
    message: String,
}

#[derive(WitType)]
enum ReadError {
    Cancelled,
    IoFailure(IoFailure),
}

#[derive(WitType)]
struct InspectionFailure {
    message: String,
    cause: ReadError,
}

// Declared in no interface: it is reached only through a method signature.
#[derive(WitType)]
struct SelectionState {
    file: Option<String>,
}

/// A filesystem session.
#[derive(WitResource)]
struct Session {
    selected_file: Option<String>,
}

impl Session {
    #[export]
    fn current_selection(&self) -> SelectionState {
        SelectionState {
            file: self.selected_file.clone(),
        }
    }

    // Not marked, so it cannot be listed.
    fn byte_length(&mut self, resource: &str) -> u64 {
        self.selected_file = Some(resource.to_owned());
        0
    }

    // Marked but not listed by the schema.
    #[export]
    fn reset(&mut self) {
        self.selected_file = None;
    }
}

impl Session {
    /// Inspect the selected resource.
    #[export]
    fn inspect(&mut self, resource: &str) -> Result<Report, InspectionFailure> {
        self.selected_file = Some(resource.to_owned());
        Err(InspectionFailure {
            message: resource.to_owned(),
            cause: ReadError::Cancelled,
        })
    }

    #[export]
    fn close(&mut self) {
        self.selected_file = None;
    }
}

#[export]
fn create_selection_session() -> Session {
    Session {
        selected_file: None,
    }
}

/// The WIT a maintainer would write by hand for [`session_schema`].
const EXPECTED_SESSION_WIT: &str = "\
package example:analysis@0.5.0;

interface binary {
  record report {
    byte-length: u64,
  }
}

interface native {
  use binary.{report};
  use example:filesystem/source@0.2.0.{read-error};

  /// A filesystem session.
  resource session {
    current-selection: func() -> selection-state;
    /// Inspect the selected resource.
    inspect: func(%resource: string) -> result<report, inspection-failure>;
    close: func();
  }

  record selection-state {
    file: option<string>,
  }

  record inspection-failure {
    message: string,
    cause: read-error,
  }

  create-selection-session: func() -> own<session>;
}

world analysis {
  import example:websocket/connections@0.1.0;
  export native: native;
  export binary;
}
";

/// The packages the contract depends on, as their owners publish them.
const FILESYSTEM_DEPENDENCY_WIT: &str = "\
package example:filesystem@0.2.0;

interface source {
  record io-failure {
    message: string,
  }

  variant read-error {
    cancelled,
    io-failure(io-failure),
  }
}
";

const WEBSOCKET_DEPENDENCY_WIT: &str = "\
package example:websocket@0.1.0;

interface connections {
  close: func();
}
";

fn session_schema() -> Result<Schema, Box<dyn Error>> {
    let package_name = "example:analysis@0.5.0".parse()?;
    let mut schema = Schema::new(package_name);
    schema
        .external_interface("example:filesystem/source@0.2.0")
        .define::<ReadError>();
    schema.interface("binary").define::<Report>();

    let mut native = schema.interface("native");
    native
        .resource::<Session>()
        .method(operation!(Session::current_selection))
        .method(operation!(Session::inspect))
        .method(operation!(Session::close));
    native.function(operation!(create_selection_session));

    schema
        .world("analysis")
        .import("example:websocket/connections@0.1.0")
        .export_as("native", "native")
        .export("binary");
    Ok(schema)
}

/// Only the listed methods of both `impl` blocks are exported, with their
/// real parameter names and result types. The factory returns an owned
/// handle. `read-error` and its `io-failure` stay with the dependency, while
/// `selection-state` is defined because a signature needs it.
#[test]
fn session_operations_render_the_handwritten_contract() -> Result<(), Box<dyn Error>> {
    let schema = session_schema()?;

    let package = schema.resolve()?;
    let rendered_wit = package.to_string();

    assert_eq!(rendered_wit, EXPECTED_SESSION_WIT);
    Ok(())
}

/// The resource, handle, world and cross-package syntax must be WIT that the
/// reference parser resolves against the dependency packages.
#[test]
fn rendered_contract_is_accepted_with_its_dependencies() -> Result<(), Box<dyn Error>> {
    let schema = session_schema()?;
    let package = schema.resolve()?;
    let rendered_wit = package.to_string();

    // 1. Make the dependency packages known, as a build would.
    let mut parser = wit_parser::Resolve::default();
    parser.push_str("filesystem.wit", FILESYSTEM_DEPENDENCY_WIT)?;
    parser.push_str("websocket.wit", WEBSOCKET_DEPENDENCY_WIT)?;

    // 2. Parse and resolve the generated package against them.
    let parse_result = parser.push_str("analysis.wit", &rendered_wit);

    assert!(
        parse_result.is_ok(),
        "wit-parser rejected: {parse_result:?}"
    );
    Ok(())
}
