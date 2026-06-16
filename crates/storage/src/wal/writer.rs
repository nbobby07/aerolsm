use std::fmt;
use std::sync::Arc;

use aerolsm_core::{Result, SeqNum, StorageBackend};

use crate::wal::record::{WalOpKind, encode_record, wal_header};

/// Append-only WAL writer.
pub struct WalWriter<B: StorageBackend> {
    backend: Arc<B>,
    initialized: bool,
}

impl<B: StorageBackend> fmt::Debug for WalWriter<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WalWriter")
            .field("initialized", &self.initialized)
            .finish_non_exhaustive()
    }
}

impl<B: StorageBackend> WalWriter<B> {
    /// Creates a writer over `backend`.
    #[must_use]
    pub fn new(backend: Arc<B>) -> Self {
        Self {
            backend,
            initialized: false,
        }
    }

    async fn ensure_header(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }
        self.backend.append(&wal_header()).await?;
        self.initialized = true;
        Ok(())
    }

    /// Appends a put record.
    pub async fn append_put(&mut self, seq: SeqNum, key: &[u8], value: &[u8]) -> Result<u64> {
        self.ensure_header().await?;
        let record = encode_record(seq, WalOpKind::Put, key, Some(value));
        self.backend.append(&record).await
    }

    /// Appends a delete record.
    pub async fn append_delete(&mut self, seq: SeqNum, key: &[u8]) -> Result<u64> {
        self.ensure_header().await?;
        let record = encode_record(seq, WalOpKind::Delete, key, None);
        self.backend.append(&record).await
    }

    /// Flushes the backing store.
    pub async fn sync(&self) -> Result<()> {
        self.backend.sync().await
    }
}
