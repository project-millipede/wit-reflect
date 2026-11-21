//! Fixture: an ordinary Rust library whose contract is generated.
//!
//! The types and operations here are written as a library would write them,
//! without wire types or conversion code. The derives and marks are the only
//! additions, and they are the whole declaration of the contract: each item
//! states its own part. The `generate` binary renders the WIT, the bindings
//! file and the forwarding file that the `session-component` fixture
//! includes; that crate's `Cargo.toml` holds the package and world.
//!
//! The [`foreign`] module stands for a dependency crate: its types carry no
//! derive and hide their fields.

#![allow(
    missing_docs,
    reason = "a fixture; the documented items are the ones the generated contract shows"
)]

use std::error::Error;
use std::fmt;

use wit_reflect::{WitResource, WitType, export, exports, named};

pub mod foreign {
    use std::fmt;

    #[derive(Clone, Copy, Debug)]
    pub enum Endianness {
        Little,
        Big,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct FileType(u32);

    impl FileType {
        #[must_use]
        pub fn new(value: u32) -> Self {
            Self(value)
        }

        #[must_use]
        pub fn into_inner(self) -> u32 {
            self.0
        }

        #[must_use]
        pub fn name(self) -> Option<&'static str> {
            (self.0 == 2).then_some("MH_EXECUTE")
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct CpuSubtype(u32);

    impl CpuSubtype {
        #[must_use]
        pub fn new(value: u32) -> Self {
            Self(value)
        }

        #[must_use]
        pub fn into_inner(self) -> u32 {
            self.0
        }
    }

    #[derive(Clone, Copy, Debug)]
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

#[derive(Debug, WitType)]
#[wit(remote = foreign::Endianness)]
pub enum Endianness {
    Little,
    Big,
}

/// Numeric evidence with an optional canonical name.
// Public fields, `Clone` and `Debug`: the bindings use this type itself.
#[derive(Clone, Debug, WitType)]
pub struct MachoValue {
    pub value: u32,
    pub name: Option<String>,
}

impl From<foreign::FileType> for MachoValue {
    fn from(file_type: foreign::FileType) -> Self {
        Self {
            value: file_type.into_inner(),
            name: file_type.name().map(str::to_owned),
        }
    }
}

/// An input-file range.
// The fields are private; the derive reads them without accessors.
#[derive(Debug, WitType)]
pub struct FileRange {
    offset: u64,
    size: u64,
}

impl FileRange {
    #[must_use]
    pub fn new(offset: u64, size: u64) -> Self {
        Self { offset, size }
    }
}

#[derive(Debug, WitType)]
pub struct Image {
    #[wit(into = Endianness)]
    pub endianness: foreign::Endianness,
    #[wit(into = MachoValue)]
    pub file_type: foreign::FileType,
    #[wit(into = MachoValue, with = Image::subtype_value)]
    pub cpu_subtype: foreign::CpuSubtype,
    #[wit(display)]
    pub minimum_os: foreign::Version,
    pub file_range: FileRange,
    #[wit(widen)]
    pub read_count: usize,
}

impl Image {
    /// The subtype's name depends on the image's file type.
    fn subtype_value(&self) -> MachoValue {
        let is_executable = self.file_type.into_inner() == 2;
        MachoValue {
            value: self.cpu_subtype.into_inner(),
            name: is_executable.then(|| "ARM64_ALL".to_owned()),
        }
    }
}

#[derive(Debug, WitType)]
#[wit(interface = "binary")]
pub struct Report {
    pub byte_length: u64,
    pub images: Vec<Image>,
}

#[derive(Debug, WitType)]
pub enum ErrorKind {
    #[wit(payload = "truncated-range")]
    Truncated {
        requested_size: u64,
        available_size: u64,
    },
    UnsupportedImage,
    Decode(#[wit(display)] foreign::DecodeError),
}

#[derive(Debug, WitType)]
#[wit(
    interface = "binary",
    field(name = "message", into = String, with = ToString::to_string)
)]
pub struct AnalysisError {
    pub kind: ErrorKind,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Truncated {
                requested_size,
                available_size,
            } => write!(
                formatter,
                "requested {requested_size} bytes, {available_size} available"
            ),
            ErrorKind::UnsupportedImage => formatter.write_str("unsupported image"),
            ErrorKind::Decode(error) => write!(formatter, "cannot decode: {error}"),
        }
    }
}

impl Error for AnalysisError {}

// Not `Clone`, so the bindings cannot use it: it is converted.
#[derive(Debug, WitType)]
pub enum FailureKind {
    Transport,
    Provider,
}

