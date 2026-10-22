// SPDX-License-Identifier: Apache-2.0
//! Central declarations, local records, extras and ZIP32 descriptors.

use cadmpeg_core::decode::{DecodeContext, View};

use super::layout::{add, signature, size, u16_at, u32_at, u64_at, Directory};
use super::{child, push, FrameParent, FramedPart, ZipEntry};

pub(super) fn member<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    directory: Directory,
    at: usize,
    (archive, central): (usize, usize),
) -> Result<(ZipEntry<'a>, usize), String> {
    let bytes = source.window();
    let name_len = usize::from(u16_at(bytes, add(at, 28)?)?);
    let extra_len = usize::from(u16_at(bytes, add(at, 30)?)?);
    let comment_len = usize::from(u16_at(bytes, add(at, 32)?)?);
    let name_start = add(at, 46)?;
    let extra_start = add(name_start, name_len)?;
    let extra_end = add(extra_start, extra_len)?;
    let end = add(extra_end, comment_len)?;
    let compressed = u32_at(bytes, add(at, 20)?)?;
    let uncompressed = u32_at(bytes, add(at, 24)?)?;
    let local_offset = u32_at(bytes, add(at, 42)?)?;
    let fields = super::zip64::mask((uncompressed, compressed, local_offset));
    let (uncompressed, compressed, local_offset) = super::zip64::resolve(
        bytes,
        extra_start..extra_end,
        (uncompressed, compressed, local_offset),
    )?;
    if u16_at(bytes, add(at, 34)?)? != 0 {
        return Err("multi-disk ZIP member is outside this framer".into());
    }
    let central_header = push(
        ctx,
        source,
        out,
        "zip.central_header",
        FrameParent::Part(central),
        at..end,
        (
            vec![
                at..add(at, 4)?,
                add(at, 8)?..add(at, 10)?,
                add(at, 20)?..add(at, 34)?,
                add(at, 42)?..add(at, 46)?,
            ],
            vec![add(at, 8)?..add(at, 12)?, name_start..extra_start]
                .into_iter()
                .filter(|range| !range.is_empty())
                .collect(),
        ),
    )?;
    extras(
        ctx,
        source,
        out,
        "zip.central_extra_field",
        central_header,
        extra_start..extra_end,
        fields,
    )?;
    let local = usize::try_from(i128::from(local_offset) + directory.shift)
        .map_err(|_| "ZIP local-header offset is outside the archive")?;
    if !signature(bytes, local, b"PK\x03\x04") {
        return Err("ZIP local-header signature is absent at its declared shifted offset".into());
    }
    let local_name = add(local, 30)?;
    let local_extra = add(local_name, usize::from(u16_at(bytes, add(local, 26)?)?))?;
    let payload_start = add(local_extra, usize::from(u16_at(bytes, add(local, 28)?)?))?;
    let payload_end = add(payload_start, size(compressed)?)?;
    if payload_end > directory.start {
        return Err("ZIP local member overlaps the central directory".into());
    }
    let local_header = push(
        ctx,
        source,
        out,
        "zip.local_header",
        FrameParent::Part(archive),
        local..payload_start,
        (
            vec![
                local..add(local, 4)?,
                add(local, 6)?..add(local, 8)?,
                add(local, 18)?..add(local, 22)?,
                add(local, 26)?..add(local, 30)?,
            ],
            vec![add(local, 8)?..add(local, 10)?, local_name..local_extra]
                .into_iter()
                .filter(|range| !range.is_empty())
                .collect(),
        ),
    )?;
    let local_zip64 = extras(
        ctx,
        source,
        out,
        "zip.local_extra_field",
        local_header,
        local_extra..payload_start,
        0b11,
    )?;
    let local_sentinel =
        u32_at(bytes, add(local, 18)?)? == u32::MAX || u32_at(bytes, add(local, 22)?)? == u32::MAX;
    if local_sentinel && !local_zip64 {
        return Err("local ZIP64 sentinel has no actual extra field".into());
    }
    let entry = push(
        ctx,
        source,
        out,
        "zip.entry",
        FrameParent::Part(archive),
        payload_start..payload_end,
        (vec![], vec![]),
    )?;
    let descriptor_crc = if u16_at(bytes, add(local, 6)?)? & 8 != 0 {
        Some(descriptor(
            ctx,
            source,
            out,
            archive,
            payload_end,
            directory.start,
            (
                u32_at(bytes, add(at, 16)?)?,
                compressed,
                uncompressed,
                fields & 0b11 != 0 || local_zip64,
            ),
        )?)
    } else {
        None
    };
    Ok((
        ZipEntry {
            name: child(source, name_start..extra_start)?,
            payload: child(source, payload_start..payload_end)?,
            frame: entry,
            method: u16_at(bytes, add(at, 10)?)?,
            flags: u16_at(bytes, add(at, 8)?)?,
            local_crc: add(local, 14)?,
            central_crc: add(at, 16)?,
            descriptor_crc,
            uncompressed,
            local_flags: u16_at(bytes, add(local, 6)?)?,
        },
        end,
    ))
}

