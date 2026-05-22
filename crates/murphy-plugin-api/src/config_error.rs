//! Error type for decoding a cop's `[cops.rules."Name"]` config table.
//!
//! Produced by [`CopOptions::from_config_json`](crate::CopOptions::from_config_json)
//! — usually the `#[derive(CopOptions)]`-generated implementation — and
//! reused by the validation gate (murphy-9cr.9) so config diagnostics
//! share one vocabulary.

use std::fmt;

/// A failure decoding a cop option table from JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    kind: ConfigErrorKind,
}

/// The specific kind of [`ConfigError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigErrorKind {
    /// The config blob was not valid JSON.
    Parse(String),
    /// The top-level JSON value was not an object.
    NotAnObject,
    /// A field was present but the wrong JSON type.
    TypeMismatch {
        /// Option key.
        field: String,
        /// Expected wire type (`"bool"` / `"int"` / `"string"` /
        /// `"string_list"`).
        expected: &'static str,
    },
    /// A `String` field carried a value outside its `enum_values` set.
    EnumViolation {
        /// Option key.
        field: String,
        /// The offending value.
        value: String,
    },
    /// A required field (no default, not `Option<_>`) was absent.
    MissingRequired {
        /// Option key.
        field: String,
    },
}

impl ConfigError {
    /// Wrap a `serde_json` syntax error.
    pub fn parse(err: serde_json::Error) -> Self {
        Self {
            kind: ConfigErrorKind::Parse(err.to_string()),
        }
    }

    /// The top-level JSON value was not an object.
    pub fn not_an_object() -> Self {
        Self {
            kind: ConfigErrorKind::NotAnObject,
        }
    }

    /// A field had the wrong JSON type.
    pub fn type_mismatch(field: impl Into<String>, expected: &'static str) -> Self {
        Self {
            kind: ConfigErrorKind::TypeMismatch {
                field: field.into(),
                expected,
            },
        }
    }

    /// A `String` field's value was outside its `enum_values` set.
    pub fn enum_violation(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            kind: ConfigErrorKind::EnumViolation {
                field: field.into(),
                value: value.into(),
            },
        }
    }

    /// A required field was absent from the config table.
    pub fn missing_required(field: impl Into<String>) -> Self {
        Self {
            kind: ConfigErrorKind::MissingRequired {
                field: field.into(),
            },
        }
    }

    /// The underlying error kind.
    pub fn kind(&self) -> &ConfigErrorKind {
        &self.kind
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ConfigErrorKind::Parse(msg) => write!(f, "config is not valid JSON: {msg}"),
            ConfigErrorKind::NotAnObject => {
                write!(f, "config must be a JSON object")
            }
            ConfigErrorKind::TypeMismatch { field, expected } => {
                write!(f, "option `{field}` must be a {expected}")
            }
            ConfigErrorKind::EnumViolation { field, value } => {
                write!(f, "option `{field}` has disallowed value `{value}`")
            }
            ConfigErrorKind::MissingRequired { field } => {
                write!(f, "required option `{field}` is missing")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
