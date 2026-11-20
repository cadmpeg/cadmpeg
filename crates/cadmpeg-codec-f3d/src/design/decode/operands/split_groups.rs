// SPDX-License-Identifier: Apache-2.0

use super::RecordFrame;
use crate::design::decode::{meta, sketch::indexed_record_header_at};
use crate::metastream::MetaStream;
use crate::records::entity_header::SegmentTypeData;

pub(super) fn registered_companion(
    bytes: &[u8],
    metadata: &MetaStream,
    group: &RecordFrame,
    companion: u32,
) -> bool {
    let Some(group_type) = registered_type(bytes, metadata, group.record_index, |offset| {
        offset == group.byte_offset
    }) else {
        return false;
    };
    group_type.version == 1
        && group_type.module == "Fusion"
        && group_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case("2CA5A1CD-C99B-4C9B-91AF-57989148E841")
        && group_type.base_type_guid.value().is_some_and(|base| {
            base.as_str()
                .eq_ignore_ascii_case("F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F")
        })
        && registered_type(bytes, metadata, companion, |offset| {
            offset > group.byte_offset
        })
        .is_some_and(|companion_type| {
            companion_type.version == 0
                && companion_type.module == "Fusion"
                && companion_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case("5FBE538E-139B-46D4-BCAF-CC796D1E1F78")
                && companion_type.base_type_guid.value().is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case("98542EB9-A4F2-4137-A808-DBB5B3CD6159")
                })
        })
}

fn registered_type<'a>(
    bytes: &[u8],
    metadata: &'a MetaStream,
    index: u32,
    offset_matches: impl FnOnce(u64) -> bool,
) -> Option<&'a SegmentTypeData> {
    let entity = u64::from(index);
    let mut records = metadata
        .records
        .iter()
        .filter(|record| record.entity_id == entity);
    let record = records.next()?;
    if records.next().is_some() || !offset_matches(record.bulk_offset) {
        return None;
    }
    let header = indexed_record_header_at(bytes, usize::try_from(record.bulk_offset).ok()?)?;
    if header.record_index != index {
        return None;
    }
    let (ordinal, row) = meta::dynamic_type(metadata, &header.class_tag)?;
    let mut registrations = metadata
        .types
        .iter()
        .enumerate()
        .flat_map(|(ordinal, row)| {
            row.entities
                .values()
                .filter(move |value| **value == entity)
                .map(move |_| ordinal)
        });
    (registrations.next() == Some(ordinal) && registrations.next().is_none()).then_some(row)
}
