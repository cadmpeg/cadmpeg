// SPDX-License-Identifier: Apache-2.0
//! Seekable directory evidence without acquiring archive payloads.

use super::preflight_central_directory;
use crate::layout::{central_header, digital_signature, end_record, zip64_end, zip64_locator};
use cadmpeg_core::{
    decode::{u64_from_index, DecodeContext, View},
    CodecError, ReadSeek,
};
use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;

const FOOTER_WINDOW: usize = end_record::LEN + 65_535 + zip64_locator::LEN;

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    source: &mut dyn ReadSeek,
) -> Result<Option<Vec<u8>>, CodecError> {
    let position = source.stream_position()?;
    let result = read_inner(ctx, source);
    source.seek(SeekFrom::Start(position))?;
    result
}

fn read_inner(
    ctx: &DecodeContext<'_>,
    source: &mut dyn ReadSeek,
) -> Result<Option<Vec<u8>>, CodecError> {
    let length = source.seek(SeekFrom::End(0))?;
    let tail_len = usize::try_from(length.min(u64_from_index(FOOTER_WINDOW)))
        .map_err(|_| CodecError::malformed("ZIP probe tail does not fit memory"))?;
    let tail_start = length - u64_from_index(tail_len);
    let tail = read_range(ctx, source, tail_start, tail_len)?;
    let Some(last_end) = tail.len().checked_sub(end_record::LEN) else {
        return Ok(None);
    };
    let candidates = (0..=last_end)
        .rev()
        .filter(|&end| {
            tail.get(end..end + 4) == Some(b"PK\x05\x06".as_slice())
                && View::u16_le_at(&tail, end + end_record::COMMENT_LENGTH).is_some_and(|comment| {
                    end.checked_add(end_record::LEN + usize::from(comment)) == Some(tail.len())
                })
        })
        .collect::<Vec<_>>();
    let locator_at = |end: usize| {
        end.checked_sub(zip64_locator::LEN)
            .filter(|start| tail.get(*start..*start + 4) == Some(b"PK\x06\x07".as_slice()))
    };
    let mut zip64_records = Zip64Records::new(
        candidates
            .iter()
            .filter_map(|&end| locator_at(end))
            .map(|locator| tail_start + u64_from_index(locator))
            .collect(),
    );
    for end in candidates {
        let directory_end = tail_start + u64_from_index(end);
        let (size, count, directory_end, zip64) = if let Some(locator) = locator_at(end) {
            let Some(hint) = View::u64_le_at(&tail, locator + zip64_locator::END_RECORD_POSITION)
            else {
                continue;
            };
            let locator_position = tail_start + u64_from_index(locator);
            let Some(record) = zip64_records.find(ctx, source, locator_position, hint)? else {
                continue;
            };
            let header = read_range(ctx, source, record, zip64_end::LEN)?;
            let Some(size) = View::u64_le_at(&header, zip64_end::DIRECTORY_SIZE) else {
                continue;
            };
            let Some(count) = View::u64_le_at(&header, zip64_end::ENTRIES) else {
                continue;
            };
            (size, count, record, Some((record, locator_position)))
        } else {
            let Some(size) = View::u32_le_at(&tail, end + end_record::DIRECTORY_SIZE) else {
                continue;
            };
            let Some(count) = View::u16_le_at(&tail, end + end_record::ENTRIES) else {
                continue;
            };
            (u64::from(size), u64::from(count), directory_end, None)
        };
        let Some(start) = directory_start(ctx, source, directory_end, size, count)? else {
            continue;
        };
        let selected_end = tail_start + u64_from_index(end + end_record::LEN);
        let Some(image_len) = selected_end
            .checked_sub(start)
            .and_then(|n| usize::try_from(n).ok())
        else {
            continue;
        };
        let mut image = read_range(ctx, source, start, image_len)?;
        let end = usize::try_from(tail_start + u64_from_index(end) - start)
            .map_err(|_| CodecError::malformed("ZIP probe end offset does not fit memory"))?;
        image[end + end_record::DIRECTORY_POSITION..end + end_record::DIRECTORY_POSITION + 4]
            .copy_from_slice(&0_u32.to_le_bytes());
        // An opaque comment can contain signatures. It is not namespace
        // evidence and must not make the dependency select another footer.
        image[end + end_record::COMMENT_LENGTH..end + end_record::COMMENT_LENGTH + 2]
            .copy_from_slice(&0_u16.to_le_bytes());
        if let Some((record, locator)) = zip64 {
            let record = usize::try_from(record - start).map_err(|_| {
                CodecError::malformed("ZIP64 probe record offset does not fit memory")
            })?;
            let locator = usize::try_from(locator - start).map_err(|_| {
                CodecError::malformed("ZIP64 probe locator offset does not fit memory")
            })?;
            image[record + zip64_end::DIRECTORY_POSITION
                ..record + zip64_end::DIRECTORY_POSITION + 8]
                .copy_from_slice(&0_u64.to_le_bytes());
            image[locator + zip64_locator::END_RECORD_POSITION
                ..locator + zip64_locator::END_RECORD_POSITION + 8]
                .copy_from_slice(&u64_from_index(record).to_le_bytes());
        }
        if let Err(error) = preflight_central_directory(ctx, &image) {
            if matches!(error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            continue;
        }
        return Ok(Some(image));
    }
    Ok(None)
}

fn directory_start(
    ctx: &DecodeContext<'_>,
    source: &mut dyn ReadSeek,
    end: u64,
    size: u64,
    count: u64,
) -> Result<Option<u64>, CodecError> {
    let Some(start) = end.checked_sub(size) else {
        return Ok(None);
    };
    if directory_is_framed(ctx, source, start, end, count)? {
        return Ok(Some(start));
    }
    // Some directories exclude the following digital signature from their
    // declared size. Its u16 length bounds the only additional boundary search.
    let length = usize::try_from(end.min(u64_from_index(digital_signature::LEN + 65_535)))
        .map_err(|_| CodecError::malformed("ZIP signature window does not fit memory"))?;
    let window_start = end - u64_from_index(length);
    let window = read_range(ctx, source, window_start, length)?;
    if let Some(last) = length.checked_sub(digital_signature::LEN) {
        for offset in (0..=last).rev() {
            if window.get(offset..offset + 4) != Some(b"PK\x05\x05".as_slice()) {
                continue;
            }
            if View::u16_le_at(&window, offset + digital_signature::DATA_LENGTH)
                .and_then(|size| offset.checked_add(digital_signature::LEN + usize::from(size)))
                != Some(length)
            {
                continue;
            }
            let entries_end = window_start + u64_from_index(offset);
            let Some(start) = entries_end.checked_sub(size) else {
                continue;
            };
            if directory_is_framed(ctx, source, start, entries_end, count)? {
                return Ok(Some(start));
            }
        }
    }
    Ok(None)
}

fn directory_is_framed(
    ctx: &DecodeContext<'_>,
    source: &mut dyn ReadSeek,
    mut position: u64,
    end: u64,
    count: u64,
) -> Result<bool, CodecError> {
    let prefix = u64_from_index(central_header::LEN);
    if count
        .checked_mul(prefix)
        .is_none_or(|size| size > end - position)
    {
        return Ok(false);
    }
    for _ in 0..count {
        if end - position < prefix {
            return Ok(false);
        }
        let header = read_range(ctx, source, position, central_header::LEN)?;
        if !header.starts_with(b"PK\x01\x02") {
            return Ok(false);
        }
        let fields = [
            central_header::NAME_LENGTH,
            central_header::EXTRA_LENGTH,
            central_header::COMMENT_LENGTH,
        ];
        let mut length = prefix;
        for field in fields {
            let Some(value) = View::u16_le_at(&header, field) else {
                return Ok(false);
            };
            length += u64::from(value);
        }
        let Some(next) = position.checked_add(length).filter(|next| *next <= end) else {
            return Ok(false);
        };
        position = next;
    }
    if position == end {
        return Ok(true);
    }
    if end - position < u64_from_index(digital_signature::LEN) {
        return Ok(false);
    }
    let header = read_range(ctx, source, position, digital_signature::LEN)?;
    Ok(header.starts_with(b"PK\x05\x05")
        && View::u16_le_at(&header, digital_signature::DATA_LENGTH).and_then(|size| {
            position
                .checked_add(u64_from_index(digital_signature::LEN))?
                .checked_add(u64::from(size))
        }) == Some(end))
}

/// The ZIP64 end records that end at the locator of a candidate end record.
///
/// Every candidate locator lies in the footer window, so all of them are
/// known before the first lookup. Embedded archives can displace the
/// locator's relative offset; one backward scan of the source, resumed by
/// each lookup that needs it, proves the declared record boundary for every
/// candidate.
pub(super) struct Zip64Records {
    locators: HashSet<u64>,
    found: HashMap<u64, u64>,
    scan_end: Option<u64>,
}

impl Zip64Records {
    pub(super) fn new(locators: HashSet<u64>) -> Self {
        Self {
            scan_end: locators.iter().max().copied(),
            locators,
            found: HashMap::new(),
        }
    }

    /// The record that ends at the locator at `end`: the one at `hint` when
    /// it declares that end, else the last record before `end` that does.
    pub(super) fn find(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: &mut dyn ReadSeek,
        end: u64,
        hint: u64,
    ) -> Result<Option<u64>, CodecError> {
        if hint
            .checked_add(u64_from_index(zip64_end::LEN))
            .is_some_and(|offset| offset <= end)
        {
            let header = read_range(ctx, source, hint, zip64_end::LEN)?;
            if header.starts_with(b"PK\x06\x06") && declared_end(&header, 0, hint) == Some(end) {
                return Ok(Some(hint));
            }
        }
        while !self.found.contains_key(&end) {
            let Some(scan_end) = self.scan_end else {
                break;
            };
            self.scan_window(ctx, source, scan_end)?;
        }
        Ok(self.found.get(&end).copied())
    }

    /// Scan the window ending at `scan_end` backwards, keeping the last
    /// record before each candidate locator that declares it as its end.
    fn scan_window(
        &mut self,
        ctx: &DecodeContext<'_>,
        source: &mut dyn ReadSeek,
        scan_end: u64,
    ) -> Result<(), CodecError> {
        let start = scan_end - scan_end.min(u64_from_index(FOOTER_WINDOW));
        let bytes = read_range(
            ctx,
            source,
            start,
            usize::try_from(scan_end - start)
                .map_err(|_| CodecError::malformed("ZIP64 probe window does not fit memory"))?,
        )?;
        if let Some(last_record) = bytes.len().checked_sub(12) {
            for offset in (0..=last_record).rev() {
                if bytes.get(offset..offset + 4) != Some(b"PK\x06\x06".as_slice()) {
                    continue;
                }
                let position = start + u64_from_index(offset);
                if let Some(end) =
                    declared_end(&bytes, offset, position).filter(|end| self.locators.contains(end))
                {
                    self.found.entry(end).or_insert(position);
                }
            }
        }
        self.scan_end = (start != 0).then_some(start + 11);
        Ok(())
    }
}

/// The end that the ZIP64 record at `offset` of `bytes`, found at source
/// position `position`, declares through its size field.
fn declared_end(bytes: &[u8], offset: usize, position: u64) -> Option<u64> {
    View::u64_le_at(bytes, offset + zip64_end::RECORD_SIZE)
        .filter(|size| *size >= u64_from_index(zip64_end::LEN - 12))
        .and_then(|size| position.checked_add(12)?.checked_add(size))
}

fn read_range(
    ctx: &DecodeContext<'_>,
    source: &mut dyn ReadSeek,
    start: u64,
    length: usize,
) -> Result<Vec<u8>, CodecError> {
    let mut bytes = ctx.alloc_filled(length, 0_u8, "ZIP detection metadata bytes")?;
    source.seek(SeekFrom::Start(start))?;
    source.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
