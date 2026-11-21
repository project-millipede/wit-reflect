//! Representation rules: wire shapes that differ from the stored Rust types.
//!
//! The fixture mirrors the gaps in the workbench's owner types. The `foreign`
//! module stands for a dependency crate: its types carry no derive and hide
//! their fields, so only rules on the owner types can describe them.

#![allow(dead_code, reason = "fixture items are reflected, never used")]

use std::error::Error;
use std::fmt;

use wit_reflect::{Schema, WitType};

mod foreign {
    use std::fmt;

    pub enum Endianness {
        Little,
        Big,
    }

    #[derive(Clone, Copy)]
    pub struct FileType(u32);

    impl FileType {
        pub fn into_inner(self) -> u32 {
            self.0
        }

        pub fn name(self) -> Option<&'static str> {
            (self.0 == 2).then_some("MH_EXECUTE")
        }
    }

    #[derive(Clone, Copy)]
    pub struct CpuSubtype(u32);

    impl CpuSubtype {
        pub fn into_inner(self) -> u32 {
            self.0
        }
    }

    pub struct Version {
        pub major: u16,
        pub minor: u8,
    }

    impl fmt::Display for Version {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{}.{}", self.major, self.minor)
        }
    }

    #[derive(Debug)]
    pub struct DecodeError;

    impl fmt::Display for DecodeError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("invalid magic")
        }
    }
}

// `remote`: a local declaration the compiler checks against the foreign enum.
#[derive(WitType)]
#[wit(remote = foreign::Endianness)]
enum Endianness {
    Little,
    Big,
}

// One wire record shared by every wrapped foreign number.
#[derive(WitType)]
struct MachoValue {
    value: u32,
    name: Option<String>,
}

impl From<foreign::FileType> for MachoValue {
    fn from(file_type: foreign::FileType) -> Self {
        Self {
            value: file_type.into_inner(),
            name: file_type.name().map(str::to_owned),
        }
    }
}

#[derive(WitType)]
struct Image {
    #[wit(into = Endianness)]
    endianness: foreign::Endianness,
    #[wit(into = MachoValue)]
    file_type: foreign::FileType,
    // The subtype's name depends on the sibling file type.
    #[wit(into = MachoValue, with = Image::subtype_value)]
    cpu_subtype: foreign::CpuSubtype,
    #[wit(display)]
    minimum_os: foreign::Version,
    header_flags: u32,
}

impl Image {
    fn subtype_value(&self) -> MachoValue {
        let is_executable = self.file_type.into_inner() == 2;
        MachoValue {
            value: self.cpu_subtype.into_inner(),
            name: is_executable.then(|| "ARM64_ALL".to_owned()),
        }
    }
}

#[derive(WitType)]
enum InputError {
    UnexpectedEof {
        offset: u64,
        remaining: u64,
    },
    InvalidReadCount {
        #[wit(widen)]
        reported: usize,
        #[wit(widen)]
        capacity: usize,
    },
    Source(#[wit(display)] std::io::Error),
}

#[derive(WitType)]
#[wit(transparent)]
struct InputErrors {
    errors: Vec<InputError>,
}

#[derive(WitType)]
enum ErrorKind {
    RangeOverflow,
    Decode(#[wit(display)] foreign::DecodeError),
}

#[derive(WitType)]
#[wit(field(name = "message", into = String, with = ToString::to_string))]
struct AnalysisError {
    kind: ErrorKind,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("analysis failed")
    }
}

#[derive(WitType)]
enum FailureKind {
    Transport,
    Provider,
}

#[derive(WitType)]
#[wit(field(name = "message", into = String, with = ToString::to_string))]
struct Failure {
    kind: FailureKind,
    #[wit(skip)]
    retries: usize,
    #[wit(skip)]
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("filesystem operation failed")
    }
}

// An enum with a computed field is a record that holds its cases.
#[derive(WitType)]
#[wit(
    field(name = "message", into = String, with = ToString::to_string),
    cases = "kind"
)]
enum InspectionFailure {
    Selection(Failure),
    Analysis(AnalysisError),
    Input(InputErrors),
}

impl fmt::Display for InspectionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("inspection failed")
    }
}

/// The WIT a maintainer would write by hand for the ruled owner types.
const EXPECTED_RULES_WIT: &str = "\
package example:rules@0.1.0;

interface inspection {
  enum endianness {
    little,
    big,
  }

  record macho-value {
    value: u32,
    name: option<string>,
  }

  record image {
    endianness: endianness,
    file-type: macho-value,
    cpu-subtype: macho-value,
    minimum-os: string,
    header-flags: u32,
  }

  enum failure-kind {
    transport,
    provider,
  }

  record failure {
    message: string,
    kind: failure-kind,
  }

  variant error-kind {
    range-overflow,
    decode(string),
  }

  record analysis-error {
    message: string,
    kind: error-kind,
  }

  record input-error-unexpected-eof {
    offset: u64,
    remaining: u64,
  }

  record input-error-invalid-read-count {
    reported: u64,
    capacity: u64,
  }

  variant input-error {
    unexpected-eof(input-error-unexpected-eof),
    invalid-read-count(input-error-invalid-read-count),
    source(string),
  }

  variant inspection-failure-kind {
    selection(failure),
    analysis(analysis-error),
    input(list<input-error>),
  }

  record inspection-failure {
    message: string,
    kind: inspection-failure-kind,
  }
}
";

/// Each rule replaces the stored type with its declared wire type: foreign
/// types through `remote`, `into` and a sibling-dependent `with`, formatted
/// values through `display`, `usize` through `widen`. Skipped fields are
/// absent, and a computed message leads a struct's record and turns an enum
/// into a record of its cases.
#[test]
fn ruled_owner_types_render_their_declared_wire_shapes() -> Result<(), Box<dyn Error>> {
    let package_name = "example:rules@0.1.0".parse()?;
    let mut schema = Schema::new(package_name);
    schema
        .interface("inspection")
        .define::<Image>()
        .define::<InspectionFailure>();

    let package = schema.resolve()?;
    let rendered_wit = package.to_string();

    assert_eq!(rendered_wit, EXPECTED_RULES_WIT);
    Ok(())
}
