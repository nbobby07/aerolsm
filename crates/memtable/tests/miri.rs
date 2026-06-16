//! A small, std-only concurrency test designed to run under Miri and
//! ThreadSanitizer.
//!
//! Tokio's runtime relies on syscalls Miri cannot emulate, so the heavyweight
//! async stress tests live elsewhere. This target deliberately uses only raw OS
//! threads and an inline future driver, exercising the unsafe skiplist core
//! (concurrent insert / overwrite / delete / read) with modest iteration counts
//! so Miri can check it for undefined behavior and data races in reasonable
//! time.
//!
//! Run it with:
//!
//! ```text
//! cargo +nightly miri test -p aerolsm-memtable --test miri
//! ```

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use aerolsm_core::{Bytes, Lookup, MemTable};
use aerolsm_memtable::SkipListMemTable;

/// Drives an immediately-ready future to completion using only `std`.
fn block_on_ready<F: Future>(future: F) -> F::Output {
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);

    // SAFETY: the vtable's functions are all no-ops over a null data pointer.
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::hint::spin_loop(),
        }
    }
}

#[test]
fn concurrent_mixed_ops_are_sound() {
    // Small counts: Miri is ~100x slower than native.
    const THREADS: u64 = 4;
    const PER_THREAD: u64 = 40;

    let mt = Arc::new(SkipListMemTable::new());
    let mut handles = Vec::new();

    // Writers on disjoint ranges, each key inserted then overwritten then
    // (for evens) tombstoned at a strictly higher seq.
    for t in 0..THREADS {
        let mt = Arc::clone(&mt);
        handles.push(std::thread::spawn(move || {
            for i in 0..PER_THREAD {
                let key = Bytes::from(format!("t{t}-{i}"));
                let base = (t * PER_THREAD + i) * 3 + 1;
                block_on_ready(mt.insert(key.clone(), Bytes::from("v0"), base)).unwrap();
                block_on_ready(mt.insert(key.clone(), Bytes::from("v1"), base + 1)).unwrap();
                if i % 2 == 0 {
                    block_on_ready(mt.delete(key, base + 2)).unwrap();
                }
            }
        }));
    }

    // Concurrent readers; they must never read torn data or hit UB.
    for _ in 0..THREADS {
        let mt = Arc::clone(&mt);
        handles.push(std::thread::spawn(move || {
            for i in 0..PER_THREAD {
                let key = format!("t0-{i}");
                let _ = block_on_ready(mt.get(key.as_bytes())).unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(mt.len(), (THREADS * PER_THREAD) as usize);
    for t in 0..THREADS {
        for i in 0..PER_THREAD {
            let key = format!("t{t}-{i}");
            let got = block_on_ready(mt.get(key.as_bytes())).unwrap();
            if i % 2 == 0 {
                assert_eq!(got, Some(Lookup::Deleted));
            } else {
                assert_eq!(got, Some(Lookup::Found(Bytes::from("v1"))));
            }
        }
    }
}
