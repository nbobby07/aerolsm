//! Miri concurrency smoke test.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use aerolsm_core::{Bytes, Lookup, MemTable};
use aerolsm_memtable::SkipListMemTable;

fn block_on_ready<F: Future>(future: F) -> F::Output {
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);

    // SAFETY: no-op waker over a null data pointer.
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
    const THREADS: u64 = 4;
    const PER_THREAD: u64 = 40;

    let mt = Arc::new(SkipListMemTable::new());
    let mut handles = Vec::new();

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
