use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aerolsm_core::{Bytes, Error, Result, StorageBackend};

/// File-backed [`StorageBackend`].
#[derive(Debug)]
pub struct FileBackend {
    path: PathBuf,
    inner: Mutex<FileState>,
}

#[derive(Debug)]
struct FileState {
    file: File,
    len: u64,
}

impl FileBackend {
    /// Creates or truncates `path`.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            path,
            inner: Mutex::new(FileState { file, len: 0 }),
        })
    }

    /// Opens an existing file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
        let len = file.seek(SeekFrom::End(0))?;
        Ok(Self {
            path,
            inner: Mutex::new(FileState { file, len }),
        })
    }

    /// Returns the backing path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl StorageBackend for FileBackend {
    async fn append(&self, data: &[u8]) -> Result<u64> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        guard.file.seek(SeekFrom::End(0))?;
        let offset = guard.len;
        guard.file.write_all(data)?;
        guard.len = guard
            .len
            .checked_add(u64::try_from(data.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| Error::InvalidArgument("file size overflow".into()))?;
        Ok(offset)
    }

    async fn read_at(&self, offset: u64, len: usize) -> Result<Bytes> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        if offset
            .checked_add(u64::try_from(len).unwrap_or(u64::MAX))
            .is_none_or(|end| end > guard.len)
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read past end of file",
            )));
        }
        guard.file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        guard.file.read_exact(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    async fn sync(&self) -> Result<()> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        guard.file.sync_all()?;
        Ok(())
    }

    async fn len(&self) -> Result<u64> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::Other("poisoned lock".into()))?;
        Ok(guard.len)
    }
}
