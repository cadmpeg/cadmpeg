// SPDX-License-Identifier: Apache-2.0
//! Transfer of source-closed CATIA sketch relations.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity, SketchEntityId,
    SketchGeometry, SketchId, SketchNativeOperand,
};

use crate::design_feature::{self, DesignFeatureTransfer};
use crate::native::{
    CatiaConstraintRange, CatiaDesignObject, CatiaEntityEvaluation, CatiaEntityRecord, CatiaNative,
    CatiaObjectRecord,
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
    let (object_records, ambiguous_object_records) = unique_object_records(native);
    let (entity_records, ambiguous_entity_records) = unique_entity_records(native);
    let (design_objects, ambiguous_design_objects) = unique_design_objects(native);
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
        if ambiguous_design_objects.contains(sketch_native_ref.as_str())
            || graph_scope.is_some_and(|scope| !scope.contains(sketch_object.parent.as_str()))
        {
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
        if ambiguous_object_records.contains(owner_record_id)
            || owner_record.parent != sketch_object.parent
            || owner_record.design_object.as_deref() != sketch_object.owner_design_object.as_deref()
        {
            continue;
        }

        let mut seen_fields = HashSet::new();
        for reference in &owner_record.references {
            let Some(target_id) = reference.target.as_deref() else {
                continue;
            };
            if reference.is_null || ambiguous_object_records.contains(target_id) {
                continue;
            }
            let Some(target_record) = object_records.get(target_id).copied() else {
                continue;
            };
            if target_record.parent != owner_record.parent
                || target_record.entity_id != Some(reference.entity_id)
                || reference.design_object.as_deref() != target_record.design_object.as_deref()
            {
                continue;
            }
            let Some(child_objects) = design_objects_by_owner_record.get(target_id) else {
                continue;
            };
            let [child_object] = child_objects.as_slice() else {
                continue;
            };
            if ambiguous_design_objects.contains(child_object.id.as_str())
                || child_object.parent != owner_record.parent
                || child_object.owner_record.as_deref() != Some(target_id)
                || child_object.owner_entity_id != reference.entity_id
            {
                continue;
            }

            let geometry_fields = child_object
                .fields
                .iter()
                .filter_map(|field_id| object_records.get(field_id.as_str()).copied())
                .filter(|field| {
                    let Some(entity_record_id) = field.entity_record.as_deref() else {
                        return false;
                    };
                    let Some(entity_record) = entity_records.get(entity_record_id) else {
                        return false;
                    };
                    field.parent == child_object.parent
                        && field.design_object.as_deref() == Some(child_object.id.as_str())
                        && field.owner_entity_id() == Some(child_object.owner_entity_id)
                        && field.entity_id.is_some()
                        && !ambiguous_entity_records.contains(entity_record_id)
                        && entity_record.object_graph == field.parent
                        && entity_record.object_record == field.id
                        && Some(entity_record.entity_id) == field.entity_id
                        && field.class_entry.is_some()
                        && field
                            .class_name
                            .as_deref()
                            .is_some_and(is_native_sketch_geometry_class)
                })
                .collect::<Vec<_>>();
            let [geometry_field] = geometry_fields.as_slice() else {
                continue;
            };
            if !seen_fields.insert(geometry_field.id.as_str()) {
                continue;
            }

            let entity_id = SketchEntityId(design_feature::neutral_history_id(
                &geometry_field.id,
                "sketch-entity",
            ));
            if ir.model.sketch_entities.iter().any(|entity| {
                entity.id == entity_id
                    || (entity.sketch == sketch_id
                        && entity.native_ref.as_deref() == Some(geometry_field.id.as_str()))
            }) {
                continue;
            }
            let Some(native_kind) = geometry_field.class_name.clone() else {
                continue;
            };
            ir.model.sketch_entities.push(SketchEntity {
                id: entity_id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: Some(geometry_field.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Native { native_kind },
            });
            transferred.insert(geometry_field.id.clone());
        }
    }

    transferred
}

fn is_native_sketch_geometry_class(class_name: &str) -> bool {
    NATIVE_SKETCH_GEOMETRY_CLASSES.contains(&class_name)
}

/// Transfer complete constraint ranges whose structural owner is one
/// transferred sketch.
///
/// A range is an opaque constraint at this layer. Its exact selectors,
/// framing, and evaluation are retained as native properties. The unique
/// source record is retained as an unresolved native operand; no geometry,
/// dimensional, or driving-parameter role is inferred from the range alone.
/// The returned object-record identities are the exact range records
/// represented by the emitted neutral constraints. The source operand remains
/// unresolved by design.
pub(crate) fn transfer_constraint_ranges(
    ir: &mut CadIr,
    native: &CatiaNative,
    feature_transfer: &DesignFeatureTransfer,
    graph_scope: Option<&HashSet<String>>,
) -> HashSet<String> {
    let indexes = ConstraintIndexes::new(native, ir);
    let mut transferred = HashSet::new();

    for entity in &native.entity_records {
        let Some(range) = entity.constraint_range.as_ref() else {
            continue;
        };
        let Some(binding) =
            constraint_binding(entity, range, &indexes, feature_transfer, graph_scope)
        else {
            continue;
        };

        let constraint_id = SketchConstraintId(design_feature::neutral_history_id(
            &entity.id,
            "sketch-constraint",
        ));
        if ir.model.sketch_constraints.iter().any(|constraint| {
            constraint.id == constraint_id
                || constraint.native_ref.as_deref() == Some(entity.id.as_str())
        }) {
            continue;
        }

        ir.model.sketch_constraints.push(SketchConstraint {
            id: constraint_id,
            sketch: binding.sketch,
            definition: SketchConstraintDefinition::Native {
                native_kind: range.constraint.value.clone(),
                native_state: None,
                native_flags: None,
                native_properties: constraint_properties(range),
                entities: Vec::new(),
                parameter: None,
                operands: vec![binding.operand],
            },
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
    }

    transferred
}

struct ConstraintBinding {
    sketch: SketchId,
    operand: SketchNativeOperand,
}

struct ConstraintIndexes<'a> {
    entity_records: HashMap<&'a str, &'a CatiaEntityRecord>,
    ambiguous_entity_records: HashSet<&'a str>,
    object_records: HashMap<&'a str, &'a CatiaObjectRecord>,
    ambiguous_object_records: HashSet<&'a str>,
    design_objects: HashMap<&'a str, &'a CatiaDesignObject>,
    ambiguous_design_objects: HashSet<&'a str>,
    sketch_ids: HashMap<String, SketchId>,
    ambiguous_sketch_ids: HashSet<String>,
}

impl<'a> ConstraintIndexes<'a> {
    fn new(native: &'a CatiaNative, ir: &CadIr) -> Self {
        let (entity_records, ambiguous_entity_records) = unique_entity_records(native);
        let (object_records, ambiguous_object_records) = unique_object_records(native);
        let (design_objects, ambiguous_design_objects) = unique_design_objects(native);
        let (sketch_ids, ambiguous_sketch_ids) = sketch_ids_by_native_ref(ir);
        Self {
            entity_records,
            ambiguous_entity_records,
            object_records,
            ambiguous_object_records,
            design_objects,
            ambiguous_design_objects,
            sketch_ids,
            ambiguous_sketch_ids,
        }
    }
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
    if indexes.ambiguous_object_records.contains(range_record_id)
        || range_record.parent != range_entity.object_graph
        || range_record.entity_id != Some(range_entity.entity_id)
        || range_record.entity_record.as_deref() != Some(range_entity.id.as_str())
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
    let source_entity = source_entity.filter(|entity| !entity.is_null)?;
    let source_entity_id = source_entity.entity.as_deref()?;
    if indexes.ambiguous_entity_records.contains(source_entity_id)
        || indexes
            .ambiguous_object_records
            .contains(source_record_id.as_str())
    {
        return None;
    }

    let source_record = indexes
        .object_records
        .get(source_record_id.as_str())
        .copied()?;
    if source_record.parent != range_entity.object_graph
        || source_record.entity_id != Some(source_entity.entity_id)
        || source_record.entity_record.as_deref() != Some(source_entity_id)
        || source_entity.class_name.as_deref() != source_record.class_name.as_deref()
    {
        return None;
    }
    let source_entity_record = indexes.entity_records.get(source_entity_id).copied()?;
    if source_entity_record.object_graph != range_entity.object_graph
        || source_entity_record.object_record != source_record.id
        || source_entity_record.entity_id != source_entity.entity_id
    {
        return None;
    }
    let source_design_object = source_record.design_object.as_deref()?;
    let sketch = sketch_owner_for_design_object(
        source_design_object,
        &indexes.design_objects,
        &indexes.ambiguous_design_objects,
        &indexes.sketch_ids,
        &indexes.ambiguous_sketch_ids,
        feature_transfer,
    )?;
    let object_index = u32::try_from(source_record.ordinal).ok()?;
    let native_kind = source_record
        .class_name
        .clone()
        .filter(|class| !class.is_empty())
        .unwrap_or_else(|| "record".to_string());
    Some(ConstraintBinding {
        sketch,
        operand: SketchNativeOperand {
            native_kind,
            native_field: Some(source_record.id.clone()),
            native_role: None,
            object_index,
            native_ref: Some(source_entity_record.id.clone()),
        },
    })
}

fn sketch_owner_for_design_object<'a>(
    start: &'a str,
    design_objects: &HashMap<&'a str, &'a CatiaDesignObject>,
    ambiguous_design_objects: &HashSet<&'a str>,
    sketch_ids: &HashMap<String, SketchId>,
    ambiguous_sketch_ids: &HashSet<String>,
    feature_transfer: &DesignFeatureTransfer,
) -> Option<SketchId> {
    let mut current = Some(start);
    let mut visited = HashSet::new();

    while let Some(current_id) = current {
        if !visited.insert(current_id) || ambiguous_design_objects.contains(current_id) {
            return None;
        }
        let object = design_objects.get(current_id).copied()?;
        if feature_transfer.feature_ids.contains_key(current_id) {
            if ambiguous_sketch_ids.contains(current_id) {
                return None;
            }
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
        crate::native::CatiaConstraintRangeFraming::ComplexC9 => "ComplexC9",
    }
}

fn unique_entity_records(
    native: &CatiaNative,
) -> (HashMap<&str, &CatiaEntityRecord>, HashSet<&str>) {
    let mut records = HashMap::new();
    let mut ambiguous = HashSet::new();
    for entity in &native.entity_records {
        if records.insert(entity.id.as_str(), entity).is_some() {
            ambiguous.insert(entity.id.as_str());
        }
    }
    (records, ambiguous)
}

fn unique_object_records(
    native: &CatiaNative,
) -> (HashMap<&str, &CatiaObjectRecord>, HashSet<&str>) {
    let mut records = HashMap::new();
    let mut ambiguous = HashSet::new();
    for record in native.object_graphs.iter().flat_map(|graph| &graph.records) {
        if records.insert(record.id.as_str(), record).is_some() {
            ambiguous.insert(record.id.as_str());
        }
    }
    (records, ambiguous)
}

fn unique_design_objects(
    native: &CatiaNative,
) -> (HashMap<&str, &CatiaDesignObject>, HashSet<&str>) {
    let mut objects = HashMap::new();
    let mut ambiguous = HashSet::new();
    for object in &native.design_objects {
        if objects.insert(object.id.as_str(), object).is_some() {
            ambiguous.insert(object.id.as_str());
        }
    }
    (objects, ambiguous)
}

fn sketch_ids_by_native_ref(ir: &CadIr) -> (HashMap<String, SketchId>, HashSet<String>) {
    let mut sketch_ids = HashMap::new();
    let mut ambiguous = HashSet::new();
    for sketch in &ir.model.sketches {
        let Some(native_ref) = sketch.native_ref.as_deref() else {
            continue;
        };
        if sketch_ids
            .insert(native_ref.to_string(), sketch.id.clone())
            .is_some()
        {
            ambiguous.insert(native_ref.to_string());
        }
    }
    (sketch_ids, ambiguous)
}

#[cfg(test)]
mod tests {
    use super::*;

    use cadmpeg_ir::sketches::{Sketch, SketchPlacement};
    use cadmpeg_ir::units::Units;

    use crate::design_feature::DesignFeatureTransfer;
    use crate::native::{
        CatiaConstraintRangeFraming, CatiaConstraintRangeIncomingReference, CatiaEntityEvaluation,
        CatiaEntitySchemaValue, CatiaObjectGraph, CatiaObjectOwner, CatiaObjectRecordReference,
        CatiaObjectRecordReferenceSource,
    };
    use crate::object_graph::{ObjectPayload, PayloadField, PayloadSubtype};

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
            entity_record: Some(entity_record.to_string()),
            entity_id: Some(entity_id),
            ordinal: 0,
            byte_offset: 0,
            byte_len: 0,
            lead: 0,
            head: Vec::new(),
            inline_body: None,
            owner: Some(CatiaObjectOwner::Entity(entity_id)),
            class_ref: None,
            class_name: Some(class_name.to_string()),
            class_entry: Some("entry".to_string()),
            storage_ref: None,
            storage_record: None,
            storage_design_object: None,
            payload: ObjectPayload {
                size: 1,
                fields: vec![PayloadField::Terminator],
            },
            repeated_reference_suffix: None,
            repeated_reference_schema_selection: None,
            subtype: PayloadSubtype::Empty,
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
            byte_len: 0,
            lead: 0,
            definition_len: 0,
            definition_prefix: Vec::new(),
            definition_schema_selections: Vec::new(),
            entity_id,
            definition_suffix: Vec::new(),
            value_len: 0,
            value_payload: Vec::new(),
            value_fields: Vec::new(),
            value_schema_selections: Vec::new(),
            relation_expression: None,
            parameter_value: None,
            constraint_range: None,
            definition_value: None,
            definition_chain_value: None,
            relation_program_instance: None,
            configuration_record: None,
            configuration_row_link: None,
            formula_relation: None,
            value_packets: Vec::new(),
            numeric_pair: None,
            reference_signature: None,
            record_suffix: Vec::new(),
            suffix_value: None,
            suffix_framing: None,
            suffix_schema_selection: None,
        }
    }

    fn fixture(storage: bool) -> (CadIr, CatiaNative, DesignFeatureTransfer, HashSet<String>) {
        let mut range_entity = entity_record("catia:outer:entity-record#range", "range-record", 10);
        range_entity.constraint_range = Some(CatiaConstraintRange {
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
        });
        let mut source_record = object_record(
            "source-record",
            Some("source-object"),
            11,
            "catia:outer:entity-record#source",
            "ConstraintField",
        );
        if storage {
            range_entity
                .constraint_range
                .as_mut()
                .expect("constraint range")
                .incoming_storage_references
                .push(
                    crate::native::CatiaConstraintRangeIncomingStorageReference {
                        object_record: "source-record".to_string(),
                        source_entity: Some(crate::native::CatiaEntityReference {
                            entity_id: 11,
                            is_null: false,
                            entity: Some("catia:outer:entity-record#source".to_string()),
                            class_name: Some("ConstraintField".to_string()),
                        }),
                    },
                );
            source_record.storage_ref = Some(10);
        } else {
            range_entity
                .constraint_range
                .as_mut()
                .expect("constraint range")
                .incoming_references
                .push(CatiaConstraintRangeIncomingReference {
                    object_record: "source-record".to_string(),
                    source_entity: Some(crate::native::CatiaEntityReference {
                        entity_id: 11,
                        is_null: false,
                        entity: Some("catia:outer:entity-record#source".to_string()),
                        class_name: Some("ConstraintField".to_string()),
                    }),
                    payload_offset: 9,
                    source: CatiaObjectRecordReferenceSource::Field,
                });
            source_record.references.push(CatiaObjectRecordReference {
                entity_id: 10,
                payload_offset: 9,
                source: CatiaObjectRecordReferenceSource::Field,
                is_null: false,
                target: Some("range-record".to_string()),
                design_object: None,
            });
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
        let mut ir = CadIr::empty(Units::default());
        ir.model.sketches.push(Sketch {
            id: SketchId("synthetic:test:sketch#0".to_string()),
            name: None,
            configuration: None,
            placement: SketchPlacement::Unresolved,
            profiles: Vec::new(),
            native_ref: Some("sketch-object".to_string()),
        });
        let feature_transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([(
                "sketch-object".to_string(),
                cadmpeg_ir::features::FeatureId("synthetic:test:feature#0".to_string()),
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
        sketch_owner.references.push(CatiaObjectRecordReference {
            entity_id: 3,
            payload_offset: 0,
            source: CatiaObjectRecordReferenceSource::Field,
            is_null: false,
            target: Some("child-owner-record".to_string()),
            design_object: Some("parent-object".to_string()),
        });

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
        let mut ir = CadIr::empty(Units::default());
        ir.model.sketches.push(Sketch {
            id: SketchId("synthetic:test:sketch#0".to_string()),
            name: None,
            configuration: None,
            placement: SketchPlacement::Unresolved,
            profiles: Vec::new(),
            native_ref: Some("sketch-object".to_string()),
        });
        let feature_transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([(
                "sketch-object".to_string(),
                cadmpeg_ir::features::FeatureId("synthetic:test:feature#0".to_string()),
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

    #[test]
    fn transfers_one_exact_native_sketch_geometry_member() {
        let (mut ir, native, transfer, graph_scope) = native_sketch_fixture("2DPoint");

        let transferred =
            transfer_native_sketch_entities(&mut ir, &native, &transfer, Some(&graph_scope));
        assert_eq!(transferred.len(), 1);
        assert!(transferred.contains("catia:outer:object-record#geometry-field"));
        assert_eq!(ir.model.sketch_entities.len(), 1);
        let entity = &ir.model.sketch_entities[0];
        assert_eq!(entity.sketch.0, "synthetic:test:sketch#0");
        assert_eq!(
            entity.native_ref.as_deref(),
            Some("catia:outer:object-record#geometry-field")
        );
        assert_eq!(entity.id.0, "catia:outer:sketch-entity#geometry-field");
        assert!(entity.geometry_ref.is_none());
        assert!(matches!(
            &entity.geometry,
            SketchGeometry::Native { native_kind } if native_kind == "2DPoint"
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
    fn transfers_a_uniquely_owned_constraint_range_as_opaque_native_constraint() {
        let (mut ir, native, transfer, graph_scope) = fixture(false);

        assert_eq!(
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
            HashSet::from(["range-record".to_string()])
        );
        assert_eq!(ir.model.sketch_constraints.len(), 1);
        let constraint = &ir.model.sketch_constraints[0];
        assert_eq!(constraint.sketch.0, "synthetic:test:sketch#0");
        assert_eq!(
            constraint.native_ref.as_deref(),
            Some("catia:outer:entity-record#range")
        );
        let SketchConstraintDefinition::Native {
            native_kind,
            native_properties,
            entities,
            parameter,
            operands,
            ..
        } = &constraint.definition
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
        assert_eq!(operands[0].native_field.as_deref(), Some("source-record"));
        assert_eq!(
            operands[0].native_ref.as_deref(),
            Some("catia:outer:entity-record#source")
        );
    }

    #[test]
    fn transfers_a_unique_storage_owned_constraint_range() {
        let (mut ir, native, transfer, graph_scope) = fixture(true);

        assert_eq!(
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
            HashSet::from(["range-record".to_string()])
        );
        assert_eq!(ir.model.sketch_constraints.len(), 1);
    }

    #[test]
    fn refuses_a_constraint_range_with_repeated_incidences() {
        let (mut ir, mut native, transfer, graph_scope) = fixture(false);
        let range = native.entity_records[0]
            .constraint_range
            .as_mut()
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
            cadmpeg_ir::features::FeatureId("source-object:feature".to_string()),
        );

        assert_eq!(
            transfer_constraint_ranges(&mut ir, &native, &transfer, Some(&graph_scope)),
            HashSet::new()
        );
        assert!(ir.model.sketch_constraints.is_empty());
    }
}
