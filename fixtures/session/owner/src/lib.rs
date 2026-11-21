//! Fixture: an ordinary Rust library whose contract is generated.
//!
//! The types and operations here are written as a library would write them,
//! without wire types or conversion code. The derives and `#[export]` markers
//! are the only additions. [`schema`] declares the contract, and
//! [`generate`] renders the WIT, the bindings file and the forwarding file
//! that the `session-component` fixture includes.
//!
//! The [`foreign`] module stands for a dependency crate: its types carry no
//! derive and hide their fields.

#![allow(
    missing_docs,
    reason = "a fixture; the documented items are the ones the generated contract shows"
)]

// The schema names operations by absolute path, which also has to resolve here.
extern crate self as session_owner;

use std::error::Error;
use std::fmt;

use wit_reflect::{
    ForwardingErrors, Schema, SchemaErrors, WitResource, WitType, export, operation,
};

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
#[wit(field(name = "message", into = String, with = ToString::to_string))]
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

pub type InspectionStepOutcome = StepOutcome<StepKind, Report, InspectionFailure>;
pub type InspectionOutcome = Outcome<InspectionStepOutcome>;

/// A client that remembers the selected resource between calls.
#[derive(Debug, WitResource)]
pub struct Session {
    selection: Option<String>,
    inspections: usize,
    last_step: Option<StepKind>,
}

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
#[export]
#[must_use]
pub fn create_session() -> Session {
    Session {
        selection: None,
        inspections: 0,
        last_step: None,
    }
}

const WORLD: &str = "session";

/// The contract: which types and operations each interface exports.
///
/// # Errors
///
/// Returns an error if the package name is not valid.
pub fn schema() -> Result<Schema, Box<dyn Error>> {
    let package_name = "example:session@0.1.0".parse()?;
    let mut schema = Schema::new(package_name);
    schema
        .interface("binary")
        .define::<Report>()
        .define::<AnalysisError>();

    let mut native = schema.interface("native");
    native
        .define_as::<InspectionStepOutcome>("inspection-step-outcome")
        .define_as::<InspectionOutcome>("inspection-outcome");
    native
        .resource::<Session>()
        .method(operation!(session_owner::Session::select))
        .method(operation!(session_owner::Session::selection))
        .method(operation!(session_owner::Session::inspect))
        .method(operation!(session_owner::Session::progress));
    native.function(operation!(session_owner::create_session));

    schema
        .world(WORLD)
        .export_as("native", "native")
        .export("binary");
    Ok(schema)
}

/// The generated artifacts of the contract, as file contents.
#[derive(Debug, PartialEq, Eq)]
pub struct Artifacts {
    /// The WIT package.
    pub wit: String,
    /// The Rust source setting up the world's bindings.
    pub bindings: String,
    /// The Rust source implementing the world's exported interfaces.
    pub forwarding: String,
}

/// Why the artifacts could not be generated.
#[derive(Debug)]
pub enum GenerationError {
    /// The schema could not be declared.
    Declaration(Box<dyn Error>),
    /// The declared schema is not a valid contract.
    Schema(SchemaErrors),
    /// The component's Rust files cannot be generated from the contract.
    Forwarding(ForwardingErrors),
}

impl fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Declaration(error) => write!(formatter, "cannot declare the schema: {error}"),
            Self::Schema(errors) => errors.fmt(formatter),
            Self::Forwarding(errors) => errors.fmt(formatter),
        }
    }
}

impl Error for GenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Declaration(error) => Some(error.as_ref()),
            Self::Schema(errors) => Some(errors),
            Self::Forwarding(errors) => Some(errors),
        }
    }
}

/// Render the artifacts from [`schema`].
///
/// # Errors
///
/// Returns the schema's problems, or those of the generated Rust files; each
/// lists every problem found.
pub fn generate() -> Result<Artifacts, GenerationError> {
    let schema = schema().map_err(GenerationError::Declaration)?;
    let package = schema.resolve().map_err(GenerationError::Schema)?;
    let bindings = package
        .bindings(WORLD)
        .map_err(GenerationError::Forwarding)?;
    let forwarding = package
        .forwarding(WORLD)
        .map_err(GenerationError::Forwarding)?;
    Ok(Artifacts {
        wit: package.to_string(),
        bindings,
        forwarding,
    })
}
