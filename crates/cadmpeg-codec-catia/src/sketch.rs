// SPDX-License-Identifier: Apache-2.0
//! Transfer of source-closed CATIA sketch relations.

pub(crate) mod frame;

use std::collections::{BTreeMap, HashSet};

use crate::unique_index::UniqueIndex;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{
    NativeOperandField, SketchConstraint, SketchConstraintId, SketchId, SketchNativeOperand,
};

use crate::design_feature::DesignFeatureTransfer;
use crate::ids::neutral_history_id;
use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{
    CatiaConstraintRange, CatiaDesignObject, CatiaEntityEvaluation, CatiaNative, CatiaObjectRecord,
};

fn insert_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let operation = "catia_sketch_constraint_property";
    let key = ctx.format_retained(key, operation)?;
    let value = ctx.format_retained(value, operation)?;
    ctx.insert_btree_map(properties, key, value, operation)?;
    Ok(())
}

/// Transfer complete constraint ranges whose structural owner is one
/// transferred sketch.
///
/// A range is an opaque constraint at this layer. Its exact selectors,
/// framing, and evaluation are retained as native properties. The unique
/// source record is retained as a native operand. No geometry, dimensional,
/// or driving-parameter role is inferred from the range alone.
/// The returned object-record identities are the exact range and source
/// operand records represented by the emitted neutral constraints. The source
/// operand's semantic role remains unresolved by design.
pub(crate) fn transfer_constraint_ranges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<HashSet<String>, cadmpeg_core::CodecError> {
    let indexes = ConstraintIndexes::new(ctx, native, ir)?;
    let mut transferred = HashSet::new();

    for entity in &native.entity_records {
        let Some(range) = entity.constraint_range() else {
            continue;
        };
        let Some(binding) =
            constraint_binding(ctx, entity, range, &indexes, feature_transfer, graph_scope)?
        else {
            continue;
        };

        let constraint_id = match neutral_history_id(
            ctx,
            &entity.id,
            &cadmpeg_ir::identity_component!("sketch-constraint"),
        )
        .map(SketchConstraintId::from)
        {
            Ok(id) => id,
            Err(cadmpeg_core::CodecError::Malformed(_)) => continue,
            Err(error) => return Err(error),
        };
        if ir.model.sketch_constraints.iter().any(|constraint| {
            constraint.id == constraint_id
                || constraint.native_ref.as_deref() == Some(entity.id.as_str())
        }) {
            continue;
        }
        let constraint_kind = ctx.copy_retained_text(
            &range.constraint.value,
            "catia_sketch_range_constraint_kind",
        )?;
        let definition = cadmpeg_ir::sketches::SketchConstraintDefinition::native_with_operand(
            cadmpeg_core::text::NonBlankString::new(constraint_kind).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("empty native sketch constraint kind")
            })?,
            constraint_properties(ctx, range)?,
            Vec::new(),
            binding.operand,
        );
        ctx.charge_entities(1, "admit CATIA sketch constraint")?;
        let native_ref = ctx.copy_retained_text(&entity.id, "catia_sketch_range_native_ref")?;
        ctx.push_vec(
            &mut ir.model.sketch_constraints,
            SketchConstraint {
                id: constraint_id,
                sketch: binding.sketch,
                definition,
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(native_ref),
            },
            "catia_sketch_range_constraints",
        )?;
        let range_record =
            ctx.copy_retained_text(&entity.object_record, "catia_sketch_range_record_id")?;
        ctx.insert_hash_set(
            &mut transferred,
            range_record,
            "catia_sketch_range_transferred",
        )?;
        ctx.insert_hash_set(
            &mut transferred,
            binding.source_object_record,
            "catia_sketch_range_transferred",
        )?;
    }

    Ok(transferred)
}

struct ConstraintBinding {
    sketch: SketchId,
    source_object_record: String,
    operand: SketchNativeOperand,
}

