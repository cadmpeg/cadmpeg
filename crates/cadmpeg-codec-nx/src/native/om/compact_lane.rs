// SPDX-License-Identifier: Apache-2.0
//! Resolved counted and ABR compact-index lanes.

use serde::{Deserialize, Serialize};
use crate::container::Container;
use crate::om::compact_lane::{AbrLane, CountedLane};
use crate::om::compact_lane::scan::{abr_lanes, counted_lanes};
use super::control_index_data_block;
use wire::{DataBlockCountedIndexLaneWire, DataBlockAbrReferenceLaneWire};

mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DataBlockCountedIndexLaneWire", into = "DataBlockCountedIndexLaneWire")]
pub(crate) struct DataBlockCountedIndexLane {
    pub(crate) id: String,
    pub(crate) data_block: String,
    pub(crate) ordinal: u32,
    pub(crate) frame: CountedLane<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DataBlockAbrReferenceLaneWire", into = "DataBlockAbrReferenceLaneWire")]
pub(crate) struct DataBlockAbrReferenceLane {
    pub(crate) id: String,
    pub(crate) section_ordinal: u32,
    pub(crate) ordinal: u32,
    pub(crate) frame: AbrLane<String, u64>,
    pub(crate) source_entry: String,
}

/// Decode complete in-range counted block-index lanes from offset-only stores.
pub(crate) fn data_block_counted_index_lanes(container: &Container) -> Vec<DataBlockCountedIndexLane> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some((_, _, records)) = section.as_offset_only() else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let block_count = records.len() + 1;
            records
                .iter()
                .enumerate()
                .flat_map(|(record_ordinal, block)| {
                    let block_ordinal = record_ordinal + 1;
                    counted_lanes(block.bytes)
                        .into_iter()
                        .filter_map(|lane| {
                            let source_base = entry_offset.checked_add(block.offset as u64)?;
                            lane.into_absolute(source_base)?.try_resolve(|atom| control_index_data_block(section_ordinal, block_count, atom.value()))
                        })
                        .enumerate()
                        .map(
                            |(ordinal, frame)| DataBlockCountedIndexLane {
                                id: format!(
                                    "nx:om-data-block-counted-index-lanes-{section_ordinal}-{block_ordinal}:lane#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                frame,
                            },
                        )
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range `ABR` reference lanes from offset-store column storage.
pub(crate) fn data_block_abr_reference_lanes(container: &Container) -> Vec<DataBlockAbrReferenceLane> {
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
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let Some(source_base) = entry_offset.checked_add(storage_offset as u64) else { return Vec::new(); };
            let block_count = records.len() + 1;
            abr_lanes(storage)
                .into_iter()
                .filter_map(|lane| {
                    lane.into_absolute(source_base)?.try_resolve(|atom| control_index_data_block(section_ordinal, block_count, atom.value()))
                })
                .enumerate()
                .map(|(ordinal, frame)| DataBlockAbrReferenceLane {
                    id: format!(
                        "nx:om-data-block-abr-reference-lanes-{section_ordinal}:lane#{ordinal}"
                    ),
                    section_ordinal: section_ordinal as u32,
                    ordinal: ordinal as u32,
                    frame,
                    source_entry: entry.name.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

