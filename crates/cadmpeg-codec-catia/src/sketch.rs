// SPDX-License-Identifier: Apache-2.0
//! Transfer of source-closed CATIA sketch relations.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::unique_index::UniqueIndex;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{
    NativeOperandField, SketchConstraint, SketchConstraintId, SketchEntity, SketchEntityId,
    SketchGeometry, SketchId, SketchNativeOperand,
};

use crate::design_feature::DesignFeatureTransfer;
use crate::ids::neutral_history_id;
use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{
    CatiaConstraintRange, CatiaDesignObject, CatiaEntityEvaluation, CatiaNative, CatiaObjectRecord,
    CatiaObjectRecordReference, CatiaObjectRecordReferenceSource,
};

/// Transfer sketch member records whose source identity is complete but whose
/// coordinate grammar is not yet typed.
///
/// A Sketch owner record selects child design-object owner records in source
/// order. A child contributes one native sketch entity only when that exact
/// owner record resolves to one child design object and that child has exactly
/// one admitted geometry field. The field remains native geometry; this lane
/// does not infer coordinates, construction state, profiles, or constraints.
/// The returned object-record identities are the exact fields represented by
/// the emitted native entities and are used to close design-record accounting.
pub(crate) fn transfer_native_sketch_entities(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<HashSet<String>, cadmpeg_core::CodecError> {
    let object_records = unique_object_records(ctx, native)?;
    let entity_records = unique_entity_records(ctx, native)?;
    let design_objects = unique_design_objects(ctx, native)?;
    let mut design_objects_by_owner_record = HashMap::<&str, Vec<&CatiaDesignObject>>::new();
    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.contains(object.parent.as_str()))
    {
        let Some(owner_record) = object.owner_record.as_deref() else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut design_objects_by_owner_record,
            &owner_record,
            "catia_sketch_owner_record_index",
        )?;
        let members = design_objects_by_owner_record
            .entry(owner_record)
            .or_default();
        ctx.push_vec(members, object, "catia_sketch_owner_record_members")?;
    }

    let mut sketches = Vec::new();
    for sketch in &ir.model.sketches {
        let Some(native_ref) = sketch.native_ref.as_deref() else {
            continue;
        };
        let id = sketch
            .id
            .try_clone_for_decode(ctx, "catia_sketch_entity_sketch_id")?;
        let native_ref = ctx.copy_retained_text(native_ref, "catia_sketch_entity_sketch_ref")?;
        ctx.push_vec(
            &mut sketches,
            (id, native_ref),
            "catia_sketch_entity_sketches",
        )?;
    }
    let mut transferred = HashSet::new();

    for (sketch_id, sketch_native_ref) in sketches {
        let Some(sketch_object) = design_objects.get(sketch_native_ref.as_str()).copied() else {
            continue;
        };
        if !graph_scope.contains(sketch_object.parent.as_str()) {
            continue;
        }
        let Some(owner_record_id) = sketch_object.owner_record.as_deref() else {
            continue;
        };
        if !feature_transfer
            .sketch_owner_records
            .contains(owner_record_id)
        {
            continue;
        }
        let Some(owner_record) = object_records.get(owner_record_id).copied() else {
            continue;
        };
        if owner_record.parent != sketch_object.parent
            || owner_record.design_object.as_deref() != sketch_object.owner_design_object.as_deref()
        {
            continue;
        }

        let mut seen_fields = HashSet::new();
        for child_object in exact_sketch_member_objects(
            ctx,
            owner_record,
            &object_records,
            &design_objects_by_owner_record,
            &design_objects,
        )? {
            let geometry_fields = admitted_sketch_geometry_fields(
                ctx,
                child_object,
                &object_records,
                &entity_records,
            )?;
            let [geometry_field] = geometry_fields.as_slice() else {
                continue;
            };
            if !ctx.insert_hash_set(
                &mut seen_fields,
                geometry_field.id.as_str(),
                "catia_sketch_entity_seen_fields",
            )? {
                continue;
            }

            let entity_id = match neutral_history_id(
                ctx,
                &geometry_field.id,
                &cadmpeg_ir::identity_component!("sketch-entity"),
            )
            .map(SketchEntityId::from)
            {
                Ok(id) => id,
                Err(cadmpeg_core::CodecError::Malformed(_)) => continue,
                Err(error) => return Err(error),
            };
            if ir.model.sketch_entities.iter().any(|entity| {
                entity.id() == &entity_id
                    || (entity.sketch == sketch_id
                        && entity.native_ref.as_deref() == Some(geometry_field.id.as_str()))
            }) {
                continue;
            }
            ctx.charge_entities(1, "admit CATIA sketch entity")?;
            let sketch_copy =
                sketch_id.try_clone_for_decode(ctx, "catia_sketch_entity_owner_id")?;
            let field_copy =
                ctx.copy_retained_text(&geometry_field.id, "catia_sketch_entity_native_ref")?;
            ctx.push_vec(
                &mut ir.model.sketch_entities,
                SketchEntity::new(
                    entity_id,
                    sketch_copy,
                    SketchGeometry::native(cadmpeg_core::nonblank_literal!("2DPoint")),
                )
                .with_native_ref(Some(field_copy)),
                "catia_sketch_entities",
            )?;
            let transferred_id =
                ctx.copy_retained_text(&geometry_field.id, "catia_sketch_entity_transferred_id")?;
            ctx.insert_hash_set(
                &mut transferred,
                transferred_id,
                "catia_sketch_entity_transferred",
            )?;
        }
    }

    Ok(transferred)
}

