// SPDX-License-Identifier: Apache-2.0
//! Traverse UnQLite hash cells and reconstruct overflow values.

use std::cell::Cell;
use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) struct MapImage {
    pub(crate) key: String,
    pub(crate) value: Vec<u8>,
    spans: Vec<(usize, usize, usize)>,
}

impl MapImage {
    pub(crate) fn source_offset(&self, logical: u64) -> u64 {
        usize::try_from(logical)
            .ok()
            .and_then(|logical| {
                self.spans.iter().find_map(|&(start, end, source)| {
                    (start <= logical && logical < end).then(|| source + logical - start)
                })
            })
            .and_then(|offset| u64::try_from(offset).ok())
            .unwrap_or(0)
    }
}

pub(crate) fn images(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Vec<MapImage>>, CodecError> {
    if !payload.starts_with(b"unqlite") || View::u32_be_at(payload, 7) != Some(0xdb7c_2712) {
        return Ok(None);
    }
    let page_size = View::u32_be_at(payload, 19)
        .and_then(|size| usize::try_from(size).ok())
        .ok_or_else(malformed)?;
    if !(512..=65536).contains(&page_size)
        || !page_size.is_power_of_two()
        || payload.len() % page_size != 0
    {
        return Err(malformed());
    }
    if View::u16_be_at(payload, 23) != Some(4) || payload.get(25..29) != Some(b"hash") {
        return Err(malformed());
    }
    let reader = Reader {
        ctx,
        payload,
        page_size,
        remaining: Cell::new(payload.len()),
    };
    let header = reader.page(1)?;
    if View::u32_be_at(header, 0) != Some(0xfa78_2dcb) {
        return Err(malformed());
    }
    let mut map_page = 1u64;
    let mut first = true;
    let mut seen_maps = BTreeSet::new();
    let mut roots = Vec::new();
    while map_page != 0 {
        if !seen_maps.insert(map_page) {
            return Err(malformed());
        }
        let page = reader.page(map_page)?;
        let offset = if first { 32 } else { 0 };
        let next = View::u64_be_at(page, offset).ok_or_else(malformed)?;
        let count = View::u32_be_at(page, offset + 8)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(malformed)?;
        let start = offset + 12;
        if count > (page_size - start) / 16 {
            return Err(malformed());
        }
        for index in 0..count {
            roots.push(View::u64_be_at(page, start + index * 16 + 8).ok_or_else(malformed)?);
        }
        map_page = next;
        first = false;
    }
    let mut seen_pages = BTreeSet::new();
    let mut images = Vec::new();
    for mut number in roots {
        while number != 0 {
            if !seen_pages.insert(number) {
                return Err(malformed());
            }
            let page = reader.page(number)?;
            let mut cell = usize::from(View::u16_be_at(page, 0).ok_or_else(malformed)?);
            let mut seen_cells = BTreeSet::new();
            while cell != 0 {
                if !seen_cells.insert(cell) || cell < 12 || cell > page_size - 26 {
                    return Err(malformed());
                }
                images.push(reader.cell(number, page, cell)?);
                cell = usize::from(View::u16_be_at(page, cell + 16).ok_or_else(malformed)?);
            }
            number = View::u64_be_at(page, 4).ok_or_else(malformed)?;
        }
    }
    Ok(Some(images))
}

struct Reader<'a, 'context> {
    ctx: &'a DecodeContext<'context>,
    payload: &'a [u8],
    page_size: usize,
    remaining: Cell<usize>,
}

impl Reader<'_, '_> {
    fn page(&self, number: u64) -> Result<&[u8], CodecError> {
        let start = usize::try_from(number)
            .ok()
            .and_then(|number| number.checked_mul(self.page_size))
            .ok_or_else(malformed)?;
        let end = start.checked_add(self.page_size).ok_or_else(malformed)?;
        self.payload.get(start..end).ok_or_else(malformed)
    }

    fn cell(&self, number: u64, page: &[u8], cell: usize) -> Result<MapImage, CodecError> {
        let key_len = View::u32_be_at(page, cell + 4)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(malformed)?;
        let value_len = View::u64_be_at(page, cell + 8)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(malformed)?;
        let size = key_len.checked_add(value_len).ok_or_else(malformed)?;
        // A declared record cannot contain more bytes than its entire database.
        if key_len == 0 || size > self.remaining.get() {
            return Err(malformed());
        }
        self.remaining.set(self.remaining.get() - size);
        let overflow = View::u64_be_at(page, cell + 18).ok_or_else(malformed)?;
        let mut bytes = self
            .ctx
            .alloc_filled(size, 0u8, "reconstruct UnQLite record")?;
        let mut spans = Vec::new();
        if overflow == 0 {
            let start = cell + 26;
            let end = start.checked_add(size).ok_or_else(malformed)?;
            bytes.copy_from_slice(page.get(start..end).ok_or_else(malformed)?);
            let source = usize::try_from(number)
                .ok()
                .and_then(|number| number.checked_mul(self.page_size))
                .and_then(|start_of_page| start_of_page.checked_add(start))
                .ok_or_else(malformed)?;
            if value_len != 0 {
                spans.push((0, value_len, source + key_len));
            }
        } else {
            let mut next = overflow;
            let mut first = true;
            let mut position = 0usize;
            let mut seen = BTreeSet::new();
            while position < size {
                if next == 0 || !seen.insert(next) {
                    return Err(malformed());
                }
                let page = self.page(next)?;
                let start = if first { 18 } else { 8 };
                let count = (size - position).min(self.page_size - start);
                bytes[position..position + count].copy_from_slice(&page[start..start + count]);
                let value_start = position.max(key_len);
                let value_end = (position + count).max(key_len);
                if value_start < value_end {
                    let source = usize::try_from(next)
                        .ok()
                        .and_then(|number| number.checked_mul(self.page_size))
                        .and_then(|page_start| {
                            page_start.checked_add(start + value_start - position)
                        })
                        .ok_or_else(malformed)?;
                    spans.push((value_start - key_len, value_end - key_len, source));
                }
                position += count;
                next = View::u64_be_at(page, 0).ok_or_else(malformed)?;
                first = false;
            }
        }
        let key = std::str::from_utf8(&bytes[..key_len])
            .map_err(|_| malformed())?
            .to_owned();
        let value = bytes.split_off(key_len);
        Ok(MapImage { key, value, spans })
    }
}

fn malformed() -> CodecError {
    CodecError::malformed("malformed UnQLite hash database record or page graph")
}
