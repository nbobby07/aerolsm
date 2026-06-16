//! The [`CompactionPolicy`] trait and its supporting metadata types.

use crate::types::Bytes;

/// A stable identifier for a single on-disk SSTable file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SsTableId(
    /// The raw, monotonically assigned file number.
    pub u64,
);

/// Lightweight metadata describing one SSTable, without its payload.
///
/// A compaction policy reasons purely over this metadata (sizes, key ranges,
/// levels) to decide *what* to compact; it never reads the actual key/value
/// blocks. Keeping the decision input this small makes policies cheap to run and
/// trivial to unit test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SsTableMeta {
    /// Unique identifier of the SSTable.
    pub id: SsTableId,
    /// The LSM level this SSTable currently resides on (0 == freshest).
    pub level: usize,
    /// Smallest user key contained in the SSTable (inclusive).
    pub smallest_key: Bytes,
    /// Largest user key contained in the SSTable (inclusive).
    pub largest_key: Bytes,
    /// Approximate on-disk size in bytes.
    pub size_bytes: u64,
    /// Number of entries (including tombstones) in the SSTable.
    pub entry_count: u64,
}

/// A unit of compaction work selected by a [`CompactionPolicy`].
///
/// It names the inputs (by id) and the level their merged output should be
/// written to. *Executing* the task (merging, writing the new SSTable, updating
/// the manifest) is the engine's job in Phase 3; the policy only decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionTask {
    /// SSTables to be merged together.
    pub inputs: Vec<SsTableId>,
    /// The destination level for the merged output.
    pub output_level: usize,
}

/// A pluggable strategy that decides which SSTables to merge, and when.
///
/// Compaction is where an LSM trades write amplification, read amplification,
/// and space amplification against one another. Different workloads want
/// different trade-offs, so AeroLSM treats the strategy as a first-class plugin:
///
/// * size-tiered (write-optimized) - merge similarly sized files,
/// * leveled (read/space-optimized) - keep non-overlapping sorted runs,
/// * FIFO / TTL - drop the oldest data, ideal for ephemeral agent scratch state,
/// * cost-based - custom heuristics for vector-metadata access patterns.
///
/// Policies are pure, synchronous decision functions: given the current shape of
/// the tree they return the next [`CompactionTask`], or `None` if nothing is
/// worth doing right now. Keeping them side-effect-free makes them easy to test
/// in isolation and safe to call from the engine's scheduler.
pub trait CompactionPolicy: Send + Sync + 'static {
    /// A short, human-readable name for diagnostics and metrics (e.g.
    /// `"size-tiered"`).
    fn name(&self) -> &str;

    /// Inspects the current per-level layout and returns the next compaction to
    /// run, or `None` if the tree is already in good shape.
    ///
    /// `levels[i]` lists the SSTables currently on level `i`. The slice is
    /// borrowed; implementations must not assume ownership.
    fn pick_compaction(&self, levels: &[Vec<SsTableMeta>]) -> Option<CompactionTask>;
}