struct ConstraintIndexes<'a> {
    entity_records: UniqueIndex<&'a str, &'a CatiaEntityRecord>,
    object_records: UniqueIndex<&'a str, &'a CatiaObjectRecord>,
    design_objects: UniqueIndex<&'a str, &'a CatiaDesignObject>,
    sketch_ids: UniqueIndex<String, SketchId>,
}

impl<'a> ConstraintIndexes<'a> {
    fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        native: &'a CatiaNative,
        ir: &CadIr,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let entity_records = unique_entity_records(ctx, native)?;
        let object_records = unique_object_records(ctx, native)?;
        let design_objects = unique_design_objects(ctx, native)?;
        let sketch_ids = sketch_ids_by_native_ref(ctx, ir)?;
        Ok(Self {
            entity_records,
            object_records,
            design_objects,
            sketch_ids,
        })
    }
}

fn constraint_binding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    range_entity: &CatiaEntityRecord,
    range: &CatiaConstraintRange,
    indexes: &ConstraintIndexes<'_>,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<Option<ConstraintBinding>, cadmpeg_core::CodecError> {
    let selected = (|| {
        if !graph_scope.contains(range_entity.object_graph.as_str()) {
            return None;
        }
        let range_record = indexes
            .object_records
            .get(range_entity.object_record.as_str())
            .copied()?;
        if range_record.parent != range_entity.object_graph
            || range_record.entity_id() != Some(range_entity.entity_id)
            || range_record.entity_record() != Some(range_entity.id.as_str())
        {
            return None;
        }
        let (source_record_id, source_entity) = match (
            range.incoming_references.as_slice(),
            range.incoming_storage_references.as_slice(),
        ) {
            ([reference], []) => (&reference.object_record, reference.source_entity.as_ref()),
            ([], [reference]) => (&reference.object_record, reference.source_entity.as_ref()),
            _ => return None,
        };
        let source_entity = source_entity.filter(|entity| !entity.is_null())?;
        let source_entity_id = source_entity.entity()?;
        let source_record = indexes
            .object_records
            .get(source_record_id.as_str())
            .copied()?;
        if source_record.parent != range_entity.object_graph
            || source_record.entity_id() != Some(source_entity.entity_id())
            || source_record.entity_record() != Some(source_entity_id)
            || source_entity.class_name() != source_record.class_name()
        {
            return None;
        }
        let source_entity_record = indexes.entity_records.get(source_entity_id).copied()?;
        if source_entity_record.object_graph != range_entity.object_graph
            || source_entity_record.object_record != source_record.id
            || source_entity_record.entity_id != source_entity.entity_id()
        {
            return None;
        }
        let source_design_object = source_record.design_object.as_deref()?;
        Some((source_record, source_entity_record, source_design_object))
    })();
    let Some((source_record, source_entity_record, source_design_object)) = selected else {
        return Ok(None);
    };
    let Some(sketch) = sketch_owner_for_design_object(
        ctx,
        source_design_object,
        &indexes.design_objects,
        &indexes.sketch_ids,
        feature_transfer,
    )?
    else {
        return Ok(None);
    };
    let Some(object_index) = u32::try_from(source_record.ordinal).ok() else {
        return Ok(None);
    };
    let native_kind = match source_record.class_name().filter(|class| !class.is_empty()) {
        Some(name) => {
            let name = ctx.copy_retained_text(name, "catia_sketch_range_operand_kind")?;
            let Some(name) = cadmpeg_core::text::NonBlankString::new(name) else {
                return Ok(None);
            };
            name
        }
        None => cadmpeg_core::nonblank_literal!("record"),
    };
    let field_id = ctx.copy_retained_text(&source_record.id, "catia_sketch_range_field_name")?;
    let Some(field_name) = cadmpeg_core::text::NonBlankString::new(field_id) else {
        return Ok(None);
    };
    let source_object_record =
        ctx.copy_retained_text(&source_record.id, "catia_sketch_range_source_record")?;
    let native_ref =
        ctx.copy_retained_text(&source_entity_record.id, "catia_sketch_range_operand_ref")?;
    Ok(Some(ConstraintBinding {
        sketch,
        source_object_record,
        operand: SketchNativeOperand {
            native_kind,
            field: Some(NativeOperandField {
                name: field_name,
                role: None,
            }),
            object_index: Some(object_index),
            native_ref: Some(native_ref),
        },
    }))
}

