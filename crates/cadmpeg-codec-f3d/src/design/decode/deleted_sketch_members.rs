// SPDX-License-Identifier: Apache-2.0
//! Sketch scope references that name deleted sketch members.

use super::sketch::indexed_record_offsets;
use crate::container::ContainerScan;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::layout::record_timestamp_entry as entry;
use crate::layout::record_timestamp_table_prefix as table;
use crate::records::{decal::DesignRecordHeader, feature::scope::DesignParameterScope};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// Record the reference members of each Sketch scope that name a deleted
/// sketch member.
pub(crate) fn mark_deleted_sketch_members(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let held = headers
        .iter()
        .filter_map(|record| Some((crate::ids::native_stream(&record.id)?, record.record_index)))
        .collect::<std::collections::HashSet<_>>();
    for stream_entry in scan.entries.iter().filter(|stream_entry| {
        super::sheet_metal_settings::is_settings_stream(
            scan,
            stream_entry,
            ContainerRole::Bulkstream,
        )
    }) {
        let stream = crate::ids::native_scope(&stream_entry.name);
        let mut bytes = None;
        for scope in scopes.iter_mut().filter(|scope| {
            design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch)
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
        }) {
            let absent = scope
                .reference_members()
                .values()
                .copied()
                .filter(|index| !held.contains(&(stream.as_str(), *index)))
                .collect::<Vec<_>>();
            if absent.is_empty() {
                continue;
            }
            let bytes = match bytes {
                Some(bytes) => bytes,
                None => *bytes.insert(scan.entry_bytes(&stream_entry.name)?),
            };
            let (Ok(start), Ok(end)) = (
                usize::try_from(scope.byte_offset()),
                usize::try_from(scope.paired_byte_offset()),
            ) else {
                continue;
            };
            scope.deleted_sketch_members =
                deleted_members(bytes, start..end, scope.record_index, &absent);
        }
    }
    Ok(())
}

/// The absent indices of scope `owner` that have no indexed header and whose
/// u32 encoding occurs only in `frame` or as a record-timestamp table entry.
fn deleted_members(
    bytes: &[u8],
    frame: std::ops::Range<usize>,
    owner: u32,
    absent: &[u32],
) -> Vec<u32> {
    let Some(entries) = timestamp_entry_offsets(bytes, owner) else {
        return Vec::new();
    };
    absent
        .iter()
        .copied()
        .filter(|index| {
            indexed_record_offsets(bytes).all(|header| header.record_index != *index)
                && memchr::memmem::find_iter(bytes, &index.to_le_bytes()).all(|at| {
                    (at >= frame.start && at.checked_add(4).is_some_and(|last| last <= frame.end))
                        || entries.contains(&at)
                })
        })
        .collect()
}

/// Offsets of the entry record indices in the one record-timestamp table
/// whose owner reference names record `owner`.
fn timestamp_entry_offsets(bytes: &[u8], owner: u32) -> Option<Vec<usize>> {
    let mut tables = indexed_record_offsets(bytes)
        .filter_map(|header| timestamp_table(bytes, header.offset))
        .filter(|(table_owner, _)| *table_owner == owner);
    let (_, offsets) = tables.next()?;
    tables.next().is_none().then_some(offsets)
}

/// The owner and entry index offsets of a record-timestamp table whose
/// indexed header begins at `start`.
fn timestamp_table(bytes: &[u8], start: usize) -> Option<(u32, Vec<usize>)> {
    let zero_run = start.checked_add(table::ZERO_RUN)?..start.checked_add(table::FIRST_MARKER)?;
    let prefix_valid = bytes.get(zero_run)?.iter().all(|byte| *byte == 0)
        && *bytes.get(start.checked_add(table::FIRST_MARKER)?)? == table::FIRST_MARKER_VALUE
        && *bytes.get(start.checked_add(table::SECOND_MARKER)?)? == table::SECOND_MARKER_VALUE;
    if !prefix_valid {
        return None;
    }
    let count = usize::try_from(View::u32_le_at(
        bytes,
        start.checked_add(table::ENTRY_COUNT)?,
    )?)
    .ok()?;
    let first = start.checked_add(table::LEN)?;
    let owner_entry = count.checked_mul(entry::LEN)?.checked_add(first)?;
    if owner_entry.checked_add(entry::LEN)? > bytes.len() {
        return None;
    }
    let mut offsets = Vec::with_capacity(count);
    for at in (first..owner_entry).step_by(entry::LEN) {
        offsets.push(entry_record_index(bytes, at)?.1);
    }
    Some((entry_record_index(bytes, owner_entry)?.0, offsets))
}

/// Record index of the entry at `at` and the offset of its encoding.
fn entry_record_index(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let index_at = at.checked_add(entry::RECORD_INDEX)?;
    let zero_run = at.checked_add(entry::ZERO_RUN)?..at.checked_add(entry::TIMESTAMP)?;
    (*bytes.get(at.checked_add(entry::MARKER)?)? == entry::MARKER_VALUE
        && bytes.get(zero_run)?.iter().all(|byte| *byte == 0))
    .then_some((View::u32_le_at(bytes, index_at)?, index_at))
}

#[cfg(test)]
mod tests;