/// Transfer one source-closed native relation between a sketch point and a
/// `ConstraintDYS` field.
///
/// The relation is admitted only when the point field is selected by an exact
/// Sketch owner-list incidence, the point field references a complete
/// `ConstraintDYS` field, and that target field is independently selected by
/// the same Sketch owner list. This proves incidence and source identity. It
/// does not assign a neutral constraint kind, coordinates, or driving state.
pub(crate) fn transfer_native_sketch_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<HashSet<String>, cadmpeg_core::CodecError> {
    let object_records = unique_object_records(ctx, native)?;
    let entity_records = unique_entity_records(ctx, native)?;
    let design_objects = unique_design_objects(ctx, native)?;
    let mut design_objects_by_owner_record = HashMap::<&str, Vec<&CatiaDesignObject>>::new();
    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.contains(object.parent.as_str()))
    {
        let Some(owner_record) = object.owner_record.as_deref() else {
            continue;
        };
        ctx.admit_hash_map_entry(
            &mut design_objects_by_owner_record,
            &owner_record,
            "catia_sketch_constraint_owner_record_index",
        )?;
        let members = design_objects_by_owner_record
            .entry(owner_record)
            .or_default();
        ctx.push_vec(
            members,
            object,
            "catia_sketch_constraint_owner_record_members",
        )?;
    }

    let mut sketches = Vec::new();
    for sketch in &ir.model.sketches {
        let Some(native_ref) = sketch.native_ref.as_deref() else {
            continue;
        };
        let id = sketch
            .id
            .try_clone_for_decode(ctx, "catia_sketch_constraint_sketch_id")?;
        let native_ref =
            ctx.copy_retained_text(native_ref, "catia_sketch_constraint_sketch_ref")?;
        ctx.push_vec(
            &mut sketches,
            (id, native_ref),
            "catia_sketch_constraint_sketches",
        )?;
    }
    // Candidates are keyed in their transfer order: target record offset, then
    // target record id, then sketch.
    let mut candidates =
        BTreeMap::<(u64, &str, SketchId), NativeSketchConstraintCandidate<'_>>::new();

    for (sketch_id, sketch_native_ref) in sketches {
        let Some(sketch_object) = design_objects.get(sketch_native_ref.as_str()).copied() else {
            continue;
        };
        if !graph_scope.contains(sketch_object.parent.as_str()) {
            continue;
        }
        let Some(owner_record_id) = sketch_object.owner_record.as_deref() else {
            continue;
        };
        if !feature_transfer
            .sketch_owner_records
            .contains(owner_record_id)
        {
            continue;
        }
        let Some(owner_record) = object_records.get(owner_record_id).copied() else {
            continue;
        };
        if owner_record.parent != sketch_object.parent
            || owner_record.design_object.as_deref() != sketch_object.owner_design_object.as_deref()
        {
            continue;
        }

        let member_objects = exact_sketch_member_objects(
            ctx,
            owner_record,
            &object_records,
            &design_objects_by_owner_record,
            &design_objects,
        )?;
        let member_object_ids = ctx.collect_hash_set(
            member_objects.iter().map(|object| object.id.as_str()),
            "catia_sketch_constraint_member_ids",
        )?;
        let mut sketch_entities = UniqueIndex::new();
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch_id)
        {
            let Some(native_ref) = entity.native_ref.as_deref() else {
                continue;
            };
            let key = ctx.copy_retained_text(native_ref, "catia_sketch_constraint_entity_key")?;
            let id = entity
                .id()
                .try_clone_for_decode(ctx, "catia_sketch_constraint_entity_id")?;
            sketch_entities.insert(ctx, key, id, "catia_sketch_constraint_entity_index")?;
        }

        for child_object in member_objects {
            for geometry_field in admitted_sketch_geometry_fields(
                ctx,
                child_object,
                &object_records,
                &entity_records,
            )? {
                let Some(sketch_entity) = sketch_entities.get(geometry_field.id.as_str()) else {
                    continue;
                };
                for reference in &geometry_field.references {
                    let Some(target_id) = reference.target() else {
                        continue;
                    };
                    if reference.is_null() {
                        continue;
                    }
                    let Some(target_record) = object_records.get(target_id).copied() else {
                        continue;
                    };
                    let (Some("ConstraintDYS"), Some(target_entry)) =
                        (target_record.class_name(), target_record.class_entry())
                    else {
                        continue;
                    };
                    if target_record.parent != owner_record.parent
                        || target_record.entity_id() != Some(reference.entity_id())
                        || reference.design_object() != target_record.design_object.as_deref()
                    {
                        continue;
                    }
                    let Some(target_design_object) = target_record.design_object.as_deref() else {
                        continue;
                    };
                    if !member_object_ids.contains(target_design_object) {
                        continue;
                    }
                    let Some(target_object) = design_objects.get(target_design_object).copied()
                    else {
                        continue;
                    };
                    if target_object.parent != owner_record.parent
                        || target_record.owner_entity_id() != Some(target_object.owner_entity_id)
                    {
                        continue;
                    }
                    let Some(target_entity_record_id) = target_record.entity_record() else {
                        continue;
                    };
                    let Some(target_entity_record) =
                        entity_records.get(target_entity_record_id).copied()
                    else {
                        continue;
                    };
                    if target_entity_record.object_graph != target_record.parent
                        || target_entity_record.object_record != target_record.id
                        || Some(target_entity_record.entity_id) != target_record.entity_id()
                    {
                        continue;
                    }

                    let key_id =
                        sketch_id.try_clone_for_decode(ctx, "catia_sketch_candidate_key_id")?;
                    let key = (target_record.byte_offset, target_record.id.as_str(), key_id);
                    let candidate = match ctx.entry_btree_map(
                        &mut candidates,
                        key,
                        "catia_sketch_constraint_candidates",
                    )? {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            let owner = sketch_id
                                .try_clone_for_decode(ctx, "catia_sketch_candidate_owner_id")?;
                            entry.insert(NativeSketchConstraintCandidate {
                                sketch: owner,
                                target_record,
                                target_entity_record,
                                target_entry,
                                entities: Vec::new(),
                                incidences: Vec::new(),
                            })
                        }
                    };
                    if !ctx.contains(
                        &candidate.entities,
                        sketch_entity,
                        "catia_sketch_candidate_entity_checks",
                    )? {
                        let id = sketch_entity
                            .try_clone_for_decode(ctx, "catia_sketch_candidate_entity_id")?;
                        ctx.push_vec(
                            &mut candidate.entities,
                            id,
                            "catia_sketch_candidate_entities",
                        )?;
                    }
                    let field =
                        ctx.copy_retained_text(&geometry_field.id, "catia_sketch_candidate_field")?;
                    ctx.push_vec(
                        &mut candidate.incidences,
                        NativeSketchConstraintIncidence {
                            field,
                            field_offset: geometry_field.byte_offset,
                            reference_offset: reference.payload_offset(),
                        },
                        "catia_sketch_candidate_incidences",
                    )?;
                }
            }
        }
    }

    let mut transferred = HashSet::new();
    for (_, candidate) in ctx.admit_iter(candidates, "catia_sketch_constraint_candidate_order")? {
        let constraint_id = match neutral_history_id(
            ctx,
            &candidate.target_entity_record.id,
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
                || constraint.native_ref.as_deref()
                    == Some(candidate.target_entity_record.id.as_str())
        }) {
            continue;
        }
        let Some(object_index) = u32::try_from(candidate.target_record.ordinal).ok() else {
            continue;
        };
        let mut native_properties = BTreeMap::new();
        insert_property(
            ctx,
            &mut native_properties,
            format_args!("catia_relation_source_class"),
            format_args!("2DPoint"),
        )?;
        insert_property(
            ctx,
            &mut native_properties,
            format_args!("catia_relation_target_class"),
            format_args!("ConstraintDYS"),
        )?;
        insert_property(
            ctx,
            &mut native_properties,
            format_args!("catia_relation_target_entry"),
            format_args!("{}", candidate.target_entry),
        )?;
        insert_property(
            ctx,
            &mut native_properties,
            format_args!("catia_relation_target_ordinal"),
            format_args!("{}", candidate.target_record.ordinal),
        )?;
        insert_property(
            ctx,
            &mut native_properties,
            format_args!("catia_relation_target_offset"),
            format_args!("{}", candidate.target_record.byte_offset),
        )?;
        insert_target_reference_properties(
            ctx,
            &mut native_properties,
            &candidate.target_record.references,
            &object_records,
        )?;
        insert_property(
            ctx,
            &mut native_properties,
            format_args!("catia_relation_incidence_count"),
            format_args!("{}", candidate.incidences.len()),
        )?;
        for (ordinal, incidence) in candidate.incidences.iter().enumerate() {
            insert_property(
                ctx,
                &mut native_properties,
                format_args!("catia_relation_incidence_{ordinal}_source_field"),
                format_args!("{}", incidence.field),
            )?;
            insert_property(
                ctx,
                &mut native_properties,
                format_args!("catia_relation_incidence_{ordinal}_source_field_offset"),
                format_args!("{}", incidence.field_offset),
            )?;
            insert_property(
                ctx,
                &mut native_properties,
                format_args!("catia_relation_incidence_{ordinal}_source_reference_offset"),
                format_args!("{}", incidence.reference_offset),
            )?;
        }
        let field_id = ctx.copy_retained_text(
            &candidate.target_record.id,
            "catia_sketch_constraint_field_name",
        )?;
        let Some(field_name) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            field_id,
            "validate nonblank text",
        )?
        else {
            continue;
        };
        let operand_ref = ctx.copy_retained_text(
            &candidate.target_entity_record.id,
            "catia_sketch_constraint_operand_ref",
        )?;
        let definition = cadmpeg_ir::sketches::SketchConstraintDefinition::native_with_operand(
            cadmpeg_core::nonblank_literal!("ConstraintDYS"),
            native_properties,
            candidate.entities,
            SketchNativeOperand {
                native_kind: cadmpeg_core::nonblank_literal!("ConstraintDYS"),
                field: Some(NativeOperandField {
                    name: field_name,
                    role: None,
                }),
                object_index: Some(object_index),
                native_ref: Some(operand_ref),
            },
        );
        ctx.charge_entities(1, "admit CATIA sketch constraint")?;
        let neutral_ref = ctx.copy_retained_text(
            &candidate.target_entity_record.id,
            "catia_sketch_constraint_native_ref",
        )?;
        ctx.push_vec(
            &mut ir.model.sketch_constraints,
            SketchConstraint {
                id: constraint_id,
                sketch: candidate.sketch,
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
                native_ref: Some(neutral_ref),
            },
            "catia_sketch_constraints",
        )?;
        let transferred_id = ctx.copy_retained_text(
            &candidate.target_record.id,
            "catia_sketch_constraint_transferred_id",
        )?;
        ctx.insert_hash_set(
            &mut transferred,
            transferred_id,
            "catia_sketch_constraint_transferred",
        )?;
    }
    Ok(transferred)
}

