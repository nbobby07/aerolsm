//! Single-threaded correctness tests for `SkipListMemTable`.
//!
//! These pin down the semantic contract of the MemTable: value round-trips,
//! last-writer-wins ordering, tombstone-vs-absent, sorted iteration, and the
//! bookkeeping counters that drive flush decisions.

use aerolsm_core::{Bytes, Lookup, MemTable, ValueEntry};
use aerolsm_memtable::SkipListMemTable;

#[tokio::test]
async fn insert_then_get_roundtrips() {
    let mt = SkipListMemTable::new();
    mt.insert(Bytes::from("alpha"), Bytes::from("one"), 1)
        .await
        .unwrap();

    assert_eq!(
        mt.get(b"alpha").await.unwrap(),
        Some(Lookup::Found(Bytes::from("one")))
    );
    assert_eq!(mt.get(b"missing").await.unwrap(), None);
}

#[tokio::test]
async fn higher_seq_overwrites_lower() {
    let mt = SkipListMemTable::new();
    mt.insert(Bytes::from("k"), Bytes::from("v1"), 1)
        .await
        .unwrap();
    mt.insert(Bytes::from("k"), Bytes::from("v2"), 2)
        .await
        .unwrap();

    assert_eq!(
        mt.get(b"k").await.unwrap(),
        Some(Lookup::Found(Bytes::from("v2")))
    );
    // Overwriting a key does not grow the key count.
    assert_eq!(mt.len(), 1);
}

#[tokio::test]
async fn stale_seq_is_rejected() {
    let mt = SkipListMemTable::new();
    mt.insert(Bytes::from("k"), Bytes::from("new"), 5)
        .await
        .unwrap();
    // A write with an older sequence number must not clobber the newer value.
    mt.insert(Bytes::from("k"), Bytes::from("old"), 3)
        .await
        .unwrap();

    assert_eq!(
        mt.get(b"k").await.unwrap(),
        Some(Lookup::Found(Bytes::from("new")))
    );
}

#[tokio::test]
async fn delete_writes_a_distinguishable_tombstone() {
    let mt = SkipListMemTable::new();
    mt.insert(Bytes::from("k"), Bytes::from("v"), 1)
        .await
        .unwrap();
    mt.delete(Bytes::from("k"), 2).await.unwrap();

    // Deleted is NOT the same as absent: that distinction is what stops a stale
    // value in a lower LSM layer from resurfacing.
    assert_eq!(mt.get(b"k").await.unwrap(), Some(Lookup::Deleted));
    assert_eq!(mt.get(b"never-written").await.unwrap(), None);
    // The tombstone still occupies a key slot.
    assert_eq!(mt.len(), 1);
}

#[tokio::test]
async fn iter_yields_latest_values_in_sorted_order() {
    let mt = SkipListMemTable::new();
    // Insert out of order, including an overwrite and a delete.
    mt.insert(Bytes::from("banana"), Bytes::from("b1"), 1)
        .await
        .unwrap();
    mt.insert(Bytes::from("apple"), Bytes::from("a1"), 2)
        .await
        .unwrap();
    mt.insert(Bytes::from("cherry"), Bytes::from("c1"), 3)
        .await
        .unwrap();
    mt.insert(Bytes::from("banana"), Bytes::from("b2"), 4)
        .await
        .unwrap();
    mt.delete(Bytes::from("apple"), 5).await.unwrap();

    let entries: Vec<_> = mt.iter().collect();
    assert_eq!(
        entries,
        vec![
            (Bytes::from("apple"), ValueEntry::Tombstone),
            (Bytes::from("banana"), ValueEntry::Value(Bytes::from("b2"))),
            (Bytes::from("cherry"), ValueEntry::Value(Bytes::from("c1"))),
        ]
    );
}

#[tokio::test]
async fn counters_track_keys_and_bytes() {
    let mt = SkipListMemTable::new();
    assert!(mt.is_empty());
    assert_eq!(mt.approximate_size(), 0);

    // key "k" (1) + value "v" (1)
    mt.insert(Bytes::from("k"), Bytes::from("v"), 1)
        .await
        .unwrap();
    assert_eq!(mt.len(), 1);
    assert_eq!(mt.approximate_size(), 2);

    // Overwrite with a longer value: key counted once, value grows by 1.
    mt.insert(Bytes::from("k"), Bytes::from("vv"), 2)
        .await
        .unwrap();
    assert_eq!(mt.len(), 1);
    assert_eq!(mt.approximate_size(), 3);

    // Tombstone drops the value bytes but keeps the key.
    mt.delete(Bytes::from("k"), 3).await.unwrap();
    assert_eq!(mt.len(), 1);
    assert_eq!(mt.approximate_size(), 1);
}

#[tokio::test]
async fn many_distinct_keys_are_all_retrievable() {
    let mt = SkipListMemTable::new();
    const N: u64 = 5_000;

    for i in 0..N {
        let key = Bytes::from(format!("key-{i:08}"));
        let val = Bytes::from(format!("val-{i}"));
        mt.insert(key, val, i + 1).await.unwrap();
    }

    assert_eq!(mt.len(), N as usize);

    for i in 0..N {
        let key = format!("key-{i:08}");
        assert_eq!(
            mt.get(key.as_bytes()).await.unwrap(),
            Some(Lookup::Found(Bytes::from(format!("val-{i}"))))
        );
    }

    // Iteration order is the lexicographic key order; the zero-padded keys make
    // that identical to numeric order.
    let keys: Vec<Bytes> = mt.iter().map(|(k, _)| k).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(keys.len(), N as usize);
}
