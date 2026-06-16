//! The [`MemTable`] trait: the in-memory write buffer of the LSM.

use std::future::Future;

use crate::error::Result;
use crate::types::{Bytes, Lookup, SeqNum, ValueEntry};

/// An in-memory, ordered, mutable buffer that absorbs writes before they are
/// flushed to immutable on-disk SSTables.
///
/// The MemTable is the hottest component of an LSM: every write lands here
/// first, and every read consults it before touching disk. Because AeroLSM
/// targets ultra-high-concurrency AI-agent workloads, implementations are
/// expected to support **many concurrent writers and readers** sharing the
/// table through `&self` (note: there is no `&mut self` anywhere in this
/// trait). The default implementation, `SkipListMemTable`, achieves this with a
/// lock-free skiplist.
///
/// # Why are the methods `async`?
///
/// AeroLSM is async-first. A purely in-memory MemTable will resolve its futures
/// immediately, but expressing the contract as `async` lets exotic
/// implementations (e.g. a MemTable backed by persistent memory, an RDMA region,
/// or a tiered buffer that spills to disk) participate without changing the
/// interface. The trait uses native `async fn` in traits, so there is **no
/// `async-trait` dependency** and no boxing on the hot path.
///
/// # Tombstones
///
/// Deletes are writes. [`MemTable::delete`] inserts a [`ValueEntry::Tombstone`]
/// rather than removing data, which is what allows the deletion to shadow older
/// values living in SSTables until compaction reclaims them.
///
/// # Example
///
/// A minimal, illustrative implementation built on a `Mutex<BTreeMap>` (the real
/// engine uses a lock-free skiplist instead):
///
/// ```
/// use std::collections::BTreeMap;
/// use std::sync::Mutex;
/// use aerolsm_core::{Bytes, Lookup, MemTable, Result, SeqNum, ValueEntry};
///
/// #[derive(Default)]
/// struct MapMemTable {
///     inner: Mutex<BTreeMap<Bytes, (SeqNum, ValueEntry)>>,
///     bytes: std::sync::atomic::AtomicUsize,
/// }
///
/// impl MemTable for MapMemTable {
///     async fn insert(&self, key: Bytes, value: Bytes, seq: SeqNum) -> Result<()> {
///         self.bytes.fetch_add(key.len() + value.len(), std::sync::atomic::Ordering::Relaxed);
///         self.inner.lock().unwrap().insert(key, (seq, ValueEntry::Value(value)));
///         Ok(())
///     }
///     async fn delete(&self, key: Bytes, seq: SeqNum) -> Result<()> {
///         self.inner.lock().unwrap().insert(key, (seq, ValueEntry::Tombstone));
///         Ok(())
///     }
///     async fn get(&self, key: &[u8]) -> Result<Option<Lookup>> {
///         Ok(self.inner.lock().unwrap().get(key).map(|(_, v)| v.clone().into()))
///     }
///     fn len(&self) -> usize { self.inner.lock().unwrap().len() }
///     fn approximate_size(&self) -> usize {
///         self.bytes.load(std::sync::atomic::Ordering::Relaxed)
///     }
///     fn iter(&self) -> impl Iterator<Item = (Bytes, ValueEntry)> + '_ {
///         let snapshot: Vec<_> = self
///             .inner
///             .lock()
///             .unwrap()
///             .iter()
///             .map(|(k, (_, v))| (k.clone(), v.clone()))
///             .collect();
///         snapshot.into_iter()
///     }
/// }
///
/// # let _example = async {
/// let mt = MapMemTable::default();
/// mt.insert(Bytes::from("k"), Bytes::from("v"), 1).await?;
/// assert_eq!(mt.get(b"k").await?, Some(Lookup::Found(Bytes::from("v"))));
/// mt.delete(Bytes::from("k"), 2).await?;
/// assert_eq!(mt.get(b"k").await?, Some(Lookup::Deleted));
/// # Ok::<(), aerolsm_core::Error>(())
/// # };
/// ```
pub trait MemTable: Send + Sync + 'static {
    /// Inserts (or overwrites) `key` with `value` at sequence number `seq`.
    ///
    /// Writes are last-writer-wins ordered by `seq`: a write with a higher
    /// sequence number supersedes a lower one for the same key. Callers are
    /// responsible for assigning monotonically increasing sequence numbers.
    fn insert(
        &self,
        key: Bytes,
        value: Bytes,
        seq: SeqNum,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Records a deletion of `key` at sequence number `seq` by inserting a
    /// [`ValueEntry::Tombstone`].
    fn delete(&self, key: Bytes, seq: SeqNum) -> impl Future<Output = Result<()>> + Send;

    /// Looks `key` up in this MemTable.
    ///
    /// Returns:
    /// * `Ok(Some(Lookup::Found(v)))` if a live value is present,
    /// * `Ok(Some(Lookup::Deleted))` if a tombstone is present,
    /// * `Ok(None)` if the key has never been written to this MemTable.
    ///
    /// See [`Lookup`] for why the deleted/absent distinction matters.
    fn get(&self, key: &[u8]) -> impl Future<Output = Result<Option<Lookup>>> + Send;

    /// Returns the number of distinct keys currently stored (including
    /// tombstones).
    fn len(&self) -> usize;

    /// Returns `true` if the MemTable holds no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns an estimate, in bytes, of the user data held by this MemTable.
    ///
    /// This drives the flush decision: when `approximate_size` crosses a
    /// configured threshold, the engine seals this MemTable and rotates in a
    /// fresh one. It is an estimate (it need not account for structural
    /// overhead) and must be cheap to call.
    fn approximate_size(&self) -> usize;

    /// Returns an iterator over the latest entry for each key, in ascending key
    /// order.
    ///
    /// This is the basis for flushing a sealed MemTable into a sorted SSTable in
    /// Phase 2. Implementations should provide a stable, point-in-time view;
    /// concurrent writes made after the iterator is created may or may not be
    /// observed, but the iterator must never expose a torn or out-of-order
    /// entry.
    fn iter(&self) -> impl Iterator<Item = (Bytes, ValueEntry)> + '_;
}
