use std::future::Future;

use crate::error::Result;
use crate::types::Bytes;

/// Append-mostly durable byte store.
pub trait StorageBackend: Send + Sync + 'static {
    /// Appends `data` and returns its offset.
    fn append(&self, data: &[u8]) -> impl Future<Output = Result<u64>> + Send;

    /// Reads `len` bytes at `offset`.
    fn read_at(&self, offset: u64, len: usize) -> impl Future<Output = Result<Bytes>> + Send;

    /// Durably flushes appended data.
    fn sync(&self) -> impl Future<Output = Result<()>> + Send;

    /// Returns the store length in bytes.
    fn len(&self) -> impl Future<Output = Result<u64>> + Send;

    /// Returns whether the store is empty.
    fn is_empty(&self) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(self.len().await? == 0) }
    }
}