#[derive(Debug, WitType)]
#[wit(field(name = "message", into = String, with = ToString::to_string))]
pub struct Failure {
    pub kind: FailureKind,
    #[wit(skip)]
    pub cause: Option<Box<dyn Error + Send + Sync>>,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            Some(cause) => write!(formatter, "filesystem operation failed: {cause}"),
            None => formatter.write_str("filesystem operation failed"),
        }
    }
}

impl Error for Failure {}

#[derive(Debug, WitType)]
#[wit(
    field(name = "message", into = String, with = ToString::to_string),
    cases = "kind"
)]
pub enum InspectionFailure {
    Selection(Failure),
    Analysis(AnalysisError),
}

impl fmt::Display for InspectionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(failure) => write!(formatter, "cannot validate selection: {failure}"),
            Self::Analysis(error) => error.fmt(formatter),
        }
    }
}

impl Error for InspectionFailure {}

// Crosses unchanged, also where a generic carrier holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, WitType)]
pub enum StepKind {
    QuerySelection,
    Inspect,
}

/// What the session has done so far.
// Everything inside crosses unchanged, so this record does too.
#[derive(Clone, Debug, WitType)]
pub struct Progress {
    pub last_step: Option<StepKind>,
    pub inspections: u64,
}

/// A step's identity and original result.
#[derive(Debug, WitType)]
pub struct StepOutcome<Kind, Output, Failed> {
    pub kind: Kind,
    pub result: Result<Output, Failed>,
}

/// Completed steps in execution order.
#[derive(Debug, WitType)]
pub struct Outcome<Step> {
    pub steps: Vec<Step>,
}

// WIT has no generic types: each instantiation is a definition of its own,
// named after the alias that names it here.
#[named]
pub type InspectionStepOutcome = StepOutcome<StepKind, Report, InspectionFailure>;
#[named]
pub type InspectionOutcome = Outcome<InspectionStepOutcome>;

/// A client that remembers the selected resource between calls.
#[derive(Debug, WitResource)]
#[wit(interface = "native")]
pub struct Session {
    selection: Option<String>,
    inspections: usize,
    last_step: Option<StepKind>,
}

#[exports]
impl Session {
    /// Remember `resource` as the current selection.
    #[export]
    pub fn select(&mut self, resource: &str) {
        self.selection = Some(resource.to_owned());
    }

    /// The current selection, if any.
    #[export]
    #[must_use]
    pub fn selection(&self) -> Option<String> {
        self.selection.clone()
    }

    /// Inspect `resource`. It must be the current selection.
    // The names `truncated` and `undecodable` stand for inputs that fail
    // analysis; every other selected name is inspected successfully.
    #[export]
    pub fn inspect(&mut self, resource: &str) -> InspectionOutcome {
        self.inspections += 1;
        self.last_step = Some(StepKind::Inspect);
        let result = if self.selection.as_deref() == Some(resource) {
            self.analyze(resource)
        } else {
            let cause = format!("`{resource}` is not selected");
            Err(InspectionFailure::Selection(Failure {
                kind: FailureKind::Provider,
                cause: Some(cause.into()),
            }))
        };
        Outcome {
            steps: vec![StepOutcome {
                kind: StepKind::Inspect,
                result,
            }],
        }
    }

    /// The last step and the number of inspections so far.
    #[export]
    #[must_use]
    pub fn progress(&self) -> Progress {
        Progress {
            last_step: self.last_step,
            inspections: u64::try_from(self.inspections).unwrap_or(u64::MAX),
        }
    }

    fn analyze(&self, resource: &str) -> Result<Report, InspectionFailure> {
        let kind = match resource {
            "truncated" => ErrorKind::Truncated {
                requested_size: 32,
                available_size: 8,
            },
            "undecodable" => ErrorKind::Decode(foreign::DecodeError),
            _ => {
                let image = Image {
                    endianness: foreign::Endianness::Little,
                    file_type: foreign::FileType::new(2),
                    cpu_subtype: foreign::CpuSubtype::new(2),
                    minimum_os: foreign::Version {
                        major: 14,
                        minor: 4,
                    },
                    file_range: FileRange::new(0, 4096),
                    read_count: self.inspections,
                };
                return Ok(Report {
                    byte_length: 4096,
                    images: vec![image],
                });
            }
        };
        Err(InspectionFailure::Analysis(AnalysisError { kind }))
    }
}

/// Create a session without a selection.
#[export(interface = "native")]
#[must_use]
pub fn create_session() -> Session {
    Session {
        selection: None,
        inspections: 0,
        last_step: None,
    }
}
