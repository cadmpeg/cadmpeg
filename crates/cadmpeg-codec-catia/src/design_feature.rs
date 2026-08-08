// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, ParameterId, PrincipalPlane, SketchSpace,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::native::{CatiaDesignObject, CatiaEntityRecord, CatiaNative, CatiaObjectRecord};
use crate::object_graph::{PayloadField, PayloadSubtype};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DesignFeatureTransfer {
    pub(crate) feature_ids: HashMap<String, FeatureId>,
    pub(crate) native_operation_records: HashSet<String>,
    pub(crate) native_operation_definition_value_count: usize,
    pub(crate) principal_plane_records: HashSet<String>,
    pub(crate) sketch_owner_records: HashSet<String>,
}

impl DesignFeatureTransfer {
    pub(crate) fn consumed_records(&self) -> HashSet<String> {
        self.principal_plane_records
            .union(&self.sketch_owner_records)
            .chain(self.native_operation_records.iter())
            .cloned()
            .collect()
    }

    /// Bind parameters to a transferred feature only through their exact
    /// entity-record and object-record ownership chain. The same exact
    /// incidences populate feature-local parameter ordinals.
    pub(crate) fn assign_parameter_owners(&self, ir: &mut CadIr, native: &CatiaNative) {
        let entities = native
            .entity_records
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect::<HashMap<_, _>>();
        let object_records = native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .map(|record| (record.id.as_str(), record))
            .collect::<HashMap<_, _>>();
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();
        let mut exact_feature_owners = HashMap::new();

        for parameter in &mut ir.model.parameters {
            let Some(native_ref) = parameter.native_ref.as_deref() else {
                continue;
            };
            let Some(entity) = entities.get(native_ref) else {
                continue;
            };
            let Some(object_record) = object_records.get(entity.object_record.as_str()) else {
                continue;
            };
            let Some(design_object) = object_record.design_object.as_deref() else {
                continue;
            };
            let Some(feature_id) =
                feature_owner_for_design_object(design_object, &design_objects, &self.feature_ids)
            else {
                continue;
            };
            if parameter.owner.is_none() {
                parameter.owner = Some(feature_id.clone());
            }
            if parameter.owner.as_ref() == Some(&feature_id) {
                exact_feature_owners.insert(parameter.id.clone(), feature_id);
            }
        }

        assign_feature_parameter_ordinals(
            ir,
            &entities,
            &object_records,
            &exact_feature_owners,
            &self.feature_ids,
        );
        normalize_parameter_names(ir);
        assign_native_operation_parameter_values(ir, &exact_feature_owners);
    }

    /// Bind a neutral feature to a transferred structural parent.
    ///
    /// The object graph records an exact owner-design-object incidence. It is
    /// a feature parent only when both endpoint design objects independently
    /// satisfy a neutral feature transfer. Do not use a field relation here:
    /// those relations are typed incidences, but their operation roles remain
    /// unresolved. A malformed owner cycle is omitted as a whole rather than
    /// creating a cyclic neutral history.
    pub(crate) fn assign_feature_parents(&self, ir: &mut CadIr, native: &CatiaNative) {
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();
        let parents = ir
            .model
            .features
            .iter()
            .filter_map(|feature| {
                let native_ref = feature.native_ref.as_deref()?;
                let object = design_objects.get(native_ref)?;
                let parent_object = object.owner_design_object.as_deref()?;
                let parent = self.feature_ids.get(parent_object)?;
                (parent != &feature.id).then(|| (feature.id.clone(), parent.clone()))
            })
            .collect::<HashMap<_, _>>();

        for feature in &mut ir.model.features {
            if feature.parent.is_some() {
                continue;
            }
            let Some(parent) = parents.get(&feature.id) else {
                continue;
            };
            if feature_parent_chain_is_acyclic(&feature.id, &parents) {
                feature.parent = Some(parent.clone());
            }
        }
    }
}

fn assign_feature_parameter_ordinals(
    ir: &mut CadIr,
    entities: &HashMap<&str, &crate::native::CatiaEntityRecord>,
    object_records: &HashMap<&str, &CatiaObjectRecord>,
    exact_feature_owners: &HashMap<ParameterId, FeatureId>,
    feature_ids: &HashMap<String, FeatureId>,
) {
    let transferred_features = feature_ids.values().cloned().collect::<HashSet<_>>();
    let mut parameters_by_feature = HashMap::<FeatureId, Vec<(u64, u64, ParameterId)>>::new();
    for parameter in &ir.model.parameters {
        let Some(feature_id) = exact_feature_owners.get(&parameter.id) else {
            continue;
        };
        if !transferred_features.contains(feature_id) {
            continue;
        }
        let Some(entity_id) = parameter.native_ref.as_deref() else {
            continue;
        };
        let Some(entity) = entities.get(entity_id) else {
            continue;
        };
        let Some(object_record) = object_records.get(entity.object_record.as_str()) else {
            continue;
        };
        parameters_by_feature
            .entry(feature_id.clone())
            .or_default()
            .push((
                object_record.byte_offset,
                entity.byte_offset,
                parameter.id.clone(),
            ));
    }

    let mut parameter_ordinals = HashMap::new();
    for parameters in parameters_by_feature.values_mut() {
        parameters.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
        });
        for (ordinal, parameter) in parameters.iter().enumerate() {
            let Some(ordinal) = u32::try_from(ordinal).ok() else {
                continue;
            };
            parameter_ordinals.insert(parameter.2.clone(), ordinal);
        }
    }

    for parameter in &mut ir.model.parameters {
        if let Some(ordinal) = parameter_ordinals.get(&parameter.id) {
            parameter.ordinal = *ordinal;
        }
    }
}

