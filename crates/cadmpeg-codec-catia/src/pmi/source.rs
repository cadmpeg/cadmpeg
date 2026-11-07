// SPDX-License-Identifier: Apache-2.0
//! Bounded source-frame and schema readers for CATIA PMI.

use cadmpeg_core::decode::{index_from_u32, index_from_u64, u64_from_index, View};

use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};
use crate::native::CatiaCatalog;

pub(super) fn identity(view: &mut View<'_>) -> Option<u32> {
    match view.u8()? {
        0x32 | 0x80 => view.u32_le(),
        code @ 0x81..=0xd0 => Some(u32::from(code - 0x81)),
        code @ 0xd1..=0xe4 => Some(u32::from(code - 0xd1) * 256 + u32::from(view.u8()?)),
        _ => None,
    }
}

pub(super) fn schema(view: &mut View<'_>) -> Option<u32> {
    match view.u8()? {
        0x32 => view.u32_le()?.checked_sub(1),
        code @ 0x34..=0xff => Some(u32::from(code - 0x34)),
        _ => None,
    }
}

pub(super) fn entry(catalog: &CatiaCatalog, index: u32) -> Option<&str> {
    let entry = catalog.entries.get(index_from_u32(index))?;
    (entry.ordinal == index).then_some(entry.value.as_str())
}

pub(super) fn framed(source: View<'_>, offset: u64, length: u64, marker: u8) -> Option<View<'_>> {
    let start = index_from_u64(offset)?;
    let end = index_from_u64(offset.checked_add(length)?)?;
    let mut frame = source.child(start, end)?;
    (frame.array::<2>()? == [0x7c, marker] && u64::from(frame.u32_le()?) == length).then_some(frame)
}

pub(super) fn same_entity_frame(source: View<'_>, entity: &CatiaEntityRecord) -> Option<()> {
    let mut frame = framed(source, entity.byte_offset, entity.byte_len(), 5)?;
    match &entity.body {
        CatiaEntityRecordBody::Inline(bytes) => (frame.unread() == bytes).then_some(()),
        CatiaEntityRecordBody::Nested {
            definition_prefix,
            definition_suffix,
            value_payload,
            record_suffix,
        } => {
            if frame.u8()? != entity.lead || frame.array::<2>()? != [0x7c, 6] {
                return None;
            }
            let definition_length = frame.u32_le()?;
            let expected = definition_prefix
                .len()
                .checked_add(definition_suffix.len())?
                .checked_add(11)?;
            if u64::from(definition_length) != u64_from_index(expected)
                || frame.take(definition_prefix.len())? != definition_prefix
                || frame.u8()? != 0xea
                || frame.u32_le()? != entity.entity_id
                || frame.take(definition_suffix.len())? != definition_suffix
                || frame.array::<2>()? != [0x7c, 7]
                || u64::from(frame.u32_le()?) != u64_from_index(value_payload.len().checked_add(6)?)
                || frame.take(value_payload.len())? != value_payload
            {
                return None;
            }
            (frame.unread() == record_suffix).then_some(())
        }
    }
}
