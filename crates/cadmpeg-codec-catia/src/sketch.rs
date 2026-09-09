// SPDX-License-Identifier: Apache-2.0
//! Transfer of source-closed CATIA sketch relations.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::unique_index::UniqueIndex;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{
    NativeOperandField, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
    SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchNativeOperand,
};

use crate::design_feature::{self, DesignFeatureTransfer};
use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{
    CatiaConstraintRange, CatiaDesignObject, CatiaEntityEvaluation, CatiaNative, CatiaObjectRecord,
    CatiaObjectRecordReference, CatiaObjectRecordReferenceSource,
};

const NATIVE_SKETCH_GEOMETRY_CLASSES: &[&str] = &["2DPoint"];

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
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: Option<&HashSet<String>>,
) -> HashSet<String> {
    let object_records = unique_object_records(native);
    let entity_records = unique_entity_records(native);
    let design_objects = unique_design_objects(native);
    let mut design_objects_by_owner_record = HashMap::<&str, Vec<&CatiaDesignObject>>::new();
    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.is_none_or(|scope| scope.contains(object.parent.as_str())))
    {
        let Some(owner_record) = object.owner_record.as_deref() else {
            continue;
        };
        design_objects_by_owner_record
            .entry(owner_record)
            .or_default()
            .push(object);
    }

    let sketches = ir
        .model
        .sketches
        .iter()
        .filter_map(|sketch| {
            sketch
                .native_ref
                .as_deref()
                .map(|native_ref| (sketch.id.clone(), native_ref.to_string()))
        })
        .collect::<Vec<_>>();
    let mut transferred = HashSet::new();

    for (sketch_id, sketch_native_ref) in sketches {
        let Some(sketch_object) = design_objects.get(sketch_native_ref.as_str()).copied() else {
            continue;
        };
        if graph_scope.is_some_and(|scope| !scope.contains(sketch_object.parent.as_str())) {
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
            owner_record,
            &object_records,
            &design_objects_by_owner_record,
            &design_objects,
        ) {
            let geometry_fields =
                admitted_sketch_geometry_fields(child_object, &object_records, &entity_records);
            let [geometry_field] = geometry_fields.as_slice() else {
                continue;
            };
            if !seen_fields.insert(geometry_field.id.as_str()) {
                continue;
            }

            let Ok(entity_id) = SketchEntityId::mint(design_feature::neutral_history_id(
                &geometry_field.id,
                "sketch-entity",
            )) else {
                continue;
            };
            if ir.model.sketch_entities.iter().any(|entity| {
                entity.id() == &entity_id
                    || (entity.sketch == sketch_id
                        && entity.native_ref.as_deref() == Some(geometry_field.id.as_str()))
            }) {
                continue;
            }
            let Some(native_kind) = geometry_field
                .class_name()
                .and_then(cadmpeg_ir::products::NonEmptyString::new)
            else {
                continue;
            };
            ir.model.sketch_entities.push(
                SketchEntity::new(
                    entity_id,
                    sketch_id.clone(),
                    SketchGeometry::native(native_kind),
                )
                .with_native_ref(Some(geometry_field.id.clone())),
            );
            transferred.insert(geometry_field.id.clone());
        }
    }

    transferred
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
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: Option<&HashSet<String>>,
) -> HashSet<String> {
    let object_records = unique_object_records(native);
    let entity_records = unique_entity_records(native);
    let design_objects = unique_design_objects(native);
    let mut design_objects_by_owner_record = HashMap::<&str, Vec<&CatiaDesignObject>>::new();
    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.is_none_or(|scope| scope.contains(object.parent.as_str())))
    {
        let Some(owner_record) = object.owner_record.as_deref() else {
            continue;
        };
        design_objects_by_owner_record
            .entry(owner_record)
            .or_default()
            .push(object);
    }

    let sketches = ir
        .model
        .sketches
        .iter()
        .filter_map(|sketch| {
            sketch
                .native_ref
                .as_deref()
                .map(|native_ref| (sketch.id.clone(), native_ref.to_string()))
        })
        .collect::<Vec<_>>();
    let mut candidates = HashMap::<(SketchId, String), NativeSketchConstraintCandidate>::new();

    for (sketch_id, sketch_native_ref) in sketches {
        let Some(sketch_object) = design_objects.get(sketch_native_ref.as_str()).copied() else {
            continue;
        };
        if graph_scope.is_some_and(|scope| !scope.contains(sketch_object.parent.as_str())) {
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
            owner_record,
            &object_records,
            &design_objects_by_owner_record,
            &design_objects,
        );
        let member_object_ids = member_objects
            .iter()
            .map(|object| object.id.as_str())
            .collect::<HashSet<_>>();
        let sketch_entities = ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch_id)
            .filter_map(|entity| {
                Some((
                    entity.native_ref.as_deref()?.to_string(),
                    entity.id().clone(),
                ))
            })
            .collect::<UniqueIndex<_, _>>();

        for child_object in member_objects {
            for geometry_field in
                admitted_sketch_geometry_fields(child_object, &object_records, &entity_records)
            {
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
                    if target_record.parent != owner_record.parent
                        || target_record.class_name() != Some("ConstraintDYS")
                        || target_record.class_entry().is_none()
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

                    let key = (sketch_id.clone(), target_record.id.clone());
                    let candidate =
                        candidates
                            .entry(key)
                            .or_insert_with(|| NativeSketchConstraintCandidate {
                                sketch: sketch_id.clone(),
                                target_record: target_record.id.clone(),
                                target_entity_record: target_entity_record.id.clone(),
                                target_class: target_record
                                    .class_name()
                                    .expect("admitted native sketch constraint class")
                                    .to_owned(),
                                target_entry: target_record
                                    .class_entry()
                                    .expect("admitted native sketch constraint entry")
                                    .to_owned(),
                                target_ordinal: target_record.ordinal,
                                target_byte_offset: target_record.byte_offset,
                                target_references: target_record.references.clone(),
                                entities: Vec::new(),
                                incidences: Vec::new(),
                            });
                    if !candidate.entities.contains(sketch_entity) {
                        candidate.entities.push(sketch_entity.clone());
                    }
                    candidate.incidences.push(NativeSketchConstraintIncidence {
                        field: geometry_field.id.clone(),
                        field_offset: geometry_field.byte_offset,
                        reference_offset: reference.payload_offset(),
                    });
                }
            }
        }
    }

    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.target_byte_offset
            .cmp(&right.target_byte_offset)
            .then(left.target_record.cmp(&right.target_record))
            .then(left.sketch.cmp(&right.sketch))
    });

    let mut transferred = HashSet::new();
    for candidate in candidates {
        let Ok(constraint_id) = SketchConstraintId::mint(design_feature::neutral_history_id(
            &candidate.target_entity_record,
            "sketch-constraint",
        )) else {
            continue;
        };
        if ir.model.sketch_constraints.iter().any(|constraint| {
            constraint.id == constraint_id
                || constraint.native_ref.as_deref() == Some(candidate.target_entity_record.as_str())
        }) {
            continue;
        }
        let Some(object_index) = u32::try_from(candidate.target_ordinal).ok() else {
            continue;
        };
        let mut native_properties = BTreeMap::new();
        native_properties.insert(
            "catia_relation_source_class".to_string(),
            "2DPoint".to_string(),
        );
        native_properties.insert(
            "catia_relation_target_class".to_string(),
            candidate.target_class.clone(),
        );
        native_properties.insert(
            "catia_relation_target_entry".to_string(),
            candidate.target_entry.clone(),
        );
        native_properties.insert(
            "catia_relation_target_ordinal".to_string(),
            candidate.target_ordinal.to_string(),
        );
        native_properties.insert(
            "catia_relation_target_offset".to_string(),
            candidate.target_byte_offset.to_string(),
        );
        insert_target_reference_properties(
            &mut native_properties,
            &candidate.target_references,
            &object_records,
        );
        native_properties.insert(
            "catia_relation_incidence_count".to_string(),
            candidate.incidences.len().to_string(),
        );
        for (ordinal, incidence) in candidate.incidences.iter().enumerate() {
            let prefix = format!("catia_relation_incidence_{ordinal}");
            native_properties.insert(format!("{prefix}_source_field"), incidence.field.clone());
            native_properties.insert(
                format!("{prefix}_source_field_offset"),
                incidence.field_offset.to_string(),
            );
            native_properties.insert(
                format!("{prefix}_source_reference_offset"),
                incidence.reference_offset.to_string(),
            );
        }
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: candidate.target_class,
                native_state: None,
                native_flags: None,
                native_properties,
                entities: candidate.entities,
                parameter: None,
                operands: vec![SketchNativeOperand {
                    native_kind: cadmpeg_ir::products::NonEmptyString::new("ConstraintDYS")
                        .expect("source operand kind is nonempty"),
                    field: Some(NativeOperandField {
                        name: cadmpeg_ir::products::NonEmptyString::new(
                            candidate.target_record.clone(),
                        )
                        .expect("source field name is nonempty"),
                        role: None,
                    }),
                    object_index,
                    native_ref: Some(candidate.target_entity_record.clone()),
                }],
            },
        ) else {
            continue;
        };
        ir.model.sketch_constraints.push(SketchConstraint {
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
            native_ref: Some(candidate.target_entity_record.clone()),
        });
        transferred.insert(candidate.target_record);
    }
    transferred
}