/// Expose exact feature-owned parameter expressions on an opaque native
/// operation without assigning operation-specific roles. The map uses the
/// neutral, scope-unique parameter names assigned by
/// [`normalize_parameter_names`]. A changed name retains its source spelling
/// in the corresponding parameter's `source_name` property.
fn assign_native_operation_parameter_values(
    ir: &mut CadIr,
    exact_feature_owners: &HashMap<ParameterId, FeatureId>,
) {
    let mut values_by_feature = HashMap::<FeatureId, BTreeMap<String, String>>::new();
    for parameter in &ir.model.parameters {
        let Some(feature_id) = exact_feature_owners.get(&parameter.id) else {
            continue;
        };
        values_by_feature
            .entry(feature_id.clone())
            .or_default()
            .insert(parameter.name.clone(), parameter.expression.clone());
    }

    for feature in &mut ir.model.features {
        let Some(values) = values_by_feature.remove(&feature.id) else {
            continue;
        };
        let FeatureDefinition::Native { parameters, .. } = &mut feature.definition else {
            continue;
        };
        if parameters.is_empty() {
            *parameters = values;
        }
    }
}

/// Give every neutral parameter a unique name within its ownership scope.
///
/// CATIA permits several source parameters with the same name under one
/// feature. The IR uses names as scope-local keys, so retaining those names
/// verbatim would produce an invalid model. Keep the first source name and
/// append a deterministic suffix to later collisions. Reserve every source
/// name before choosing a suffix so a generated name cannot hide a later
/// source parameter with that name. The original spelling remains available
/// in `properties["source_name"]` whenever the neutral name changes.
fn normalize_parameter_names(ir: &mut CadIr) {
    let mut reserved_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    for parameter in &ir.model.parameters {
        if !parameter.name.is_empty() {
            reserved_by_scope
                .entry(parameter.owner.clone())
                .or_default()
                .insert(parameter.name.clone());
        }
    }

    let mut used_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    for parameter in &mut ir.model.parameters {
        let scope = parameter.owner.clone();
        let reserved = reserved_by_scope
            .get(&scope)
            .expect("every parameter scope has a reserved-name set");
        let used = used_by_scope.entry(scope).or_default();
        let source_name = parameter.name.clone();
        if !source_name.is_empty() && used.insert(source_name.clone()) {
            continue;
        }

        let base = if source_name.is_empty() {
            "Parameter"
        } else {
            source_name.as_str()
        };
        let mut suffix = 1u32;
        let neutral_name = loop {
            let candidate = format!("{base}#{suffix}");
            suffix = suffix.saturating_add(1);
            if !reserved.contains(&candidate) && used.insert(candidate.clone()) {
                break candidate;
            }
        };
        parameter.name = neutral_name;
        parameter
            .properties
            .insert("source_name".to_string(), source_name);
    }
}

fn feature_parent_chain_is_acyclic(
    feature_id: &FeatureId,
    parents: &HashMap<FeatureId, FeatureId>,
) -> bool {
    let mut current = Some(feature_id);
    let mut visited = HashSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return false;
        }
        current = parents.get(id);
    }
    true
}

/// Derive one neutral history identity from a canonical CATIA native identity.
///
/// Native identities use the form `<format>:<scope>:<kind>#<key>`. Neutral
/// history identities keep the same format and scope and replace only the
/// source kind. Synthetic unit fixtures may use short IDs, for which the
/// legacy suffix form remains deterministic.
pub(crate) fn neutral_history_id(native_id: &str, kind: &str) -> String {
    let Some((namespace, key)) = native_id.rsplit_once('#') else {
        return format!("{native_id}:{kind}");
    };
    let mut components = namespace.split(':');
    let Some(format) = components.next() else {
        return format!("{native_id}:{kind}");
    };
    let Some(scope) = components.next() else {
        return format!("{native_id}:{kind}");
    };
    let Some(source_kind) = components.next() else {
        return format!("{native_id}:{kind}");
    };
    if format.is_empty()
        || scope.is_empty()
        || source_kind.is_empty()
        || components.next().is_some()
        || key.is_empty()
        || key.contains(':')
        || kind.is_empty()
    {
        return format!("{native_id}:{kind}");
    }
    format!("{format}:{scope}:{kind}#{key}")
}

