// SPDX-License-Identifier: Apache-2.0
//! Class-selected creation-display rows and their optional target resolutions.

use super::{rmfastload_target_object_id, RmFastLoadObjectId};
use crate::container::Container;
use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use serde::{Deserialize, Serialize};

mod wire;
const CLASS_NAME: &str = "UGS::RM_creation_display_data";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RmCreationDisplayDataEncoding {
    Index(IndexRow<(), u64>),
    Linked {
        row: LinkedRow<(), u64>,
        target_object_id: Option<String>,
    },
    Target {
        row: TargetRow<(), u64>,
        target_object_id: Option<String>,
    },
}

impl RmCreationDisplayDataEncoding {
    pub(crate) fn offset(&self) -> u64 {
        match self {
            Self::Index(row) => row.offset(),
            Self::Linked { row, .. } => row.offset(),
            Self::Target { row, .. } => row.offset(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "wire::RmCreationDisplayDataRelationWire",
    into = "wire::RmCreationDisplayDataRelationWire"
)]
pub(crate) struct RmCreationDisplayDataRelation {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) class_definition: String,
    pub(crate) encoding: RmCreationDisplayDataEncoding,
    pub(crate) source_entry: String,
}

/// Decode class-selected creation-display relations from `RMFastLoad` record
/// areas. The compact indices remain uninterpreted until their object roles are
/// established independently.
pub fn rm_creation_display_data_relations(
    container: &Container,
    object_ids: &[RmFastLoadObjectId],
) -> Vec<RmCreationDisplayDataRelation> {
    let mut candidates = Vec::new();
    for (entry, section) in container
        .om_sections()
        .into_iter()
        .filter(|(entry, _)| entry.name == "/Root/FastLoad/RMFastLoad")
    {
        let Some(record_area) = section.record_area else {
            continue;
        };
        let record_area_offset = record_area.offset;
        let record_area = record_area.bytes;
        let Some((class_ordinal, definition)) = section
            .types
            .iter()
            .enumerate()
            .find(|(_, definition)| definition.name == CLASS_NAME)
        else {
            continue;
        };
        let Ok(class_ordinal) = u32::try_from(class_ordinal) else {
            continue;
        };
        let entry_index = entry.index();
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let source_base = entry_offset + record_area_offset as u64;
        let class_definition = format!("nx:om-entry-{entry_index}:class#{}", definition.offset);

        for row in crate::om::column_row::scan::index_rows(record_area) {
            if row.indices()[3].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            candidates.push((
                RmCreationDisplayDataEncoding::Index(row),
                class_definition.clone(),
                entry.name.clone(),
            ));
        }
        for row in crate::om::column_row::scan::linked_rows(record_area) {
            if row.indices()[2].atom.value() != class_ordinal {
                continue;
            }
            let target_object_id =
                rmfastload_target_object_id(object_ids, row.target_index().atom.value());
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            candidates.push((
                RmCreationDisplayDataEncoding::Linked {
                    row,
                    target_object_id,
                },
                class_definition.clone(),
                entry.name.clone(),
            ));
        }
        for row in crate::om::column_row::scan::target_rows(record_area) {
            if row.indices()[2].atom.value() != class_ordinal {
                continue;
            }
            let target_object_id =
                rmfastload_target_object_id(object_ids, row.target_index().atom.value());
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            candidates.push((
                RmCreationDisplayDataEncoding::Target {
                    row,
                    target_object_id,
                },
                class_definition.clone(),
                entry.name.clone(),
            ));
        }
    }

    candidates.sort_by_key(|(encoding, _, _)| encoding.offset());
    candidates
        .into_iter()
        .enumerate()
        .map(|(ordinal, (encoding, class_definition, source_entry))| {
            RmCreationDisplayDataRelation {
                id: format!("nx:rm-creation-display-data-relations:relation#{ordinal}"),
                ordinal: ordinal as u32,
                class_definition,
                encoding,
                source_entry,
            }
        })
        .collect()
}