fn sketch_owner_for_design_object<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    start: &'a str,
    design_objects: &UniqueIndex<&'a str, &'a CatiaDesignObject>,
    sketch_ids: &UniqueIndex<String, SketchId>,
    feature_transfer: &DesignFeatureTransfer,
) -> Result<Option<SketchId>, cadmpeg_core::CodecError> {
    let mut current = Some(start);
    let mut steps = 0usize;
    while let Some(current_id) = current {
        if steps >= design_objects.len() {
            return Ok(None);
        }
        steps += 1;
        ctx.charge_work(1, "catia_sketch_owner_chain")?;
        let Some(object) = design_objects.get(current_id).copied() else {
            return Ok(None);
        };
        if feature_transfer.feature_ids.contains_key(current_id) {
            return match sketch_ids.get(current_id) {
                Some(id) => Ok(Some(
                    id.try_clone_for_decode(ctx, "catia_sketch_range_owner_id")?,
                )),
                None => Ok(None),
            };
        }
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }
    Ok(None)
}

fn constraint_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    range: &CatiaConstraintRange,
) -> Result<BTreeMap<String, String>, cadmpeg_core::CodecError> {
    let mut properties = BTreeMap::new();
    insert_selector(ctx, &mut properties, "catia_range", &range.range)?;
    insert_selector(ctx, &mut properties, "catia_constraint", &range.constraint)?;
    insert_property(
        ctx,
        &mut properties,
        format_args!("catia_framing"),
        format_args!("{}", framing_name(range.framing)),
    )?;
    match range.evaluation {
        CatiaEntityEvaluation::Unset => {
            insert_property(
                ctx,
                &mut properties,
                format_args!("catia_evaluation"),
                format_args!("unset"),
            )?;
        }
        CatiaEntityEvaluation::Scalar { bits } => {
            insert_property(
                ctx,
                &mut properties,
                format_args!("catia_evaluation"),
                format_args!("scalar"),
            )?;
            insert_property(
                ctx,
                &mut properties,
                format_args!("catia_evaluation_bits"),
                format_args!("{bits:016x}"),
            )?;
        }
    }
    insert_property(
        ctx,
        &mut properties,
        format_args!("catia_evaluation_opcode_offset"),
        format_args!("{}", range.evaluation_opcode_offset),
    )?;
    Ok(properties)
}

fn insert_selector(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    selector: &crate::native::CatiaEntitySchemaValue,
) -> Result<(), cadmpeg_core::CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_entry"),
        format_args!("{}", selector.entry),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_ordinal"),
        format_args!("{}", selector.ordinal),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_offset"),
        format_args!("{}", selector.offset),
    )?;
    insert_property(
        ctx,
        properties,
        format_args!("{prefix}_value"),
        format_args!("{}", selector.value),
    )?;
    Ok(())
}

fn framing_name(framing: crate::native::CatiaConstraintRangeFraming) -> &'static str {
    match framing {
        crate::native::CatiaConstraintRangeFraming::DimensionB8 => "DimensionB8",
        crate::native::CatiaConstraintRangeFraming::DimensionC1 => "DimensionC1",
        crate::native::CatiaConstraintRangeFraming::DimensionDC => "DimensionDC",
        crate::native::CatiaConstraintRangeFraming::DimensionDF => "DimensionDF",
        crate::native::CatiaConstraintRangeFraming::ComplexC9 => "ComplexC9",
    }
}

