use std::fmt;
use std::sync::Arc;

use aerolsm_core::{Bytes, MemtableEntry, Result, SsTableId, SsTableMeta, StorageBackend};

use crate::codec::crc32;
use crate::sstable::format::{
    FOOTER_SIZE, Footer, IndexEntry, encode_data_entry, encode_footer, encode_index,
};

/// Writes immutable SSTables.
pub struct SsTableWriter<B: StorageBackend> {
    backend: Arc<B>,
}

impl<B: StorageBackend> fmt::Debug for SsTableWriter<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SsTableWriter").finish_non_exhaustive()
    }
}

impl<B: StorageBackend> SsTableWriter<B> {
    /// Creates a writer over `backend`.
    #[must_use]
    pub fn new(backend: Arc<B>) -> Self {
        Self { backend }
    }

    /// Writes sorted `entries` and returns metadata.
    pub async fn write(
        &self,
        id: SsTableId,
        level: usize,
        entries: impl IntoIterator<Item = MemtableEntry>,
    ) -> Result<SsTableMeta> {
        let entries: Vec<MemtableEntry> = entries.into_iter().collect();
        let entry_count = entries.len();

        let mut data = Vec::new();
        let mut index = Vec::with_capacity(entry_count);
        let mut data_offset_acc = 0u64;

        for entry in &entries {
            index.push(IndexEntry {
                key: entry.key.clone(),
                offset: data_offset_acc,
            });
            let (encoded, _, _, _) = encode_data_entry(entry);
            data_offset_acc = data_offset_acc
                .checked_add(u64::try_from(encoded.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| aerolsm_core::Error::InvalidArgument("sstable too large".into()))?;
            data.extend_from_slice(&encoded);
        }

        let index_bytes = encode_index(&index);
        let data_offset = 0u64;
        let index_offset = u64::try_from(data.len()).map_err(|_| {
            aerolsm_core::Error::InvalidArgument("sstable data section too large".into())
        })?;

        let mut body = Vec::with_capacity(data.len() + index_bytes.len());
        body.extend_from_slice(&data);
        body.extend_from_slice(&index_bytes);
        let body_crc = crc32(&body);

        let footer = Footer {
            data_offset,
            index_offset,
            entry_count: u64::try_from(entry_count).unwrap_or(u64::MAX),
            body_crc,
        };
        let footer_bytes = encode_footer(&footer);

        self.backend.append(&body).await?;
        self.backend.append(&footer_bytes).await?;
        self.backend.sync().await?;

        let size_bytes = u64::try_from(body.len() + FOOTER_SIZE).unwrap_or(u64::MAX);
        let smallest_key = entries
            .first()
            .map(|e| e.key.clone())
            .unwrap_or_else(|| Bytes::from(""));
        let largest_key = entries
            .last()
            .map(|e| e.key.clone())
            .unwrap_or_else(|| Bytes::from(""));

        Ok(SsTableMeta {
            id,
            level,
            smallest_key,
            largest_key,
            size_bytes,
            entry_count: u64::try_from(entry_count).unwrap_or(u64::MAX),
        })
    }
}

/// Flushes a [`MemTable`](aerolsm_core::MemTable) into a new SSTable.
pub async fn flush_memtable<M, B>(
    memtable: &M,
    backend: Arc<B>,
    id: SsTableId,
    level: usize,
) -> Result<SsTableMeta>
where
    M: aerolsm_core::MemTable + ?Sized,
    B: StorageBackend,
{
    let entries: Vec<MemtableEntry> = memtable.iter().collect();
    SsTableWriter::new(backend).write(id, level, entries).await
}
