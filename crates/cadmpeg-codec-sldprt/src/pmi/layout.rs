// SPDX-License-Identifier: Apache-2.0
//! Exact MessagePack map extents and source-coordinate handoff.

use cadmpeg_core::decode::{DecodeContext, View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{source_fidelity::RecordLayout, SourceFidelity};
use std::collections::BTreeMap;

pub(crate) fn install(
    ctx: &DecodeContext<'_>,
    mut pointers: BTreeMap<String, BTreeMap<String, u64>>,
    fidelity: &mut SourceFidelity,
) -> Result<(), CodecError> {
    for capture in ctx.take_record_captures()? {
        let (id, mut layout) = RecordLayout::from_capture(capture);
        if let Some(fields) = pointers.remove(&id) {
            for (field, value) in fields {
                layout.insert_pointer(field, value)?;
            }
        }
        fidelity.insert_record_layout(id, layout)?;
    }
    if !pointers.is_empty() {
        return Err(CodecError::malformed(
            "PMI source pointers lack their record capture",
        ));
    }
    Ok(())
}

pub(super) fn map_extent<'a>(
    source: View<'a>,
    offset: usize,
    work: &WorkBudget,
) -> Option<View<'a>> {
    let mut cursor = offset;
    value_end(source.window(), &mut cursor, 0, work)?;
    source.child(
        source.start().checked_add(offset)?,
        source.start().checked_add(cursor)?,
    )
}

fn advance(bytes: &[u8], cursor: &mut usize, count: usize) -> Option<()> {
    *cursor = cursor
        .checked_add(count)
        .filter(|end| *end <= bytes.len())?;
    Some(())
}

fn length(bytes: &[u8], cursor: &mut usize, width: usize) -> Option<usize> {
    let result = match width {
        1 => usize::from(*bytes.get(*cursor)?),
        2 => usize::from(View::u16_be_at(bytes, *cursor)?),
        4 => usize::try_from(View::u32_be_at(bytes, *cursor)?).ok()?,
        _ => return None,
    };
    advance(bytes, cursor, width)?;
    Some(result)
}

fn value_end(bytes: &[u8], cursor: &mut usize, depth: usize, work: &WorkBudget) -> Option<()> {
    if depth > 16 || !work.charge() {
        return None;
    }
    let marker = *bytes.get(*cursor)?;
    advance(bytes, cursor, 1)?;
    let (count, payload) = match marker {
        0x00..=0x7f | 0xc0 | 0xc2 | 0xc3 | 0xe0..=0xff => (0, 0),
        0x80..=0x8f => (usize::from(marker & 15).checked_mul(2)?, 0),
        0x90..=0x9f => (usize::from(marker & 15), 0),
        0xa0..=0xbf => (0, usize::from(marker & 31)),
        0xc4 | 0xd9 => (0, length(bytes, cursor, 1)?),
        0xc5 | 0xda => (0, length(bytes, cursor, 2)?),
        0xc6 | 0xdb => (0, length(bytes, cursor, 4)?),
        0xc7 => (0, length(bytes, cursor, 1)?.checked_add(1)?),
        0xc8 => (0, length(bytes, cursor, 2)?.checked_add(1)?),
        0xc9 => (0, length(bytes, cursor, 4)?.checked_add(1)?),
        0xca | 0xce | 0xd2 => (0, 4),
        0xcb | 0xcf | 0xd3 => (0, 8),
        0xcc | 0xd0 => (0, 1),
        0xcd | 0xd1 => (0, 2),
        0xd4 => (0, 2),
        0xd5 => (0, 3),
        0xd6 => (0, 5),
        0xd7 => (0, 9),
        0xd8 => (0, 17),
        0xdc => (length(bytes, cursor, 2)?, 0),
        0xdd => (length(bytes, cursor, 4)?, 0),
        0xde => (length(bytes, cursor, 2)?.checked_mul(2)?, 0),
        0xdf => (length(bytes, cursor, 4)?.checked_mul(2)?, 0),
        0xc1 => return None,
    };
    advance(bytes, cursor, payload)?;
    if count > bytes.len().checked_sub(*cursor)? {
        return None;
    }
    for _ in 0..count {
        value_end(bytes, cursor, depth + 1, work)?;
    }
    Some(())
}

#[cfg(test)]
mod tests;
