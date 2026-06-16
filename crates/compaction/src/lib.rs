//! # AeroLSM Compaction
//!
//! Concrete [`CompactionPolicy`] strategies that decide which SSTables AeroLSM
//! merges, and when.
//!
//! The [`CompactionPolicy`] trait (and its metadata types [`SsTableMeta`],
//! [`CompactionTask`], [`SsTableId`]) are defined in `aerolsm-core` and
//! re-exported here for convenience.
//!
//! ## Roadmap
//!
//! Phase 1 (current) ships only the trait seam. The strategies themselves land
//! once SSTables exist:
//!
//! * **size-tiered** - write-optimized; merges similarly sized runs.
//! * **leveled** - read/space-optimized; maintains non-overlapping sorted runs.
//! * **FIFO / TTL** - drops the oldest data; ideal for ephemeral agent scratch
//!   state.
//!
//! Because policies are pure decision functions over [`SsTableMeta`], they are
//! trivial to unit test in isolation. Want to contribute one? Implement
//! [`CompactionPolicy`] and open a PR - see `CONTRIBUTING.md`.

#![deny(missing_docs)]

pub use aerolsm_core::{CompactionPolicy, CompactionTask, SsTableId, SsTableMeta};
