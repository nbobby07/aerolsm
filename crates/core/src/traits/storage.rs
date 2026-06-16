//! The [`StorageBackend`] trait: the durable byte-addressable substrate.

use std::future::Future;

use crate::error::Result;
use crate::types::Bytes;

/// An append-mostly, randomly-readable durable byte store.
///
/// `StorageBackend` is the seam between AeroLSM's logical layers (WAL, SSTables)
/// and the physical world. By hiding all I/O behind this trait, the engine can
/// run on top of:
///
/// * buffered standard files (the portable default, Phase 2),
/// * `io_uring` on Linux for zero-syscall-overhead async I/O,
/// * object storage (S3-style) for disaggregated deployments,
/// * an in-memory buffer for tests.
///
/// The contract is deliberately small: LSM files are written once, sequentially,
/// then read at arbitrary offsets many times, and never mutated in place. That
/// "write-once, read-many" shape is what makes a pluggable backend tractable.
///
/// # Zero-copy reads
///
/// [`StorageBackend::read_at`] returns [`Bytes`] rather than filling a
/// caller-provided `&mut [u8]`. This lets backends that already hold the data in
/// memory (a page cache, an `mmap`, an `io_uring` registered buffer) hand back a
/// reference-counted slice without copying.
///
/// This trait will gain default implementations in the `aerolsm-storage` crate
/// during Phase 2; for now it defines the standard every backend will meet.
pub trait StorageBackend: Send + Sync + 'static {
    /// Appends `data` to the end of the backing store and returns the byte
    /// offset at which it was written.
    ///
    /// Appends are logically atomic with respect to other appends: the returned
    /// offset uniquely identifies this record's position.
    fn append(&self, data: &[u8]) -> impl Future<Output = Result<u64>> + Send;

    /// Reads exactly `len` bytes starting at `offset`.
    ///
    /// Returns [`crate::Error::Io`] (with an unexpected-EOF kind) if fewer than
    /// `len` bytes are available from `offset`.
    fn read_at(&self, offset: u64, len: usize) -> impl Future<Output = Result<Bytes>> + Send;

    /// Flushes all previously appended data to the durable medium.
    ///
    /// After this future resolves successfully, the data is guaranteed to
    /// survive a process or power loss (subject to the medium's own
    /// guarantees).
    fn sync(&self) -> impl Future<Output = Result<()>> + Send;

    /// Returns the current length of the backing store in bytes.
    fn len(&self) -> impl Future<Output = Result<u64>> + Send;

    /// Returns `true` if the backing store is empty.
    fn is_empty(&self) -> impl Future<Output = Result<bool>> + Send {
        async { Ok(self.len().await? == 0) }
    }
}