struct NativeSketchConstraintCandidate {
    sketch: SketchId,
    target_record: String,
    target_entity_record: String,
    target_class: String,
    target_entry: String,
    target_ordinal: u64,
    target_byte_offset: u64,
    target_references: Vec<CatiaObjectRecordReference>,
    entities: Vec<SketchEntityId>,
    incidences: Vec<NativeSketchConstraintIncidence>,
}

struct NativeSketchConstraintIncidence {
    field: String,
    field_offset: u64,
    reference_offset: u64,
}

fn insert_target_reference_properties(
    properties: &mut BTreeMap<String, String>,
    references: &[CatiaObjectRecordReference],
    object_records: &UniqueIndex<&str, &CatiaObjectRecord>,
) {
    properties.insert(
        "catia_relation_target_reference_count".to_string(),
        references.len().to_string(),
    );
    for (ordinal, reference) in references.iter().enumerate() {
        let prefix = format!("catia_relation_target_reference_{ordinal}");
        properties.insert(
            format!("{prefix}_entity_id"),
            reference.entity_id().to_string(),
        );
        properties.insert(
            format!("{prefix}_payload_offset"),
            reference.payload_offset().to_string(),
        );
        let state = if reference.is_null() {
            "null"
        } else if reference.target().is_some() {
            "resolved"
        } else {
            "unresolved"
        };
        properties.insert(format!("{prefix}_state"), state.to_string());
        match reference.source() {
            CatiaObjectRecordReferenceSource::Field => {
                properties.insert(format!("{prefix}_source"), "field".to_string());
            }
            CatiaObjectRecordReferenceSource::ListItem {
                list_payload_offset,
                item_ordinal,
            } => {
                properties.insert(format!("{prefix}_source"), "list_item".to_string());
                properties.insert(
                    format!("{prefix}_list_payload_offset"),
                    list_payload_offset.to_string(),
                );
                properties.insert(format!("{prefix}_item_ordinal"), item_ordinal.to_string());
            }
        }
        if let Some(target) = reference.target() {
            properties.insert(format!("{prefix}_target_record"), target.to_string());
            if let Some(target_record) = object_records.get(target) {
                if let Some(class_name) = target_record.class_name() {
                    properties.insert(format!("{prefix}_target_class"), class_name.to_string());
                }
                if let Some(class_entry) = target_record.class_entry() {
                    properties.insert(format!("{prefix}_target_entry"), class_entry.to_string());
                }
            }
        }
        if let Some(design_object) = reference.design_object() {
            properties.insert(
                format!("{prefix}_target_design_object"),
                design_object.to_string(),
            );
        }
    }
}

