//! A lock-free, insert-only skiplist - the data structure behind
//! [`SkipListMemTable`](crate::SkipListMemTable).
//!
//! # Why insert-only?
//!
//! In a log-structured engine the MemTable is a *write buffer*: it accumulates
//! mutations and is then flushed wholesale into an immutable SSTable, after which
//! it is dropped. Crucially, a "delete" is not the physical removal of a node -
//! it is the insertion of a [`ValueEntry::Tombstone`]. This means individual
//! nodes are **never unlinked or freed during the table's lifetime**.
//!
//! That single property is what makes a *from-scratch* lock-free implementation
//! tractable without epoch-based reclamation (e.g. `crossbeam-epoch`) or hazard
//! pointers. The hardest part of lock-free data structures is safely freeing
//! memory that a concurrent thread might still be reading. Here we sidestep it:
//!
//! * Nodes are allocated and linked with atomic CAS, and live until the whole
//!   structure is dropped.
//! * Overwriting a key's value swaps an [`AtomicPtr`] and *retires* the old
//!   value onto a lock-free stack; retired values are also only freed at
//!   [`Drop`].
//! * [`Drop`] takes `&mut self`, which statically guarantees no other thread is
//!   accessing the structure, so freeing everything in one pass is sound.
//!
//! Reads and writes are wait-free / lock-free respectively and require only
//! `&self`, so any number of threads (or async tasks) can share one skiplist.

use std::cell::Cell;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use aerolsm_core::{Bytes, SeqNum, ValueEntry};

/// Maximum number of levels a node tower can have. 32 levels comfortably indexes
/// billions of entries at branching factor 4.
const MAX_HEIGHT: usize = 32;

/// Inverse probability of promoting a node to the next level (1-in-4).
const BRANCHING: u64 = 4;

/// The value stored against a key, tagged with the sequence number that wrote
/// it so updates can enforce last-writer-wins.
struct ValueCell {
    seq: SeqNum,
    entry: ValueEntry,
}

/// A skiplist node. The `tower` has exactly `height` forward pointers; a node is
/// only ever linked into levels `0..height`, which guarantees that any node
/// reached while scanning level `L` has `height > L` (so indexing `tower[L]` is
/// always in bounds).
struct Node {
    key: Bytes,
    value: AtomicPtr<ValueCell>,
    tower: Box<[AtomicPtr<Node>]>,
}

/// A node in the lock-free "retired values" stack (a Treiber stack). Old value
/// cells are pushed here when overwritten and freed only at [`SkipList::drop`].
struct Retired {
    cell: *mut ValueCell,
    next: *mut Retired,
}

/// A concurrent, ordered, insert-only map from bytes to [`ValueEntry`].
pub(crate) struct SkipList {
    /// Sentinel head node with a full-height tower. Never freed until `drop`.
    head: *mut Node,
    /// Number of distinct keys (including tombstones).
    len: AtomicUsize,
    /// Approximate user bytes (keys + live value payloads) held.
    size: AtomicUsize,
    /// Lock-free stack of value cells awaiting reclamation at `drop`.
    retired: AtomicPtr<Retired>,
}

// SAFETY: Every field that is mutated after construction is an atomic, and all
// shared access goes through those atomics with appropriate orderings. The
// payload types (`Bytes`, `ValueEntry`) are themselves `Send + Sync`. No node or
// value cell is freed before `drop`, so there are no use-after-free hazards
// across threads. Therefore the structure is safe to share and move across
// threads.
unsafe impl Send for SkipList {}
unsafe impl Sync for SkipList {}