struct NativeSketchConstraintCandidate<'a> {
    sketch: SketchId,
    target_record: &'a CatiaObjectRecord,
    target_entity_record: &'a CatiaEntityRecord,
    target_entry: &'a str,
    entities: Vec<SketchEntityId>,
    incidences: Vec<NativeSketchConstraintIncidence>,
}

struct NativeSketchConstraintIncidence {
    field: String,
    field_offset: u64,
    reference_offset: u64,
}

fn insert_target_reference_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    references: &[CatiaObjectRecordReference],
    object_records: &UniqueIndex<&str, &CatiaObjectRecord>,
) -> Result<(), cadmpeg_core::CodecError> {
    insert_property(
        ctx,
        properties,
        format_args!("catia_relation_target_reference_count"),
        format_args!("{}", references.len()),
    )?;
    for (ordinal, reference) in references.iter().enumerate() {
        insert_property(
            ctx,
            properties,
            format_args!("catia_relation_target_reference_{ordinal}_entity_id"),
            format_args!("{}", reference.entity_id()),
        )?;
        insert_property(
            ctx,
            properties,
            format_args!("catia_relation_target_reference_{ordinal}_payload_offset"),
            format_args!("{}", reference.payload_offset()),
        )?;
        let state = if reference.is_null() {
            "null"
        } else if reference.target().is_some() {
            "resolved"
        } else {
            "unresolved"
        };
        insert_property(
            ctx,
            properties,
            format_args!("catia_relation_target_reference_{ordinal}_state"),
            format_args!("{state}"),
        )?;
        match reference.source() {
            CatiaObjectRecordReferenceSource::Field => {
                insert_property(
                    ctx,
                    properties,
                    format_args!("catia_relation_target_reference_{ordinal}_source"),
                    format_args!("field"),
                )?;
            }
            CatiaObjectRecordReferenceSource::ListItem {
                list_payload_offset,
                item_ordinal,
            } => {
                insert_property(
                    ctx,
                    properties,
                    format_args!("catia_relation_target_reference_{ordinal}_source"),
                    format_args!("list_item"),
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("catia_relation_target_reference_{ordinal}_list_payload_offset"),
                    format_args!("{list_payload_offset}"),
                )?;
                insert_property(
                    ctx,
                    properties,
                    format_args!("catia_relation_target_reference_{ordinal}_item_ordinal"),
                    format_args!("{item_ordinal}"),
                )?;
            }
        }
        if let Some(target) = reference.target() {
            insert_property(
                ctx,
                properties,
                format_args!("catia_relation_target_reference_{ordinal}_target_record"),
                format_args!("{target}"),
            )?;
            if let Some(target_record) = object_records.get(target) {
                if let Some(class_name) = target_record.class_name() {
                    insert_property(
                        ctx,
                        properties,
                        format_args!("catia_relation_target_reference_{ordinal}_target_class"),
                        format_args!("{class_name}"),
                    )?;
                }
                if let Some(class_entry) = target_record.class_entry() {
                    insert_property(
                        ctx,
                        properties,
                        format_args!("catia_relation_target_reference_{ordinal}_target_entry"),
                        format_args!("{class_entry}"),
                    )?;
                }
            }
        }
        if let Some(design_object) = reference.design_object() {
            insert_property(
                ctx,
                properties,
                format_args!("catia_relation_target_reference_{ordinal}_target_design_object"),
                format_args!("{design_object}"),
            )?;
        }
    }
    Ok(())
}

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

