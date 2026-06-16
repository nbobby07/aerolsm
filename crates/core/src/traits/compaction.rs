use crate::types::Bytes;

/// SSTable file identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SsTableId(
    /// File number.
    pub u64,
);

/// SSTable metadata used by compaction policies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SsTableMeta {
    /// File id.
    pub id: SsTableId,
    /// LSM level.
    pub level: usize,
    /// Smallest key.
    pub smallest_key: Bytes,
    /// Largest key.
    pub largest_key: Bytes,
    /// On-disk size in bytes.
    pub size_bytes: u64,
    /// Entry count.
    pub entry_count: u64,
}

/// Compaction work unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionTask {
    /// Input SSTables.
    pub inputs: Vec<SsTableId>,
    /// Output level.
    pub output_level: usize,
}

/// Picks the next compaction to run.
pub trait CompactionPolicy: Send + Sync + 'static {
    /// Policy name.
    fn name(&self) -> &str;

    /// Returns the next compaction, if any.
    fn pick_compaction(&self, levels: &[Vec<SsTableMeta>]) -> Option<CompactionTask>;
}
