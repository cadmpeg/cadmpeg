// SPDX-License-Identifier: Apache-2.0
//! Seekable directory evidence without acquiring archive payloads.

use super::preflight_central_directory;
use crate::layout::{central_header, digital_signature, end_record, zip64_end, zip64_locator};
use cadmpeg_core::{
    decode::{u64_from_index, DecodeContext, ScopedReservation, View},
    CodecError, ReadSeek,
};
use std::io::SeekFrom;

const FOOTER_WINDOW: usize = end_record::LEN + 65_535 + zip64_locator::LEN;

pub(super) fn read<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    source: &mut dyn ReadSeek,
) -> Result<Option<(Vec<u8>, ScopedReservation<'ctx>)>, CodecError> {
    let position = source.stream_position()?;
    let result = read_inner(ctx, source);
    source.seek(SeekFrom::Start(position))?;
    result
}

fn read_inner<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    source: &mut dyn ReadSeek,
) -> Result<Option<(Vec<u8>, ScopedReservation<'ctx>)>, CodecError> {
    let length = source.seek(SeekFrom::End(0))?;
    let tail_len = usize::try_from(length.min(u64_from_index(FOOTER_WINDOW)))
        .map_err(|_| CodecError::malformed("ZIP probe tail does not fit memory"))?;
    let tail_start = length - u64_from_index(tail_len);
    let (tail, _tail_storage) = read_range(ctx, source, tail_start, tail_len)?;
    ctx.charge_work(u64_from_index(tail.len()), "ZIP probe end records")?;
    let Some(last_end) = tail.len().checked_sub(end_record::LEN) else {
        return Ok(None);
    };
    for end in (0..=last_end).rev() {
        if tail.get(end..end + 4) != Some(b"PK\x05\x06".as_slice()) {
            continue;
        }
        let Some(comment) = View::u16_le_at(&tail, end + end_record::COMMENT_LENGTH) else {
            continue;
        };
        if end.checked_add(end_record::LEN + usize::from(comment)) != Some(tail.len()) {
            continue;
        }
        let directory_end = tail_start + u64_from_index(end);
        let locator = end
            .checked_sub(zip64_locator::LEN)
            .filter(|start| tail.get(*start..*start + 4) == Some(b"PK\x06\x07".as_slice()));
        let (size, count, directory_end, zip64) = if let Some(locator) = locator {
            let Some(hint) = View::u64_le_at(&tail, locator + zip64_locator::END_RECORD_POSITION)
            else {
                continue;
            };
            let locator_position = tail_start + u64_from_index(locator);
            let Some(record) = find_zip64(ctx, source, tail_start, locator, hint)? else {
                continue;
            };
            let (header, _storage) = read_range(ctx, source, record, zip64_end::LEN)?;
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
        // A declared directory is metadata. Its storage is scoped independently
        // of input acquisition and does not consume one collection slot per byte.
        let (mut image, storage) = read_range(ctx, source, start, image_len)?;
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
        return Ok(Some((image, storage)));
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
    let (window, _storage) = read_range(ctx, source, window_start, length)?;
    ctx.charge_work(
        u64_from_index(length),
        "ZIP directory signature boundary search",
    )?;
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
        let (header, _storage) = read_range(ctx, source, position, central_header::LEN)?;
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
    let (header, _storage) = read_range(ctx, source, position, digital_signature::LEN)?;
    Ok(header.starts_with(b"PK\x05\x05")
        && View::u16_le_at(&header, digital_signature::DATA_LENGTH).and_then(|size| {
            position
                .checked_add(u64_from_index(digital_signature::LEN))?
                .checked_add(u64::from(size))
        }) == Some(end))
}

pub(super) fn find_zip64(
    ctx: &DecodeContext<'_>,
    source: &mut dyn ReadSeek,
    tail_start: u64,
    locator: usize,
    hint: u64,
) -> Result<Option<u64>, CodecError> {
    let end = tail_start + u64_from_index(locator);
    if hint
        .checked_add(u64_from_index(zip64_end::LEN))
        .is_some_and(|offset| offset <= end)
    {
        let (header, _storage) = read_range(ctx, source, hint, zip64_end::LEN)?;
        if header.starts_with(b"PK\x06\x06")
            && View::u64_le_at(&header, zip64_end::RECORD_SIZE)
                .filter(|size| *size >= u64_from_index(zip64_end::LEN - 12))
                .and_then(|size| hint.checked_add(12)?.checked_add(size))
                == Some(end)
        {
            return Ok(Some(hint));
        }
    }
    // Embedded archives can displace the locator's relative offset. Search
    // backwards in bounded windows and prove the declared record boundary.
    let mut scan_end = end;
    loop {
        let start = scan_end - scan_end.min(u64_from_index(FOOTER_WINDOW));
        let (bytes, _storage) = read_range(
            ctx,
            source,
            start,
            usize::try_from(scan_end - start)
                .map_err(|_| CodecError::malformed("ZIP64 probe window does not fit memory"))?,
        )?;
        ctx.charge_work(u64_from_index(bytes.len()), "ZIP64 probe boundary search")?;
        if let Some(last_record) = bytes.len().checked_sub(12) {
            for offset in (0..=last_record).rev() {
                if bytes.get(offset..offset + 4) == Some(b"PK\x06\x06".as_slice())
                    && View::u64_le_at(&bytes, offset + zip64_end::RECORD_SIZE)
                        .filter(|size| *size >= u64_from_index(zip64_end::LEN - 12))
                        .and_then(|size| {
                            (start + u64_from_index(offset))
                                .checked_add(12)?
                                .checked_add(size)
                        })
                        == Some(end)
                {
                    return Ok(Some(start + u64_from_index(offset)));
                }
            }
        }
        if start == 0 {
            break;
        }
        scan_end = start + 11;
    }
    Ok(None)
}

fn read_range<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    source: &mut dyn ReadSeek,
    start: u64,
    length: usize,
) -> Result<(Vec<u8>, ScopedReservation<'ctx>), CodecError> {
    ctx.charge_work(u64_from_index(length), "read ZIP detection metadata")?;
    let (mut bytes, storage) = ctx.scoped_vector_storage(length, "ZIP detection metadata bytes")?;
    bytes.resize(length, 0);
    source.seek(SeekFrom::Start(start))?;
    source.read_exact(&mut bytes)?;
    Ok((bytes, storage))
}

#[cfg(test)]
mod tests;
