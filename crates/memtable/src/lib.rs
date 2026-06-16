//! # AeroLSM MemTable
//!
//! This crate provides AeroLSM's default in-memory write buffer,
//! [`SkipListMemTable`], built on a from-scratch **lock-free, insert-only
//! skiplist**.
//!
//! It implements the [`aerolsm_core::MemTable`] trait, so it can be swapped for
//! any alternative MemTable a contributor wishes to plug in. See the `skiplist`
//! module source for the design rationale behind the insert-only,
//! reclamation-free concurrency strategy.
//!
//! ```
//! use std::sync::Arc;
//! use aerolsm_core::MemTable;
//! use aerolsm_memtable::SkipListMemTable;
//!
//! # tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
//! let mt = Arc::new(SkipListMemTable::new());
//! mt.insert("k".into(), "v".into(), 1).await.unwrap();
//! assert_eq!(mt.len(), 1);
//! # });
//! ```
//!
//! Unsafe code is confined to the `skiplist` module and is accompanied by
//! safety documentation on every `unsafe` item.

#![deny(missing_docs)]

mod memtable;
mod skiplist;

pub use memtable::SkipListMemTable;
