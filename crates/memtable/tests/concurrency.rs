//! MemTable concurrency tests.

use std::collections::HashSet;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_disjoint_writes_lose_nothing() {
    const TASKS: u64 = 8;
    const PER_TASK: u64 = 4_000;

    let mt = Arc::new(SkipListMemTable::new());
    let mut handles = Vec::new();

    for task in 0..TASKS {
        let mt = Arc::clone(&mt);
        handles.push(tokio::spawn(async move {
            for i in 0..PER_TASK {
                let seq = task * PER_TASK + i + 1;
                let key = Bytes::from(format!("t{task:02}-k{i:06}"));
                let val = Bytes::from(format!("v{seq}"));
                mt.insert(key, val, seq).await.unwrap();
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(mt.len(), (TASKS * PER_TASK) as usize);

    for task in 0..TASKS {
        for i in 0..PER_TASK {
            let seq = task * PER_TASK + i + 1;
            let key = format!("t{task:02}-k{i:06}");
            assert_eq!(
                mt.get(key.as_bytes()).await.unwrap(),
                Some(Lookup::Found(Bytes::from(format!("v{seq}")))),
                "missing/incorrect key {key}"
            );
        }
    }

    let keys: Vec<Bytes> = mt.iter().map(|e| e.key).collect();
    let unique: HashSet<&Bytes> = keys.iter().collect();
    assert_eq!(
        unique.len(),
        keys.len(),
        "snapshot contained duplicate keys"
    );
    assert!(keys.windows(2).all(|w| w[0] < w[1]), "snapshot not sorted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn highest_seq_wins_on_a_contended_key() {
    const TASKS: u64 = 16;
    const PER_TASK: u64 = 5_000;
    const HOT: &[u8] = b"contended";

    let mt = Arc::new(SkipListMemTable::new());
    let mut handles = Vec::new();

    for task in 0..TASKS {
        let mt = Arc::clone(&mt);
        handles.push(tokio::spawn(async move {
            for i in 0..PER_TASK {
                let seq = task * PER_TASK + i + 1;
                let val = Bytes::from(format!("{seq}"));
                mt.insert(Bytes::from(HOT), val, seq).await.unwrap();
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let max_seq = TASKS * PER_TASK;
    assert_eq!(mt.len(), 1);
    assert_eq!(
        mt.get(HOT).await.unwrap(),
        Some(Lookup::Found(Bytes::from(format!("{max_seq}"))))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interleaved_insert_and_delete_stay_consistent() {
    const KEYS: u64 = 4_000;
    let mt = Arc::new(SkipListMemTable::new());

    let writer = {
        let mt = Arc::clone(&mt);
        tokio::spawn(async move {
            for i in 0..KEYS {
                let key = Bytes::from(format!("k{i:06}"));
                mt.insert(key, Bytes::from("present"), 2 * i + 1)
                    .await
                    .unwrap();
            }
        })
    };

    let deleter = {
        let mt = Arc::clone(&mt);
        tokio::spawn(async move {
            for i in 0..KEYS {
                if i % 2 == 0 {
                    let key = Bytes::from(format!("k{i:06}"));
                    mt.delete(key, 2 * i + 2).await.unwrap();
                }
            }
        })
    };

    writer.await.unwrap();
    deleter.await.unwrap();

    assert_eq!(mt.len(), KEYS as usize);
    for i in 0..KEYS {
        let key = format!("k{i:06}");
        let got = mt.get(key.as_bytes()).await.unwrap();
        if i % 2 == 0 {
            assert_eq!(got, Some(Lookup::Deleted), "key {key} should be deleted");
        } else {
            assert_eq!(got, Some(Lookup::Found(Bytes::from("present"))));
        }
    }
}

#[test]
fn raw_os_threads_hammer_the_memtable() {
    const THREADS: u64 = 8;
    const PER_THREAD: u64 = 10_000;

    let mt = Arc::new(SkipListMemTable::new());
    let mut handles = Vec::new();

    for t in 0..THREADS {
        let mt = Arc::clone(&mt);
        handles.push(std::thread::spawn(move || {
            for i in 0..PER_THREAD {
                let seq = t * PER_THREAD + i + 1;
                let key = Bytes::from(format!("th{t:02}-{i:06}"));
                block_on_ready(mt.insert(key, Bytes::from("x"), seq)).unwrap();
            }
        }));
    }

    for _ in 0..THREADS {
        let mt = Arc::clone(&mt);
        handles.push(std::thread::spawn(move || {
            for i in 0..PER_THREAD {
                let key = format!("th00-{i:06}");
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
            let key = format!("th{t:02}-{i:06}");
            assert_eq!(
                block_on_ready(mt.get(key.as_bytes())).unwrap(),
                Some(Lookup::Found(Bytes::from("x")))
            );
        }
    }
}
