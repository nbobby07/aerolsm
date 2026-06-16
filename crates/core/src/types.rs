use std::fmt;
use std::sync::Arc;

/// Monotonic write sequence number.
pub type SeqNum = u64;

/// Reference-counted immutable byte buffer.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bytes(Arc<[u8]>);

impl Bytes {
    /// Copies `data` into a new buffer.
    #[must_use]
    pub fn copy_from_slice(data: &[u8]) -> Self {
        Self(Arc::from(data))
    }

    /// Returns the underlying slice.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns the byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the buffer is empty.
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

/// Stored value or tombstone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueEntry {
    /// Live value.
    Value(Bytes),
    /// Deletion marker.
    Tombstone,
}

impl ValueEntry {
    /// Returns whether this entry is a tombstone.
    #[must_use]
    pub fn is_tombstone(&self) -> bool {
        matches!(self, ValueEntry::Tombstone)
    }

    /// Returns the value, if any.
    #[must_use]
    pub fn value(&self) -> Option<&Bytes> {
        match self {
            ValueEntry::Value(v) => Some(v),
            ValueEntry::Tombstone => None,
        }
    }
}

/// Result of a single-layer key lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// Key present with a value.
    Found(Bytes),
    /// Key present as a tombstone.
    Deleted,
}

impl Lookup {
    /// Returns the value, or `None` for a tombstone.
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

/// One MemTable row with its winning sequence number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemtableEntry {
    /// User key.
    pub key: Bytes,
    /// Value or tombstone.
    pub entry: ValueEntry,
    /// Sequence number of the winning write.
    pub seq: SeqNum,
}
