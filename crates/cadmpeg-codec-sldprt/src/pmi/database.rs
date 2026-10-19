// SPDX-License-Identifier: Apache-2.0
//! Author a genuine UnQLite linear-hash database without a source image.
//!
//! Page semantics follow the publicly published Symisc UnQLite 1.2.1 storage
//! implementation (`pager_write_db_header`, `lhash_write_header`,
//! `lhCellWriteHeader`, and `lhCellWriteOvflPayload`). This implementation
//! authors one logical bucket with an ordered chain of slave pages. Each cell
//! owns one page; values that exceed its local capacity own overflow pages.

use cadmpeg_core::CodecError;

pub(super) mod read;

const PAGE_SIZE: usize = 4096;
const PAGE_SIZE_U32: u32 = 4096;
const CELL_OFFSET: usize = 12;
const CELL_HEADER_SIZE: usize = 26;
const LOCAL_PAYLOAD: usize = CELL_OFFSET + CELL_HEADER_SIZE;
const OVERFLOW_HEADER: usize = 18;

pub(super) fn encode(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, CodecError> {
    let page_count = entries.len().checked_add(2).ok_or_else(capacity)?;
    let mut pages = (0..page_count)
        .map(|_| vec![0u8; PAGE_SIZE])
        .collect::<Vec<_>>();
    let header = &mut pages[0];
    header[..7].copy_from_slice(b"unqlite");
    header[7..11].copy_from_slice(&0xdb7c_2712u32.to_be_bytes());
    // A deterministic DOS creation timestamp: 1980-01-01 00:00:00.
    header[11..15].copy_from_slice(&0x0021_0000u32.to_be_bytes());
    header[15..19].copy_from_slice(&512u32.to_be_bytes());
    header[19..23].copy_from_slice(&PAGE_SIZE_U32.to_be_bytes());
    header[23..25].copy_from_slice(&4u16.to_be_bytes());
    header[25..29].copy_from_slice(b"hash");
    let hash = &mut pages[1];
    hash[..4].copy_from_slice(&0xfa78_2dcbu32.to_be_bytes());
    hash[4..8].copy_from_slice(&key_hash(b"chm@symisc").to_be_bytes());
    hash[24..32].copy_from_slice(&1u64.to_be_bytes());
    if !entries.is_empty() {
        hash[40..44].copy_from_slice(&1u32.to_be_bytes());
        hash[52..60].copy_from_slice(&2u64.to_be_bytes());
    }
    for (index, (key, value)) in entries.iter().enumerate() {
        if key.len() != 36 || !super::write::valid_guid(key) {
            // (C): semantic database identities must be valid GUID keys.
            return Err(CodecError::malformed(
                "SLDPRT PMI database key is not a GUID",
            ));
        }
        let key_len = u32::try_from(key.len()).map_err(|_| capacity())?;
        let value_len = u64::try_from(value.len()).map_err(|_| capacity())?;
        let local_size = LOCAL_PAYLOAD
            .checked_add(key.len())
            .and_then(|size| size.checked_add(value.len()))
            .ok_or_else(capacity)?;
        let page_index = index + 2;
        let next = if index + 1 < entries.len() {
            u64::try_from(page_index + 1).map_err(|_| capacity())?
        } else {
            0
        };
        let overflow = if local_size <= PAGE_SIZE - 4 {
            None
        } else {
            Some(write_overflow(&mut pages, key.as_bytes(), value)?)
        };
        let page = &mut pages[page_index];
        page[..2].copy_from_slice(&12u16.to_be_bytes());
        page[4..12].copy_from_slice(&next.to_be_bytes());
        page[12..16].copy_from_slice(&key_hash(key.as_bytes()).to_be_bytes());
        page[16..20].copy_from_slice(&key_len.to_be_bytes());
        page[20..28].copy_from_slice(&value_len.to_be_bytes());
        let end = if let Some(overflow) = overflow {
            page[30..38].copy_from_slice(&overflow.to_be_bytes());
            LOCAL_PAYLOAD
        } else {
            page[LOCAL_PAYLOAD..LOCAL_PAYLOAD + key.len()].copy_from_slice(key.as_bytes());
            page[LOCAL_PAYLOAD + key.len()..local_size].copy_from_slice(value);
            local_size
        };
        let offset = u16::try_from(end).map_err(|_| capacity())?;
        let free = u16::try_from(PAGE_SIZE - end).map_err(|_| capacity())?;
        page[2..4].copy_from_slice(&offset.to_be_bytes());
        page[end + 2..end + 4].copy_from_slice(&free.to_be_bytes());
    }
    let size = pages.len().checked_mul(PAGE_SIZE).ok_or_else(capacity)?;
    let mut out = Vec::with_capacity(size);
    for page in pages {
        out.extend(page);
    }
    Ok(out)
}

fn write_overflow(pages: &mut Vec<Vec<u8>>, key: &[u8], value: &[u8]) -> Result<u64, CodecError> {
    let first = u64::try_from(pages.len()).map_err(|_| capacity())?;
    let data_offset = u16::try_from(OVERFLOW_HEADER + key.len()).map_err(|_| capacity())?;
    let mut input = key.iter().chain(value).copied().peekable();
    let mut offset = OVERFLOW_HEADER;
    loop {
        let mut page = vec![0u8; PAGE_SIZE];
        for target in &mut page[offset..] {
            let Some(byte) = input.next() else {
                break;
            };
            *target = byte;
        }
        if offset == OVERFLOW_HEADER {
            page[8..16].copy_from_slice(&first.to_be_bytes());
            page[16..18].copy_from_slice(&data_offset.to_be_bytes());
        }
        let more = input.peek().is_some();
        if more {
            let next = pages.len().checked_add(1).ok_or_else(capacity)?;
            page[..8].copy_from_slice(&u64::try_from(next).map_err(|_| capacity())?.to_be_bytes());
        }
        pages.push(page);
        if !more {
            break;
        }
        offset = 8;
    }
    Ok(first)
}

fn key_hash(bytes: &[u8]) -> u32 {
    bytes.iter().take(2048).fold(5381u32, |value, byte| {
        value.wrapping_mul(33).wrapping_add(u32::from(*byte))
    })
}

/// (O-capacity): a database page number or payload length exceeds its field width.
fn capacity() -> CodecError {
    CodecError::malformed("SLDPRT PMI database exceeds its address or field capacity")
}
