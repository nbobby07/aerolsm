//! WAL, SSTables, and [`StorageBackend`] implementations.

#![deny(missing_docs)]

mod backends;
mod codec;
mod sstable;
mod wal;

pub use aerolsm_core::StorageBackend;
pub use backends::{FileBackend, MemoryBackend};
pub use sstable::{FOOTER_SIZE, Footer, IndexEntry, SsTableReader, SsTableWriter, flush_memtable};
pub use wal::{WalOpKind, WalReader, WalRecord, WalWriter};
