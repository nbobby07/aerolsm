use std::fmt;

/// AeroLSM result type.
pub type Result<T> = std::result::Result<T, Error>;

/// AeroLSM error type.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// I/O failure.
    Io(std::io::Error),
    /// Component is closed.
    Closed,
    /// On-disk corruption.
    Corruption(String),
    /// Invalid argument.
    InvalidArgument(String),
    /// Other failure.
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