fn exact_sketch_member_objects<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    owner_record: &'a CatiaObjectRecord,
    object_records: &UniqueIndex<&'a str, &'a CatiaObjectRecord>,
    design_objects_by_owner_record: &HashMap<&'a str, Vec<&'a CatiaDesignObject>>,
    design_objects: &UniqueIndex<&'a str, &'a CatiaDesignObject>,
) -> Result<Vec<&'a CatiaDesignObject>, cadmpeg_core::CodecError> {
    ctx.collect_vec(
        owner_record.references.iter().filter_map(|reference| {
            let target_id = reference.target()?;
            if reference.is_null() {
                return None;
            }
            let target_record = object_records.get(target_id).copied()?;
            if target_record.parent != owner_record.parent
                || target_record.entity_id() != Some(reference.entity_id())
                || reference.design_object() != target_record.design_object.as_deref()
            {
                return None;
            }
            let child_objects = design_objects_by_owner_record.get(target_id)?;
            let [child_object] = child_objects.as_slice() else {
                return None;
            };
            if design_objects.get(child_object.id.as_str()).is_none()
                || child_object.parent != owner_record.parent
                || child_object.owner_record.as_deref() != Some(target_id)
                || child_object.owner_entity_id != reference.entity_id()
            {
                return None;
            }
            Some(*child_object)
        }),
        "catia_sketch_member_objects",
    )
}

fn admitted_sketch_geometry_fields<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    child_object: &'a CatiaDesignObject,
    object_records: &UniqueIndex<&'a str, &'a CatiaObjectRecord>,
    entity_records: &UniqueIndex<&'a str, &'a CatiaEntityRecord>,
) -> Result<Vec<&'a CatiaObjectRecord>, cadmpeg_core::CodecError> {
    ctx.collect_vec(
        child_object
            .fields
            .iter()
            .filter_map(|field_id| object_records.get(field_id.as_str()).copied())
            .filter(|field| {
                let Some(entity_record_id) = field.entity_record() else {
                    return false;
                };
                let Some(entity_record) = entity_records.get(entity_record_id) else {
                    return false;
                };
                field.parent == child_object.parent
                    && field.design_object.as_deref() == Some(child_object.id.as_str())
                    && field.owner_entity_id() == Some(child_object.owner_entity_id)
                    && field.entity_id().is_some()
                    && entity_record.object_graph == field.parent
                    && entity_record.object_record == field.id
                    && Some(entity_record.entity_id) == field.entity_id()
                    && field.class_entry().is_some()
                    && field.class_name() == Some("2DPoint")
            }),
        "catia_sketch_geometry_fields",
    )
}

