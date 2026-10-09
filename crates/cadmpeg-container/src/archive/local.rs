// SPDX-License-Identifier: Apache-2.0
//! Sequential local-frame recovery when directory framing is unavailable.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, ByteRange, DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{ArchiveSnapshot, Declaration, EntryRecord, ZipCompression};
use crate::layout::local_header;

#[derive(Debug)]
pub(super) struct Recovery {
    pub(super) reason: String,
    pub(super) tail: Option<ByteRange>,
}

pub(super) fn recover<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    error: CodecError,
) -> Result<ArchiveSnapshot<'a>, CodecError> {
    // There is no directory to establish an embedded archive's base. Do not
    // search arbitrary payload bytes for a replacement namespace.
    if !root.window().starts_with(b"PK\x03\x04") {
        return Err(error);
    }
    let reason = ctx.format_retained(
        format_args!("local ZIP recovery: {error}"),
        "ZIP recovery diagnostic",
    )?;
    let mut walk = root;
    let mut entries = Vec::new();
    let mut by_name = BTreeMap::new();
    while walk.unread().starts_with(b"PK\x03\x04") {
        ctx.charge_work(1, "ZIP recovery local frame")?;
        let Some(entry) = read(ctx, &mut walk)? else {
            break;
        };
        if by_name.contains_key(&entry.name) {
            return Err(CodecError::malformed("duplicate recovered ZIP entry name"));
        }
        let name = ctx.copy_retained_text(&entry.name, "ZIP recovered indexed name")?;
        ctx.insert_btree_map(
            &mut by_name,
            name,
            entries.len(),
            "ZIP recovered name index",
        )?;
        ctx.push_vec(&mut entries, entry, "ZIP recovered entry records")?;
    }
    let end = u64_from_index(root.window().len());
    let tail = (!walk.is_empty()).then_some(ByteRange {
        start: u64_from_index(walk.read_len()),
        end,
    });
    ctx.charge_retained(
        u64_from_index(std::mem::size_of::<Recovery>()),
        "ZIP recovery state",
    )?;
    Ok(ArchiveSnapshot {
        root,
        // Uninterpreted tail bytes are partitioned as padding. No footer or
        // damaged central declaration is admitted by the recovery walk.
        central_start: end,
        entries,
        by_name,
        recovery: Some(Box::new(Recovery { reason, tail })),
    })
}

fn read(ctx: &DecodeContext<'_>, walk: &mut View<'_>) -> Result<Option<EntryRecord>, CodecError> {
    let start = u64_from_index(walk.read_len());
    let mut header = *walk;
    if header.remaining() < local_header::LEN {
        return Ok(None);
    }
    header.req_take(6)?; // Signature and extraction version.
    let flags = header.req_u16_le()?;
    let method = header.req_u16_le()?;
    header.req_take(4)?; // Time and date.
    let crc32 = header.req_u32_le()?;
    let compressed = u64::from(header.req_u32_le()?);
    let expanded = u64::from(header.req_u32_le()?);
    let name_len = usize::from(header.req_u16_le()?);
    let extra_len = usize::from(header.req_u16_le()?);
    let Some(raw_name) = header.take(name_len) else {
        return Ok(None);
    };
    let name = decode_name(ctx, raw_name, flags & 0x800 != 0)?;
    ctx.charge_work(u64_from_index(extra_len), "ZIP recovery local extra")?;
    let extra_start = header.position();
    let mut record = EntryRecord {
        name,
        compression: match method {
            0 => ZipCompression::Stored,
            8 => ZipCompression::Deflate,
            20 | 93 => ZipCompression::Zstd,
            method => ZipCompression::Unsupported(method),
        },
        encrypted: flags & 1 != 0,
        crc32,
        compressed_size: compressed,
        uncompressed_size: expanded,
        header_start: start,
        data_start: None,
        central_start: start,
        declaration: Declaration::Local(ByteRange {
            start,
            end: u64_from_index(header.read_len()),
        }),
        utf8_name: flags & 0x800 != 0,
        local_header_error: None,
        unreadable_range: None,
    };
    let frame = (|| {
        header.take(extra_len).ok_or("truncated ZIP local extra")?;
        record.declaration = Declaration::Local(ByteRange {
            start,
            end: u64_from_index(header.read_len()),
        });
        if compressed == u64::from(u32::MAX) || expanded == u64::from(u32::MAX) {
            let mut extra = header
                .child(extra_start, header.position())
                .ok_or("ZIP local extra escapes header")?;
            zip64_sizes(&mut extra, &mut record)?;
        }
        if flags & 8 != 0 && record.compressed_size == 0 {
            return Err("ZIP data descriptor has no local sizes");
        }
        // Encrypted stored bytes also include encryption framing.
        if record.compression == ZipCompression::Stored
            && (record.compressed_size < record.uncompressed_size
                || (!record.encrypted && record.compressed_size > record.uncompressed_size))
        {
            return Err("stored ZIP local sizes disagree");
        }
        let data_start = u64_from_index(header.read_len());
        let payload_size = usize::try_from(record.compressed_size)
            .map_err(|_| "ZIP local payload size exceeds memory")?;
        header
            .skip(payload_size)
            .ok_or("truncated ZIP local payload")?;
        record.data_start = Some(data_start);
        if flags & 8 != 0 {
            let descriptor_end = super::parse_data_descriptor(
                header.window(),
                &record,
                u64_from_index(header.window().len()),
            )
            .map_err(|_| "invalid ZIP local data descriptor")?;
            let descriptor_len = usize::try_from(
                descriptor_end
                    - record
                        .data_end()
                        .map_err(|_| "ZIP local payload end overflows")?,
            )
            .map_err(|_| "ZIP descriptor size exceeds memory")?;
            header
                .skip(descriptor_len)
                .ok_or("truncated ZIP local data descriptor")?;
        }
        Ok(())
    })();
    if let Err(error) = frame {
        record.data_start = None;
        record.local_header_error = Some(error);
        record.unreadable_range = Some(ByteRange {
            start,
            end: u64_from_index(walk.window().len()),
        });
        walk.seek_to_end();
    } else {
        *walk = header;
    }
    Ok(Some(record))
}

