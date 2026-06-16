use std::fmt;

use aerolsm_core::{Bytes, Lookup, MemTable, MemtableEntry, Result, SeqNum, ValueEntry};

use crate::skiplist::SkipList;

/// Lock-free [`MemTable`] backed by an insert-only skiplist.
pub struct SkipListMemTable {
    list: SkipList,
}

impl SkipListMemTable {
    /// Creates an empty MemTable.
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

    fn iter(&self) -> impl Iterator<Item = MemtableEntry> + '_ {
        self.list
            .snapshot()
            .into_iter()
            .map(|(key, entry, seq)| MemtableEntry { key, entry, seq })
    }
}
