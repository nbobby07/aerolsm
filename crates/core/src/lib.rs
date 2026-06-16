//! # AeroLSM Core
//!
//! `aerolsm-core` defines the vocabulary and the pluggable trait surface that
//! the rest of AeroLSM is built on. It is intentionally:
//!
//! * **dependency-free** - it pulls in no third-party crates, and
//! * **runtime-agnostic** - it does not depend on Tokio, async-std, or any I/O
//!   runtime. Async methods are expressed with native `async fn` in traits, so
//!   any runtime (or none) can drive them.
//!
//! ## What lives here
//!
//! * Core value types: [`Bytes`], [`SeqNum`], [`ValueEntry`], [`Lookup`].
//! * The error type: [`Error`] and [`Result`].
//! * The architecture-defining traits: [`MemTable`], [`StorageBackend`], and
//!   [`CompactionPolicy`].
//!
//! Concrete implementations live in sibling crates (`aerolsm-memtable`,
//! `aerolsm-storage`, `aerolsm-compaction`) so that the contract and its
//! implementations can evolve independently. This separation is what makes
//! AeroLSM approachable for contributors: pick a trait, write an impl, plug it
//! in.
//!
//! ```
//! use aerolsm_core::{Bytes, ValueEntry};
//!
//! let key = Bytes::from("agent:42:memory");
//! let entry = ValueEntry::Value(Bytes::from(b"embedding-id-7".to_vec()));
//! assert_eq!(entry.value().map(Bytes::as_slice), Some(&b"embedding-id-7"[..]));
//! # let _ = key;
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod traits;
pub mod types;

pub use error::{Error, Result};
pub use traits::{
    CompactionPolicy, CompactionTask, MemTable, SsTableId, SsTableMeta, StorageBackend,
};
pub use types::{Bytes, Lookup, SeqNum, ValueEntry};
