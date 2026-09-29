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
    ctx.charge_retained(
        u64_from_index(length),
        "retain NX creation display identity",
    )?;
    let mut id = String::new();
    id.try_reserve_exact(length)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX creation display identity", 0, 1))?;
    write!(id, "{prefix}{ordinal}")
        .map_err(|_| ctx.refuse_codec_limit("write NX creation display identity", 0, 1))?;
    Ok(id)
}

fn retained_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.charge_retained(
        u64_from_index(text.len()),
        "retain NX creation display text",
    )?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ctx.refuse_codec_limit("allocate NX creation display text", 0, 1))?;
    owned.push_str(text);
    Ok(owned)
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
    ctx.charge_collection_items(1, "NX creation display relations")?;
    ctx.charge_retained(
        u64_from_index(std::mem::size_of::<RmCreationDisplayDataRelation>()),
        "retain NX creation display relations",
    )?;
    relations
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX creation display relations", 0, 1))?;
    let class_len = "nx:om-entry-:class#"
        .len()
        .checked_add(decimal_len(entry_index))
        .and_then(|length| length.checked_add(decimal_len(definition_offset)))
        .ok_or_else(|| ctx.refuse_codec_limit("NX creation display class identity length", 0, 1))?;
    ctx.charge_retained(
        u64_from_index(class_len),
        "retain NX creation display class identity",
    )?;
    let mut class_definition = String::new();
    class_definition
        .try_reserve_exact(class_len)
        .map_err(|_| ctx.refuse_codec_limit("allocate NX creation display class identity", 0, 1))?;
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
        source_entry: retained_text(ctx, source_entry)?,
    });
    Ok(())
}

fn finalize_relations(
    ctx: &DecodeContext<'_>,
    mut relations: Vec<RmCreationDisplayDataRelation>,
) -> Result<Vec<RmCreationDisplayDataRelation>, CodecError> {
    let sort_bytes = relations
        .len()
        .checked_mul(std::mem::size_of::<RmCreationDisplayDataRelation>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX creation display sort bytes", 0, 1))?;
    let sort_reservation = ctx.reserve_scoped(
        u64_from_index(sort_bytes),
        "sort NX creation display relations",
    )?;
    let sort_work = relations
        .len()
        .checked_mul(relations.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX creation display sort work", 0, 1))?;
    ctx.charge_work(
        u64_from_index(sort_work),
        "sort NX creation display relations",
    )?;
    relations.sort_by_key(|relation| relation.encoding.offset());
    drop(sort_reservation);
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
        let source_base = entry_offset + record_area_offset as u64;
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
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const ROW: &str = r#"{"id":"relation","ordinal":0,"first_index":1,"raw_first_index":[128,1],"first_index_source_offset":8,"class_name":"UGS::RM_creation_display_data","class_definition":"definition","encoding":{"kind":"index","flag":3,"indices":[2,3,4,5],"raw_indices":[[2],[3],[4],[5]],"index_source_offsets":[13,14,15,16]},"source_entry":"entry","source_offset":5}"#;

    fn with_policy<T>(policy: DecodePolicy, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        run(&ctx)
    }

    #[test]
    fn creation_display_relation_refuses_collection_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let error = with_policy(policy, |ctx| {
            push_relation(ctx, &mut Vec::new(), row.encoding, 0, 10, "entry").unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems));
    }

    #[test]
    fn creation_display_relation_refuses_retained_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let error = with_policy(policy, |ctx| {
            push_relation(ctx, &mut Vec::new(), row.encoding, 0, 10, "entry").unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes));
    }

    #[test]
    fn creation_display_relation_preserves_class_identity() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        with_policy(DecodePolicy::default(), |ctx| {
            let mut relations = Vec::new();
            push_relation(ctx, &mut relations, row.encoding, 7, 11, "entry").unwrap();
            assert_eq!(relations[0].class_definition, "nx:om-entry-7:class#11");
            assert_eq!(relations[0].source_entry, "entry");
        });
    }

    #[test]
    fn creation_display_finalization_refuses_scoped_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = 0;
        let error = with_policy(policy, |ctx| {
            finalize_relations(ctx, vec![row]).unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes));
    }

    #[test]
    fn creation_display_finalization_refuses_work_limit() {
        let row: RmCreationDisplayDataRelation = serde_json::from_str(ROW).unwrap();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let error = with_policy(policy, |ctx| {
            finalize_relations(ctx, vec![row]).unwrap_err()
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits));
    }
}