/// Transfer complete constraint ranges whose structural owner is one
/// transferred sketch.
///
/// A range is an opaque constraint at this layer. Its exact selectors,
/// framing, and evaluation are retained as native properties. The unique
/// source record is retained as a native operand. If that exact source record
/// is already represented by one entity in the same sketch, the entity is
/// bound by identity; no geometry, dimensional, or driving-parameter role is
/// inferred from the range alone.
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
        let mut bound_entities = Vec::new();
        if let Some(entity_id) = binding.entity {
            ctx.push_vec(
                &mut bound_entities,
                entity_id,
                "catia_sketch_range_bound_entities",
            )?;
        }
        let definition = cadmpeg_ir::sketches::SketchConstraintDefinition::native_with_operand(
            cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                constraint_kind,
                "validate nonblank text",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("empty native sketch constraint kind")
            })?,
            constraint_properties(ctx, range)?,
            bound_entities,
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
    entity: Option<SketchEntityId>,
}

struct ConstraintIndexes<'a> {
    entity_records: UniqueIndex<&'a str, &'a CatiaEntityRecord>,
    object_records: UniqueIndex<&'a str, &'a CatiaObjectRecord>,
    design_objects: UniqueIndex<&'a str, &'a CatiaDesignObject>,
    sketch_ids: UniqueIndex<String, SketchId>,
    sketch_entities: UniqueIndex<String, (SketchEntityId, SketchId)>,
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
        let sketch_entities = sketch_entities_by_native_ref(ctx, ir)?;
        Ok(Self {
            entity_records,
            object_records,
            design_objects,
            sketch_ids,
            sketch_entities,
        })
    }
}

