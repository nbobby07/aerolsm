//! Fundamental value types shared across every AeroLSM component.
//!
//! These types are deliberately tiny and dependency-free so they can flow
//! through traits ([`crate::MemTable`], [`crate::StorageBackend`], ...) without
//! pulling a runtime or serialization framework into the core crate.

use std::fmt;
use std::sync::Arc;

/// A monotonically increasing logical timestamp assigned to every write.
///
/// Sequence numbers give AeroLSM a total order over mutations. When the same
/// user key is written more than once, the entry with the highest `SeqNum`
/// wins. They are also what lets future layers (SSTables, snapshots, MVCC
/// reads) reason about "what did the database look like at time `T`".
pub type SeqNum = u64;

/// An immutable, cheaply clonable byte buffer.
///
/// `Bytes` is the universal currency of keys and values in AeroLSM. It wraps an
/// [`Arc<[u8]>`], so cloning is a single atomic reference-count bump rather than
/// a heap copy. This is the foundation of AeroLSM's zero-copy story: the same
/// backing allocation can be shared between the MemTable, an iterator, a
/// read response, and (later) an SSTable block without duplication.
///
/// Ordering and equality are defined purely by byte content, matching the
/// lexicographic ordering an LSM relies on for sorted iteration.
///
/// # Examples
///
/// ```
/// use aerolsm_core::Bytes;
///
/// let a = Bytes::from("apple");
/// let b = Bytes::from(b"banana".to_vec());
///
/// assert!(a < b);
/// assert_eq!(a.as_slice(), b"apple");
///
/// // Cloning is O(1): it shares the same allocation.
/// let a2 = a.clone();
/// assert_eq!(a, a2);
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bytes(Arc<[u8]>);

impl Bytes {
    /// Creates a `Bytes` by copying the given slice into a fresh allocation.
    #[must_use]
    pub fn copy_from_slice(data: &[u8]) -> Self {
        Self(Arc::from(data))
    }

    /// Returns a view of the underlying bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns the number of bytes in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the buffer contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::ops::Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::borrow::Borrow<[u8]> for Bytes {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self(Arc::from(value.into_boxed_slice()))
    }
}

impl From<&[u8]> for Bytes {
    fn from(value: &[u8]) -> Self {
        Self::copy_from_slice(value)
    }
}

impl<const N: usize> From<&[u8; N]> for Bytes {
    fn from(value: &[u8; N]) -> Self {
        Self::copy_from_slice(value)
    }
}

impl From<&str> for Bytes {
    fn from(value: &str) -> Self {
        Self::copy_from_slice(value.as_bytes())
    }
}

impl From<String> for Bytes {
    fn from(value: String) -> Self {
        Self::from(value.into_bytes())
    }
}

/// Renders printable ASCII verbatim and escapes everything else, so logs stay
/// readable for the common case of textual keys without lying about binary data.
impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b\"")?;
        for &byte in self.0.iter() {
            match byte {
                b'\\' => write!(f, "\\\\")?,
                b'"' => write!(f, "\\\"")?,
                0x20..=0x7e => write!(f, "{}", byte as char)?,
                _ => write!(f, "\\x{byte:02x}")?,
            }
        }
        write!(f, "\"")
    }
}

/// What is stored against a key inside a MemTable (or any LSM layer).
///
/// In a log-structured engine a deletion is not the physical removal of data;
/// it is the *insertion* of a tombstone that shadows older values during reads
/// and is only reclaimed during compaction. Modelling that explicitly keeps the
/// write path uniform: every mutation is an append of a [`ValueEntry`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueEntry {
    /// A live value written by the user.
    Value(Bytes),
    /// A tombstone marking the key as deleted at some sequence number.
    Tombstone,
}

impl ValueEntry {
    /// Returns `true` if this entry is a [`ValueEntry::Tombstone`].
    #[must_use]
    pub fn is_tombstone(&self) -> bool {
        matches!(self, ValueEntry::Tombstone)
    }

    /// Returns the contained value, or `None` if this is a tombstone.
    #[must_use]
    pub fn value(&self) -> Option<&Bytes> {
        match self {
            ValueEntry::Value(v) => Some(v),
            ValueEntry::Tombstone => None,
        }
    }
}

/// The outcome of looking a key up in a single LSM layer.
///
/// This is distinct from `Option<Bytes>` on purpose. A layered LSM read walks
/// from the freshest layer (the MemTable) to the oldest (SSTables). It must be
/// able to tell three states apart:
///
/// * [`Lookup::Found`] - the key exists here; stop and return it.
/// * [`Lookup::Deleted`] - the key was deleted here; stop and report "absent".
/// * `None` (the surrounding [`Option`]) - the key is simply not in this layer;
///   keep searching older layers.
///
/// Collapsing "deleted here" into "not here" would let a stale value from an
/// older SSTable incorrectly resurface, so the distinction is load-bearing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// The key is present with the given value.
    Found(Bytes),
    /// The key is present as a tombstone (deleted).
    Deleted,
}

impl Lookup {
    /// Collapses the layer-aware result into the user-facing answer for this
    /// layer: `Some(value)` for a live value, `None` for a tombstone.
    #[must_use]
    pub fn into_value(self) -> Option<Bytes> {
        match self {
            Lookup::Found(v) => Some(v),
            Lookup::Deleted => None,
        }
    }
}

impl From<ValueEntry> for Lookup {
    fn from(entry: ValueEntry) -> Self {
        match entry {
            ValueEntry::Value(v) => Lookup::Found(v),
            ValueEntry::Tombstone => Lookup::Deleted,
        }
    }
}
