use std::sync::RwLock;

use aerolsm_core::{Bytes, Error, Result, StorageBackend};

/// In-memory [`StorageBackend`].
#[derive(Debug, Default)]
pub struct MemoryBackend {
    data: RwLock<Vec<u8>>,
}

impl MemoryBackend {
    /// Creates an empty backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a copy of all stored bytes.
    pub fn snapshot(&self) -> Vec<u8> {
        self.data.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl StorageBackend for MemoryBackend {
    async fn append(&self, data: &[u8]) -> Result<u64> {
        let mut guard = self
            .data
            .write()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        let offset =
            u64::try_from(guard.len()).map_err(|_| Error::InvalidArgument("buffer full".into()))?;
        guard.extend_from_slice(data);
        Ok(offset)
    }

    async fn read_at(&self, offset: u64, len: usize) -> Result<Bytes> {
        let guard = self
            .data
            .read()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        let start = usize::try_from(offset)
            .map_err(|_| Error::InvalidArgument("offset out of range".into()))?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| Error::InvalidArgument("read overflow".into()))?;
        if end > guard.len() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read past end of buffer",
            )));
        }
        Ok(Bytes::copy_from_slice(&guard[start..end]))
    }

    async fn sync(&self) -> Result<()> {
        Ok(())
    }

    async fn len(&self) -> Result<u64> {
        let guard = self
            .data
            .read()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        Ok(u64::try_from(guard.len()).unwrap_or(u64::MAX))
    }
}

impl From<Vec<u8>> for MemoryBackend {
    fn from(data: Vec<u8>) -> Self {
        Self {
            data: RwLock::new(data),
        }
    }
}
