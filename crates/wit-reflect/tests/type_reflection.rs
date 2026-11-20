//! Reflection of owner types into a WIT package.
//!
//! The fixture mirrors the shape that motivated this crate: generic outcome
//! carriers instantiated through Rust aliases, a nested `result` field, a
//! struct-like error case and types shared between two interfaces.

#![allow(dead_code, reason = "fixture types are reflected, never constructed")]

use std::error::Error;

use wit_reflect::{PackageName, Schema, SchemaError, WitType};

/// Completed steps in execution order.
#[derive(WitType)]
struct Outcome<Step> {
    steps: Vec<Step>,
}

#[derive(WitType)]
struct StepOutcome<Kind, Output, Failure> {
    kind: Kind,
    result: Result<Output, Failure>,
}

#[derive(WitType)]
enum StepKind {
    SelectFile,
    Inspect,
}

/// An input-file range.
#[derive(WitType)]
struct FileRange {
    offset: u64,
    size: u64,
}

#[derive(WitType)]
struct Report {
    byte_length: u64,
    ranges: Vec<FileRange>,
    resource: Option<String>,
}

#[derive(WitType)]
enum ErrorKind {
    #[wit(payload = "truncated-range")]
    Truncated {
        requested_size: u64,
        available_size: u64,
    },
    RangeOverflow,
    /// Upstream decoding message.
    Decode(String),
}

#[derive(WitType)]
struct AnalysisError {
    message: String,
    kind: ErrorKind,
}

#[derive(WitType)]
struct SelectionState {
    file: Option<String>,
}

#[derive(WitType)]
struct Failure {
    message: String,
}

type InspectionStepOutcome = StepOutcome<StepKind, Report, AnalysisError>;
type InspectionOutcome = Outcome<InspectionStepOutcome>;
type SelectionStepOutcome = StepOutcome<StepKind, Option<SelectionState>, Failure>;
type SelectionOutcome = Outcome<SelectionStepOutcome>;

/// The WIT a maintainer would write by hand for [`analysis_schema`].
const EXPECTED_ANALYSIS_WIT: &str = "\
package example:analysis@0.5.0;

interface binary {
  /// An input-file range.
  record file-range {
    offset: u64,
    size: u64,
  }

  record report {
    byte-length: u64,
    ranges: list<file-range>,
    %resource: option<string>,
  }

  record truncated-range {
    requested-size: u64,
    available-size: u64,
  }

  variant error-kind {
    truncated(truncated-range),
    range-overflow,
    /// Upstream decoding message.
    decode(string),
  }

  record analysis-error {
    message: string,
    kind: error-kind,
  }
}

interface native {
  use binary.{analysis-error, report};

  enum step-kind {
    select-file,
    inspect,
  }

  record inspection-step-outcome {
    kind: step-kind,
    %result: result<report, analysis-error>,
  }

  /// Completed steps in execution order.
  record inspection-outcome {
    steps: list<inspection-step-outcome>,
  }

  record selection-state {
    file: option<string>,
  }

  record failure {
    message: string,
  }

  record selection-step-outcome {
    kind: step-kind,
    %result: result<option<selection-state>, failure>,
  }

  /// Completed steps in execution order.
  record selection-outcome {
    steps: list<selection-step-outcome>,
  }
}
";

fn example_package_name() -> Result<PackageName, Box<dyn Error>> {
    let package_name = "example:analysis@0.5.0".parse()?;
    Ok(package_name)
}

/// Two interfaces: `binary` owns the report and its error, `native` owns both
/// outcome instantiations and imports what `binary` defines.
fn analysis_schema() -> Result<Schema, Box<dyn Error>> {
    let package_name = example_package_name()?;
    let mut schema = Schema::new(package_name);
    schema
        .interface("binary")
        .define::<Report>()
        .define::<AnalysisError>();
    schema
        .interface("native")
        .define_as::<InspectionStepOutcome>("inspection-step-outcome")
        .define_as::<InspectionOutcome>("inspection-outcome")
        .define_as::<SelectionStepOutcome>("selection-step-outcome")
        .define_as::<SelectionOutcome>("selection-outcome");
    Ok(schema)
}

