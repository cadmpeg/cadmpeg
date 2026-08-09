// SPDX-License-Identifier: Apache-2.0
//! Shared Fusion `MetaStream` segment framing.

use cadmpeg_core::le::{u32_at, u64_at};
use cadmpeg_core::CodecError;

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded};
use crate::records::SegmentType;

/// One primary record-index entry locating a sibling `BulkStream` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordIndexEntry {
    pub(crate) entity_id: u64,
    pub(crate) bulk_offset: u64,
}

/// One completely framed `MetaStream` segment.
pub(crate) struct MetaStream {
    pub(crate) types: Vec<SegmentType>,
    pub(crate) records: Vec<RecordIndexEntry>,
}

fn take_counted_run(bytes: &[u8], at: &mut usize, stride: usize) -> Option<()> {
    let count = usize::try_from(u32_at(bytes, *at)?).ok()?;
    let start = at.checked_add(4)?;
    let end = count.checked_mul(stride)?.checked_add(start)?;
    bytes.get(start..end)?;
    *at = end;
    Some(())
}

fn take_record_index(bytes: &[u8], at: &mut usize) -> Option<Vec<RecordIndexEntry>> {
    let count = usize::try_from(u32_at(bytes, *at)?).ok()?;
    *at = at.checked_add(4)?;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let entity_id = u64_at(bytes, *at)?;
        let bulk_offset = u64_at(bytes, at.checked_add(8)?)?;
        *at = at.checked_add(16)?;
        records.push(RecordIndexEntry {
            entity_id,
            bulk_offset,
        });
    }
    Some(records)
}

fn parse_inner(bytes: &[u8]) -> Option<MetaStream> {
    // Header: short segment type name, segment id, asset GUID, serializer
    // magic and its magic-gated integer group, full segment type name, add-in
    // name, and the segment type code.
    let (_, at) = lp_ascii_filtered(bytes, 0, 1..=256, u8::is_ascii_graphic)?;
    let at = at.checked_add(4)?;
    let (_, at) = lp_utf16_bounded(bytes, at, 0..=256)?;
    let magic = u32_at(bytes, at)?;
    let at = at.checked_add(if magic == 1234 { 16 } else { 8 })?;
    let (_, at) = lp_ascii_filtered(bytes, at, 1..=256, u8::is_ascii_graphic)?;
    let (_, at) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)?;
    let mut at = at.checked_add(8)?;

    let count = u32_at(bytes, at)?;
    at = at.checked_add(4)?;
    let mut types = Vec::new();
    for _ in 0..count {
        let entry_at = at;
        let type_guid_offset = at.checked_add(4)?;
        let (type_guid, next) = lp_ascii_filtered(bytes, at, 1..=256, u8::is_ascii_graphic)
            .filter(|(guid, _)| is_guid_relaxed(guid))?;
        at = next;
        let base_type_guid_offset = at.checked_add(4)?;
        let (base_type_guid, next) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)
            .filter(|(guid, _)| guid.is_empty() || is_guid_relaxed(guid))?;
        at = next;
        let version_offset = at;
        let version = u32_at(bytes, at)?;
        at = at.checked_add(4)?;
        let (module, next) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)?;
        at = next;
        let id_count = usize::try_from(u32_at(bytes, at)?).ok()?;
        let ids_at = at.checked_add(4)?;
        let ids_end = id_count.checked_mul(8)?.checked_add(ids_at)?;
        let raw_ids = bytes.get(ids_at..ids_end)?;
        at = ids_end;
        types.push(SegmentType {
            id: String::new(),
            byte_offset: entry_at as u64,
            type_guid,
            type_guid_offset: type_guid_offset as u64,
            base_type_guid_offset: (!base_type_guid.is_empty())
                .then_some(base_type_guid_offset as u64),
            base_type_guid: (!base_type_guid.is_empty()).then_some(base_type_guid),
            version,
            version_offset: version_offset as u64,
            module,
            entity_ids: raw_ids
                .chunks_exact(8)
                .map(|raw| {
                    u64::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    )
                })
                .collect(),
            entity_id_offsets: (0..id_count)
                .map(|index| (ids_at + index * 8) as u64)
                .collect(),
        });
    }

    // Named entities, the primary record index, and the secondary index.
    take_counted_run(bytes, &mut at, 8)?;
    let records = take_record_index(bytes, &mut at)?;
    take_counted_run(bytes, &mut at, 16)?;

    // A legacy segment can end after the secondary index, after the
    // next-entity counter, or after the complete flag/property suffix.
    if at < bytes.len() {
        bytes.get(at..at.checked_add(8)?)?;
        at += 8;
    }
    if at < bytes.len() {
        bytes.get(at..at.checked_add(8)?)?;
        at += 4;
        let properties = u32_at(bytes, at)?;
        at = at.checked_add(4)?;
        for _ in 0..properties {
            let (_, next) = lp_ascii_filtered(bytes, at, 0..=256, u8::is_ascii_graphic)?;
            at = next.checked_add(4)?;
        }
    }
    (at == bytes.len()).then_some(MetaStream { types, records })
}

/// Parse one complete `MetaStream` segment and reject any unframed remainder.
pub(crate) fn parse(bytes: &[u8], stream: &str) -> Result<MetaStream, CodecError> {
    parse_inner(bytes).ok_or_else(|| {
        CodecError::Malformed(format!("invalid F3D MetaStream segment framing: {stream}"))
    })
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn rejects_unframed_empty_input() {
        assert!(parse(&[], "empty").is_err());
    }
}
