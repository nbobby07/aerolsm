use std::fmt;
use std::sync::Arc;

use aerolsm_core::{Error, Result, StorageBackend};

use crate::wal::record::{WalRecord, decode_record, validate_header};

/// WAL replay reader.
pub struct WalReader<B: StorageBackend> {
    backend: Arc<B>,
}

impl<B: StorageBackend> fmt::Debug for WalReader<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WalReader").finish_non_exhaustive()
    }
}

impl<B: StorageBackend> WalReader<B> {
    /// Creates a reader over `backend`.
    #[must_use]
    pub fn new(backend: Arc<B>) -> Self {
        Self { backend }
    }

    /// Replays all records in order.
    pub async fn replay(&self) -> Result<Vec<WalRecord>> {
        let len = usize::try_from(self.backend.len().await?).unwrap_or(usize::MAX);
        if len == 0 {
            return Ok(Vec::new());
        }
        let bytes = self.backend.read_at(0, len).await?;
        let data = bytes.as_slice();
        let mut offset = validate_header(data)?;
        let mut records = Vec::new();
        while offset < data.len() {
            records.push(decode_record(data, &mut offset)?);
        }
        if offset != data.len() {
            return Err(Error::Corruption("wal trailing garbage".into()));
        }
        Ok(records)
    }
}