/// Resolve one unique transferred feature on a complete structural owner chain.
///
/// An immediate field group is not always the semantic feature object. CATIA
/// may store a feature's parameter in a child design object. The parent links
/// are exact object-graph incidences, so following them is safe when the chain
/// is complete, has no non-reflexive cycle, and reaches exactly one transferred
/// feature.
fn feature_owner_for_design_object(
    design_object_id: &str,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    feature_ids: &HashMap<String, FeatureId>,
) -> Option<FeatureId> {
    let mut current = Some(design_object_id);
    let mut visited = HashSet::new();
    let mut feature = None;

    while let Some(current_id) = current {
        if !visited.insert(current_id) {
            return None;
        }
        let object = design_objects.get(current_id).copied()?;
        if let Some(candidate) = feature_ids.get(current_id) {
            if feature.replace(candidate.clone()).is_some() {
                return None;
            }
        }
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }

    feature
}

/// Transfer exact owner-bound reference history nodes.
pub(crate) fn transfer_design_features(
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: Option<&HashSet<String>>,
) -> DesignFeatureTransfer {
    let records = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let entities = native
        .entity_records
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    let mut transfer = DesignFeatureTransfer::default();

    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.is_none_or(|scope| scope.contains(object.parent.as_str())))
    {
        let plane_candidate = principal_plane_candidate(object, &records);
        let sketch_owner = sketch_candidate(object, &records);
        let native_operation = native_operation_candidate(object, &records);
        match (plane_candidate, sketch_owner, native_operation) {
            (Some(candidate), None, None) => {
                transfer_principal_plane(ir, &mut transfer, candidate);
            }
            (None, Some(owner_record), None) => {
                transfer_sketch(ir, &mut transfer, object, owner_record);
            }
            (None, None, Some(candidate)) => {
                transfer_native_operation(ir, &mut transfer, &candidate, &entities);
            }
            (Some(_), Some(_), _) | (Some(_), None, Some(_)) | (None, Some(_), Some(_)) => {
                // One object cannot safely occupy two neutral feature identities.
                // Leave all declarations unresolved so the feature-id map cannot
                // overwrite one transfer with another.
            }
            (None, None, None) => {}
        }
    }

    transfer.assign_feature_parents(ir, native);
    transfer
}

fn transfer_principal_plane(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: PrincipalPlaneCandidate<'_>,
) {
    let object = candidate.object;
    let feature_id = FeatureId(neutral_history_id(&object.id, "feature"));
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some(candidate.declaration_class.to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPrincipalPlane {
            plane: candidate.plane,
        },
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer.principal_plane_records.extend(
        candidate
            .declarations
            .into_iter()
            .map(|record| record.id.clone()),
    );
}

fn transfer_sketch(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    object: &CatiaDesignObject,
    owner_record: &CatiaObjectRecord,
) {
    let sketch_id = SketchId(neutral_history_id(&object.id, "sketch"));
    let feature_id = FeatureId(neutral_history_id(&object.id, "feature"));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        placement: SketchPlacement::Unresolved,
        profiles: Vec::new(),
        native_ref: Some(object.id.clone()),
    });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Sketch".to_string()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Unresolved,
            sketch: Some(sketch_id),
        },
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer
        .sketch_owner_records
        .insert(owner_record.id.clone());
}

struct NativeOperationCandidate<'a> {
    object: &'a CatiaDesignObject,
    owner_record: &'a CatiaObjectRecord,
    kind: &'a str,
}

fn native_operation_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<NativeOperationCandidate<'a>> {
    let owner_class = object.owner_class.as_ref()?;
    is_admitted_native_operation_class(&owner_class.name).then_some(())?;
    let owner_record_id = object.owner_record.as_deref()?;
    let owner_record = records.get(owner_record_id).copied()?;
    (owner_record.class_name.as_deref() == Some(owner_class.name.as_str())
        && owner_record.class_entry.as_deref() == Some(owner_class.entry.as_str())
        && owner_record.entity_id == Some(object.owner_entity_id)
        && owner_record.design_object.as_deref() == object.owner_design_object.as_deref())
    .then_some(NativeOperationCandidate {
        object,
        owner_record,
        kind: owner_class.name.as_str(),
    })
}

fn is_admitted_native_operation_class(name: &str) -> bool {
    matches!(
        name,
        "EdgeFillet"
            | "Prism_ThickThin1"
            | "Prism_ThickThin2"
            | "Revol_ThickThin1"
            | "Sweep_ThickThin1"
    )
}

fn transfer_native_operation(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &NativeOperationCandidate<'_>,
    entities: &HashMap<&str, &CatiaEntityRecord>,
) {
    let object = candidate.object;
    let kind = candidate.kind.to_string();
    let (properties, definition_value_count) =
        native_operation_definition_properties(object, entities);
    let feature_id = FeatureId(neutral_history_id(&object.id, "feature"));
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some(kind.clone()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind,
            parameters: BTreeMap::new(),
            properties,
        },
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer
        .native_operation_records
        .insert(candidate.owner_record.id.clone());
    transfer.native_operation_definition_value_count += definition_value_count;
}