/// Each instantiation keeps its own generic arguments, aliases resolve to
/// their instantiation, keywords are escaped, the struct-like case gets its
/// declared payload record, and `native` imports instead of redefining.
#[test]
fn generic_owner_types_render_the_handwritten_contract() -> Result<(), Box<dyn Error>> {
    let schema = analysis_schema()?;

    let package = schema.resolve()?;
    let rendered_wit = package.to_string();

    assert_eq!(rendered_wit, EXPECTED_ANALYSIS_WIT);
    Ok(())
}

/// Matching the expected text does not establish that the text is WIT. The
/// reference parser must accept it, including escapes and cross-interface use.
#[test]
fn rendered_package_is_accepted_by_the_wit_parser() -> Result<(), Box<dyn Error>> {
    let schema = analysis_schema()?;
    let package = schema.resolve()?;
    let rendered_wit = package.to_string();

    let mut parser = wit_parser::Resolve::default();
    let parse_result = parser.push_str("analysis.wit", &rendered_wit);

    assert!(
        parse_result.is_ok(),
        "wit-parser rejected: {parse_result:?}"
    );
    Ok(())
}

/// Two instantiations share the default name `outcome`. Keeping either one
/// would drop a contract type, so both Rust types are reported instead.
#[test]
fn unnamed_instantiations_of_one_generic_type_are_rejected() -> Result<(), Box<dyn Error>> {
    let package_name = example_package_name()?;
    let mut schema = Schema::new(package_name);
    schema
        .interface("native")
        .define::<InspectionOutcome>()
        .define::<SelectionOutcome>();

    let resolution = schema.resolve();

    let errors = resolution.expect_err("colliding instantiations must not resolve");
    let outcome_clash = errors.iter().find_map(|error| match error {
        SchemaError::DuplicateTypeName { name, origins, .. } if name == "outcome" => Some(origins),
        _ => None,
    });
    let origins = outcome_clash.expect("the clash on `outcome` should be reported");
    let names_instantiation = |payload: &str| origins.iter().any(|origin| origin.contains(payload));
    assert!(
        names_instantiation("Report") && names_instantiation("SelectionState"),
        "both instantiations should be named, got {origins:?}"
    );
    Ok(())
}

#[derive(WitType)]
struct Header {
    range: FileRange,
}

#[derive(WitType)]
struct Segment {
    range: FileRange,
}

/// `FileRange` is declared nowhere and reached from both interfaces. Defining
/// it twice, or in whichever interface came first, would be an arbitrary
/// contract decision.
#[test]
fn undeclared_type_shared_by_two_interfaces_is_rejected() -> Result<(), Box<dyn Error>> {
    let package_name = example_package_name()?;
    let mut schema = Schema::new(package_name);
    schema.interface("headers").define::<Header>();
    schema.interface("segments").define::<Segment>();

    let resolution = schema.resolve();

    let errors = resolution.expect_err("a shared type without an owner must not resolve");
    let reported: Vec<&SchemaError> = errors.iter().collect();
    let expected_interfaces = ["`headers`".to_owned(), "`segments`".to_owned()];
    assert!(
        matches!(
            reported.as_slice(),
            [SchemaError::AmbiguousPlacement { rust_type, interfaces }]
                if rust_type.ends_with("FileRange") && *interfaces == expected_interfaces
        ),
        "unexpected errors: {reported:?}"
    );
    Ok(())
}

#[derive(WitType)]
struct Directory {
    children: Vec<Directory>,
}

/// A self-containing type has no WIT representation. Reflection must finish
/// and report the cycle rather than recurse until the stack overflows.
#[test]
fn recursive_types_are_reported() -> Result<(), Box<dyn Error>> {
    let package_name = example_package_name()?;
    let mut schema = Schema::new(package_name);
    schema.interface("tree").define::<Directory>();

    let resolution = schema.resolve();

    let errors = resolution.expect_err("a recursive type must not resolve");
    let reports_cycle = errors.iter().any(|error| {
        matches!(
            error,
            SchemaError::RecursiveType { cycle }
                if cycle.len() == 2 && cycle.iter().all(|name| name.ends_with("Directory"))
        )
    });
    assert!(reports_cycle, "unexpected errors: {errors}");
    Ok(())
}
