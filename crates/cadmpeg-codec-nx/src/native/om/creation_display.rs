// SPDX-License-Identifier: Apache-2.0
//! Class-selected creation-display rows and their optional target resolutions.

use super::{rmfastload_target_object_id, RmFastLoadObjectId};
use crate::container::Container;
use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

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

fn decimal_len(mut value: usize) -> usize {
    let mut length = 1;
    while value >= 10 {
        value /= 10;
        length += 1;
    }
    length
}

fn retained_identity(
    ctx: &DecodeContext<'_>,
    prefix: &'static str,
    ordinal: usize,
) -> Result<String, CodecError> {
    let length = prefix
        .len()
        .checked_add(decimal_len(ordinal))
        .ok_or_else(|| ctx.refuse_codec_limit("NX creation display identity length", 0, 1))?;
    let mut id = ctx.retained_string(length, "retain NX creation display identity")?;
    write!(id, "{prefix}{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit("write NX creation display identity", 0, 1))?;
    Ok(id)
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
    ctx: &DecodeContext<'_>,
    relations: &mut Vec<RmCreationDisplayDataRelation>,
    encoding: RmCreationDisplayDataEncoding,
    entry_index: usize,
    definition_offset: usize,
    source_entry: &str,
) -> Result<(), CodecError> {
    ctx.reserve_retained_vec(relations, 1, "NX creation display relations")?;
    let class_len = "nx:om-entry-:class#"
        .len()
        .checked_add(decimal_len(entry_index))
        .and_then(|length| length.checked_add(decimal_len(definition_offset)))
        .ok_or_else(|| ctx.refuse_codec_limit("NX creation display class identity length", 0, 1))?;
    let mut class_definition =
        ctx.retained_string(class_len, "retain NX creation display class identity")?;
    write!(
        class_definition,
        "nx:om-entry-{entry_index}:class#{definition_offset}"
    )
    .map_err(|_| ctx.refuse_codec_limit("write NX creation display class identity", 0, 1))?;
    relations.push(RmCreationDisplayDataRelation {
        id: String::new(),
        ordinal: 0,
        class_definition,
        encoding,
        source_entry: ctx.copy_retained_text(source_entry, "retain NX creation display text")?,
    });
    Ok(())
}

fn finalize_relations(
    ctx: &DecodeContext<'_>,
    mut relations: Vec<RmCreationDisplayDataRelation>,
) -> Result<Vec<RmCreationDisplayDataRelation>, CodecError> {
    ctx.stable_sort_by(
        &mut relations,
        |left, right| left.encoding.offset().cmp(&right.encoding.offset()),
        |_| 0,
        "sort NX creation display relations",
    )?;
    for (ordinal, relation) in relations.iter_mut().enumerate() {
        relation.ordinal = u32::try_from(ordinal)
            .map_err(|_| ctx.refuse_codec_limit("NX creation display ordinal", 0, 1))?;
        relation.id = retained_identity(
            ctx,
            "nx:rm-creation-display-data-relations:relation#",
            ordinal,
        )?;
    }
    Ok(relations)
}

/// Decode class-selected creation-display relations from `RMFastLoad` record
/// areas. The compact indices remain uninterpreted until their object roles are
/// established independently.
pub(in crate::native) fn rm_creation_display_data_relations(
    ctx: &DecodeContext<'_>,
    container: &Container,
    object_ids: &[RmFastLoadObjectId],
) -> Result<Vec<RmCreationDisplayDataRelation>, CodecError> {
    let mut relations = Vec::new();
    for (entry, section) in container
        .om_sections(ctx)?
        .into_iter()
        .filter(|(entry, _)| entry.name == "/Root/FastLoad/RMFastLoad")
    {
        let Some(record_area) = section.record_area else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(section.types.len()),
            "find NX creation display class",
        )?;
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
        for row in crate::om::column_row::scan::index_rows(ctx, record_area)? {
            if row.indices()[3].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            push_relation(
                ctx,
                &mut relations,
                RmCreationDisplayDataEncoding::Index(row),
                entry_index,
                definition.offset,
                &entry.name,
            )?;
        }
        for row in crate::om::column_row::scan::linked_rows(ctx, record_area)? {
            if row.indices()[2].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let target_object_id =
                rmfastload_target_object_id(ctx, object_ids, row.target_index().atom.value())?;
            push_relation(
                ctx,
                &mut relations,
                RmCreationDisplayDataEncoding::Linked {
                    row,
                    target_object_id,
                },
                entry_index,
                definition.offset,
                &entry.name,
            )?;
        }
        for row in crate::om::column_row::scan::target_rows(ctx, record_area)? {
            if row.indices()[2].atom.value() != class_ordinal {
                continue;
            }
            let Some(row) = row.into_absolute(source_base) else {
                continue;
            };
            let target_object_id =
                rmfastload_target_object_id(ctx, object_ids, row.target_index().atom.value())?;
            push_relation(
                ctx,
                &mut relations,
                RmCreationDisplayDataEncoding::Target {
                    row,
                    target_object_id,
                },
                entry_index,
                definition.offset,
                &entry.name,
            )?;
        }
    }
    finalize_relations(ctx, relations)
}

#[cfg(test)]
mod admission_tests {
    use super::{finalize_relations, push_relation, RmCreationDisplayDataRelation};
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    const ROW: &str = r#"{"id":"relation","ordinal":0,"first_index":1,"raw_first_index":[128,1],"first_index_source_offset":8,"class_name":"UGS::RM_creation_display_data","class_definition":"definition","encoding":{"kind":"index","flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"index_source_offsets":[13,14,15,16]},"source_entry":"entry","source_offset":5}"#;

    #[test]
    fn creation_display_relation_refuses_collection_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_collection_items = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            push_relation(ctx, &mut Vec::new(), row.encoding, 0, 10, "entry").unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems));
    }

    #[test]
    fn creation_display_relation_refuses_retained_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_retained_bytes = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            push_relation(ctx, &mut Vec::new(), row.encoding, 0, 10, "entry").unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes));
    }

    #[test]
    fn creation_display_relation_preserves_class_identity() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        crate::test_support::with_decode_context_over(
            &[],
            |_| {},
            |ctx| {
                let mut relations = Vec::new();
                push_relation(ctx, &mut relations, row.encoding, 7, 11, "entry").unwrap();
                assert_eq!(relations[0].class_definition, "nx:om-entry-7:class#11");
                assert_eq!(relations[0].source_entry, "entry");
            },
        );
    }

    #[test]
    fn creation_display_finalization_refuses_scoped_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_materialized_bytes = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            finalize_relations(ctx, vec![row]).unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn creation_display_finalization_refuses_work_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
            policy.limits.max_work_units = 0;
        };
        let error = crate::test_support::with_decode_context_over(&[], adjust_policy, |ctx| {
            finalize_relations(ctx, vec![row]).unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits));
    }
}