fn unique_entity_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &'a CatiaNative,
) -> Result<UniqueIndex<&'a str, &'a CatiaEntityRecord>, cadmpeg_core::CodecError> {
    UniqueIndex::collect(
        ctx,
        native
            .entity_records
            .iter()
            .map(|entity| (entity.id.as_str(), entity)),
        "catia_sketch_entity_records",
    )
}

fn unique_object_records<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &'a CatiaNative,
) -> Result<UniqueIndex<&'a str, &'a CatiaObjectRecord>, cadmpeg_core::CodecError> {
    UniqueIndex::collect(
        ctx,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .map(|record| (record.id.as_str(), record)),
        "catia_sketch_object_records",
    )
}

fn unique_design_objects<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &'a CatiaNative,
) -> Result<UniqueIndex<&'a str, &'a CatiaDesignObject>, cadmpeg_core::CodecError> {
    UniqueIndex::collect(
        ctx,
        native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object)),
        "catia_sketch_design_objects",
    )
}

fn sketch_ids_by_native_ref(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<UniqueIndex<String, SketchId>, cadmpeg_core::CodecError> {
    let mut index = UniqueIndex::new();
    for sketch in &ir.model.sketches {
        let Some(native_ref) = sketch.native_ref.as_deref() else {
            continue;
        };
        let key = ctx.copy_retained_text(native_ref, "catia_sketch_native_key")?;
        let id = sketch
            .id
            .try_clone_for_decode(ctx, "catia_sketch_native_id")?;
        index.insert(ctx, key, id, "catia_sketch_native_index")?;
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::transfer_constraint_ranges;
    use crate::native::entity_record::CatiaEntityRecord;
    use crate::native::CatiaConstraintRange;
    use crate::native::CatiaNative;
    use crate::native::CatiaObjectRecord;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::sketches::SketchId;
    use std::collections::HashMap;
    use std::collections::HashSet;

    use cadmpeg_ir::sketches::{Sketch, SketchConstraintDefinitionInput, SketchPlacement};

    use crate::design_feature::DesignFeatureTransfer;
    use crate::native::entity_record::CatiaEntityRecordBody;
    use crate::native::{
        CatiaConstraintRangeFraming, CatiaEntityEvaluation, CatiaEntityIncomingReference,
        CatiaEntitySchemaValue, CatiaObjectGraph, CatiaObjectOwner, CatiaObjectRecordReference,
        CatiaObjectRecordReferenceSource,
    };
    use crate::object_graph::{ObjectPayload, PayloadField};
    use crate::test_support::test_object_graph::design_object;

    fn object_record(
        id: &str,
        design_object: Option<&str>,
        entity_id: u32,
        entity_record: &str,
        class_name: &str,
    ) -> CatiaObjectRecord {
        CatiaObjectRecord {
            id: id.to_string(),
            parent: "graph".to_string(),
            design_object: design_object.map(str::to_string),
            entity: Some(crate::native::CatiaObjectEntity {
                record: entity_record.to_string(),
                id: entity_id,
            }),
            ordinal: 0,
            byte_offset: 0,
            byte_len: 0,
            lead: 0,
            head: Vec::new(),
            inline_body: None,
            owner: Some(CatiaObjectOwner::Entity(entity_id)),
            class: Some(crate::native::CatiaObjectClass {
                ordinal: 0,
                name: Some(class_name.to_string()),
                entry: Some("entry".to_string()),
            }),
            storage: None,
            payload: ObjectPayload {
                size: 1,
                fields: vec![PayloadField::Terminator],
            },
            repeated_reference_schema_selection: None,
            references: Vec::new(),
        }
    }

    fn entity_record(id: &str, object_record: &str, entity_id: u32) -> CatiaEntityRecord {
        CatiaEntityRecord {
            id: id.to_string(),
            object_graph: "graph".to_string(),
            object_record: object_record.to_string(),
            ordinal: 0,
            byte_offset: 0,
            lead: 0,
            body: CatiaEntityRecordBody::empty_nested(),
            definition_schema_selections: Vec::new(),
            entity_id,
            value_schema_selections: Vec::new(),
            range_interval: None,
            object_production: None,
            value_production: None,

            reference_signature: None,
            suffix: None,
            suffix_schema_selection: None,
        }
    }

    fn fixture(
        storage: bool,
    ) -> (
        CadIr,
        CatiaNative,
        DesignFeatureTransfer,
        crate::decode::ModelingGraphScope,
    ) {
        let mut range_entity = entity_record("catia:outer:entity-record#range", "range-record", 10);
        range_entity.value_production = Some(
            crate::native::entity_record::CatiaEntityValueProduction::ConstraintRange(
                CatiaConstraintRange {
                    range: CatiaEntitySchemaValue {
                        offset: 2,
                        ordinal: 3,
                        entry: "range-entry".to_string(),
                        value: "Range".to_string(),
                    },
                    constraint: CatiaEntitySchemaValue {
                        offset: 4,
                        ordinal: 5,
                        entry: "constraint-entry".to_string(),
                        value: "CstAttr_Dimension".to_string(),
                    },
                    framing: CatiaConstraintRangeFraming::DimensionC1,
                    evaluation: CatiaEntityEvaluation::Scalar {
                        bits: 128.0_f64.to_bits(),
                    },
                    evaluation_opcode_offset: 6,
                    incoming_references: Vec::new(),
                    incoming_storage_references: Vec::new(),
                },
            ),
        );
        let mut source_record = object_record(
            "source-record",
            Some("source-object"),
            11,
            "catia:outer:entity-record#source",
            "ConstraintField",
        );
        if storage {
            range_entity
                .constraint_range_mut()
                .expect("constraint range")
                .incoming_storage_references
                .push(crate::native::CatiaEntityIncomingStorageReference {
                    object_record: "source-record".to_string(),
                    source_entity: Some(
                        crate::native::CatiaEntityReference::resolved_or_unresolved(
                            11,
                            Some("catia:outer:entity-record#source".to_string()),
                            Some("ConstraintField".to_string()),
                        ),
                    ),
                });
            source_record.storage = Some(crate::native::CatiaObjectStorage {
                reference: 10,
                record: None,
                design_object: None,
            });
        } else {
            range_entity
                .constraint_range_mut()
                .expect("constraint range")
                .incoming_references
                .push(CatiaEntityIncomingReference {
                    object_record: "source-record".to_string(),
                    source_entity: Some(
                        crate::native::CatiaEntityReference::resolved_or_unresolved(
                            11,
                            Some("catia:outer:entity-record#source".to_string()),
                            Some("ConstraintField".to_string()),
                        ),
                    ),
                    payload_offset: 9,
                    source: CatiaObjectRecordReferenceSource::Field,
                });
            source_record
                .references
                .push(CatiaObjectRecordReference::from_parts(
                    10,
                    9,
                    CatiaObjectRecordReferenceSource::Field,
                    false,
                    Some("range-record".to_string()),
                    None,
                ));
        }
        let range_record = object_record(
            "range-record",
            None,
            10,
            "catia:outer:entity-record#range",
            "RangeField",
        );
        let native = CatiaNative {
            design_objects: vec![
                design_object("sketch-object", None),
                design_object("source-object", Some("sketch-object")),
            ],
            entity_records: vec![
                range_entity,
                entity_record("catia:outer:entity-record#source", "source-record", 11),
            ],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![range_record, source_record],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty();
        ir.model.sketches.push(Sketch {
            id: SketchId::mint("synthetic:test:sketch#0".to_string()).expect("valid test fixture"),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some("sketch-object".to_string()),
        });
        let feature_transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([(
                "sketch-object".to_string(),
                cadmpeg_ir::features::FeatureId::mint("synthetic:test:feature#0".to_string())
                    .expect("identity grammar"),
            )]),
            ..DesignFeatureTransfer::default()
        };
        (
            ir,
            native,
            feature_transfer,
            crate::decode::ModelingGraphScope::Scoped("graph".to_string()),
        )
    }

    #[test]
    fn sketch_unique_record_indexes_refuse_collection_limit() {
        let (_, native, _, _) = fixture(false);
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            super::unique_entity_records(ctx, &native)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_sketch_entity_records")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            super::unique_entity_records(ctx, &native)
        })
        .expect("service profile admits sketch entity index");
        assert!(admitted.get(native.entity_records[0].id.as_str()).is_some());
    }

    #[test]
    fn sketch_native_identity_indexes_refuse_retained_limit() {
        let (ir, _, _, _) = fixture(false);
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::sketch_ids_by_native_ref(ctx, &ir)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_sketch_native_key")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            super::sketch_ids_by_native_ref(ctx, &ir)
        })
        .expect("service profile admits sketch identity index");
        assert!(admitted.get("sketch-object").is_some());
    }

    #[test]
    fn blank_native_operand_fields_remain_unresolved() {
        for name in ["", " \t"] {
            let (mut ir, mut native, transfer, graph_scope) = fixture(false);
            native.object_graphs[0].records[1].id = name.to_owned();
            native.entity_records[1].object_record = name.to_owned();
            native.entity_records[0]
                .constraint_range_mut()
                .expect("constraint range")
                .incoming_references[0]
                .object_record = name.to_owned();

            let transferred = crate::test_support::with_service_context(|ctx| {
                transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
            })
            .expect("unresolved source operand");
            assert!(transferred.is_empty());
            assert!(ir.model.sketch_constraints.is_empty());
        }
    }

    #[test]
    fn blank_native_operand_classes_remain_unresolved() {
        let (mut ir, mut native, transfer, graph_scope) = fixture(false);
        native.object_graphs[0].records[1]
            .class
            .as_mut()
            .expect("source class")
            .name = Some(" \t".to_owned());
        native.entity_records[0]
            .constraint_range_mut()
            .expect("constraint range")
            .incoming_references[0]
            .source_entity = Some(crate::native::CatiaEntityReference::resolved_or_unresolved(
            11,
            Some("catia:outer:entity-record#source".to_owned()),
            Some(" \t".to_owned()),
        ));

        let transferred = crate::test_support::with_service_context(|ctx| {
            transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("unresolved source operand");
        assert!(transferred.is_empty());
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn transfers_a_uniquely_owned_constraint_range_as_opaque_native_constraint() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);

        assert_eq!(
            crate::test_support::with_service_context(|ctx| transfer_constraint_ranges(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            ))
            .expect("valid sketch constraint transfer"),
            HashSet::from(["range-record".to_string(), "source-record".to_string()])
        );
        assert_eq!(ir.model.sketch_constraints.len(), 1);
        let constraint = &ir.model.sketch_constraints[0];
        assert_eq!(constraint.sketch.as_str(), "synthetic:test:sketch#0");
        assert_eq!(
            constraint.native_ref.as_deref(),
            Some("catia:outer:entity-record#range")
        );
        let SketchConstraintDefinitionInput::Native {
            native_kind,
            native_properties,
            entities,
            parameter,
            operands,
            ..
        } = constraint.definition.kind()
        else {
            panic!("expected opaque native constraint");
        };
        assert_eq!(native_kind, "CstAttr_Dimension");
        assert_eq!(native_properties["catia_range_value"], "Range");
        assert_eq!(
            native_properties["catia_constraint_value"],
            "CstAttr_Dimension"
        );
        assert_eq!(
            native_properties["catia_evaluation_bits"],
            "4060000000000000"
        );
        assert_eq!(native_properties["catia_framing"], "DimensionC1");
        assert!(entities.is_empty());
        assert!(parameter.is_none());
        assert_eq!(operands.len(), 1);
        assert_eq!(operands[0].native_kind, "ConstraintField");
        assert_eq!(
            operands[0].field.as_ref().map(|field| field.name.as_str()),
            Some("source-record")
        );
        assert_eq!(
            operands[0].native_ref.as_deref(),
            Some("catia:outer:entity-record#source")
        );
    }

    #[test]
    fn sketch_range_entity_limit_refuses_before_constraint_push() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);
        let error = crate::test_support::with_entity_limit(0, |ctx| {
            transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect_err("one range constraint exceeds zero entities");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA sketch constraint")
        );
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn sketch_range_properties_refuse_before_retained_projection() {
        let (_, native, _, _) = fixture(false);
        let range = native.entity_records[0]
            .constraint_range()
            .expect("constraint range");
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::constraint_properties(ctx, range)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_sketch_constraint_property")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            super::constraint_properties(ctx, range)
        })
        .expect("service profile admits range properties");
        assert_eq!(admitted["catia_constraint_value"], "CstAttr_Dimension");
    }

    #[test]
    fn sketch_dimension_scalar_remains_native_without_a_quantity() {
        let (mut ir, mut native, transfer, graph_scope) = fixture(false);
        native.entity_records[0].range_interval = Some(crate::native::CatiaRangeInterval {
            range: CatiaEntitySchemaValue {
                offset: 0,
                ordinal: 3,
                entry: "range-entry".to_string(),
                value: "Range".to_string(),
            },
            interval: crate::entity_table::RangeInterval {
                prefix: crate::entity_table::RangeIntervalPrefix::Compact { value: 7, width: 1 },
                slots: None,
            },
            nominal: Some(crate::native::CatiaRangeNominal {
                framing: crate::native::CatiaRangeNominalFraming::DCToken81DB,
                bits: 128.0_f64.to_bits(),
                evaluation_opcode_offset: 4,
            }),
            incoming_references: Vec::new(),
            incoming_storage_references: Vec::new(),
            project_target: None,
        });

        crate::test_support::with_service_context(|ctx| {
            transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("valid sketch constraint transfer");

        assert!(ir.model.parameters.is_empty());

        let SketchConstraintDefinitionInput::Native {
            parameter: constraint_parameter,
            ..
        } = ir.model.sketch_constraints[0].definition.kind()
        else {
            panic!("expected native constraint");
        };
        assert!(constraint_parameter.is_none());
    }

    #[test]
    fn transfers_a_unique_storage_owned_constraint_range() {
        let (mut ir, native, transfer, graph_scope) = fixture(true);

        assert_eq!(
            crate::test_support::with_service_context(|ctx| transfer_constraint_ranges(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            ))
            .expect("valid sketch constraint transfer"),
            HashSet::from(["range-record".to_string(), "source-record".to_string()])
        );
        assert_eq!(ir.model.sketch_constraints.len(), 1);
    }

    #[test]
    fn refuses_a_constraint_range_with_repeated_incidences() {
        let (mut ir, mut native, transfer, graph_scope) = fixture(false);
        let range = native.entity_records[0]
            .constraint_range_mut()
            .expect("constraint range");
        range
            .incoming_references
            .push(range.incoming_references[0].clone());

        assert_eq!(
            crate::test_support::with_service_context(|ctx| transfer_constraint_ranges(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            ))
            .expect("valid sketch constraint transfer"),
            HashSet::new()
        );
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn refuses_a_constraint_range_owned_by_a_non_sketch_feature() {
        let (mut ir, native, mut transfer, graph_scope) = fixture(false);
        transfer.feature_ids.insert(
            "source-object".to_string(),
            cadmpeg_ir::features::FeatureId::mint(
                "synthetic:test:id#source-object:feature".to_string(),
            )
            .expect("identity grammar"),
        );

        assert_eq!(
            crate::test_support::with_service_context(|ctx| transfer_constraint_ranges(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            ))
            .expect("valid sketch constraint transfer"),
            HashSet::new()
        );
        assert!(ir.model.sketch_constraints.is_empty());
    }
}