/// Retain complete definition-bound values on the exact native operation
/// object that owns them. A one-definition value carries a source definition
/// and a typed suffix payload, but it does not establish a neutral parameter
/// type or operation role. Store its exact selectors and payload state as
/// native properties until that role grammar is settled.
fn native_operation_definition_properties(
    object: &CatiaDesignObject,
    entities: &HashMap<&str, &CatiaEntityRecord>,
) -> (BTreeMap<String, String>, usize) {
    let mut properties = BTreeMap::new();
    let mut value_count = 0;

    for (ordinal, entity_id) in object.definition_values.iter().enumerate() {
        let Some(entity) = entities.get(entity_id.as_str()) else {
            continue;
        };
        let Some(value) = entity.definition_value.as_ref() else {
            continue;
        };
        value_count += 1;
        let prefix = format!("catia_definition_value_{ordinal}");
        properties.insert(format!("{prefix}_entity"), entity.id.clone());
        insert_schema_value_properties(
            &mut properties,
            &format!("{prefix}_definition"),
            &value.definition,
        );
        insert_suffix_payload_properties(
            &mut properties,
            &format!("{prefix}_payload"),
            &value.payload,
        );
        if let Some(selection) = value.schema_selection.as_ref() {
            insert_schema_selection_properties(
                &mut properties,
                &format!("{prefix}_schema_selection"),
                selection,
            );
        }
    }

    (properties, value_count)
}

fn insert_schema_value_properties(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    value: &crate::native::CatiaEntitySchemaValue,
) {
    properties.insert(format!("{prefix}_entry"), value.entry.clone());
    properties.insert(format!("{prefix}_ordinal"), value.ordinal.to_string());
    properties.insert(format!("{prefix}_offset"), value.offset.to_string());
    properties.insert(format!("{prefix}_value"), value.value.clone());
}

fn insert_suffix_payload_properties(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    payload: &crate::native::CatiaEntitySuffixPayload,
) {
    match payload {
        crate::native::CatiaEntitySuffixPayload::Evaluation {
            opcode_offset,
            evaluation,
            encoding,
        } => {
            properties.insert(format!("{prefix}_kind"), "evaluation".to_string());
            properties.insert(format!("{prefix}_opcode_offset"), opcode_offset.to_string());
            properties.insert(
                format!("{prefix}_encoding"),
                evaluation_encoding_name(*encoding).to_string(),
            );
            insert_evaluation_properties(properties, prefix, evaluation);
        }
        crate::native::CatiaEntitySuffixPayload::Atom { value } => {
            properties.insert(format!("{prefix}_kind"), "atom".to_string());
            properties.insert(format!("{prefix}_value"), value.to_string());
        }
        crate::native::CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset,
            selector,
            value,
        } => {
            properties.insert(format!("{prefix}_kind"), "schema_selected".to_string());
            properties.insert(
                format!("{prefix}_selector_offset"),
                selector_offset.to_string(),
            );
            properties.insert(format!("{prefix}_selector"), selector.to_string());
            insert_selected_value_properties(properties, &format!("{prefix}_selected"), value);
        }
        crate::native::CatiaEntitySuffixPayload::ControlE8 => {
            properties.insert(format!("{prefix}_kind"), "control_e8".to_string());
        }
        crate::native::CatiaEntitySuffixPayload::ControlE9 => {
            properties.insert(format!("{prefix}_kind"), "control_e9".to_string());
        }
        crate::native::CatiaEntitySuffixPayload::Separator37 => {
            properties.insert(format!("{prefix}_kind"), "separator_37".to_string());
        }
    }
}

fn insert_selected_value_properties(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    value: &crate::native::CatiaEntitySuffixSelectedValue,
) {
    match value {
        crate::native::CatiaEntitySuffixSelectedValue::Atom { value } => {
            properties.insert(format!("{prefix}_kind"), "atom".to_string());
            properties.insert(format!("{prefix}_value"), value.to_string());
        }
        crate::native::CatiaEntitySuffixSelectedValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            properties.insert(format!("{prefix}_kind"), "evaluation".to_string());
            properties.insert(format!("{prefix}_opcode_offset"), opcode_offset.to_string());
            insert_evaluation_properties(properties, prefix, evaluation);
        }
        crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
            properties.insert(format!("{prefix}_kind"), "control_e8".to_string());
        }
        crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
            properties.insert(format!("{prefix}_kind"), "separator_37".to_string());
        }
        crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector { offset, ordinal } => {
            properties.insert(format!("{prefix}_kind"), "schema_selector".to_string());
            properties.insert(format!("{prefix}_offset"), offset.to_string());
            properties.insert(format!("{prefix}_ordinal"), ordinal.to_string());
        }
    }
}

