use aerolsm_core::{Bytes, Error, Result, SeqNum};

use crate::codec::{
    crc32, put_bytes, put_u32, put_u64, read_bytes, read_len_prefixed, read_u32, read_u64,
};

pub(super) const WAL_MAGIC: [u8; 4] = *b"ALOG";
pub(super) const WAL_VERSION: u8 = 1;

/// WAL mutation kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalOpKind {
    /// Put.
    Put,
    /// Delete.
    Delete,
}

/// Decoded WAL record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalRecord {
    /// Sequence number.
    pub seq: SeqNum,
    /// Mutation kind.
    pub kind: WalOpKind,
    /// User key.
    pub key: Bytes,
    /// Value, if any.
    pub value: Option<Bytes>,
}

#[must_use]
pub(super) fn wal_header() -> [u8; 8] {
    let mut buf = [0u8; 8];
    buf[..4].copy_from_slice(&WAL_MAGIC);
    buf[4] = WAL_VERSION;
    buf
}

pub(super) fn encode_record(
    seq: SeqNum,
    kind: WalOpKind,
    key: &[u8],
    value: Option<&[u8]>,
) -> Vec<u8> {
    let value = value.unwrap_or(&[]);
    let mut payload = Vec::with_capacity(1 + 8 + 4 + key.len() + 4 + value.len());
    payload.push(match kind {
        WalOpKind::Put => 0,
        WalOpKind::Delete => 1,
    });
    put_u64(&mut payload, seq);
    put_bytes(&mut payload, key);
    put_bytes(&mut payload, value);

    let mut out = Vec::with_capacity(4 + 4 + payload.len());
    put_u32(&mut out, u32::try_from(payload.len()).unwrap_or(u32::MAX));
    put_u32(&mut out, crc32(&payload));
    out.extend_from_slice(&payload);
    out
}

pub(super) fn decode_record(data: &[u8], offset: &mut usize) -> Result<WalRecord> {
    let payload_len = read_u32(data, offset)? as usize;
    let stored_crc = read_u32(data, offset)?;
    let payload_start = *offset;
    let payload_end = payload_start
        .checked_add(payload_len)
        .ok_or_else(|| Error::Corruption("wal record overflow".into()))?;
    if payload_end > data.len() {
        return Err(Error::Corruption("truncated wal record".into()));
    }
    let payload = &data[payload_start..payload_end];
    if crc32(payload) != stored_crc {
        return Err(Error::Corruption("wal record checksum mismatch".into()));
    }
    *offset = payload_end;

    let mut poff = 0usize;
    let kind_byte = *read_bytes(payload, &mut poff, 1)?
        .first()
        .ok_or_else(|| Error::Corruption("empty wal payload".into()))?;
    let kind = match kind_byte {
        0 => WalOpKind::Put,
        1 => WalOpKind::Delete,
        other => {
            return Err(Error::Corruption(format!("unknown wal op {other}")));
        }
    };
    let seq = read_u64(payload, &mut poff)?;
    let key = Bytes::copy_from_slice(read_len_prefixed(payload, &mut poff)?);
    let value_slice = read_len_prefixed(payload, &mut poff)?;
    let value = if kind == WalOpKind::Delete {
        None
    } else {
        Some(Bytes::copy_from_slice(value_slice))
    };
    if poff != payload.len() {
        return Err(Error::Corruption("wal record trailing bytes".into()));
    }
    Ok(WalRecord {
        seq,
        kind,
        key,
        value,
    })
}

pub(super) fn validate_header(data: &[u8]) -> Result<usize> {
    if data.len() < 8 {
        return Err(Error::Corruption("wal header too short".into()));
    }
    if data[..4] != WAL_MAGIC {
        return Err(Error::Corruption("wal magic mismatch".into()));
    }
    if data[4] != WAL_VERSION {
        return Err(Error::Corruption(format!(
            "unsupported wal version {}",
            data[4]
        )));
    }
    Ok(8)
}