fn exact_sketch_member_objects<'a>(
    owner_record: &'a CatiaObjectRecord,
    object_records: &UniqueIndex<&'a str, &'a CatiaObjectRecord>,
    design_objects_by_owner_record: &HashMap<&'a str, Vec<&'a CatiaDesignObject>>,
    design_objects: &UniqueIndex<&'a str, &'a CatiaDesignObject>,
) -> Vec<&'a CatiaDesignObject> {
    owner_record
        .references
        .iter()
        .filter_map(|reference| {
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
        })
        .collect()
}

fn admitted_sketch_geometry_fields<'a>(
    child_object: &'a CatiaDesignObject,
    object_records: &UniqueIndex<&'a str, &'a CatiaObjectRecord>,
    entity_records: &UniqueIndex<&'a str, &'a CatiaEntityRecord>,
) -> Vec<&'a CatiaObjectRecord> {
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
                && field
                    .class_name()
                    .is_some_and(is_native_sketch_geometry_class)
        })
        .collect()
}

fn is_native_sketch_geometry_class(class_name: &str) -> bool {
    NATIVE_SKETCH_GEOMETRY_CLASSES.contains(&class_name)
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
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: Option<&HashSet<String>>,
) -> HashSet<String> {
    let indexes = ConstraintIndexes::new(native, ir);
    let mut transferred = HashSet::new();

    for entity in &native.entity_records {
        let Some(range) = entity.constraint_range() else {
            continue;
        };
        let Some(binding) =
            constraint_binding(entity, range, &indexes, feature_transfer, graph_scope)
        else {
            continue;
        };

        let Ok(constraint_id) = SketchConstraintId::mint(design_feature::neutral_history_id(
            &entity.id,
            "sketch-constraint",
        )) else {
            continue;
        };
        if ir.model.sketch_constraints.iter().any(|constraint| {
            constraint.id == constraint_id
                || constraint.native_ref.as_deref() == Some(entity.id.as_str())
        }) {
            continue;
        }
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: range.constraint.value.clone(),
                native_state: None,
                native_flags: None,
                native_properties: constraint_properties(range),
                entities: binding.entity.into_iter().collect(),
                parameter: None,
                operands: vec![binding.operand],
            },
        ) else {
            continue;
        };
        ir.model.sketch_constraints.push(SketchConstraint {
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
            native_ref: Some(entity.id.clone()),
        });
        transferred.insert(entity.object_record.clone());
        transferred.insert(binding.source_object_record);
    }

    transferred
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
    fn new(native: &'a CatiaNative, ir: &CadIr) -> Self {
        let entity_records = unique_entity_records(native);
        let object_records = unique_object_records(native);
        let design_objects = unique_design_objects(native);
        let sketch_ids = sketch_ids_by_native_ref(ir);
        let sketch_entities = sketch_entities_by_native_ref(ir);
        Self {
            entity_records,
            object_records,
            design_objects,
            sketch_ids,
            sketch_entities,
        }
    }
}