impl SkipList {
    /// Creates an empty skiplist.
    pub(crate) fn new() -> Self {
        let tower = (0..MAX_HEIGHT)
            .map(|_| AtomicPtr::new(ptr::null_mut()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let head = Box::into_raw(Box::new(Node {
            key: Bytes::copy_from_slice(&[]),
            value: AtomicPtr::new(ptr::null_mut()),
            tower,
        }));
        Self {
            head,
            len: AtomicUsize::new(0),
            size: AtomicUsize::new(0),
            retired: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Number of distinct keys currently stored (tombstones included).
    pub(crate) fn entry_count(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    /// Approximate user-data footprint in bytes.
    pub(crate) fn byte_size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// Inserts or overwrites `key` with `entry` at sequence number `seq`.
    ///
    /// Writes are last-writer-wins by `seq`: an update whose `seq` is lower than
    /// the value currently stored for the key is discarded.
    pub(crate) fn insert(&self, key: Bytes, entry: ValueEntry, seq: SeqNum) {
        let new_bytes = entry_bytes(&entry);
        let mut preds: [*mut Node; MAX_HEIGHT] = [self.head; MAX_HEIGHT];
        let mut succs: [*mut Node; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];

        loop {
            // SAFETY: `preds`/`succs` are fully populated by `find`, and every
            // pointer it returns is either the head or a live node.
            let existing = unsafe { self.find(key.as_slice(), &mut preds, &mut succs) };
            if !existing.is_null() {
                // SAFETY: `existing` is a live node owned by this list.
                unsafe { self.update_value(existing, entry, seq, new_bytes) };
                return;
            }

            let height = random_height();
            let mut tower_vec: Vec<AtomicPtr<Node>> = Vec::with_capacity(height);
            for &succ in succs.iter().take(height) {
                tower_vec.push(AtomicPtr::new(succ));
            }
            let cell = Box::into_raw(Box::new(ValueCell {
                seq,
                entry: entry.clone(),
            }));
            let node = Box::into_raw(Box::new(Node {
                key: key.clone(),
                value: AtomicPtr::new(cell),
                tower: tower_vec.into_boxed_slice(),
            }));

            // Publish at the bottom level first: this is the linearization point
            // that makes the node visible. On contention, reclaim and retry.
            // SAFETY: `preds[0]` is head or a node with height > 0.
            let linked = unsafe {
                (*preds[0]).tower[0]
                    .compare_exchange(succs[0], node, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
            };
            if !linked {
                // SAFETY: `node`/`cell` were just allocated here and never
                // published, so reclaiming them cannot race with anyone.
                unsafe {
                    drop(Box::from_raw(node));
                    drop(Box::from_raw(cell));
                }
                continue;
            }

            self.len.fetch_add(1, Ordering::Relaxed);
            self.size
                .fetch_add(key.len() + new_bytes, Ordering::Relaxed);

            // Link the upper levels as an index. Even if these never completed,
            // level 0 alone keeps the map correct; they only accelerate search.
            for level in 1..height {
                loop {
                    let pred = preds[level];
                    let succ = succs[level];
                    // SAFETY: `node` is not yet linked at `level`, so no reader
                    // can observe this store before the CAS below publishes it.
                    unsafe { (*node).tower[level].store(succ, Ordering::Release) };
                    // SAFETY: `pred` is head or a node with height > `level`.
                    let ok = unsafe {
                        (*pred).tower[level]
                            .compare_exchange(succ, node, Ordering::Release, Ordering::Relaxed)
                            .is_ok()
                    };
                    if ok {
                        break;
                    }
                    // Refresh neighbors and retry this level. The returned
                    // "found" node is our own; we only consume `preds`/`succs`.
                    // SAFETY: same invariants as the call above.
                    unsafe { self.find(key.as_slice(), &mut preds, &mut succs) };
                }
            }
            return;
        }
    }

    /// Looks up `key`, returning a clone of its current entry if present.
    pub(crate) fn get(&self, key: &[u8]) -> Option<ValueEntry> {
        // SAFETY: traversal only dereferences head/live nodes.
        let node = unsafe { self.find_node(key) };
        if node.is_null() {
            return None;
        }
        // SAFETY: a live node always has a non-null value cell that lives until
        // `drop`, so this load-and-clone cannot race with a free.
        let cell = unsafe { (*node).value.load(Ordering::Acquire) };
        Some(unsafe { (*cell).entry.clone() })
    }

    /// Returns every (key, latest-entry) pair in ascending key order.
    ///
    /// This is a point-in-time-ish snapshot: writes concurrent with the walk may
    /// or may not be observed, but the result is always sorted and internally
    /// consistent (never torn).
    pub(crate) fn snapshot(&self) -> Vec<(Bytes, ValueEntry)> {
        let mut out = Vec::with_capacity(self.entry_count());
        // SAFETY: head is always valid; each `curr` is a live node.
        let mut curr = unsafe { (*self.head).tower[0].load(Ordering::Acquire) };
        while !curr.is_null() {
            let node = unsafe { &*curr };
            let cell = node.value.load(Ordering::Acquire);
            out.push((node.key.clone(), unsafe { (*cell).entry.clone() }));
            curr = node.tower[0].load(Ordering::Acquire);
        }
        out
    }

    /// Finds the predecessors and successors of `key` at every level.
    ///
    /// Fills `preds[L]`/`succs[L]` with, respectively, the last node whose key is
    /// `< key` and the first node whose key is `>= key` at level `L`. Returns the
    /// node equal to `key`, or null if absent.
    ///
    /// # Safety
    ///
    /// `self.head` and all linked nodes must be valid (they always are for a
    /// live `SkipList`). The caller must not retain the returned raw pointer past
    /// the lifetime of `&self`.
    unsafe fn find(
        &self,
        key: &[u8],
        preds: &mut [*mut Node; MAX_HEIGHT],
        succs: &mut [*mut Node; MAX_HEIGHT],
    ) -> *mut Node {
        let mut pred = self.head;
        for level in (0..MAX_HEIGHT).rev() {
            // SAFETY: `pred` is head or a node reached at a level >= `level`,
            // hence its tower has at least `level + 1` slots.
            let mut curr = unsafe { (*pred).tower[level].load(Ordering::Acquire) };
            while !curr.is_null() {
                let curr_ref = unsafe { &*curr };
                if curr_ref.key.as_slice() < key {
                    pred = curr;
                    curr = curr_ref.tower[level].load(Ordering::Acquire);
                } else {
                    break;
                }
            }
            preds[level] = pred;
            succs[level] = curr;
        }
        let cand = succs[0];
        if !cand.is_null() && unsafe { (*cand).key.as_slice() == key } {
            cand
        } else {
            ptr::null_mut()
        }
    }

    /// Lightweight lookup that returns the node equal to `key`, or null.
    ///
    /// # Safety
    ///
    /// Same requirements as [`SkipList::find`].
    unsafe fn find_node(&self, key: &[u8]) -> *mut Node {
        let mut pred = self.head;
        for level in (0..MAX_HEIGHT).rev() {
            // SAFETY: see `find`.
            let mut curr = unsafe { (*pred).tower[level].load(Ordering::Acquire) };
            while !curr.is_null() {
                let curr_ref = unsafe { &*curr };
                let ck = curr_ref.key.as_slice();
                if ck < key {
                    pred = curr;
                    curr = curr_ref.tower[level].load(Ordering::Acquire);
                } else if ck == key {
                    return curr;
                } else {
                    break;
                }
            }
        }
        ptr::null_mut()
    }

    /// Atomically replaces the value of an existing node, enforcing
    /// last-writer-wins by sequence number and retiring the displaced cell.
    ///
    /// # Safety
    ///
    /// `node` must point to a live node owned by this list.
    unsafe fn update_value(
        &self,
        node: *mut Node,
        entry: ValueEntry,
        seq: SeqNum,
        new_bytes: usize,
    ) {
        let new_cell = Box::into_raw(Box::new(ValueCell { seq, entry }));
        loop {
            let cur = unsafe { (*node).value.load(Ordering::Acquire) };
            let cur_seq = unsafe { (*cur).seq };
            if seq < cur_seq {
                // A newer write already won; discard this stale update.
                // SAFETY: `new_cell` was just allocated and never published.
                unsafe { drop(Box::from_raw(new_cell)) };
                return;
            }
            let old_bytes = entry_bytes(unsafe { &(*cur).entry });
            let swapped = unsafe {
                (*node)
                    .value
                    .compare_exchange(cur, new_cell, Ordering::AcqRel, Ordering::Acquire)
            };
            match swapped {
                Ok(_) => {
                    if new_bytes >= old_bytes {
                        self.size
                            .fetch_add(new_bytes - old_bytes, Ordering::Relaxed);
                    } else {
                        self.size
                            .fetch_sub(old_bytes - new_bytes, Ordering::Relaxed);
                    }
                    self.retire(cur);
                    return;
                }
                Err(_) => continue,
            }
        }
    }

    /// Pushes a displaced value cell onto the lock-free retire stack.
    fn retire(&self, cell: *mut ValueCell) {
        let node = Box::into_raw(Box::new(Retired {
            cell,
            next: ptr::null_mut(),
        }));
        loop {
            let head = self.retired.load(Ordering::Acquire);
            // SAFETY: `node` is owned exclusively here until published.
            unsafe { (*node).next = head };
            if self
                .retired
                .compare_exchange(head, node, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
}

impl Drop for SkipList {
    fn drop(&mut self) {
        // `&mut self` proves exclusive access: no other thread can be reading,
        // so we can free every allocation in straight-line, non-atomic passes.

        // 1. Free all data nodes and their current value cells (walk level 0).
        let mut curr = unsafe { (*self.head).tower[0].load(Ordering::Relaxed) };
        while !curr.is_null() {
            // SAFETY: `curr` was produced by `Box::into_raw` and is unreachable
            // by any other thread at drop time.
            let boxed = unsafe { Box::from_raw(curr) };
            let next = boxed.tower[0].load(Ordering::Relaxed);
            let cell = boxed.value.load(Ordering::Relaxed);
            if !cell.is_null() {
                // SAFETY: live nodes own their value cell.
                unsafe { drop(Box::from_raw(cell)) };
            }
            drop(boxed);
            curr = next;
        }

        // 2. Free all retired value cells.
        let mut r = self.retired.load(Ordering::Relaxed);
        while !r.is_null() {
            // SAFETY: retire-stack nodes were produced by `Box::into_raw`.
            let boxed = unsafe { Box::from_raw(r) };
            if !boxed.cell.is_null() {
                unsafe { drop(Box::from_raw(boxed.cell)) };
            }
            r = boxed.next;
        }

        // 3. Free the sentinel head (its value cell is always null).
        // SAFETY: head was produced by `Box::into_raw` in `new`.
        unsafe { drop(Box::from_raw(self.head)) };
    }
}

/// User-visible byte cost of a value entry (tombstones are free).
fn entry_bytes(entry: &ValueEntry) -> usize {
    match entry {
        ValueEntry::Value(v) => v.len(),
        ValueEntry::Tombstone => 0,
    }
}

thread_local! {
    /// Per-thread xorshift state so level generation needs no shared RNG and no
    /// `rand` dependency.
    static RNG: Cell<u64> = Cell::new(seed());
}

/// Produces a non-zero per-thread seed by mixing the wall clock with a stack
/// address (cheap thread divergence without OS thread-id plumbing).
fn seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0u64, |d| d.as_nanos() as u64);
    let local = 0u8;
    let addr = ptr::addr_of!(local) as u64;
    let s = nanos ^ addr.rotate_left(32) ^ 0x9E37_79B9_7F4A_7C15;
    if s == 0 { 0x1234_5678_9ABC_DEF0 } else { s }
}

/// Draws a tower height in `1..=MAX_HEIGHT`, promoting with probability
/// `1/BRANCHING` per level.
fn random_height() -> usize {
    RNG.with(|cell| {
        let mut x = cell.get();
        let mut height = 1;
        while height < MAX_HEIGHT {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if x % BRANCHING != 0 {
                break;
            }
            height += 1;
        }
        cell.set(x);
        height
    })
}
