//! The pluggable trait surface that defines AeroLSM's architecture.
//!
//! Every major subsystem is expressed as a trait so contributors can swap in
//! their own implementation without touching the engine core:
//!
//! * [`MemTable`] - the in-memory write buffer,
//! * [`StorageBackend`] - the durable byte substrate (files, `io_uring`, ...),
//! * [`CompactionPolicy`] - the strategy for merging SSTables.

pub mod compaction;
pub mod memtable;
pub mod storage;

pub use compaction::{CompactionPolicy, CompactionTask, SsTableId, SsTableMeta};
pub use memtable::MemTable;
pub use storage::StorageBackend;
