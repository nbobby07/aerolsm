//! Core types and pluggable traits for AeroLSM.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Error types.
pub mod error;
/// Pluggable subsystem traits.
pub mod traits;
/// Shared value types.
pub mod types;

pub use error::{Error, Result};
pub use traits::{
    CompactionPolicy, CompactionTask, MemTable, SsTableId, SsTableMeta, StorageBackend,
};
pub use types::{Bytes, Lookup, MemtableEntry, SeqNum, ValueEntry};
