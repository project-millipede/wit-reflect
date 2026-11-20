//! WIT identifier grammar: validation and keyword escaping.
//!
//! This module is the single owner of both rules. Names arrive from derives and
//! from schema declarations; all of them are checked here during resolution.

use std::fmt;

/// Reserved words of the WIT grammar, as lexed by `wit-parser` 0.261.
///
/// An identifier equal to one of these must be written with a leading `%`.
const KEYWORDS: &[&str] = &[
    "as",
    "async",
    "bool",
    "borrow",
    "char",
    "constructor",
    "enum",
    "error-context",
    "export",
    "f32",
    "f64",
    "flags",
    "from",
    "func",
    "future",
    "import",
    "include",
    "interface",
    "list",
    "map",
    "option",
    "own",
    "package",
    "record",
    "resource",
    "result",
    "s16",
    "s32",
    "s64",
    "s8",
    "static",
    "stream",
    "string",
    "tuple",
    "type",
    "u16",
    "u32",
    "u64",
    "u8",
    "use",
    "variant",
    "with",
    "world",
];

/// Why a name is not a valid WIT identifier.
///
/// A WIT identifier is one or more words separated by `-`. Each word is either
/// all lowercase or all uppercase ASCII and may contain digits. The identifier
/// must start with a letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdentifierError {
    /// The name has no characters.
    #[error("it is empty")]
    Empty,
    /// Two `-` are adjacent, or the name starts or ends with `-`.
    #[error("it has an empty word between `-` separators")]
    EmptyWord,
    /// The first character is not an ASCII letter.
    #[error("it starts with `{0}` instead of an ASCII letter")]
    LeadingCharacter(char),
    /// One word contains both uppercase and lowercase letters.
    #[error("one of its words mixes uppercase and lowercase letters")]
    MixedCase,
    /// A character outside ASCII letters, digits and `-`.
    #[error("it contains `{0}`; only ASCII letters, digits and `-` are allowed")]
    InvalidCharacter(char),
}

/// Check `name` against the WIT identifier grammar.
///
/// A keyword is a valid identifier; [`Escaped`] writes its `%` prefix.
pub(crate) fn validate(name: &str) -> Result<(), IdentifierError> {
    let Some(leading_character) = name.chars().next() else {
        return Err(IdentifierError::Empty);
    };
    if leading_character == '-' {
        return Err(IdentifierError::EmptyWord);
    }
    if !leading_character.is_ascii_alphabetic() {
        return Err(IdentifierError::LeadingCharacter(leading_character));
    }

    for word in name.split('-') {
        validate_word(word)?;
    }
    Ok(())
}

fn validate_word(word: &str) -> Result<(), IdentifierError> {
    if word.is_empty() {
        return Err(IdentifierError::EmptyWord);
    }

    let mut has_uppercase = false;
    let mut has_lowercase = false;
    for character in word.chars() {
        if character.is_ascii_uppercase() {
            has_uppercase = true;
        } else if character.is_ascii_lowercase() {
            has_lowercase = true;
        } else if !character.is_ascii_digit() {
            return Err(IdentifierError::InvalidCharacter(character));
        }
    }

    if has_uppercase && has_lowercase {
        return Err(IdentifierError::MixedCase);
    }
    Ok(())
}

/// Displays an identifier as WIT source, prefixing keywords with `%`.
pub(crate) struct Escaped<'name>(pub(crate) &'name str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if KEYWORDS.contains(&self.0) {
            formatter.write_str("%")?;
        }
        formatter.write_str(self.0)
    }
}
