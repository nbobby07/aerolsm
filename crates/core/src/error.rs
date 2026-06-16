//! The shared error type for AeroLSM.
//!
//! The error type is hand-written rather than derived with `thiserror` so the
//! core crate stays completely dependency-free. It implements the standard
//! [`std::error::Error`] trait, so it composes with `?`, `Box<dyn Error>`, and
//! any error-reporting crate a downstream user prefers.

use std::fmt;

/// A convenient [`Result`] alias used throughout AeroLSM.
///
/// [`Result`]: std::result::Result
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can be produced by AeroLSM components.
///
/// The variants are intentionally coarse for Phase 1. As storage backends and
/// compaction land, this enum will grow (always additively) to describe richer
/// failure modes such as corruption or I/O faults.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// An I/O operation failed in a storage backend.
    Io(std::io::Error),

    /// The engine, or one of its components, has been shut down and can no
    /// longer accept operations.
    Closed,

    /// Stored data failed an integrity check (bad magic, checksum mismatch,
    /// truncated record, ...).
    Corruption(String),

    /// An operation received an argument that violates an invariant.
    InvalidArgument(String),

    /// A component-specific error described by a human-readable message.
    ///
    /// This is the escape hatch for pluggable implementations that need to
    /// surface a failure without first extending this enum.
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::Closed => write!(f, "engine component is closed"),
            Error::Corruption(msg) => write!(f, "data corruption: {msg}"),
            Error::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
            Error::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Io(value)
    }
}
