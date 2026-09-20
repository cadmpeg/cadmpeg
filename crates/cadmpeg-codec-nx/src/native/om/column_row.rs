// SPDX-License-Identifier: Apache-2.0
//! Native column rows retain one checked source frame with resolved targets.

use super::{column_storage_block_at, control_index_data_block};
use crate::container::Container;
use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use serde::{Deserialize, Serialize};

mod wire;

/// Self-framed index row in contiguous offset-store column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::DataBlockIndexRowWire",
    into = "wire::DataBlockIndexRowWire"
)]
pub(in crate::native) struct DataBlockIndexRow {
    /// Globally unique row identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub(in crate::native) ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub(in crate::native) frame: IndexRow<String, u64>,
    /// Directory entry containing the offset-only store.
    pub(in crate::native) source_entry: String,
    /// Column block containing the row's opening byte.
    pub(in crate::native) opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub(in crate::native) opening_block_offset: u32,
}

/// Self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::DataBlockLinkedIndexRowWire",
    into = "wire::DataBlockLinkedIndexRowWire"
)]
pub(in crate::native) struct DataBlockLinkedIndexRow {
    /// Globally unique row identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub(in crate::native) ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub(in crate::native) frame: LinkedRow<String, u64>,
    /// Directory entry containing the store.
    pub(in crate::native) source_entry: String,
    /// Column block containing the row's opening byte.
    pub(in crate::native) opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub(in crate::native) opening_block_offset: u32,
}

/// Self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::DataBlockTargetIndexRowWire",
    into = "wire::DataBlockTargetIndexRowWire"
)]
pub(in crate::native) struct DataBlockTargetIndexRow {
    /// Globally unique row identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub(in crate::native) ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub(in crate::native) frame: TargetRow<String, u64>,
    /// Directory entry containing the store.
    pub(in crate::native) source_entry: String,
    /// Column block containing the row's opening byte.
    pub(in crate::native) opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub(in crate::native) opening_block_offset: u32,
}

/// Decode complete index rows from offset-store column storage.
pub(in crate::native) fn data_block_index_rows(container: &Container) -> Vec<DataBlockIndexRow> {
    project_column_rows(
        container,
        |storage, section, block_count, source_base| {
            crate::om::column_row::scan::index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let offset = row.offset();
                    let frame = row.into_absolute(source_base)?.try_resolve(|atom| {
                        control_index_data_block(section, block_count, atom.value())
                    })?;
                    Some((offset, frame))
                })
                .collect()
        },
        |section_ordinal, ordinal, frame, source_entry, opening| DataBlockIndexRow {
            id: format!("nx:om-data-block-index-rows-{section_ordinal}:row#{ordinal}"),
            section_ordinal: section_ordinal as u32,
            ordinal: ordinal as u32,
            frame,
            source_entry,
            opening_data_block: opening.0,
            opening_block_offset: opening.1,
        },
    )
}

/// Decode complete in-range linked index rows from column storage.
pub(in crate::native) fn data_block_linked_index_rows(
    container: &Container,
) -> Vec<DataBlockLinkedIndexRow> {
    project_column_rows(
        container,
        |storage, section, block_count, source_base| {
            crate::om::column_row::scan::linked_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let offset = row.offset();
                    let frame = row.into_absolute(source_base)?.try_resolve(|atom| {
                        control_index_data_block(section, block_count, atom.value())
                    })?;
                    Some((offset, frame))
                })
                .collect()
        },
        |section_ordinal, ordinal, frame, source_entry, opening| DataBlockLinkedIndexRow {
            id: format!("nx:om-data-block-linked-index-rows-{section_ordinal}:row#{ordinal}"),
            section_ordinal: section_ordinal as u32,
            ordinal: ordinal as u32,
            frame,
            source_entry,
            opening_data_block: opening.0,
            opening_block_offset: opening.1,
        },
    )
}

/// Decode complete in-range target-index rows from column storage.
pub(in crate::native) fn data_block_target_index_rows(
    container: &Container,
) -> Vec<DataBlockTargetIndexRow> {
    project_column_rows(
        container,
        |storage, section, block_count, source_base| {
            crate::om::column_row::scan::target_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let offset = row.offset();
                    let frame = row.into_absolute(source_base)?.try_resolve(|atom| {
                        control_index_data_block(section, block_count, atom.value())
                    })?;
                    Some((offset, frame))
                })
                .collect()
        },
        |section_ordinal, ordinal, frame, source_entry, opening| DataBlockTargetIndexRow {
            id: format!("nx:om-data-block-target-index-rows-{section_ordinal}:row#{ordinal}"),
            section_ordinal: section_ordinal as u32,
            ordinal: ordinal as u32,
            frame,
            source_entry,
            opening_data_block: opening.0,
            opening_block_offset: opening.1,
        },
    )
}

/// One owner for section framing, source locations and admitted row ordinals.
fn project_column_rows<F, T>(
    container: &Container,
    scan: impl Fn(&[u8], usize, usize, u64) -> Vec<(usize, F)>,
    project: impl Fn(usize, usize, F, String, (String, u32)) -> T,
) -> Vec<T> {
    let mut result = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        let Some((_, storage, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(storage_offset) = records.first().map(|record| record.offset) else {
            continue;
        };
        let source_base = entry.file_span().map_or(0, |(offset, _)| offset) + storage_offset as u64;
        let rows = scan(storage, section_ordinal, records.len() + 1, source_base);
        for (ordinal, (frame, opening)) in rows
            .into_iter()
            .filter_map(|(offset, frame)| {
                let opening =
                    column_storage_block_at(section_ordinal, records, storage_offset + offset)?;
                Some((frame, opening))
            })
            .enumerate()
        {
            result.push(project(
                section_ordinal,
                ordinal,
                frame,
                entry.name.clone(),
                opening,
            ));
        }
    }
    result
}
