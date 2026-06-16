//! Storage integration tests.

use std::sync::Arc;

use aerolsm_core::{Bytes, Lookup, MemTable, MemtableEntry, SsTableId, ValueEntry};
use aerolsm_memtable::SkipListMemTable;
use aerolsm_storage::{
    FileBackend, MemoryBackend, SsTableReader, SsTableWriter, WalOpKind, WalReader, WalWriter,
    flush_memtable,
};

#[tokio::test]
async fn wal_roundtrip_on_memory_backend() {
    let backend = Arc::new(MemoryBackend::new());
    let mut writer = WalWriter::new(Arc::clone(&backend));

    writer.append_put(1, b"alpha", b"one").await.unwrap();
    writer.append_put(2, b"beta", b"two").await.unwrap();
    writer.append_delete(3, b"alpha").await.unwrap();
    writer.sync().await.unwrap();

    let records = WalReader::new(backend).replay().await.unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, WalOpKind::Put);
    assert_eq!(records[0].key.as_slice(), b"alpha");
    assert_eq!(
        records[0].value.as_ref().map(Bytes::as_slice),
        Some(b"one".as_ref())
    );
    assert_eq!(records[2].kind, WalOpKind::Delete);
    assert!(records[2].value.is_none());
}

#[tokio::test]
async fn sstable_write_read_roundtrip() {
    let backend = Arc::new(MemoryBackend::new());
    let entries = vec![
        MemtableEntry {
            key: Bytes::from("a"),
            entry: ValueEntry::Value(Bytes::from("1")),
            seq: 1,
        },
        MemtableEntry {
            key: Bytes::from("b"),
            entry: ValueEntry::Tombstone,
            seq: 2,
        },
        MemtableEntry {
            key: Bytes::from("c"),
            entry: ValueEntry::Value(Bytes::from("3")),
            seq: 3,
        },
    ];

    let meta = SsTableWriter::new(Arc::clone(&backend))
        .write(SsTableId(7), 0, entries)
        .await
        .unwrap();
    assert_eq!(meta.entry_count, 3);
    assert_eq!(meta.smallest_key.as_slice(), b"a");
    assert_eq!(meta.largest_key.as_slice(), b"c");

    let reader = SsTableReader::open(backend.as_ref()).await.unwrap();
    assert_eq!(reader.len(), 3);
    assert_eq!(
        reader.get(b"a").unwrap(),
        Some(Lookup::Found(Bytes::from("1")))
    );
    assert_eq!(reader.get(b"b").unwrap(), Some(Lookup::Deleted));
    assert_eq!(reader.get(b"missing").unwrap(), None);

    let keys: Vec<_> = reader.iter().map(|e| e.key).collect();
    assert_eq!(
        keys,
        vec![Bytes::from("a"), Bytes::from("b"), Bytes::from("c")]
    );
}

#[tokio::test]
async fn flush_memtable_produces_readable_sstable() {
    let backend = Arc::new(MemoryBackend::new());
    let mt = SkipListMemTable::new();
    mt.insert(Bytes::from("agent:1"), Bytes::from("state-a"), 10)
        .await
        .unwrap();
    mt.insert(Bytes::from("agent:2"), Bytes::from("state-b"), 11)
        .await
        .unwrap();
    mt.delete(Bytes::from("agent:1"), 12).await.unwrap();

    flush_memtable(&mt, Arc::clone(&backend), SsTableId(1), 0)
        .await
        .unwrap();

    let reader = SsTableReader::open(backend.as_ref()).await.unwrap();
    assert_eq!(reader.get(b"agent:1").unwrap(), Some(Lookup::Deleted));
    assert_eq!(
        reader.get(b"agent:2").unwrap(),
        Some(Lookup::Found(Bytes::from("state-b")))
    );
}

#[tokio::test]
async fn file_backend_wal_and_sstable() {
    let dir = std::env::temp_dir().join(format!("aerolsm-phase2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let wal_path = dir.join("test.wal");
    let sst_path = dir.join("test.sst");

    let wal_backend = Arc::new(FileBackend::create(&wal_path).unwrap());
    let mut wal = WalWriter::new(Arc::clone(&wal_backend));
    wal.append_put(1, b"k", b"v").await.unwrap();
    wal.sync().await.unwrap();
    assert!(wal_path.exists());

    let records = WalReader::new(wal_backend).replay().await.unwrap();
    assert_eq!(records.len(), 1);

    let sst_backend = Arc::new(FileBackend::create(&sst_path).unwrap());
    let mt = SkipListMemTable::new();
    mt.insert(Bytes::from("k"), Bytes::from("v"), 1)
        .await
        .unwrap();
    flush_memtable(&mt, sst_backend, SsTableId(1), 0)
        .await
        .unwrap();

    let reopened = Arc::new(FileBackend::open(&sst_path).unwrap());
    let reader = SsTableReader::open(reopened.as_ref()).await.unwrap();
    assert_eq!(
        reader.get(b"k").unwrap(),
        Some(Lookup::Found(Bytes::from("v")))
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn wal_replay_recovers_memtable_state() {
    let backend = Arc::new(MemoryBackend::new());
    let mut wal = WalWriter::new(Arc::clone(&backend));
    wal.append_put(1, b"x", b"1").await.unwrap();
    wal.append_put(2, b"y", b"2").await.unwrap();
    wal.append_delete(3, b"x").await.unwrap();
    wal.sync().await.unwrap();

    let mt = SkipListMemTable::new();
    for rec in WalReader::new(backend).replay().await.unwrap() {
        match rec.kind {
            WalOpKind::Put => {
                let value = rec.value.expect("put has value");
                mt.insert(rec.key, value, rec.seq).await.unwrap();
            }
            WalOpKind::Delete => {
                mt.delete(rec.key, rec.seq).await.unwrap();
            }
        }
    }

    assert_eq!(mt.get(b"x").await.unwrap(), Some(Lookup::Deleted));
    assert_eq!(
        mt.get(b"y").await.unwrap(),
        Some(Lookup::Found(Bytes::from("2")))
    );
}
