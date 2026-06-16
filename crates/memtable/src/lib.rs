//! Lock-free insert-only skiplist MemTable.

#![deny(missing_docs)]

mod memtable;
mod skiplist;

pub use memtable::SkipListMemTable;
