// SPDX-License-Identifier: Apache-2.0
//! Central declarations and deferred admission of local member frames.

use super::{
    signature_at, u16_at, u32_at, u64_at, Admission, Declaration, EntryRecord, ZipCompression,
};
use crate::layout::{central_header, local_header};
use cadmpeg_core::decode::{ByteRange, DecodeContext};
use cadmpeg_core::CodecError;

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    central: u64,
    name: String,
    archive_offset: u64,
    directory_start: u64,
) -> Result<Admission<(EntryRecord, u64)>, CodecError> {
    ctx.charge_work(1, "ZIP entry central declaration")?;
    let flags = u16_at(
        bytes,
        central + cadmpeg_core::decode::u64_from_index(central_header::FLAGS),
    )?;
    let method = u16_at(
        bytes,
        central + cadmpeg_core::decode::u64_from_index(central_header::COMPRESSION),
    )?;
    let mut compressed = u64::from(u32_at(
        bytes,
        central + cadmpeg_core::decode::u64_from_index(central_header::COMPRESSED_LENGTH),
    )?);
    let mut expanded = u64::from(u32_at(
        bytes,
        central + cadmpeg_core::decode::u64_from_index(central_header::EXPANDED_LENGTH),
    )?);
    let mut local = u64::from(u32_at(
        bytes,
        central + cadmpeg_core::decode::u64_from_index(central_header::LOCAL_HEADER_POSITION),
    )?);
    let name_end = central
        + cadmpeg_core::decode::u64_from_index(central_header::LEN)
        + u64::from(u16_at(
            bytes,
            central + cadmpeg_core::decode::u64_from_index(central_header::NAME_LENGTH),
        )?);
    let extra_end = name_end
        + u64::from(u16_at(
            bytes,
            central + cadmpeg_core::decode::u64_from_index(central_header::EXTRA_LENGTH),
        )?);
    let next = extra_end
        + u64::from(u16_at(
            bytes,
            central + cadmpeg_core::decode::u64_from_index(central_header::COMMENT_LENGTH),
        )?);
    if next > cadmpeg_core::decode::u64_from_index(bytes.len()) {
        return Ok(Admission::Unreadable(CodecError::malformed(
            "ZIP central entry is truncated",
        )));
    }
    if [compressed, expanded, local].contains(&u64::from(u32::MAX)) {
        let mut extra = name_end;
        let mut found = false;
        while extra < extra_end {
            ctx.charge_work(1, "ZIP64 central extra fields")?;
            let tag = u16_at(bytes, extra)?;
            let end = extra + 4 + u64::from(u16_at(bytes, extra + 2)?);
            if end > extra_end {
                return Ok(Admission::Unreadable(CodecError::malformed(
                    "ZIP central extra is truncated",
                )));
            }
            if tag == 1 {
                if found {
                    return Ok(Admission::Unreadable(CodecError::malformed(
                        "duplicate ZIP64 central extra",
                    )));
                }
                found = true;
                let mut value = extra + 4;
                for target in [&mut expanded, &mut compressed, &mut local] {
                    if *target == u64::from(u32::MAX) {
                        if value + 8 > end {
                            return Ok(Admission::Unreadable(CodecError::malformed(
                                "ZIP64 central value is truncated",
                            )));
                        }
                        *target = u64_at(bytes, value)?;
                        value += 8;
                    }
                }
            }
            extra = end;
        }
        if !found {
            return Ok(Admission::Unreadable(CodecError::malformed(
                "ZIP64 central sizes have no extra field",
            )));
        }
    }
    let header_start = local
        .checked_add(archive_offset)
        .filter(|offset| *offset < directory_start)
        .ok_or_else(|| CodecError::malformed("ZIP local-header declaration escapes member area"))?;
    let (data_start, local_header_error) =
        match local_data_start(bytes, header_start, directory_start, compressed) {
            Ok(offset) => (Some(offset), None),
            Err(error) => (None, Some(error)),
        };
    Ok(Admission::Indexed((
        EntryRecord {
            name,
            compression: match method {
                0 => ZipCompression::Stored,
                8 => ZipCompression::Deflate,
                20 | 93 => ZipCompression::Zstd,
                method => ZipCompression::Unsupported(method),
            },
            encrypted: flags & 1 != 0,
            crc32: u32_at(
                bytes,
                central + cadmpeg_core::decode::u64_from_index(central_header::CRC32),
            )?,
            compressed_size: compressed,
            uncompressed_size: expanded,
            header_start,
            data_start,
            central_start: central,
            declaration: Declaration::Central(ByteRange {
                start: central,
                end: next,
            }),
            utf8_name: flags & 0x800 != 0,
            local_header_error,
            unreadable_range: None,
        },
        next,
    )))
}

fn local_data_start(
    bytes: &[u8],
    start: u64,
    directory: u64,
    compressed: u64,
) -> Result<u64, &'static str> {
    if signature_at(bytes, start) != Some(*b"PK\x03\x04") {
        return Err("invalid ZIP local header signature");
    }
    let fixed_end = start
        .checked_add(cadmpeg_core::decode::u64_from_index(local_header::LEN))
        .filter(|end| *end <= directory)
        .ok_or("truncated ZIP local header")?;
    let name = u16_at(
        bytes,
        start + cadmpeg_core::decode::u64_from_index(local_header::NAME_LENGTH),
    )
    .map_err(|_| "truncated ZIP local name length")?;
    let extra = u16_at(
        bytes,
        start + cadmpeg_core::decode::u64_from_index(local_header::EXTRA_LENGTH),
    )
    .map_err(|_| "truncated ZIP local extra length")?;
    let data = fixed_end
        .checked_add(u64::from(name))
        .and_then(|end| end.checked_add(u64::from(extra)))
        .filter(|end| *end <= directory)
        .ok_or("ZIP local header lengths escape member area")?;
    data.checked_add(compressed)
        .filter(|end| *end <= directory)
        .ok_or("ZIP declared payload escapes member area")?;
    Ok(data)
}

pub(super) fn frame_unreadable_members(
    ctx: &DecodeContext<'_>,
    entries: &mut [EntryRecord],
    directory: u64,
) -> Result<(), CodecError> {
    if entries.iter().all(|entry| entry.data_start.is_some()) {
        return Ok(());
    }
    let mut order = ctx.collection_vec(entries.len(), "ZIP member frame order")?;
    order.extend(0..entries.len());
    ctx.stable_sort_by(
        &mut order,
        |left, right| {
            entries[*left]
                .header_start
                .cmp(&entries[*right].header_start)
        },
        |_| 0,
        "ZIP member frame order",
    )?;
    for (position, index) in order.iter().copied().enumerate() {
        if entries[index].data_start.is_none() {
            let end = order
                .get(position + 1)
                .map_or(directory, |next| entries[*next].header_start);
            entries[index].unreadable_range = Some(ByteRange {
                start: entries[index].header_start,
                end,
            });
        }
    }
    Ok(())
}
