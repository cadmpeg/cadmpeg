// SPDX-License-Identifier: Apache-2.0
//! Class-selected creation-display rows and their optional target resolutions.

use super::{rmfastload_target_object_id, RmFastLoadObjectId};
use crate::container::Container;
use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

mod borrowed_wires;
mod wire;
const CLASS_NAME: &str = "UGS::RM_creation_display_data";

#[derive(Debug, Clone, PartialEq, Eq)]
enum RmCreationDisplayDataEncoding {
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
    fn offset(&self) -> u64 {
        match self {
            Self::Index(row) => row.offset(),
            Self::Linked { row, .. } => row.offset(),
            Self::Target { row, .. } => row.offset(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "wire::RmCreationDisplayDataRelationWire")]
pub(in crate::native) struct RmCreationDisplayDataRelation {
    id: String,
    ordinal: u32,
    class_definition: String,
    encoding: RmCreationDisplayDataEncoding,
    source_entry: String,
}

fn push_relation(
    relations: &mut Vec<RmCreationDisplayDataRelation>,
    encoding: RmCreationDisplayDataEncoding,
    entry_index: usize,
    definition_offset: usize,
    source_entry: &str,
) {
    relations.push(RmCreationDisplayDataRelation {
        id: String::new(),
        ordinal: 0,
        class_definition: format!("nx:om-entry-{entry_index}:class#{definition_offset}"),
        encoding,
        source_entry: source_entry.to_owned(),
    });
}

fn finalize_relations(
    mut relations: Vec<RmCreationDisplayDataRelation>,
) -> Result<Vec<RmCreationDisplayDataRelation>, CodecError> {
    relations.sort_by_key(|left| left.encoding.offset());
    for (ordinal, relation) in relations.iter_mut().enumerate() {
        relation.ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("NX creation display ordinal overflows u32"))?;
        relation.id = format!("nx:rm-creation-display-data-relations:relation#{ordinal}");
    }
    Ok(relations)
}

/// Decode class-selected creation-display relations from `RMFastLoad` record
/// areas. The compact indices remain uninterpreted until their object roles are
/// established independently.
pub(in crate::native) fn rm_creation_display_data_relations(
    container: &Container,
    object_ids: &[RmFastLoadObjectId],
) -> Result<Vec<RmCreationDisplayDataRelation>, CodecError> {
    let mut relations = Vec::new();
    for (entry, section) in container
        .om_sections()?
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
        let source_base = entry_offset + cadmpeg_core::decode::u64_from_index(record_area_offset);
        for row in crate::om::column_row::scan::index_rows(record_area) {
            if row.indices()[3].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            push_relation(
                &mut relations,
                RmCreationDisplayDataEncoding::Index(row),
                entry_index,
                definition.offset,
                &entry.name,
            );
        }
        for row in crate::om::column_row::scan::linked_rows(record_area) {
            if row.indices()[2].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let target_object_id =
                rmfastload_target_object_id(object_ids, row.target_index().atom.value());
            push_relation(
                &mut relations,
                RmCreationDisplayDataEncoding::Linked {
                    row,
                    target_object_id,
                },
                entry_index,
                definition.offset,
                &entry.name,
            );
        }
        for row in crate::om::column_row::scan::target_rows(record_area) {
            if row.indices()[2].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let target_object_id =
                rmfastload_target_object_id(object_ids, row.target_index().atom.value());
            push_relation(
                &mut relations,
                RmCreationDisplayDataEncoding::Target {
                    row,
                    target_object_id,
                },
                entry_index,
                definition.offset,
                &entry.name,
            );
        }
    }
    finalize_relations(relations)
}

#[cfg(test)]
mod admission_tests {
    use super::{push_relation, RmCreationDisplayDataRelation};

    const ROW: &str = r#"{"id":"relation","ordinal":0,"first_index":1,"raw_first_index":"8001","first_index_source_offset":8,"class_name":"UGS::RM_creation_display_data","class_definition":"definition","encoding":{"kind":"index","flag":3,"indices":[2,3,4,5],"raw_indices":["02","03","04","05"],"index_source_offsets":[13,14,15,16]},"source_entry":"entry","source_offset":5}"#;

    #[test]
    fn creation_display_relation_preserves_class_identity() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let mut relations = Vec::new();
        push_relation(&mut relations, row.encoding, 7, 11, "entry");
        assert_eq!(relations[0].class_definition, "nx:om-entry-7:class#11");
        assert_eq!(relations[0].source_entry, "entry");
    }
}
