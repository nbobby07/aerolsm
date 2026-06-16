//! [`SkipListMemTable`]: AeroLSM's default [`MemTable`] implementation.

use std::fmt;

use aerolsm_core::{Bytes, Lookup, MemTable, Result, SeqNum, ValueEntry};

use crate::skiplist::SkipList;

/// A thread-safe, lock-free MemTable backed by an insert-only skiplist.
///
/// `SkipListMemTable` is the reference implementation of [`MemTable`]. It is
/// designed for the AI-agent workloads AeroLSM targets: many concurrent tasks
/// reading and writing small keys (agent memory slots, vector metadata) with
/// minimal contention. All operations take `&self`, so a single instance can be
/// wrapped in an [`std::sync::Arc`] and shared across an entire async runtime.
///
/// * **Writes** ([`insert`](MemTable::insert) / [`delete`](MemTable::delete))
///   are lock-free: they link a node, or swap a value pointer, with a CAS.
/// * **Reads** ([`get`](MemTable::get)) are wait-free traversals.
/// * **Deletes** insert a tombstone rather than unlinking, preserving the
///   layered-read semantics an LSM needs.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use aerolsm_core::{Bytes, Lookup, MemTable};
/// use aerolsm_memtable::SkipListMemTable;
///
/// # tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
/// let mt = Arc::new(SkipListMemTable::new());
///
/// mt.insert(Bytes::from("agent:1:goal"), Bytes::from("ship aerolsm"), 1).await.unwrap();
/// assert_eq!(
///     mt.get(b"agent:1:goal").await.unwrap(),
///     Some(Lookup::Found(Bytes::from("ship aerolsm"))),
/// );
///
/// // A delete is a tombstone, distinguishable from "never written".
/// mt.delete(Bytes::from("agent:1:goal"), 2).await.unwrap();
/// assert_eq!(mt.get(b"agent:1:goal").await.unwrap(), Some(Lookup::Deleted));
/// assert_eq!(mt.get(b"agent:1:unknown").await.unwrap(), None);
/// # });
/// ```
pub struct SkipListMemTable {
    list: SkipList,
}

impl SkipListMemTable {
    /// Creates a new, empty MemTable.
    #[must_use]
    pub fn new() -> Self {
        Self {
            list: SkipList::new(),
        }
    }
}

impl Default for SkipListMemTable {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SkipListMemTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkipListMemTable")
            .field("len", &self.list.entry_count())
            .field("approximate_size", &self.list.byte_size())
            .finish()
    }
}

impl MemTable for SkipListMemTable {
    async fn insert(&self, key: Bytes, value: Bytes, seq: SeqNum) -> Result<()> {
        self.list.insert(key, ValueEntry::Value(value), seq);
        Ok(())
    }

    async fn delete(&self, key: Bytes, seq: SeqNum) -> Result<()> {
        self.list.insert(key, ValueEntry::Tombstone, seq);
        Ok(())
    }

    async fn get(&self, key: &[u8]) -> Result<Option<Lookup>> {
        Ok(self.list.get(key).map(Lookup::from))
    }

    fn len(&self) -> usize {
        self.list.entry_count()
    }

    fn approximate_size(&self) -> usize {
        self.list.byte_size()
    }

    fn iter(&self) -> impl Iterator<Item = (Bytes, ValueEntry)> + '_ {
        self.list.snapshot().into_iter()
    }
}
