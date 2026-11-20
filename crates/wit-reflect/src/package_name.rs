//! WIT package identity: `namespace:name@version`.

use std::fmt;
use std::str::FromStr;

use crate::identifier::{self, IdentifierError};

/// A validated WIT package name such as `apple-code:analysis@0.5.0`.
///
/// Parse it from text; the version is optional:
///
/// ```
/// use wit_reflect::PackageName;
///
/// let package_name: PackageName = "example:analysis@0.5.0".parse()?;
/// assert_eq!(package_name.to_string(), "example:analysis@0.5.0");
/// # Ok::<(), wit_reflect::PackageNameError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageName {
    namespace: String,
    name: String,
    version: Option<semver::Version>,
}

impl FromStr for PackageName {
    type Err = PackageNameError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| PackageNameError {
            input: input.to_owned(),
            reason,
        };

        let (path, version_text) = match input.split_once('@') {
            Some((path, version_text)) => (path, Some(version_text)),
            None => (input, None),
        };
        let Some((namespace, name)) = path.split_once(':') else {
            return Err(invalid(Reason::MissingNamespace));
        };
        identifier::validate(namespace).map_err(|error| invalid(Reason::Namespace(error)))?;
        identifier::validate(name).map_err(|error| invalid(Reason::Name(error)))?;

        let version = match version_text {
            Some(version_text) => {
                let parsed_version = semver::Version::parse(version_text)
                    .map_err(|error| invalid(Reason::Version(error.to_string())))?;
                Some(parsed_version)
            }
            None => None,
        };

        Ok(Self {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            version,
        })
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.namespace, self.name)?;
        if let Some(version) = &self.version {
            write!(formatter, "@{version}")?;
        }
        Ok(())
    }
}

/// A package name that does not follow `namespace:name` or `namespace:name@version`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid WIT package name `{input}`: {reason}")]
pub struct PackageNameError {
    input: String,
    reason: Reason,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
enum Reason {
    #[error("expected `namespace:name`")]
    MissingNamespace,
    #[error("the namespace is not a WIT identifier because {0}")]
    Namespace(IdentifierError),
    #[error("the name is not a WIT identifier because {0}")]
    Name(IdentifierError),
    /// The semantic-version parser's message. Its error type stays out of this
    /// crate's public API.
    #[error("the version is not a semantic version: {0}")]
    Version(String),
}