fn insert_schema_selection_properties(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    selection: &crate::native::CatiaEntitySuffixSchemaSelection,
) {
    properties.insert(format!("{prefix}_offset"), selection.offset.to_string());
    properties.insert(format!("{prefix}_ordinal"), selection.ordinal.to_string());
    properties.insert(format!("{prefix}_entry"), selection.entry.clone());
    properties.insert(format!("{prefix}_name"), selection.name.clone());
    insert_schema_selected_value_properties(
        properties,
        &format!("{prefix}_value"),
        &selection.value,
    );
}

fn insert_schema_selected_value_properties(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    value: &crate::native::CatiaEntitySuffixSchemaValue,
) {
    match value {
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value } => {
            properties.insert(format!("{prefix}_kind"), "atom".to_string());
            properties.insert(format!("{prefix}_atom"), value.to_string());
        }
        crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            properties.insert(format!("{prefix}_kind"), "evaluation".to_string());
            properties.insert(format!("{prefix}_opcode_offset"), opcode_offset.to_string());
            insert_evaluation_properties(properties, prefix, evaluation);
        }
        crate::native::CatiaEntitySuffixSchemaValue::ControlE8 => {
            properties.insert(format!("{prefix}_kind"), "control_e8".to_string());
        }
        crate::native::CatiaEntitySuffixSchemaValue::Separator37 => {
            properties.insert(format!("{prefix}_kind"), "separator_37".to_string());
        }
        crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset,
            ordinal,
            entry,
            name,
        } => {
            properties.insert(format!("{prefix}_kind"), "schema_selector".to_string());
            properties.insert(format!("{prefix}_offset"), offset.to_string());
            properties.insert(format!("{prefix}_ordinal"), ordinal.to_string());
            if let Some(entry) = entry {
                properties.insert(format!("{prefix}_entry"), entry.clone());
            }
            if let Some(name) = name {
                properties.insert(format!("{prefix}_name"), name.clone());
            }
        }
    }
}

fn insert_evaluation_properties(
    properties: &mut BTreeMap<String, String>,
    prefix: &str,
    evaluation: &crate::native::CatiaEntityEvaluation,
) {
    match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            properties.insert(format!("{prefix}_evaluation"), "unset".to_string());
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => {
            properties.insert(format!("{prefix}_evaluation"), "scalar".to_string());
            properties.insert(format!("{prefix}_evaluation_bits"), format!("{bits:016x}"));
        }
    }
}

fn evaluation_encoding_name(
    encoding: crate::native::CatiaEntityEvaluationEncoding,
) -> &'static str {
    match encoding {
        crate::native::CatiaEntityEvaluationEncoding::Direct => "direct",
        crate::native::CatiaEntityEvaluationEncoding::ZeroPaddedScalar => "zero_padded_scalar",
    }
}

struct PrincipalPlaneCandidate<'a> {
    object: &'a CatiaDesignObject,
    declarations: Vec<&'a CatiaObjectRecord>,
    plane: PrincipalPlane,
    declaration_class: &'a str,
}

fn principal_plane_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<PrincipalPlaneCandidate<'a>> {
    object.owner_record.as_ref()?;
    let declarations = object
        .fields
        .iter()
        .map(|field| records.get(field.as_str()).copied())
        .collect::<Option<Vec<_>>>()?;
    let first = declarations.first()?;
    let class_name = first.class_name.as_deref()?;
    let class_entry = first.class_entry.as_deref()?;
    let plane = principal_plane(class_name)?;
    declarations
        .iter()
        .all(|record| {
            record.class_name.as_deref() == Some(class_name)
                && record.class_entry.as_deref() == Some(class_entry)
                && complete_empty_declaration(record, &object.id, object.owner_entity_id)
        })
        .then_some(PrincipalPlaneCandidate {
            object,
            declarations,
            plane,
            declaration_class: class_name,
        })
}

fn sketch_candidate<'a>(
    object: &CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<&'a CatiaObjectRecord> {
    let owner_class = object.owner_class.as_ref()?;
    (owner_class.name == "Sketch").then_some(())?;
    let owner_record_id = object.owner_record.as_deref()?;
    let owner_record = records.get(owner_record_id).copied()?;
    (owner_record.class_name.as_deref() == Some("Sketch")
        && owner_record.class_entry.as_deref() == Some(owner_class.entry.as_str())
        && owner_record.design_object.as_deref() == object.owner_design_object.as_deref())
    .then_some(owner_record)
}

fn principal_plane(class_name: &str) -> Option<PrincipalPlane> {
    match class_name {
        "xy-plane" => Some(PrincipalPlane::Top),
        "yz-plane" => Some(PrincipalPlane::Right),
        "zx-plane" => Some(PrincipalPlane::Front),
        _ => None,
    }
}

fn bound_declaration(
    record: &CatiaObjectRecord,
    design_object: &str,
    owner_entity_id: u32,
) -> bool {
    record.design_object.as_deref() == Some(design_object)
        && record.owner_entity_id() == Some(owner_entity_id)
}

