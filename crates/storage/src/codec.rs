use aerolsm_core::{Error, Result};

#[must_use]
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    !crc
}

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut c = i;
        let mut j = 0;
        while j < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            j += 1;
        }
        table[i as usize] = c;
        i += 1;
    }
    table
};

pub(crate) fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::Corruption("integer overflow".into()))?;
    if end > data.len() {
        return Err(Error::Corruption("truncated u32".into()));
    }
    let v = u32::from_le_bytes(data[*offset..end].try_into().unwrap());
    *offset = end;
    Ok(v)
}

pub(crate) fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| Error::Corruption("integer overflow".into()))?;
    if end > data.len() {
        return Err(Error::Corruption("truncated u64".into()));
    }
    let v = u64::from_le_bytes(data[*offset..end].try_into().unwrap());
    *offset = end;
    Ok(v)
}

pub(crate) fn read_bytes<'a>(data: &'a [u8], offset: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| Error::Corruption("integer overflow".into()))?;
    if end > data.len() {
        return Err(Error::Corruption("truncated byte slice".into()));
    }
    let slice = &data[*offset..end];
    *offset = end;
    Ok(slice)
}

pub(crate) fn put_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    put_u32(buf, u32::try_from(data.len()).unwrap_or(u32::MAX));
    buf.extend_from_slice(data);
}

pub(crate) fn read_len_prefixed<'a>(data: &'a [u8], offset: &mut usize) -> Result<&'a [u8]> {
    let len = read_u32(data, offset)? as usize;
    read_bytes(data, offset, len)
}
