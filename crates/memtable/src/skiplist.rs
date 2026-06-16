use std::cell::Cell;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use aerolsm_core::{Bytes, SeqNum, ValueEntry};

const MAX_HEIGHT: usize = 32;
const BRANCHING: u64 = 4;

struct ValueCell {
    seq: SeqNum,
    entry: ValueEntry,
}

struct Node {
    key: Bytes,
    value: AtomicPtr<ValueCell>,
    tower: Box<[AtomicPtr<Node>]>,
}

struct Retired {
    cell: *mut ValueCell,
    next: *mut Retired,
}

pub(crate) struct SkipList {
    head: *mut Node,
    len: AtomicUsize,
    size: AtomicUsize,
    retired: AtomicPtr<Retired>,
}

// SAFETY: shared mutation goes through atomics; nodes are freed only in Drop.
unsafe impl Send for SkipList {}
unsafe impl Sync for SkipList {}

impl SkipList {
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

    pub(crate) fn entry_count(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    pub(crate) fn byte_size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    pub(crate) fn insert(&self, key: Bytes, entry: ValueEntry, seq: SeqNum) {
        let new_bytes = entry_bytes(&entry);
        let mut preds: [*mut Node; MAX_HEIGHT] = [self.head; MAX_HEIGHT];
        let mut succs: [*mut Node; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];

        loop {
            let existing = unsafe { self.find(key.as_slice(), &mut preds, &mut succs) };
            if !existing.is_null() {
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

            let linked = unsafe {
                (*preds[0]).tower[0]
                    .compare_exchange(succs[0], node, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
            };
            if !linked {
                unsafe {
                    drop(Box::from_raw(node));
                    drop(Box::from_raw(cell));
                }
                continue;
            }

            self.len.fetch_add(1, Ordering::Relaxed);
            self.size
                .fetch_add(key.len() + new_bytes, Ordering::Relaxed);

            for level in 1..height {
                loop {
                    let pred = preds[level];
                    let succ = succs[level];
                    unsafe { (*node).tower[level].store(succ, Ordering::Release) };
                    let ok = unsafe {
                        (*pred).tower[level]
                            .compare_exchange(succ, node, Ordering::Release, Ordering::Relaxed)
                            .is_ok()
                    };
                    if ok {
                        break;
                    }
                    unsafe { self.find(key.as_slice(), &mut preds, &mut succs) };
                }
            }
            return;
        }
    }

    pub(crate) fn get(&self, key: &[u8]) -> Option<ValueEntry> {
        let node = unsafe { self.find_node(key) };
        if node.is_null() {
            return None;
        }
        let cell = unsafe { (*node).value.load(Ordering::Acquire) };
        Some(unsafe { (*cell).entry.clone() })
    }

    pub(crate) fn snapshot(&self) -> Vec<(Bytes, ValueEntry, SeqNum)> {
        let mut out = Vec::with_capacity(self.entry_count());
        let mut curr = unsafe { (*self.head).tower[0].load(Ordering::Acquire) };
        while !curr.is_null() {
            let node = unsafe { &*curr };
            let cell = node.value.load(Ordering::Acquire);
            let cell_ref = unsafe { &*cell };
            out.push((node.key.clone(), cell_ref.entry.clone(), cell_ref.seq));
            curr = node.tower[0].load(Ordering::Acquire);
        }
        out
    }

    unsafe fn find(
        &self,
        key: &[u8],
        preds: &mut [*mut Node; MAX_HEIGHT],
        succs: &mut [*mut Node; MAX_HEIGHT],
    ) -> *mut Node {
        let mut pred = self.head;
        for level in (0..MAX_HEIGHT).rev() {
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

    unsafe fn find_node(&self, key: &[u8]) -> *mut Node {
        let mut pred = self.head;
        for level in (0..MAX_HEIGHT).rev() {
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

    fn retire(&self, cell: *mut ValueCell) {
        let node = Box::into_raw(Box::new(Retired {
            cell,
            next: ptr::null_mut(),
        }));
        loop {
            let head = self.retired.load(Ordering::Acquire);
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
        let mut curr = unsafe { (*self.head).tower[0].load(Ordering::Relaxed) };
        while !curr.is_null() {
            let boxed = unsafe { Box::from_raw(curr) };
            let next = boxed.tower[0].load(Ordering::Relaxed);
            let cell = boxed.value.load(Ordering::Relaxed);
            if !cell.is_null() {
                unsafe { drop(Box::from_raw(cell)) };
            }
            drop(boxed);
            curr = next;
        }

        let mut r = self.retired.load(Ordering::Relaxed);
        while !r.is_null() {
            let boxed = unsafe { Box::from_raw(r) };
            if !boxed.cell.is_null() {
                unsafe { drop(Box::from_raw(boxed.cell)) };
            }
            r = boxed.next;
        }

        unsafe { drop(Box::from_raw(self.head)) };
    }
}

fn entry_bytes(entry: &ValueEntry) -> usize {
    match entry {
        ValueEntry::Value(v) => v.len(),
        ValueEntry::Tombstone => 0,
    }
}

thread_local! {
    static RNG: Cell<u64> = Cell::new(seed());
}

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