fn sketch_entities_by_native_ref(ir: &CadIr) -> UniqueIndex<String, (SketchEntityId, SketchId)> {
    ir.model
        .sketch_entities
        .iter()
        .filter_map(|entity| {
            Some((
                entity.native_ref.as_deref()?.to_string(),
                (entity.id().clone(), entity.sketch.clone()),
            ))
        })
        .collect()
}

fn constraint_binding(
    range_entity: &CatiaEntityRecord,
    range: &CatiaConstraintRange,
    indexes: &ConstraintIndexes<'_>,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: Option<&HashSet<String>>,
) -> Option<ConstraintBinding> {
    if graph_scope.is_some_and(|scope| !scope.contains(range_entity.object_graph.as_str())) {
        return None;
    }

    let range_record_id = range_entity.object_record.as_str();
    let range_record = indexes.object_records.get(range_record_id).copied()?;
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
    let sketch = sketch_owner_for_design_object(
        source_design_object,
        &indexes.design_objects,
        &indexes.sketch_ids,
        feature_transfer,
    )?;
    let entity = indexes
        .sketch_entities
        .get(source_record_id)
        .filter(|(_, entity_sketch)| entity_sketch == &sketch)
        .map(|(entity, _)| entity.clone());
    let object_index = u32::try_from(source_record.ordinal).ok()?;
    let native_kind = source_record
        .class_name()
        .filter(|class| !class.is_empty())
        .unwrap_or("record")
        .to_owned();
    Some(ConstraintBinding {
        sketch,
        source_object_record: source_record.id.clone(),
        operand: SketchNativeOperand {
            native_kind: cadmpeg_ir::products::NonEmptyString::new(native_kind)
                .expect("source operand kind is nonempty"),
            field: Some(NativeOperandField {
                name: cadmpeg_ir::products::NonEmptyString::new(source_record.id.clone())
                    .expect("source field name is nonempty"),
                role: None,
            }),
            object_index,
            native_ref: Some(source_entity_record.id.clone()),
        },
        entity,
    })
}

