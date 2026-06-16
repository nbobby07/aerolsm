use aerolsm_core::{Bytes, Error, Lookup, MemtableEntry, Result, StorageBackend};

use crate::codec::crc32;
use crate::sstable::format::{
    FOOTER_SIZE, Footer, IndexEntry, decode_data_entry, decode_footer, decode_index,
};

/// Reads immutable SSTables.
#[derive(Debug)]
pub struct SsTableReader {
    footer: Footer,
    index: Vec<IndexEntry>,
    data: Bytes,
}

impl SsTableReader {
    /// Opens and validates an SSTable in `backend`.
    pub async fn open<B: StorageBackend>(backend: &B) -> Result<Self> {
        let file_len = backend.len().await?;
        if file_len < FOOTER_SIZE as u64 {
            return Err(Error::Corruption("sstable file too small".into()));
        }
        let footer_offset = file_len - FOOTER_SIZE as u64;
        let footer_bytes = backend.read_at(footer_offset, FOOTER_SIZE).await?;
        let footer = decode_footer(footer_bytes.as_slice())?;

        let body_len = usize::try_from(file_len - FOOTER_SIZE as u64)
            .map_err(|_| Error::Corruption("sstable length overflow".into()))?;
        let body = backend.read_at(0, body_len).await?;
        if crc32(body.as_slice()) != footer.body_crc {
            return Err(Error::Corruption("sstable body checksum mismatch".into()));
        }

        let data_len = usize::try_from(footer.index_offset)
            .map_err(|_| Error::Corruption("data offset overflow".into()))?;
        if data_len > body.len() {
            return Err(Error::Corruption("data section out of bounds".into()));
        }
        let data = Bytes::copy_from_slice(&body.as_slice()[..data_len]);
        let index = decode_index(&body.as_slice()[data_len..])?;

        if u64::try_from(index.len()).unwrap_or(u64::MAX) != footer.entry_count {
            return Err(Error::Corruption("sstable entry count mismatch".into()));
        }

        Ok(Self {
            footer,
            index,
            data,
        })
    }

    /// Returns the parsed footer.
    #[must_use]
    pub fn footer(&self) -> &Footer {
        &self.footer
    }

    /// Returns the entry count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Point lookup in this layer.
    pub fn get(&self, key: &[u8]) -> Result<Option<Lookup>> {
        let pos = self.index.binary_search_by(|e| e.key.as_slice().cmp(key));
        let Ok(idx) = pos else {
            return Ok(None);
        };
        let entry = decode_data_entry(self.data.as_slice(), self.index[idx].offset)?;
        Ok(Some(Lookup::from(entry.entry)))
    }

    /// Returns entries in sorted key order.
    pub fn iter(&self) -> impl Iterator<Item = MemtableEntry> + '_ {
        self.index.iter().map(|idx| {
            decode_data_entry(self.data.as_slice(), idx.offset)
                .expect("index offsets validated at open time")
        })
    }

    /// Returns the smallest key, if any.
    #[must_use]
    pub fn smallest_key(&self) -> Option<&Bytes> {
        self.index.first().map(|e| &e.key)
    }

    /// Returns the largest key, if any.
    #[must_use]
    pub fn largest_key(&self) -> Option<&Bytes> {
        self.index.last().map(|e| &e.key)
    }
}
