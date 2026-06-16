//! Pluggable subsystem traits.

/// Compaction policy trait and metadata.
pub mod compaction;
/// MemTable trait.
pub mod memtable;
/// Storage backend trait.
pub mod storage;

pub use compaction::{CompactionPolicy, CompactionTask, SsTableId, SsTableMeta};
pub use memtable::MemTable;
pub use storage::StorageBackend;