fn complete_empty_declaration(
    record: &CatiaObjectRecord,
    design_object: &str,
    owner_entity_id: u32,
) -> bool {
    bound_declaration(record, design_object, owner_entity_id)
        && record.storage_ref.is_none()
        && record.references.is_empty()
        && record.subtype == PayloadSubtype::Empty
        && record.payload.size == 1
        && record.payload.fields == [PayloadField::Terminator]
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;

    use crate::native::{
        CatiaDefinitionValue, CatiaDesignClass, CatiaEntityEvaluation,
        CatiaEntityEvaluationEncoding, CatiaEntityRecord, CatiaEntitySchemaValue,
        CatiaEntitySuffixPayload, CatiaObjectGraph, CatiaObjectOwner,
    };
    use crate::object_graph::ObjectPayload;

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

    fn feature(id: &str, native_ref: &str) -> Feature {
        Feature {
            id: FeatureId::from(id),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::StoredGeometry,
            native_ref: Some(native_ref.to_string()),
        }
    }

    fn native_operation_object(
        id: &str,
        owner_design_object: Option<&str>,
        owner_entity_id: u32,
        owner_record: &str,
        class_name: &str,
        class_entry: &str,
    ) -> CatiaDesignObject {
        let mut object = design_object(id, owner_design_object);
        object.owner_entity_id = owner_entity_id;
        object.owner_record = Some(owner_record.to_string());
        object.owner_class = Some(CatiaDesignClass {
            entry: class_entry.to_string(),
            name: class_name.to_string(),
        });
        object
    }

    fn object_record(
        id: &str,
        design_object: Option<&str>,
        entity_id: Option<u32>,
        owner: Option<u32>,
        class_name: Option<&str>,
        class_entry: Option<&str>,
    ) -> CatiaObjectRecord {
        CatiaObjectRecord {
            id: id.to_string(),
            parent: "graph".to_string(),
            design_object: design_object.map(str::to_string),
            entity_record: None,
            entity_id,
            ordinal: 0,
            byte_offset: 0,
            byte_len: 0,
            lead: 0,
            head: Vec::new(),
            inline_body: None,
            owner: owner.map(CatiaObjectOwner::Entity),
            class_ref: None,
            class_name: class_name.map(str::to_string),
            class_entry: class_entry.map(str::to_string),
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

    fn entity_record(
        id: &str,
        object_record: &str,
        byte_offset: u64,
        entity_id: u32,
    ) -> CatiaEntityRecord {
        CatiaEntityRecord {
            id: id.to_string(),
            object_graph: "graph".to_string(),
            object_record: object_record.to_string(),
            ordinal: 0,
            byte_offset,
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

    fn parameter(id: &str, native_ref: &str) -> cadmpeg_ir::features::DesignParameter {
        cadmpeg_ir::features::DesignParameter {
            id: ParameterId(id.to_string()),
            owner: None,
            ordinal: 99,
            name: id.to_string(),
            expression: "1 mm".to_string(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(1.0),
            )),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some(native_ref.to_string()),
        }
    }

    #[test]
    fn assigns_parent_from_an_exact_transferred_owner_chain() {
        let native = CatiaNative {
            design_objects: vec![
                design_object("parent-object", None),
                design_object("child-object", Some("parent-object")),
            ],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .features
            .push(feature("parent-feature", "parent-object"));
        ir.model
            .features
            .push(feature("child-feature", "child-object"));
        let transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([
                (
                    "parent-object".to_string(),
                    FeatureId::from("parent-feature"),
                ),
                ("child-object".to_string(), FeatureId::from("child-feature")),
            ]),
            ..DesignFeatureTransfer::default()
        };

        transfer.assign_feature_parents(&mut ir, &native);

        assert_eq!(ir.model.features[0].parent, None);
        assert_eq!(
            ir.model.features[1].parent,
            Some(FeatureId::from("parent-feature"))
        );
    }

    #[test]
    fn does_not_assign_a_self_parent() {
        let native = CatiaNative {
            design_objects: vec![design_object("feature-object", Some("feature-object"))],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model.features.push(feature("feature", "feature-object"));
        let transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([(
                "feature-object".to_string(),
                FeatureId::from("feature"),
            )]),
            ..DesignFeatureTransfer::default()
        };

        transfer.assign_feature_parents(&mut ir, &native);

        assert_eq!(ir.model.features[0].parent, None);
    }

    #[test]
    fn omits_all_parents_in_an_owner_cycle() {
        let native = CatiaNative {
            design_objects: vec![
                design_object("first-object", Some("second-object")),
                design_object("second-object", Some("first-object")),
            ],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .features
            .push(feature("first-feature", "first-object"));
        ir.model
            .features
            .push(feature("second-feature", "second-object"));
        let transfer = DesignFeatureTransfer {
            feature_ids: HashMap::from([
                ("first-object".to_string(), FeatureId::from("first-feature")),
                (
                    "second-object".to_string(),
                    FeatureId::from("second-feature"),
                ),
            ]),
            ..DesignFeatureTransfer::default()
        };

        transfer.assign_feature_parents(&mut ir, &native);

        assert!(ir
            .model
            .features
            .iter()
            .all(|feature| feature.parent.is_none()));
    }

    #[test]
    fn transfers_admitted_native_operations_with_exact_parentage() {
        let mut parent = native_operation_object(
            "parent-object",
            None,
            1,
            "parent-record",
            "Prism_ThickThin1",
            "parent-entry",
        );
        parent.first_field_byte_offset = 10;
        let mut child = native_operation_object(
            "child-object",
            Some("parent-object"),
            2,
            "child-record",
            "EdgeFillet",
            "child-entry",
        );
        child.first_field_byte_offset = 20;
        let native = CatiaNative {
            design_objects: vec![parent, child],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![
                    object_record(
                        "parent-record",
                        None,
                        Some(1),
                        None,
                        Some("Prism_ThickThin1"),
                        Some("parent-entry"),
                    ),
                    object_record(
                        "child-record",
                        Some("parent-object"),
                        Some(2),
                        Some(1),
                        Some("EdgeFillet"),
                        Some("child-entry"),
                    ),
                ],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transfer = transfer_design_features(&mut ir, &native, None);

        assert_eq!(ir.model.features.len(), 2);
        assert_eq!(ir.model.features[0].ordinal, 10);
        assert_eq!(ir.model.features[1].ordinal, 20);
        assert_eq!(
            ir.model.features[1].parent,
            Some(FeatureId::from("parent-object:feature"))
        );
        for (feature, kind) in ir
            .model
            .features
            .iter()
            .zip(["Prism_ThickThin1", "EdgeFillet"])
        {
            let FeatureDefinition::Native {
                kind: actual_kind,
                parameters,
                properties,
            } = &feature.definition
            else {
                panic!("expected an opaque native operation");
            };
            assert_eq!(actual_kind, kind);
            assert!(parameters.is_empty());
            assert!(properties.is_empty());
        }
        assert_eq!(
            transfer.native_operation_records,
            HashSet::from(["parent-record".to_string(), "child-record".to_string()])
        );
        assert_eq!(
            transfer.consumed_records(),
            transfer.native_operation_records
        );
    }

    #[test]
    fn transfers_exact_definition_values_as_opaque_native_properties() {
        let mut operation = native_operation_object(
            "operation-object",
            None,
            1,
            "operation-record",
            "Prism_ThickThin1",
            "operation-entry",
        );
        operation
            .definition_values
            .push("definition-entity".to_string());
        let mut definition_entity = entity_record("definition-entity", "operation-record", 20, 1);
        definition_entity.definition_value = Some(CatiaDefinitionValue {
            definition: CatiaEntitySchemaValue {
                offset: 4,
                ordinal: 2,
                entry: "definition-entry".to_string(),
                value: "Mirror".to_string(),
            },
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 8,
                evaluation: CatiaEntityEvaluation::Scalar {
                    bits: 12.5_f64.to_bits(),
                },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            schema_selection: None,
        });
        let native = CatiaNative {
            design_objects: vec![operation],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![object_record(
                    "operation-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("operation-entry"),
                )],
            }],
            entity_records: vec![definition_entity],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transfer = transfer_design_features(&mut ir, &native, None);

        let FeatureDefinition::Native {
            parameters,
            properties,
            ..
        } = &ir.model.features[0].definition
        else {
            panic!("expected an opaque native operation");
        };
        assert!(parameters.is_empty());
        assert_eq!(
            properties,
            &BTreeMap::from([
                (
                    "catia_definition_value_0_definition_entry".to_string(),
                    "definition-entry".to_string(),
                ),
                (
                    "catia_definition_value_0_definition_offset".to_string(),
                    "4".to_string(),
                ),
                (
                    "catia_definition_value_0_definition_ordinal".to_string(),
                    "2".to_string(),
                ),
                (
                    "catia_definition_value_0_definition_value".to_string(),
                    "Mirror".to_string(),
                ),
                (
                    "catia_definition_value_0_entity".to_string(),
                    "definition-entity".to_string(),
                ),
                (
                    "catia_definition_value_0_payload_encoding".to_string(),
                    "direct".to_string(),
                ),
                (
                    "catia_definition_value_0_payload_evaluation".to_string(),
                    "scalar".to_string(),
                ),
                (
                    "catia_definition_value_0_payload_evaluation_bits".to_string(),
                    format!("{:016x}", 12.5_f64.to_bits()),
                ),
                (
                    "catia_definition_value_0_payload_kind".to_string(),
                    "evaluation".to_string(),
                ),
                (
                    "catia_definition_value_0_payload_opcode_offset".to_string(),
                    "8".to_string(),
                ),
            ])
        );
        assert_eq!(transfer.native_operation_definition_value_count, 1);
    }

    #[test]
    fn orders_exact_feature_parameters_by_serialized_field_position() {
        let operation = native_operation_object(
            "operation-object",
            None,
            1,
            "operation-record",
            "Prism_ThickThin1",
            "operation-entry",
        );
        let operation_record = object_record(
            "operation-record",
            None,
            Some(1),
            None,
            Some("Prism_ThickThin1"),
            Some("operation-entry"),
        );
        let mut late_parameter_record = object_record(
            "late-parameter-record",
            Some("operation-object"),
            Some(3),
            Some(1),
            None,
            None,
        );
        late_parameter_record.byte_offset = 30;
        late_parameter_record.entity_record = Some("late-parameter-entity".to_string());
        let mut early_parameter_record = object_record(
            "early-parameter-record",
            Some("operation-object"),
            Some(2),
            Some(1),
            None,
            None,
        );
        early_parameter_record.byte_offset = 20;
        early_parameter_record.entity_record = Some("early-parameter-entity".to_string());
        let native = CatiaNative {
            design_objects: vec![operation],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![
                    operation_record,
                    late_parameter_record,
                    early_parameter_record,
                ],
            }],
            entity_records: vec![
                entity_record("late-parameter-entity", "late-parameter-record", 300, 3),
                entity_record("early-parameter-entity", "early-parameter-record", 200, 2),
            ],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .parameters
            .push(parameter("late-parameter", "late-parameter-entity"));
        ir.model
            .parameters
            .push(parameter("early-parameter", "early-parameter-entity"));

        let transfer = transfer_design_features(&mut ir, &native, None);
        transfer.assign_parameter_owners(&mut ir, &native);

        assert_eq!(
            ir.model
                .parameters
                .iter()
                .map(|parameter| (
                    parameter.id.clone(),
                    parameter.owner.clone(),
                    parameter.ordinal
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    ParameterId("late-parameter".to_string()),
                    Some(FeatureId::from("operation-object:feature")),
                    1,
                ),
                (
                    ParameterId("early-parameter".to_string()),
                    Some(FeatureId::from("operation-object:feature")),
                    0,
                ),
            ]
        );
        let FeatureDefinition::Native { parameters, .. } = &ir.model.features[0].definition else {
            panic!("expected an opaque native operation");
        };
        assert_eq!(
            parameters,
            &BTreeMap::from([
                ("early-parameter".to_string(), "1 mm".to_string()),
                ("late-parameter".to_string(), "1 mm".to_string()),
            ])
        );
    }

    #[test]
    fn native_parameter_map_uses_disambiguated_names_when_source_names_collide() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.features.push(Feature {
            id: FeatureId::from("feature"),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Prism_ThickThin1".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "Prism_ThickThin1".to_string(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
            native_ref: Some("native-feature".to_string()),
        });
        let mut first = parameter("first", "first-native");
        first.name = "Length".to_string();
        let mut second = parameter("second", "second-native");
        second.name = "Length".to_string();
        ir.model.parameters.extend([first, second]);

        normalize_parameter_names(&mut ir);
        assign_native_operation_parameter_values(
            &mut ir,
            &HashMap::from([
                (ParameterId("first".to_string()), FeatureId::from("feature")),
                (
                    ParameterId("second".to_string()),
                    FeatureId::from("feature"),
                ),
            ]),
        );

        let FeatureDefinition::Native { parameters, .. } = &ir.model.features[0].definition else {
            panic!("expected an opaque native operation");
        };
        assert_eq!(
            parameters,
            &BTreeMap::from([
                ("Length".to_string(), "1 mm".to_string()),
                ("Length#1".to_string(), "1 mm".to_string()),
            ])
        );
    }

    #[test]
    fn disambiguates_parameter_names_without_hiding_a_later_source_name() {
        let owner = FeatureId::from("feature");
        let mut ir = CadIr::empty(Units::default());
        let mut first = parameter("first", "first-native");
        first.owner = Some(owner.clone());
        first.name = "Angle".to_string();
        let mut second = parameter("second", "second-native");
        second.owner = Some(owner.clone());
        second.name = "Angle".to_string();
        let mut later_source_name = parameter("later", "later-native");
        later_source_name.owner = Some(owner);
        later_source_name.name = "Angle#1".to_string();
        ir.model
            .parameters
            .extend([first, second, later_source_name]);

        normalize_parameter_names(&mut ir);

        assert_eq!(
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["Angle", "Angle#2", "Angle#1"]
        );
        assert_eq!(
            ir.model.parameters[1].properties.get("source_name"),
            Some(&"Angle".to_string())
        );
        assert!(!ir.model.parameters[2]
            .properties
            .contains_key("source_name"));
    }

    #[test]
    fn does_not_promote_an_unadmitted_helper_owner_class() {
        let object = native_operation_object(
            "helper-object",
            None,
            1,
            "helper-record",
            "Prism_EndLimit_Length",
            "helper-entry",
        );
        let native = CatiaNative {
            design_objects: vec![object],
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".to_string(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records: vec![object_record(
                    "helper-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_EndLimit_Length"),
                    Some("helper-entry"),
                )],
            }],
            ..CatiaNative::default()
        };
        let mut ir = CadIr::empty(Units::default());

        let transfer = transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.features.is_empty());
        assert!(transfer.native_operation_records.is_empty());
    }
}
