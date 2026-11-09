//! Typed errors. Every error maps to one stable CLI exit code (SPEC §10).

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum Error {
    #[error("usage or request schema error: {0}")]
    Request(String),
    #[error("I/O error on {path}: {message}")]
    Io { path: String, message: String },
    #[error("output already exists: {0}")]
    OutputExists(String),
    #[error("resource limit exceeded: {0}")]
    Limit(String),
    #[error("malformed {what}: {detail}")]
    Malformed { what: &'static str, detail: String },
    #[error("BIOS/profile identity mismatch: {0}")]
    IdentityMismatch(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("setting validation failed: {0}")]
    Validation(String),
    #[error("unresolved migration conflicts: {0}")]
    Unresolved(String),
    #[error("generated output verification failed: {0}")]
    Verification(String),
    #[error("input binding changed: {0}")]
    BindingChanged(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    pub fn malformed(what: &'static str, detail: impl Into<String>) -> Self {
        Error::Malformed {
            what,
            detail: detail.into(),
        }
    }

    /// Stable exit code (SPEC §10).
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Internal(_) => 1,
            Error::Request(_) => 2,
            Error::Io { .. } | Error::OutputExists(_) | Error::Limit(_) => 3,
            Error::Malformed { .. } | Error::IdentityMismatch(_) => 4,
            Error::Unsupported(_) => 5,
            Error::Validation(_) => 6,
            Error::Unresolved(_) => 7,
            Error::Verification(_) => 8,
            Error::BindingChanged(_) => 9,
        }
    }

    /// Stable machine-readable code for JSON envelopes.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Internal(_) => "internal",
            Error::Request(_) => "request_invalid",
            Error::Io { .. } => "io",
            Error::OutputExists(_) => "output_exists",
            Error::Limit(_) => "resource_limit",
            Error::Malformed { .. } => "malformed_input",
            Error::IdentityMismatch(_) => "identity_mismatch",
            Error::Unsupported(_) => "unsupported",
            Error::Validation(_) => "validation_failed",
            Error::Unresolved(_) => "unresolved_conflicts",
            Error::Verification(_) => "verification_failed",
            Error::BindingChanged(_) => "binding_changed",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