fn sketch_entities_by_native_ref(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<UniqueIndex<String, (SketchEntityId, SketchId)>, cadmpeg_core::CodecError> {
    let mut index = UniqueIndex::new();
    for entity in &ir.model.sketch_entities {
        let Some(native_ref) = entity.native_ref.as_deref() else {
            continue;
        };
        let key = ctx.copy_retained_text(native_ref, "catia_sketch_entity_native_key")?;
        let id = entity
            .id()
            .try_clone_for_decode(ctx, "catia_sketch_entity_index_id")?;
        let sketch = entity
            .sketch
            .try_clone_for_decode(ctx, "catia_sketch_entity_index_sketch")?;
        index.insert(ctx, key, (id, sketch), "catia_sketch_entity_native_index")?;
    }
    Ok(index)
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
        Some((
            source_record_id,
            source_record,
            source_entity_record,
            source_design_object,
        ))
    })();
    let Some((source_record_id, source_record, source_entity_record, source_design_object)) =
        selected
    else {
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
    let entity = match indexes
        .sketch_entities
        .get(source_record_id)
        .filter(|(_, entity_sketch)| entity_sketch == &sketch)
    {
        Some((entity, _)) => {
            Some(entity.try_clone_for_decode(ctx, "catia_sketch_range_entity_id")?)
        }
        None => None,
    };
    let Some(object_index) = u32::try_from(source_record.ordinal).ok() else {
        return Ok(None);
    };
    let native_kind = match source_record.class_name().filter(|class| !class.is_empty()) {
        Some(name) => {
            let name = ctx.copy_retained_text(name, "catia_sketch_range_operand_kind")?;
            let Some(name) = cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                name,
                "validate nonblank text",
            )?
            else {
                return Ok(None);
            };
            name
        }
        None => cadmpeg_core::nonblank_literal!("record"),
    };
    let field_id = ctx.copy_retained_text(&source_record.id, "catia_sketch_range_field_name")?;
    let Some(field_name) =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, field_id, "validate nonblank text")?
    else {
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
        entity,
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
        ctx.charge_work(1, "catia_sketch_owner_chain")?;
        if steps >= design_objects.len() {
            return Ok(None);
        }
        steps += 1;
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
    use super::{
        transfer_constraint_ranges, transfer_native_sketch_constraints,
        transfer_native_sketch_entities,
    };
    use crate::native::entity_record::CatiaEntityRecord;
    use crate::native::CatiaConstraintRange;
    use crate::native::CatiaNative;
    use crate::native::CatiaObjectRecord;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::sketches::SketchEntity;
    use cadmpeg_ir::sketches::SketchEntityId;
    use cadmpeg_ir::sketches::SketchGeometry;
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
        let (mut ir, _, _, _) = fixture(false);
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

        ir.model.sketch_entities.push(
            SketchEntity::new(
                SketchEntityId::mint("synthetic:test:sketch-entity#0".to_string())
                    .expect("identity grammar"),
                ir.model.sketches[0].id.clone(),
                SketchGeometry::native(cadmpeg_core::nonblank_literal!("2DPoint")),
            )
            .with_native_ref(Some("field-record".to_string())),
        );
        let refused = crate::test_support::with_retained_limit(0, |ctx| {
            super::sketch_entities_by_native_ref(ctx, &ir)
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_sketch_entity_native_key")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            super::sketch_entities_by_native_ref(ctx, &ir)
        })
        .expect("service profile admits native entity index");
        assert!(admitted.get("field-record").is_some());
    }

    fn native_sketch_fixture(
        geometry_class: &str,
    ) -> (
        CadIr,
        CatiaNative,
        DesignFeatureTransfer,
        crate::decode::ModelingGraphScope,
    ) {
        let mut sketch_owner = object_record(
            "sketch-owner-record",
            Some("parent-object"),
            1,
            "sketch-owner-entity",
            "Sketch",
        );
        sketch_owner.owner = Some(CatiaObjectOwner::Entity(2));
        sketch_owner
            .references
            .push(CatiaObjectRecordReference::from_parts(
                3,
                0,
                CatiaObjectRecordReferenceSource::Field,
                false,
                Some("child-owner-record".to_string()),
                Some("parent-object".to_string()),
            ));

        let mut child_owner = object_record(
            "child-owner-record",
            Some("parent-object"),
            3,
            "child-owner-entity",
            "Prism_EndLimit_Length",
        );
        child_owner.owner = Some(CatiaObjectOwner::Entity(2));

        let geometry_field_id = "catia:outer:object-record#geometry-field";
        let geometry_entity_id = "catia:outer:entity-record#geometry-field";
        let mut geometry_field = object_record(
            geometry_field_id,
            Some("child-object"),
            4,
            geometry_entity_id,
            geometry_class,
        );
        geometry_field.owner = Some(CatiaObjectOwner::Entity(3));

        let native = CatiaNative {
            design_objects: vec![
                {
                    let mut object = design_object("parent-object", None);
                    object.owner_entity_id = 2;
                    object
                },
                {
                    let mut object = design_object("sketch-object", Some("parent-object"));
                    object.owner_entity_id = 1;
                    object.owner_record = Some("sketch-owner-record".to_string());
                    object.owner_class = Some(crate::native::CatiaDesignClass {
                        entry: "entry".to_string(),
                        name: "Sketch".to_string(),
                    });
                    object
                },
                {
                    let mut object = design_object("child-object", Some("parent-object"));
                    object.owner_entity_id = 3;
                    object.owner_record = Some("child-owner-record".to_string());
                    object.fields.push(geometry_field_id.to_string());
                    object
                },
            ],
            entity_records: vec![entity_record(geometry_entity_id, geometry_field_id, 4)],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![sketch_owner, child_owner, geometry_field],
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
            sketch_owner_records: HashSet::from(["sketch-owner-record".to_string()]),
            ..DesignFeatureTransfer::default()
        };
        (
            ir,
            native,
            feature_transfer,
            crate::decode::ModelingGraphScope::Scoped("graph".to_string()),
        )
    }

    fn native_sketch_constraint_fixture() -> (
        CadIr,
        CatiaNative,
        DesignFeatureTransfer,
        crate::decode::ModelingGraphScope,
    ) {
        let (ir, mut native, transfer, graph_scope) = native_sketch_fixture("2DPoint");
        let constraint_owner_record = object_record(
            "constraint-owner-record",
            Some("parent-object"),
            5,
            "constraint-owner-entity",
            "Prism_EndLimit_Length",
        );
        let constraint_field_id = "catia:outer:object-record#constraint-field";
        let constraint_entity_id = "catia:outer:entity-record#constraint-field";
        let mut constraint_field = object_record(
            constraint_field_id,
            Some("constraint-object"),
            6,
            constraint_entity_id,
            "ConstraintDYS",
        );
        constraint_field.owner = Some(CatiaObjectOwner::Entity(5));
        constraint_field
            .references
            .push(CatiaObjectRecordReference::from_parts(
                7,
                4,
                CatiaObjectRecordReferenceSource::Field,
                false,
                Some("constraint-target-record".to_string()),
                Some("parent-object".to_string()),
            ));
        constraint_field.references.extend([
            CatiaObjectRecordReference::from_parts(
                8,
                8,
                CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 6,
                    item_ordinal: 2,
                },
                true,
                None,
                None,
            ),
            CatiaObjectRecordReference::from_parts(
                9,
                12,
                CatiaObjectRecordReferenceSource::Field,
                false,
                None,
                None,
            ),
        ]);
        let constraint_target_record = object_record(
            "constraint-target-record",
            Some("parent-object"),
            7,
            "catia:outer:entity-record#constraint-target",
            "Sketch",
        );

        let mut constraint_object = design_object("constraint-object", Some("parent-object"));
        constraint_object.owner_entity_id = 5;
        constraint_object.owner_record = Some("constraint-owner-record".to_string());
        constraint_object
            .fields
            .push(constraint_field_id.to_string());
        native.design_objects.push(constraint_object);
        native.entity_records.extend([
            entity_record(constraint_entity_id, constraint_field_id, 6),
            entity_record(
                "catia:outer:entity-record#constraint-target",
                "constraint-target-record",
                7,
            ),
        ]);
        native.object_graphs[0].records.extend([
            constraint_owner_record,
            constraint_field,
            constraint_target_record,
        ]);

        let sketch_owner = native.object_graphs[0]
            .records
            .iter_mut()
            .find(|record| record.id == "sketch-owner-record")
            .expect("sketch owner record");
        sketch_owner
            .references
            .push(CatiaObjectRecordReference::from_parts(
                5,
                1,
                CatiaObjectRecordReferenceSource::Field,
                false,
                Some("constraint-owner-record".to_string()),
                Some("parent-object".to_string()),
            ));
        let geometry_field = native.object_graphs[0]
            .records
            .iter_mut()
            .find(|record| record.id == "catia:outer:object-record#geometry-field")
            .expect("geometry field");
        geometry_field
            .references
            .push(CatiaObjectRecordReference::from_parts(
                6,
                2,
                CatiaObjectRecordReferenceSource::Field,
                false,
                Some(constraint_field_id.to_string()),
                Some("constraint-object".to_string()),
            ));

        (ir, native, transfer, graph_scope)
    }

    #[test]
    fn transfers_one_exact_native_sketch_geometry_member() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("2DPoint");

        let transferred = crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        });
        assert_eq!(transferred.len(), 1);
        assert!(transferred.contains("catia:outer:object-record#geometry-field"));
        assert_eq!(ir.model.sketch_entities.len(), 1);
        let entity = &ir.model.sketch_entities[0];
        assert_eq!(entity.sketch.as_str(), "synthetic:test:sketch#0");
        assert_eq!(
            entity.native_ref.as_deref(),
            Some("catia:outer:object-record#geometry-field")
        );
        assert_eq!(
            entity.id().as_str(),
            "catia:outer:sketch-entity#geometry-field"
        );
        assert!(entity.geometry_ref.is_none());
        assert!(matches!(entity.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Native { native_kind } if native_kind.as_str() == "2DPoint"
        ));
    }

    #[test]
    fn sketch_entity_limit_refuses_before_native_member_push() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("2DPoint");
        let error = crate::test_support::with_entity_limit(0, |ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect_err("one sketch member exceeds zero entities");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA sketch entity")
        );
        assert!(ir.model.sketch_entities.is_empty());
    }

    #[test]
    fn does_not_promote_an_unadmitted_native_sketch_member() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("Point");

        assert!(
            crate::test_support::with_service_context(|ctx| transfer_native_sketch_entities(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            )
            .expect("service profile admits sketch transfer"))
            .is_empty()
        );
        assert!(ir.model.sketch_entities.is_empty());
    }

    #[test]
    fn sketch_transfer_refuses_before_unadmitted_member_allocation() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("Point");
        let refused = crate::test_support::with_retained_refusal(
            &[],
            "catia_sketch_entity_sketch_id",
            |ctx| {
                let mut trial_ir = ir.clone();
                transfer_native_sketch_entities(
                    ctx,
                    &mut trial_ir,
                    &native,
                    &transfer,
                    &graph_scope,
                )
            },
        );
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_sketch_entity_sketch_id")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("service profile admits sketch scan");
        assert!(admitted.is_empty());
    }

    #[test]
    fn sketch_constraint_scan_refuses_before_unadmitted_candidate_allocation() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("Point");
        let refused = crate::test_support::with_retained_refusal(
            &[],
            "catia_sketch_constraint_sketch_id",
            |ctx| {
                let mut trial_ir = ir.clone();
                transfer_native_sketch_constraints(
                    ctx,
                    &mut trial_ir,
                    &native,
                    &transfer,
                    &graph_scope,
                )
            },
        );
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_sketch_constraint_sketch_id")
        );
        let admitted = crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_constraints(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("service profile admits constraint scan");
        assert!(admitted.is_empty());
    }

    #[test]
    fn refuses_an_ambiguous_native_sketch_geometry_group() {
        let (mut ir, mut native, transfer, graph_scope) = native_sketch_fixture("2DPoint");
        let second_field_id = "catia:outer:object-record#second-geometry-field";
        let second_entity_id = "catia:outer:entity-record#second-geometry-field";
        let mut second = object_record(
            second_field_id,
            Some("child-object"),
            5,
            second_entity_id,
            "2DPoint",
        );
        second.owner = Some(CatiaObjectOwner::Entity(3));
        native.object_graphs[0].records.push(second);
        native
            .entity_records
            .push(entity_record(second_entity_id, second_field_id, 5));
        native
            .design_objects
            .iter_mut()
            .find(|object| object.id == "child-object")
            .expect("child design object")
            .fields
            .push(second_field_id.to_string());

        assert!(
            crate::test_support::with_service_context(|ctx| transfer_native_sketch_entities(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            )
            .expect("service profile admits sketch transfer"))
            .is_empty()
        );
        assert!(ir.model.sketch_entities.is_empty());
    }

    #[test]
    fn refuses_constraint_relations_with_duplicate_sketch_entity_references() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_constraint_fixture();
        crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        });
        assert!(!ir.model.sketch_entities.is_empty());
        let duplicates = ir.model.sketch_entities.clone();
        ir.model.sketch_entities.extend(duplicates.clone());
        ir.model.sketch_entities.extend(duplicates);
        assert!(crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_constraints(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        })
        .is_empty());
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn transfers_a_source_closed_native_sketch_constraint_relation() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_constraint_fixture();

        crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        });
        assert_eq!(
            crate::test_support::with_service_context(|ctx| transfer_native_sketch_constraints(
                ctx,
                &mut ir,
                &native,
                &transfer,
                &graph_scope
            )
            .expect("service profile admits sketch transfer")),
            HashSet::from(["catia:outer:object-record#constraint-field".to_string()])
        );
        assert_eq!(ir.model.sketch_constraints.len(), 1);
        let constraint = &ir.model.sketch_constraints[0];
        assert_eq!(constraint.sketch.as_str(), "synthetic:test:sketch#0");
        assert_eq!(
            constraint.native_ref.as_deref(),
            Some("catia:outer:entity-record#constraint-field")
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
            panic!("expected opaque native sketch constraint");
        };
        assert_eq!(native_kind.as_str(), "ConstraintDYS");
        assert_eq!(native_properties["catia_relation_source_class"], "2DPoint");
        assert_eq!(
            native_properties["catia_relation_target_class"],
            "ConstraintDYS"
        );
        assert_eq!(native_properties["catia_relation_target_entry"], "entry");
        assert_eq!(
            native_properties["catia_relation_target_reference_count"],
            "3"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_0_entity_id"],
            "7"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_0_payload_offset"],
            "4"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_0_state"],
            "resolved"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_0_source"],
            "field"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_0_target_record"],
            "constraint-target-record"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_0_target_class"],
            "Sketch"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_1_entity_id"],
            "8"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_1_state"],
            "null"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_1_source"],
            "list_item"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_1_list_payload_offset"],
            "6"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_1_item_ordinal"],
            "2"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_2_entity_id"],
            "9"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_2_state"],
            "unresolved"
        );
        assert_eq!(
            native_properties["catia_relation_target_reference_2_source"],
            "field"
        );
        assert_eq!(native_properties["catia_relation_incidence_count"], "1");
        assert_eq!(entities.len(), 1);
        assert_eq!(
            entities[0].as_str(),
            "catia:outer:sketch-entity#geometry-field"
        );
        assert!(parameter.is_none());
        assert_eq!(operands.len(), 1);
        assert_eq!(operands[0].native_kind.as_str(), "ConstraintDYS");
        assert_eq!(
            operands[0].field.as_ref().map(|field| field.name.as_str()),
            Some("catia:outer:object-record#constraint-field")
        );
        assert_eq!(
            operands[0].native_ref.as_deref(),
            Some("catia:outer:entity-record#constraint-field")
        );
    }

    #[test]
    fn sketch_constraint_entity_limit_refuses_before_native_relation_push() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_constraint_fixture();
        crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch member")
        });
        let error = crate::test_support::with_entity_limit(0, |ctx| {
            transfer_native_sketch_constraints(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect_err("one sketch relation exceeds zero entities");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && limit.operation == "admit CATIA sketch constraint")
        );
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn refuses_a_native_sketch_constraint_without_source_incidence() {
        let (mut ir, mut native, transfer, graph_scope) = native_sketch_constraint_fixture();
        native.object_graphs[0]
            .records
            .iter_mut()
            .find(|record| record.id == "catia:outer:object-record#geometry-field")
            .expect("geometry field")
            .references
            .clear();

        crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        });
        assert!(crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_constraints(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        })
        .is_empty());
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn refuses_a_native_sketch_constraint_without_target_sketch_membership() {
        let (mut ir, mut native, transfer, graph_scope) = native_sketch_constraint_fixture();
        native.object_graphs[0]
            .records
            .iter_mut()
            .find(|record| record.id == "sketch-owner-record")
            .expect("sketch owner record")
            .references
            .retain(|reference| reference.target() != Some("constraint-owner-record"));

        crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_entities(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        });
        assert!(crate::test_support::with_service_context(|ctx| {
            transfer_native_sketch_constraints(ctx, &mut ir, &native, &transfer, &graph_scope)
                .expect("service profile admits sketch transfer")
        })
        .is_empty());
        assert!(ir.model.sketch_constraints.is_empty());
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
        assert_eq!(native_kind.as_str(), "CstAttr_Dimension");
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
        assert_eq!(operands[0].native_kind.as_str(), "ConstraintField");
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
    fn binds_a_constraint_to_an_exact_native_sketch_entity() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);
        let entity_id = SketchEntityId::mint("synthetic:test:sketch-entity#source".to_string())
            .expect("valid test fixture");
        ir.model.sketch_entities.push(
            SketchEntity::new(
                entity_id.clone(),
                SketchId::mint("synthetic:test:sketch#0".to_string()).expect("valid test fixture"),
                SketchGeometry::native(
                    cadmpeg_core::text::NonBlankString::try_from("2DPoint")
                        .expect("nonempty source identity"),
                ),
            )
            .with_native_ref(Some("source-record".to_string())),
        );

        crate::test_support::with_service_context(|ctx| {
            transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("valid sketch constraint transfer");

        let constraint = &ir.model.sketch_constraints[0];
        let SketchConstraintDefinitionInput::Native { entities, .. } = constraint.definition.kind()
        else {
            panic!("expected opaque native constraint");
        };
        assert_eq!(entities, &vec![entity_id]);
    }

    #[test]
    fn refuses_a_constraint_entity_binding_when_native_identity_is_ambiguous() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);
        for suffix in ["first", "second"] {
            ir.model.sketch_entities.push(
                SketchEntity::new(
                    SketchEntityId::mint(format!("synthetic:test:sketch-entity#{suffix}"))
                        .expect("valid test fixture"),
                    SketchId::mint("synthetic:test:sketch#0".to_string())
                        .expect("valid test fixture"),
                    SketchGeometry::native(
                        cadmpeg_core::text::NonBlankString::try_from("2DPoint")
                            .expect("nonempty source identity"),
                    ),
                )
                .with_native_ref(Some("source-record".to_string())),
            );
        }

        crate::test_support::with_service_context(|ctx| {
            transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("valid sketch constraint transfer");

        let constraint = &ir.model.sketch_constraints[0];
        let SketchConstraintDefinitionInput::Native { entities, .. } = constraint.definition.kind()
        else {
            panic!("expected opaque native constraint");
        };
        assert!(entities.is_empty());
    }

    #[test]
    fn refuses_a_constraint_entity_binding_from_another_sketch() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);
        ir.model.sketch_entities.push(
            SketchEntity::new(
                SketchEntityId::mint("synthetic:test:other-sketch-entity#source".to_string())
                    .expect("valid test fixture"),
                SketchId::mint("synthetic:test:other-sketch#0".to_string())
                    .expect("valid test fixture"),
                SketchGeometry::native(
                    cadmpeg_core::text::NonBlankString::try_from("2DPoint")
                        .expect("nonempty source identity"),
                ),
            )
            .with_native_ref(Some("source-record".to_string())),
        );

        crate::test_support::with_service_context(|ctx| {
            transfer_constraint_ranges(ctx, &mut ir, &native, &transfer, &graph_scope)
        })
        .expect("valid sketch constraint transfer");

        let constraint = &ir.model.sketch_constraints[0];
        let SketchConstraintDefinitionInput::Native { entities, .. } = constraint.definition.kind()
        else {
            panic!("expected opaque native constraint");
        };
        assert!(entities.is_empty());
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