pub(super) fn extras<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    level: &str,
    parent: usize,
    extent: std::ops::Range<usize>,
    fields: u8,
) -> Result<bool, String> {
    let actual = super::zip64::field(source.window(), extent.clone())?;
    let mut at = extent.start;
    while at < extent.end {
        let end = add(
            add(at, 4)?,
            usize::from(u16_at(source.window(), add(at, 2)?)?),
        )?;
        if end > extent.end {
            return Err("ZIP extra field exceeds its declared extra block".into());
        }
        let framing_end = if u16_at(source.window(), at)? == 1 {
            let width = usize::try_from(fields.count_ones())
                .map_err(|_| "ZIP64 scalar count exceeds usize")?
                .checked_mul(8)
                .ok_or("ZIP64 scalar widths overflow")?;
            add(add(at, 4)?, width)?
        } else {
            add(at, 4)?
        };
        if framing_end > end {
            return Err("cut-short ZIP64 extra scalar fields".into());
        }
        push(
            ctx,
            source,
            out,
            level,
            FrameParent::Part(parent),
            at..end,
            (vec![at..framing_end], vec![at..add(at, 2)?]),
        )?;
        at = end;
    }
    Ok(actual.is_some())
}

pub(super) fn descriptor<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    parent: usize,
    start: usize,
    limit: usize,
    (crc, compressed, uncompressed, wide): (u32, u64, u64, bool),
) -> Result<usize, String> {
    let (prefix, crc_at, end) = descriptor_layout(
        source.window(),
        start,
        limit,
        (crc, compressed, uncompressed, wide),
    )?;
    let framing = if prefix == 0 {
        vec![add(crc_at, 4)?..end]
    } else {
        vec![start..crc_at, add(crc_at, 4)?..end]
    };
    push(
        ctx,
        source,
        out,
        "zip.data_descriptor",
        FrameParent::Part(parent),
        start..end,
        (framing, vec![]),
    )?;
    Ok(crc_at)
}

pub(super) fn descriptor_layout(
    bytes: &[u8],
    start: usize,
    limit: usize,
    (crc, compressed, uncompressed, wide): (u32, u64, u64, bool),
) -> Result<(usize, usize, usize), String> {
    let mut selected = None;
    for prefix in [0, 4] {
        if prefix == 4 && !signature(bytes, start, b"PK\x07\x08") {
            continue;
        }
        let crc_at = add(start, prefix)?;
        let end = add(crc_at, if wide { 20 } else { 12 })?;
        if end > limit || u32_at(bytes, crc_at)? != crc {
            continue;
        }
        let (actual_compressed, actual_uncompressed) = if wide {
            (
                u64_at(bytes, add(crc_at, 4)?)?,
                u64_at(bytes, add(crc_at, 12)?)?,
            )
        } else {
            (
                u64::from(u32_at(bytes, add(crc_at, 4)?)?),
                u64::from(u32_at(bytes, add(crc_at, 8)?)?),
            )
        };
        if actual_compressed != compressed || actual_uncompressed != uncompressed {
            continue;
        }
        if selected.replace((prefix, crc_at, end)).is_some() {
            return Err("ambiguous ZIP descriptor size".into());
        }
    }
    selected.ok_or_else(|| "ZIP descriptor does not match its central declaration".into())
}
