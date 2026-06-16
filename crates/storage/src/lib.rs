//! # AeroLSM Storage
//!
//! Concrete [`StorageBackend`] implementations that give AeroLSM its durability.
//!
//! This crate is the home for the pluggable I/O layer. The trait it builds on,
//! [`StorageBackend`], is defined in `aerolsm-core` and re-exported here for
//! convenience.
//!
//! ## Roadmap
//!
//! Phase 1 (current) ships only the trait seam so the architecture is visible to
//! contributors. Implementations land in later phases:
//!
//! * **Phase 2** - a portable buffered-file backend and an in-memory backend for
//!   tests.
//! * **Phase 3+** - a Linux `io_uring` backend for zero-syscall-overhead async
//!   I/O, and an object-storage backend for disaggregated deployments.
//!
//! Want to contribute a backend? Implement [`StorageBackend`] and open a PR -
//! see `CONTRIBUTING.md` at the repository root.

#![deny(missing_docs)]

pub use aerolsm_core::StorageBackend;