fn zip64_sizes(extra: &mut View<'_>, record: &mut EntryRecord) -> Result<(), &'static str> {
    let mut found = false;
    while !extra.is_empty() {
        let tag = extra.u16_le().ok_or("truncated ZIP local extra tag")?;
        let size = usize::from(extra.u16_le().ok_or("truncated ZIP local extra size")?);
        let start = extra.position();
        extra.skip(size).ok_or("truncated ZIP local extra field")?;
        if tag != 1 {
            continue;
        }
        if found {
            return Err("duplicate ZIP64 local extra");
        }
        found = true;
        let mut values = extra
            .child(start, extra.position())
            .ok_or("ZIP local extra field escapes header")?;
        // APPNOTE 4.5.3 requires both sizes in a local ZIP64 extra field.
        // Central records instead omit fields whose ordinary value fits.
        let expanded = values.u64_le().ok_or("truncated ZIP64 local size")?;
        let compressed = values.u64_le().ok_or("truncated ZIP64 local size")?;
        for (target, size) in [
            (&mut record.uncompressed_size, expanded),
            (&mut record.compressed_size, compressed),
        ] {
            if *target != u64::from(u32::MAX) && *target != size {
                return Err("ZIP64 local sizes disagree");
            }
            *target = size;
        }
    }
    if !found {
        return Err("ZIP64 local sizes have no extra field");
    }
    Ok(())
}

fn decode_name(ctx: &DecodeContext<'_>, bytes: &[u8], utf8: bool) -> Result<String, CodecError> {
    // ZIP's legacy filename encoding is IBM code page 437. ASCII controls keep
    // their byte values, as in the directory parser.
    const HIGH: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ',
        'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú',
        'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡',
        '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟',
        '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘',
        '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ',
        '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²',
        '■', '\u{a0}',
    ];
    ctx.charge_work(
        u64_from_index(bytes.len()) * 2,
        "ZIP recovered name decoding",
    )?;
    if utf8 {
        return ctx.copy_retained_lossy_utf8(bytes, "ZIP recovered UTF-8 name");
    }
    let decode = |byte: u8| {
        if byte < 128 {
            char::from(byte)
        } else {
            HIGH[usize::from(byte - 128)]
        }
    };
    let length = bytes.iter().map(|byte| decode(*byte).len_utf8()).sum();
    let mut name = ctx.retained_string(length, "ZIP recovered legacy name")?;
    name.extend(bytes.iter().map(|byte| decode(*byte)));
    Ok(name)
}

#[cfg(test)]
mod tests;