fn sketch_owner_for_design_object<'a>(
    start: &'a str,
    design_objects: &UniqueIndex<&'a str, &'a CatiaDesignObject>,
    sketch_ids: &UniqueIndex<String, SketchId>,
    feature_transfer: &DesignFeatureTransfer,
) -> Option<SketchId> {
    let mut current = Some(start);
    let mut visited = HashSet::new();

    while let Some(current_id) = current {
        if !visited.insert(current_id) {
            return None;
        }
        let object = design_objects.get(current_id).copied()?;
        if feature_transfer.feature_ids.contains_key(current_id) {
            return sketch_ids.get(current_id).cloned();
        }
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }

    None
}

fn constraint_properties(range: &CatiaConstraintRange) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    insert_selector(&mut properties, "catia_range", &range.range);
    insert_selector(&mut properties, "catia_constraint", &range.constraint);
    properties.insert(
        "catia_framing".to_string(),
        framing_name(range.framing).to_string(),
    );
    match range.evaluation {
        CatiaEntityEvaluation::Unset => {
            properties.insert("catia_evaluation".to_string(), "unset".to_string());
        }
        CatiaEntityEvaluation::Scalar { bits } => {
            properties.insert("catia_evaluation".to_string(), "scalar".to_string());
            properties.insert("catia_evaluation_bits".to_string(), format!("{bits:016x}"));
        }
    }
    properties.insert(
        "catia_evaluation_opcode_offset".to_string(),
        range.evaluation_opcode_offset.to_string(),
    );
    properties
}

