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
pub struct DataBlockIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub frame: IndexRow<String, u64>,
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
}

/// Self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::DataBlockLinkedIndexRowWire",
    into = "wire::DataBlockLinkedIndexRowWire"
)]
pub struct DataBlockLinkedIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub frame: LinkedRow<String, u64>,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
}

/// Self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::DataBlockTargetIndexRowWire",
    into = "wire::DataBlockTargetIndexRowWire"
)]
pub struct DataBlockTargetIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub frame: TargetRow<String, u64>,
    /// Directory entry containing the store.
    pub source_entry: String,
    /// Column block containing the row's opening byte.
    pub opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub opening_block_offset: u32,
}

/// Decode complete index rows from offset-store column storage.
pub fn data_block_index_rows(container: &Container) -> Vec<DataBlockIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span().map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = records.len() + 1;
            crate::om::column_row::scan::index_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let opening = column_storage_block_at(
                        section_ordinal,
                        records,
                        storage_offset + row.offset(),
                    )?;
                    let frame = row.into_absolute(source_base)?.try_resolve(|atom| {
                        control_index_data_block(section_ordinal, block_count, atom.value())
                    })?;
                    Some((frame, opening))
                })
                .enumerate()
                .map(|(ordinal, (frame, opening))| DataBlockIndexRow {
                    id: format!("nx:om-data-block-index-rows-{section_ordinal}:row#{ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    ordinal: ordinal as u32,
                    frame,
                    source_entry: entry.name.clone(),
                    opening_data_block: opening.0,
                    opening_block_offset: opening.1,
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range linked index rows from column storage.
pub fn data_block_linked_index_rows(container: &Container) -> Vec<DataBlockLinkedIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span().map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = records.len() + 1;
            crate::om::column_row::scan::linked_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let opening = column_storage_block_at(
                        section_ordinal,
                        records,
                        storage_offset + row.offset(),
                    )?;
                    let frame = row.into_absolute(source_base)?.try_resolve(|atom| {
                        control_index_data_block(section_ordinal, block_count, atom.value())
                    })?;
                    Some((frame, opening))
                })
                .enumerate()
                .map(|(ordinal, (frame, opening))| DataBlockLinkedIndexRow {
                    id: format!(
                        "nx:om-data-block-linked-index-rows-{section_ordinal}:row#{ordinal}"
                    ),
                    section_ordinal: section_ordinal as u32,
                    ordinal: ordinal as u32,
                    frame,
                    source_entry: entry.name.clone(),
                    opening_data_block: opening.0,
                    opening_block_offset: opening.1,
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range target-index rows from column storage.
pub fn data_block_target_index_rows(container: &Container) -> Vec<DataBlockTargetIndexRow> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some((_, storage, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let Some(storage_offset) = records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let source_base =
                entry.file_span().map_or(0, |(offset, _)| offset) + storage_offset as u64;
            let block_count = records.len() + 1;
            crate::om::column_row::scan::target_rows(storage)
                .into_iter()
                .filter_map(|row| {
                    let opening = column_storage_block_at(
                        section_ordinal,
                        records,
                        storage_offset + row.offset(),
                    )?;
                    let frame = row.into_absolute(source_base)?.try_resolve(|atom| {
                        control_index_data_block(section_ordinal, block_count, atom.value())
                    })?;
                    Some((frame, opening))
                })
                .enumerate()
                .map(|(ordinal, (frame, opening))| DataBlockTargetIndexRow {
                    id: format!(
                        "nx:om-data-block-target-index-rows-{section_ordinal}:row#{ordinal}"
                    ),
                    section_ordinal: section_ordinal as u32,
                    ordinal: ordinal as u32,
                    frame,
                    source_entry: entry.name.clone(),
                    opening_data_block: opening.0,
                    opening_block_offset: opening.1,
                })
                .collect()
        })
        .collect()
}
