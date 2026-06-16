use std::future::Future;

use crate::error::Result;
use crate::types::{Bytes, Lookup, MemtableEntry, SeqNum};

/// In-memory write buffer flushed to SSTables.
pub trait MemTable: Send + Sync + 'static {
    /// Inserts or overwrites `key` at `seq`.
    fn insert(
        &self,
        key: Bytes,
        value: Bytes,
        seq: SeqNum,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Tombstones `key` at `seq`.
    fn delete(&self, key: Bytes, seq: SeqNum) -> impl Future<Output = Result<()>> + Send;

    /// Looks up `key` in this layer.
    fn get(&self, key: &[u8]) -> impl Future<Output = Result<Option<Lookup>>> + Send;

    /// Number of distinct keys, including tombstones.
    fn len(&self) -> usize;

    /// Returns whether the table is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Approximate user-data size in bytes.
    fn approximate_size(&self) -> usize;

    /// Latest entry per key, sorted by key.
    fn iter(&self) -> impl Iterator<Item = MemtableEntry> + '_;
}