fn insert_selector(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    selector: &crate::native::CatiaEntitySchemaValue,
) {
    properties.insert(format!("{prefix}_entry"), selector.entry.clone());
    properties.insert(format!("{prefix}_ordinal"), selector.ordinal.to_string());
    properties.insert(format!("{prefix}_offset"), selector.offset.to_string());
    properties.insert(format!("{prefix}_value"), selector.value.clone());
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

fn unique_entity_records(native: &CatiaNative) -> UniqueIndex<&str, &CatiaEntityRecord> {
    native
        .entity_records
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect()
}

fn unique_object_records(native: &CatiaNative) -> UniqueIndex<&str, &CatiaObjectRecord> {
    native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect()
}

fn unique_design_objects(native: &CatiaNative) -> UniqueIndex<&str, &CatiaDesignObject> {
    native
        .design_objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect()
}

fn sketch_ids_by_native_ref(ir: &CadIr) -> UniqueIndex<String, SketchId> {
    ir.model
        .sketches
        .iter()
        .filter_map(|sketch| Some((sketch.native_ref.as_deref()?.to_string(), sketch.id.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use cadmpeg_ir::sketches::{Sketch, SketchPlacement};

    use crate::design_feature::DesignFeatureTransfer;
    use crate::native::entity_record::CatiaEntityRecordBody;
    use crate::native::{
        CatiaConstraintRangeFraming, CatiaEntityEvaluation, CatiaEntityIncomingReference,
        CatiaEntitySchemaValue, CatiaObjectGraph, CatiaObjectOwner, CatiaObjectRecordReference,
        CatiaObjectRecordReferenceSource,
    };
    use crate::object_graph::{ObjectPayload, PayloadField};

    fn design_object(id: &str, owner_design_object: Option<&str>) -> CatiaDesignObject {
        CatiaDesignObject {
            id: id.to_string(),
            parent: "graph".to_string(),
            ordinal: 0,
            first_field_byte_offset: 0,
            owner_entity_id: 0,
            owner_record: None,
            owner_design_object: owner_design_object.map(str::to_string),
            owner_class: None,
            owner_storage_ref: None,
            fields: Vec::new(),
            field_classes: Vec::new(),
            definition_values: Vec::new(),
            definition_chain_values: Vec::new(),
            relations: Vec::new(),
            parallel_reference_table: None,
        }
    }

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
                class_ref: 0,
                class_name: Some(class_name.to_string()),
                class_entry: Some("entry".to_string()),
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

    fn fixture(storage: bool) -> (CadIr, CatiaNative, DesignFeatureTransfer, HashSet<String>) {
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
                storage_ref: 10,
                storage_record: None,
                storage_design_object: None,
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
            placement: SketchPlacement::Unresolved,
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
            HashSet::from(["graph".to_string()]),
        )
    }

    fn native_sketch_fixture(
        geometry_class: &str,
    ) -> (CadIr, CatiaNative, DesignFeatureTransfer, HashSet<String>) {
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
            placement: SketchPlacement::Unresolved,
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
            HashSet::from(["graph".to_string()]),
        )
    }

    fn native_sketch_constraint_fixture(
    ) -> (CadIr, CatiaNative, DesignFeatureTransfer, HashSet<String>) {
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

        let transferred =
            transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope));
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
            cadmpeg_ir::sketches::SketchGeometryDefinition::Native { native_kind } if native_kind == "2DPoint"
        ));
    }

    #[test]
    fn does_not_promote_an_unadmitted_native_sketch_member() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("Point");

        assert!(
            transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope))
                .is_empty()
        );
        assert!(ir.model.sketch_entities.is_empty());
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
            transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope))
                .is_empty()
        );
        assert!(ir.model.sketch_entities.is_empty());
    }

    #[test]
    fn refuses_constraint_relations_with_duplicate_sketch_entity_references() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_constraint_fixture();
        transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope));
        assert!(!ir.model.sketch_entities.is_empty());
        let duplicates = ir.model.sketch_entities.clone();
        ir.model.sketch_entities.extend(duplicates.clone());
        ir.model.sketch_entities.extend(duplicates);
        assert!(transfer_native_sketch_constraints(
            &mut ir,
            &native,
            &transfer,
            Some(&graph_scope)
        )
        .is_empty());
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn transfers_a_source_closed_native_sketch_constraint_relation() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_constraint_fixture();

        transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope));
        assert_eq!(
            transfer_native_sketch_constraints(&mut ir, &native, &transfer, Some(&graph_scope)),
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
        assert_eq!(native_kind, "ConstraintDYS");
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
        assert_eq!(operands[0].native_kind, "ConstraintDYS");
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
    fn refuses_a_native_sketch_constraint_without_source_incidence() {
        let (mut ir, mut native, transfer, graph_scope) = native_sketch_constraint_fixture();
        native.object_graphs[0]
            .records
            .iter_mut()
            .find(|record| record.id == "catia:outer:object-record#geometry-field")
            .expect("geometry field")
            .references
            .clear();

        transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope));
        assert!(transfer_native_sketch_constraints(
            &mut ir,
            &native,
            &transfer,
            Some(&graph_scope)
        )
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

        transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope));
        assert!(transfer_native_sketch_constraints(
            &mut ir,
            &native,
            &transfer,
            Some(&graph_scope)
        )
        .is_empty());
        assert!(ir.model.sketch_constraints.is_empty());
    }

    #[test]
    fn transfers_a_uniquely_owned_constraint_range_as_opaque_native_constraint() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);

        assert_eq!(
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
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

        transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope));

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
                    cadmpeg_ir::products::NonEmptyString::new("2DPoint")
                        .expect("nonempty source identity"),
                ),
            )
            .with_native_ref(Some("source-record".to_string())),
        );

        transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope));

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
                        cadmpeg_ir::products::NonEmptyString::new("2DPoint")
                            .expect("nonempty source identity"),
                    ),
                )
                .with_native_ref(Some("source-record".to_string())),
            );
        }

        transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope));

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
                    cadmpeg_ir::products::NonEmptyString::new("2DPoint")
                        .expect("nonempty source identity"),
                ),
            )
            .with_native_ref(Some("source-record".to_string())),
        );

        transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope));

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
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
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
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
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
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
            HashSet::new()
        );
        assert!(ir.model.sketch_constraints.is_empty());
    }
}
