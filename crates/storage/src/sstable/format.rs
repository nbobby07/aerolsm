use aerolsm_core::{Bytes, Error, MemtableEntry, Result, SeqNum, ValueEntry};

use crate::codec::{
    put_bytes, put_u32, put_u64, read_bytes, read_len_prefixed, read_u32, read_u64,
};

pub(super) const SST_MAGIC: [u8; 4] = *b"SST1";
pub(super) const SST_VERSION: u8 = 1;
pub(super) const FLAG_TOMBSTONE: u8 = 0x01;

/// SSTable footer size in bytes.
pub const FOOTER_SIZE: usize = 40;

/// Parsed SSTable footer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Footer {
    /// Data section offset.
    pub data_offset: u64,
    /// Index section offset.
    pub index_offset: u64,
    /// Entry count.
    pub entry_count: u64,
    /// Body CRC-32.
    pub body_crc: u32,
}

/// One SSTable index row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// User key.
    pub key: Bytes,
    /// Offset in the data section.
    pub offset: u64,
}

pub(super) fn encode_data_entry(entry: &MemtableEntry) -> (Vec<u8>, Bytes, SeqNum, ValueEntry) {
    let mut buf = Vec::new();
    put_bytes(&mut buf, entry.key.as_slice());
    put_u64(&mut buf, entry.seq);
    let flags = if entry.entry.is_tombstone() {
        FLAG_TOMBSTONE
    } else {
        0
    };
    buf.push(flags);
    match &entry.entry {
        ValueEntry::Value(v) => put_bytes(&mut buf, v.as_slice()),
        ValueEntry::Tombstone => put_bytes(&mut buf, &[]),
    }
    (buf, entry.key.clone(), entry.seq, entry.entry.clone())
}

pub(super) fn decode_data_entry(data: &[u8], offset: u64) -> Result<MemtableEntry> {
    let mut off =
        usize::try_from(offset).map_err(|_| Error::Corruption("bad data offset".into()))?;
    let key = Bytes::copy_from_slice(read_len_prefixed(data, &mut off)?);
    let seq = read_u64(data, &mut off)?;
    let flags = *read_bytes(data, &mut off, 1)?
        .first()
        .ok_or_else(|| Error::Corruption("missing entry flags".into()))?;
    let value_slice = read_len_prefixed(data, &mut off)?;
    let entry = if flags & FLAG_TOMBSTONE != 0 {
        ValueEntry::Tombstone
    } else {
        ValueEntry::Value(Bytes::copy_from_slice(value_slice))
    };
    Ok(MemtableEntry { key, entry, seq })
}

pub(super) fn encode_index(entries: &[IndexEntry]) -> Vec<u8> {
    let mut buf = Vec::new();
    put_u32(&mut buf, u32::try_from(entries.len()).unwrap_or(u32::MAX));
    for e in entries {
        put_bytes(&mut buf, e.key.as_slice());
        put_u64(&mut buf, e.offset);
    }
    buf
}

pub(super) fn decode_index(data: &[u8]) -> Result<Vec<IndexEntry>> {
    let mut off = 0usize;
    let count = read_u32(data, &mut off)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let key = Bytes::copy_from_slice(read_len_prefixed(data, &mut off)?);
        let offset = read_u64(data, &mut off)?;
        out.push(IndexEntry { key, offset });
    }
    if off != data.len() {
        return Err(Error::Corruption("index trailing bytes".into()));
    }
    Ok(out)
}

#[must_use]
pub(super) fn encode_footer(footer: &Footer) -> [u8; FOOTER_SIZE] {
    let mut buf = [0u8; FOOTER_SIZE];
    buf[..4].copy_from_slice(&SST_MAGIC);
    buf[4] = SST_VERSION;
    buf[8..16].copy_from_slice(&footer.data_offset.to_le_bytes());
    buf[16..24].copy_from_slice(&footer.index_offset.to_le_bytes());
    buf[24..32].copy_from_slice(&footer.entry_count.to_le_bytes());
    buf[32..36].copy_from_slice(&footer.body_crc.to_le_bytes());
    buf
}

pub(super) fn decode_footer(footer_bytes: &[u8]) -> Result<Footer> {
    if footer_bytes.len() != FOOTER_SIZE {
        return Err(Error::Corruption("bad footer size".into()));
    }
    if footer_bytes[..4] != SST_MAGIC {
        return Err(Error::Corruption("sstable magic mismatch".into()));
    }
    if footer_bytes[4] != SST_VERSION {
        return Err(Error::Corruption(format!(
            "unsupported sstable version {}",
            footer_bytes[4]
        )));
    }
    let data_offset = u64::from_le_bytes(footer_bytes[8..16].try_into().unwrap());
    let index_offset = u64::from_le_bytes(footer_bytes[16..24].try_into().unwrap());
    let entry_count = u64::from_le_bytes(footer_bytes[24..32].try_into().unwrap());
    let body_crc = u32::from_le_bytes(footer_bytes[32..36].try_into().unwrap());
    Ok(Footer {
        data_offset,
        index_offset,
        entry_count,
        body_crc,
    })
}
