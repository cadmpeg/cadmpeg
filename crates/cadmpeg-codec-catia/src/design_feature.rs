// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
    PrincipalPlane, UnresolvedFamily,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::entity_table::{RangeIntervalPrefix, RangeIntervalSlot};
use crate::ids::neutral_history_id;
use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{
    CatiaDesignObject, CatiaDesignObjectRelationSource, CatiaNative, CatiaObjectRecord,
    CatiaRangeInterval, CatiaRangeNominalFraming,
};
use crate::object_graph::{PayloadField, PayloadSubtype};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DesignFeatureTransfer {
    pub(crate) feature_ids: HashMap<String, FeatureId>,
    pub(crate) native_operation_records: HashSet<String>,
    pub(crate) native_operation_definition_value_count: usize,
    pub(crate) native_operation_definition_chain_value_count: usize,
    pub(crate) native_operation_range_count: usize,
    pub(crate) native_operation_definition_value_records: HashSet<String>,
    pub(crate) native_operation_definition_chain_value_records: HashSet<String>,
    pub(crate) native_operation_range_records: HashSet<String>,
    pub(crate) principal_plane_records: HashSet<String>,
    pub(crate) reference_plane_records: HashSet<String>,
    pub(crate) sketch_owner_records: HashSet<String>,
}

impl DesignFeatureTransfer {
    pub(crate) fn consumed_records(&self) -> HashSet<String> {
        self.principal_plane_records
            .union(&self.sketch_owner_records)
            .chain(self.reference_plane_records.iter())
            .chain(self.native_operation_records.iter())
            .chain(self.native_operation_definition_value_records.iter())
            .chain(self.native_operation_definition_chain_value_records.iter())
            .chain(self.native_operation_range_records.iter())
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
            let Some(feature_id) = nearest_feature_for_design_object(
                design_object,
                &design_objects,
                &self.feature_ids,
            ) else {
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
        assign_document_parameter_ordinals(ir);
        normalize_parameter_names(ir);
        assign_native_operation_parameter_values(ir, &exact_feature_owners);
    }

    /// Bind a neutral feature to a transferred structural parent.
    ///
    /// The object graph records an exact owner-design-object chain. The
    /// nearest transferred feature on a complete chain is a feature parent;
    /// intermediate non-feature groups do not change that identity. Do not use
    /// a field relation here: those relations are typed incidences, but their
    /// operation roles remain unresolved. A malformed owner cycle or a parent
    /// that does not precede its child is omitted rather than creating an
    /// invalid neutral history.
    pub(crate) fn assign_feature_parents(
        &self,
        ir: &mut CadIr,
        native: &CatiaNative,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let parents = self.feature_parents(ir, native);
        for feature in &mut ir.model.features {
            feature.dependencies.retain(|dependency| {
                parents
                    .get(&feature.id)
                    .is_none_or(|parent| dependency != parent)
            });
        }
        for (child, parent) in parents {
            ir.model
                .set_feature_regeneration_parent(child, parent)
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        Ok(())
    }

    pub(crate) fn feature_parent_count(&self, ir: &CadIr, native: &CatiaNative) -> usize {
        self.feature_parents(ir, native).len()
    }

    fn feature_parents(&self, ir: &CadIr, native: &CatiaNative) -> HashMap<FeatureId, FeatureId> {
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();
        let feature_ordinals = ir
            .model
            .features
            .iter()
            .map(|feature| (feature.id.clone(), feature.ordinal))
            .collect::<HashMap<_, _>>();
        let mut parents = ir
            .model
            .features
            .iter()
            .filter_map(|feature| {
                let native_ref = feature.native_ref.as_deref()?;
                let object = design_objects.get(native_ref)?;
                let parent_object = object.owner_design_object.as_deref()?;
                let parent = nearest_feature_for_design_object(
                    parent_object,
                    &design_objects,
                    &self.feature_ids,
                )?;
                let parent_ordinal = feature_ordinals.get(&parent)?;
                (parent != feature.id && *parent_ordinal < feature.ordinal)
                    .then(|| (feature.id.clone(), parent))
            })
            .collect::<HashMap<_, _>>();
        let acyclic = parents
            .keys()
            .filter(|feature| feature_parent_chain_is_acyclic(feature, &parents))
            .cloned()
            .collect::<HashSet<_>>();
        parents.retain(|feature, _| acyclic.contains(feature));
        parents
    }

    /// Bind exact payload references to earlier transferred features as
    /// structural dependencies. A target may resolve through its complete
    /// owner-design-object chain. Storage selectors, unresolved targets,
    /// self-links, and forward targets do not establish history edges.
    pub(crate) fn assign_feature_dependencies(&self, ir: &mut CadIr, native: &CatiaNative) {
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<HashMap<_, _>>();
        let feature_ordinals = ir
            .model
            .features
            .iter()
            .map(|feature| (feature.id.clone(), feature.ordinal))
            .collect::<HashMap<_, _>>();
        let mut dependencies_by_feature = HashMap::<FeatureId, Vec<FeatureId>>::new();

        for feature in &ir.model.features {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some(object) = design_objects.get(native_ref) else {
                continue;
            };
            let mut seen = feature.dependencies.iter().cloned().collect::<HashSet<_>>();
            for relation in &object.relations {
                if !matches!(
                    &relation.source,
                    CatiaDesignObjectRelationSource::Payload { .. }
                ) {
                    continue;
                }
                let Some(target_object) = relation.target_design_object.as_deref() else {
                    continue;
                };
                let Some(target) = nearest_feature_for_design_object(
                    target_object,
                    &design_objects,
                    &self.feature_ids,
                ) else {
                    continue;
                };
                if target == feature.id || seen.contains(&target) {
                    continue;
                }
                let Some(target_ordinal) = feature_ordinals.get(&target) else {
                    continue;
                };
                if *target_ordinal >= feature.ordinal {
                    continue;
                }
                seen.insert(target.clone());
                dependencies_by_feature
                    .entry(feature.id.clone())
                    .or_default()
                    .push(target.clone());
            }
        }

        for feature in &mut ir.model.features {
            if let Some(dependencies) = dependencies_by_feature.remove(&feature.id) {
                feature.dependencies.extend(dependencies);
            }
        }
    }
}

fn assign_feature_parameter_ordinals(
    ir: &mut CadIr,
    entities: &HashMap<&str, &crate::native::entity_record::CatiaEntityRecord>,
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

/// Normalize the document scope after feature ownership is known.
///
/// Formula transfer assigns a source-order ordinal before ownership can be
/// established. Feature-owned parameters receive a feature-local ordinal
/// above; document parameters must receive their own contiguous scope instead
/// of retaining gaps left by those feature parameters.
fn assign_document_parameter_ordinals(ir: &mut CadIr) {
    let mut parameters = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
        .map(|parameter| (parameter.ordinal, parameter.id.clone()))
        .collect::<Vec<_>>();
    parameters.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    let parameter_ordinals = parameters
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, (_, parameter))| Some((parameter, u32::try_from(ordinal).ok()?)))
        .collect::<HashMap<_, _>>();
    for parameter in &mut ir.model.parameters {
        if let Some(ordinal) = parameter_ordinals.get(&parameter.id) {
            parameter.ordinal = *ordinal;
        }
    }
}

/// Expose exact feature-owned parameter expressions without assigning
/// operation-specific roles. The map uses the neutral, scope-unique parameter
/// names assigned by [`normalize_parameter_names`]. A changed name retains its
/// source spelling in the corresponding parameter's `source_name` property.
/// Typed unresolved operation families retain the same expressions in feature
/// source properties because their neutral definitions have no generic
/// parameter map.
fn assign_native_operation_parameter_values(
    ir: &mut CadIr,
    exact_feature_owners: &HashMap<ParameterId, FeatureId>,
) {
    let mut values_by_feature =
        HashMap::<FeatureId, BTreeMap<cadmpeg_core::text::NonBlankString, String>>::new();
    for parameter in &ir.model.parameters {
        let Some(feature_id) = exact_feature_owners.get(&parameter.id) else {
            continue;
        };
        let Some(name) = cadmpeg_core::text::NonBlankString::new(parameter.name.clone()) else {
            continue;
        };
        values_by_feature
            .entry(feature_id.clone())
            .or_default()
            .insert(name, parameter.expression.clone());
    }

    for feature in &mut ir.model.features {
        let Some(values) = values_by_feature.remove(&feature.id) else {
            continue;
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters }) => {
                if parameters.is_empty() {
                    feature
                        .evaluation
                        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: kind.clone(),
                            parameters: values,
                        }));
                }
            }
            FeatureDefinition::Operation(
                FeatureOperation::Unresolved {
                    family:
                        UnresolvedFamily::Extrude | UnresolvedFamily::Revolve | UnresolvedFamily::Fillet,
                }
                | FeatureOperation::Pattern { .. }
                | FeatureOperation::Sweep { .. },
            ) => {
                for (name, expression) in values {
                    feature.source_properties.insert(
                        cadmpeg_core::nonblank_literal!("catia_parameter_{name}"),
                        expression,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Give every neutral parameter a unique name within its ownership scope.
///
/// Keep the first source name; suffix later collisions. Reserve every source
/// name before choosing a suffix. Original spelling stays in
/// `properties["source_name"]` when the neutral name changes.
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
        let reserved = reserved_by_scope.entry(scope.clone()).or_default();
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
            .insert(cadmpeg_core::nonblank_literal!("source_name"), source_name);
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

/// Resolve the nearest transferred feature on one exact owner chain.
///
/// Structural groups may sit between a native feature object and another
/// feature object. Stop at the first transferred feature so a nested feature
/// keeps its immediate structural parent. A missing link or a cycle rejects
/// the chain instead of inferring a relationship from field vocabulary.
fn nearest_feature_for_design_object(
    start: &str,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    feature_ids: &HashMap<String, FeatureId>,
) -> Option<FeatureId> {
    let mut current = Some(start);
    let mut visited = HashSet::new();

    while let Some(current_id) = current {
        if !visited.insert(current_id) {
            return None;
        }
        let object = design_objects.get(current_id).copied()?;
        if let Some(feature) = feature_ids.get(current_id) {
            return Some(feature.clone());
        }
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }

    None
}

/// Transfer exact owner-bound reference history nodes.
pub(crate) fn transfer_design_features(
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<DesignFeatureTransfer, cadmpeg_core::CodecError> {
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
    let design_objects = native
        .design_objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<HashMap<_, _>>();
    let native_operation_object_ids = native
        .design_objects
        .iter()
        .filter(|object| native_operation_candidate(object, &records).is_some())
        .map(|object| object.id.as_str())
        .collect::<HashSet<_>>();
    let mut transfer = DesignFeatureTransfer::default();

    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.contains(object.parent.as_str()))
    {
        let plane_candidate = principal_plane_candidate(object, &records);
        let sketch_owner = sketch_candidate(object, &records);
        let reference_plane = reference_plane_candidate(object, &records);
        let native_operation = native_operation_candidate(object, &records);
        match (
            plane_candidate,
            sketch_owner,
            reference_plane,
            native_operation,
        ) {
            (Some(candidate), None, None, None) => {
                transfer_principal_plane(ir, &mut transfer, candidate)?;
            }
            (None, Some(owner_record), None, None) => {
                transfer_sketch(ir, &mut transfer, object, owner_record);
            }
            (None, None, Some(candidate), None) => {
                transfer_reference_plane(ir, &mut transfer, &candidate)?;
            }
            (None, None, None, Some(candidate)) => {
                transfer_native_operation(
                    ir,
                    &mut transfer,
                    &candidate,
                    &records,
                    &entities,
                    &design_objects,
                    &native_operation_object_ids,
                )?;
            }
            _ => {
                // One object cannot safely occupy two neutral feature identities.
                // Leave all declarations unresolved so the feature-id map cannot
                // overwrite one transfer with another.
            }
        }
    }

    transfer.assign_feature_parents(ir, native)?;
    transfer.assign_feature_dependencies(ir, native);
    Ok(transfer)
}

fn transfer_principal_plane(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: PrincipalPlaneCandidate<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let object = candidate.object;
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(candidate.declaration_class.to_string()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: candidate.plane,
            }),
        ),
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer.principal_plane_records.extend(
        candidate
            .declarations
            .into_iter()
            .map(|record| record.id.clone()),
    );
    Ok(())
}

fn transfer_reference_plane(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &ReferencePlaneCandidate<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let object = candidate.object;
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(candidate.kind.to_string()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            }),
        ),
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer
        .reference_plane_records
        .insert(candidate.owner_record.id.clone());
    Ok(())
}

fn transfer_sketch(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    object: &CatiaDesignObject,
    owner_record: &CatiaObjectRecord,
) {
    let Ok(identity) = cadmpeg_ir::ids::Identity::new(object.id.clone()) else {
        return;
    };
    let sketch_id = SketchId::from(identity.with_kind(&cadmpeg_ir::identity_component!("sketch")));
    let feature_id =
        FeatureId::from(identity.with_kind(&cadmpeg_ir::identity_component!("feature")));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(object.id.clone()),
    });
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some("Sketch".to_string()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
            }),
        ),
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
    kind: NativeOperationClass,
}

struct ReferencePlaneCandidate<'a> {
    object: &'a CatiaDesignObject,
    owner_record: &'a CatiaObjectRecord,
    kind: &'a str,
}

fn reference_plane_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<ReferencePlaneCandidate<'a>> {
    let owner_class = object.owner_class.as_ref()?;
    is_admitted_native_reference_plane_class(&owner_class.name).then_some(())?;
    let owner_record_id = object.owner_record.as_deref()?;
    let owner_record = records.get(owner_record_id).copied()?;
    (owner_record.class_name() == Some(owner_class.name.as_str())
        && owner_record.class_entry() == Some(owner_class.entry.as_str())
        && owner_record.entity_id() == Some(object.owner_entity_id)
        && owner_record.design_object.as_deref() == object.owner_design_object.as_deref())
    .then_some(ReferencePlaneCandidate {
        object,
        owner_record,
        kind: owner_class.name.as_str(),
    })
}

fn native_operation_candidate<'a>(
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Option<NativeOperationCandidate<'a>> {
    let owner_record_id = object.owner_record.as_deref()?;
    let owner_record = records.get(owner_record_id).copied()?;
    // `owner_class` is populated only by a complete separator-form owner
    // declaration. A compact root's class entry remains field vocabulary.
    let owner_class = object.owner_class.as_ref()?;
    let owner_class_name = owner_class.name.as_str();
    let owner_class_entry = owner_class.entry.as_str();
    let kind = NativeOperationClass::try_from(owner_class_name).ok()?;
    (owner_record.class_name() == Some(owner_class_name)
        && owner_record.class_entry() == Some(owner_class_entry)
        && owner_record.entity_id() == Some(object.owner_entity_id)
        && owner_record.design_object.as_deref() == object.owner_design_object.as_deref())
    .then_some(NativeOperationCandidate {
        object,
        owner_record,
        kind,
    })
}

/// Admitted native operation class.
#[derive(Clone, Copy)]
pub(crate) enum NativeOperationClass {
    EdgeFillet,
    PrismEndLimitLength,
    PrismThickThin1,
    PrismThickThin2,
    RevolThickThin1,
    CircPatternRadialNumber,
    SweepThickThin1,
}

impl TryFrom<&str> for NativeOperationClass {
    type Error = ();

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        match name {
            "EdgeFillet" => Ok(Self::EdgeFillet),
            "Prism_EndLimit_Length" => Ok(Self::PrismEndLimitLength),
            "Prism_ThickThin1" => Ok(Self::PrismThickThin1),
            "Prism_ThickThin2" => Ok(Self::PrismThickThin2),
            "Revol_ThickThin1" => Ok(Self::RevolThickThin1),
            "CircPattern_RadialNumber" => Ok(Self::CircPatternRadialNumber),
            "Sweep_ThickThin1" => Ok(Self::SweepThickThin1),
            _ => Err(()),
        }
    }
}

impl NativeOperationClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::EdgeFillet => "EdgeFillet",
            Self::PrismEndLimitLength => "Prism_EndLimit_Length",
            Self::PrismThickThin1 => "Prism_ThickThin1",
            Self::PrismThickThin2 => "Prism_ThickThin2",
            Self::RevolThickThin1 => "Revol_ThickThin1",
            Self::CircPatternRadialNumber => "CircPattern_RadialNumber",
            Self::SweepThickThin1 => "Sweep_ThickThin1",
        }
    }
}

pub(crate) fn is_admitted_native_reference_plane_class(name: &str) -> bool {
    matches!(name, "GSMPlaneAngle" | "GSMPlaneOffset")
}

fn transfer_native_operation(
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &NativeOperationCandidate<'_>,
    object_records: &HashMap<&str, &CatiaObjectRecord>,
    entities: &HashMap<&str, &CatiaEntityRecord>,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    native_operation_object_ids: &HashSet<&str>,
) -> Result<(), cadmpeg_core::CodecError> {
    let object = candidate.object;
    let kind = candidate.kind;
    let NativeOperationDefinitionProperties {
        source_properties,
        definition_value_count,
        definition_chain_value_count,
        range_count,
        definition_value_records,
        definition_chain_value_records,
        range_records,
    } = native_operation_definition_properties(
        object,
        object_records,
        entities,
        design_objects,
        native_operation_object_ids,
    );
    let definition = native_operation_definition(kind, &object.id);
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties,
        source_tag: Some(kind.as_str().to_string()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(object.id.clone()),
    });
    transfer.feature_ids.insert(object.id.clone(), feature_id);
    transfer
        .native_operation_records
        .insert(candidate.owner_record.id.clone());
    transfer.native_operation_definition_value_count += definition_value_count;
    transfer.native_operation_definition_chain_value_count += definition_chain_value_count;
    transfer.native_operation_range_count += range_count;
    transfer
        .native_operation_definition_value_records
        .extend(definition_value_records);
    transfer
        .native_operation_definition_chain_value_records
        .extend(definition_chain_value_records);
    transfer
        .native_operation_range_records
        .extend(range_records);
    Ok(())
}

/// Project an admitted CATIA operation class into the neutral family while
/// keeping every unresolved operand explicit. The exact owner declaration
/// proves the family identity; it does not prove profile, axis, extent,
/// result, edge group, pattern seed, pattern axis, pattern angle, pattern
/// count, or operation-specific dependency roles.
fn native_operation_definition(kind: NativeOperationClass, native_ref: &str) -> FeatureDefinition {
    match kind {
        NativeOperationClass::PrismEndLimitLength
        | NativeOperationClass::PrismThickThin1
        | NativeOperationClass::PrismThickThin2 => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Extrude,
            })
        }
        NativeOperationClass::RevolThickThin1 => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Revolve,
            })
        }
        NativeOperationClass::CircPatternRadialNumber => {
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            })
        }
        NativeOperationClass::SweepThickThin1 => {
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape: cadmpeg_ir::features::SweepShape::unresolved(Some(native_ref.to_string())),
                path: Some(cadmpeg_ir::features::PathRef::Unresolved(
                    native_ref.to_string(),
                )),
                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                path_extent: None,
                guide_rail: None,
                taper: None,
                scale: None,
                allow_multi_profile_faces: None,
            })
        }
        NativeOperationClass::EdgeFillet => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Fillet,
            })
        }
    }
}

/// Exact source properties and records retained for one native operation.
struct NativeOperationDefinitionProperties {
    source_properties: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    definition_value_count: usize,
    definition_chain_value_count: usize,
    range_count: usize,
    definition_value_records: HashSet<String>,
    definition_chain_value_records: HashSet<String>,
    range_records: HashSet<String>,
}

/// Retain complete definition-bound values and source-schema `Range` fields
/// on the exact native operation owner chain. A one-definition value carries
/// a source definition and typed suffix payload, while a two-definition value
/// carries its repeated selector, role, and selected payload. Neither
/// production assigns an operation role here. Supported two-definition roles
/// are exposed separately through the typed-parameter transfer.
fn native_operation_definition_properties(
    object: &CatiaDesignObject,
    object_records: &HashMap<&str, &CatiaObjectRecord>,
    entities: &HashMap<&str, &CatiaEntityRecord>,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    native_operation_object_ids: &HashSet<&str>,
) -> NativeOperationDefinitionProperties {
    let mut properties = BTreeMap::new();
    let mut definition_value_count = 0;
    let mut definition_chain_value_count = 0;
    let mut range_count = 0;
    let mut definition_value_records = HashSet::new();
    let mut definition_chain_value_records = HashSet::new();
    let mut range_records = HashSet::new();

    let owned_objects = design_objects
        .values()
        .filter(|candidate| {
            candidate.parent == object.parent
                && native_operation_owner_chain_reaches(
                    candidate.id.as_str(),
                    object.id.as_str(),
                    design_objects,
                    native_operation_object_ids,
                )
        })
        .copied()
        .collect::<Vec<_>>();
    let mut definition_values = owned_objects
        .iter()
        .flat_map(|owned| owned.definition_values.iter())
        .filter_map(|entity_id| entities.get(entity_id.as_str()).copied())
        .filter_map(|entity| entity.definition_value().map(|value| (entity, value)))
        .collect::<Vec<_>>();
    definition_values.sort_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    for (ordinal, (entity, value)) in definition_values.into_iter().enumerate() {
        definition_value_count += 1;
        definition_value_records.insert(entity.object_record.clone());
        let prefix = cadmpeg_core::nonblank_literal!("catia_definition_value_{ordinal}");
        properties.insert(prefix.with_suffix("_entity"), entity.id.clone());
        insert_schema_value_properties(
            &mut properties,
            &prefix.with_suffix("_definition"),
            &value.definition,
        );
        insert_suffix_payload_properties(
            &mut properties,
            &prefix.with_suffix("_payload"),
            &value.payload,
        );
        if let Some(selection) = value.schema_selection.as_ref() {
            insert_schema_selection_properties(
                &mut properties,
                &prefix.with_suffix("_schema_selection"),
                selection,
            );
        }
    }

    let mut definition_chain_values = owned_objects
        .iter()
        .flat_map(|owned| owned.definition_chain_values.iter())
        .filter_map(|entity_id| entities.get(entity_id.as_str()).copied())
        .filter_map(|entity| entity.definition_chain_value().map(|value| (entity, value)))
        .collect::<Vec<_>>();
    definition_chain_values.sort_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    for (ordinal, (entity, value)) in definition_chain_values.into_iter().enumerate() {
        definition_chain_value_count += 1;
        definition_chain_value_records.insert(entity.object_record.clone());
        let prefix = cadmpeg_core::nonblank_literal!("catia_definition_chain_value_{ordinal}");
        properties.insert(prefix.with_suffix("_entity"), entity.id.clone());
        insert_schema_value_properties(
            &mut properties,
            &prefix.with_suffix("_selector"),
            &value.selector,
        );
        insert_schema_value_properties(&mut properties, &prefix.with_suffix("_role"), &value.role);
        insert_schema_selected_value_properties(
            &mut properties,
            &prefix.with_suffix("_value"),
            &value.value,
        );
    }

    let mut range_intervals = owned_objects
        .iter()
        .flat_map(|owned| {
            owned.fields.iter().filter_map(|field_id| {
                let field = object_records.get(field_id.as_str()).copied()?;
                (field.design_object.as_deref() == Some(owned.id.as_str())).then_some(field)
            })
        })
        .filter_map(|field| {
            let entity_id = field.entity_record()?;
            let entity = entities.get(entity_id).copied()?;
            (entity.object_record == field.id).then_some(())?;
            entity.range_interval.as_ref().map(|range| (entity, range))
        })
        .collect::<Vec<_>>();
    range_intervals.sort_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    range_intervals.dedup_by(|(left, _), (right, _)| left.id == right.id);
    for (ordinal, (entity, range)) in range_intervals.into_iter().enumerate() {
        range_count += 1;
        range_records.insert(entity.object_record.clone());
        let prefix = cadmpeg_core::nonblank_literal!("catia_range_{ordinal}");
        properties.insert(prefix.with_suffix("_entity"), entity.id.clone());
        insert_range_interval_properties(&mut properties, &prefix, range);
    }

    NativeOperationDefinitionProperties {
        source_properties: properties,
        definition_value_count,
        definition_chain_value_count,
        range_count,
        definition_value_records,
        definition_chain_value_records,
        range_records,
    }
}

/// Return whether a design object belongs to one operation's exact structural
/// owner chain. A nearer admitted operation owns the value instead of an
/// outer operation. Missing links and non-reflexive cycles reject the whole
/// chain so a partial owner path cannot invent feature properties.
fn native_operation_owner_chain_reaches(
    design_object_id: &str,
    operation_object_id: &str,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    native_operation_object_ids: &HashSet<&str>,
) -> bool {
    let mut current = Some(design_object_id);
    let mut visited = HashSet::new();
    while let Some(current_id) = current {
        if !visited.insert(current_id) {
            return false;
        }
        if current_id == operation_object_id {
            return true;
        }
        if native_operation_object_ids.contains(current_id) {
            return false;
        }
        let Some(object) = design_objects.get(current_id).copied() else {
            return false;
        };
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }
    false
}

fn insert_schema_value_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySchemaValue,
) {
    properties.insert(prefix.with_suffix("_entry"), value.entry.clone());
    properties.insert(prefix.with_suffix("_ordinal"), value.ordinal.to_string());
    properties.insert(prefix.with_suffix("_offset"), value.offset.to_string());
    properties.insert(prefix.with_suffix("_value"), value.value.clone());
}

fn insert_range_interval_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    range: &CatiaRangeInterval,
) {
    insert_schema_value_properties(properties, &prefix.with_suffix("_selector"), &range.range);
    match &range.interval.prefix {
        RangeIntervalPrefix::Compact { value, width } => {
            properties.insert(prefix.with_suffix("_prefix_kind"), "compact".to_string());
            properties.insert(prefix.with_suffix("_prefix_value"), value.to_string());
            properties.insert(prefix.with_suffix("_prefix_width"), width.to_string());
        }
        RangeIntervalPrefix::EscapedWord { word } => {
            properties.insert(
                prefix.with_suffix("_prefix_kind"),
                "escaped_word".to_string(),
            );
            properties.insert(prefix.with_suffix("_prefix_word"), word.to_string());
        }
    }
    match &range.interval.slots {
        Some([lower, upper]) => {
            properties.insert(prefix.with_suffix("_slots"), "two".to_string());
            insert_range_slot_properties(properties, &prefix.with_suffix("_lower"), lower);
            insert_range_slot_properties(properties, &prefix.with_suffix("_upper"), upper);
        }
        None => {
            properties.insert(prefix.with_suffix("_slots"), "none".to_string());
        }
    }
    if let Some(nominal) = range.nominal.as_ref() {
        properties.insert(prefix.with_suffix("_nominal_kind"), "finite".to_string());
        properties.insert(
            prefix.with_suffix("_nominal_framing"),
            range_nominal_framing_name(nominal.framing).to_string(),
        );
        properties.insert(
            prefix.with_suffix("_nominal_bits"),
            format!("{:016x}", nominal.bits),
        );
        properties.insert(
            prefix.with_suffix("_nominal_opcode_offset"),
            nominal.evaluation_opcode_offset.to_string(),
        );
    } else {
        properties.insert(prefix.with_suffix("_nominal_kind"), "absent".to_string());
    }
    properties.insert(
        prefix.with_suffix("_incoming_payload_reference_count"),
        range.incoming_references.len().to_string(),
    );
    properties.insert(
        prefix.with_suffix("_incoming_storage_reference_count"),
        range.incoming_storage_references.len().to_string(),
    );
}

fn insert_range_slot_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    slot: &RangeIntervalSlot,
) {
    match slot {
        RangeIntervalSlot::Binary64 { bits, offset } => {
            properties.insert(prefix.with_suffix("_kind"), "binary64".to_string());
            properties.insert(prefix.with_suffix("_bits"), format!("{bits:016x}"));
            properties.insert(prefix.with_suffix("_offset"), offset.to_string());
        }
        RangeIntervalSlot::Unset { offset } => {
            properties.insert(prefix.with_suffix("_kind"), "unset".to_string());
            properties.insert(prefix.with_suffix("_offset"), offset.to_string());
        }
    }
}

fn range_nominal_framing_name(framing: CatiaRangeNominalFraming) -> &'static str {
    match framing {
        CatiaRangeNominalFraming::D8Token8193 => "D8Token8193",
        CatiaRangeNominalFraming::D8Token81DB => "D8Token81DB",
        CatiaRangeNominalFraming::DCToken81DB => "DCToken81DB",
        CatiaRangeNominalFraming::DFToken8192 => "DFToken8192",
    }
}

fn insert_suffix_payload_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    payload: &crate::native::CatiaEntitySuffixPayload,
) {
    match payload {
        crate::native::CatiaEntitySuffixPayload::Evaluation {
            opcode_offset,
            evaluation,
            encoding,
        } => {
            properties.insert(prefix.with_suffix("_kind"), "evaluation".to_string());
            properties.insert(
                prefix.with_suffix("_opcode_offset"),
                opcode_offset.to_string(),
            );
            properties.insert(
                prefix.with_suffix("_encoding"),
                evaluation_encoding_name(*encoding).to_string(),
            );
            insert_evaluation_properties(properties, prefix, evaluation);
        }
        crate::native::CatiaEntitySuffixPayload::Atom { value } => {
            properties.insert(prefix.with_suffix("_kind"), "atom".to_string());
            properties.insert(prefix.with_suffix("_value"), value.to_string());
        }
        crate::native::CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset,
            selector,
            value,
        } => {
            properties.insert(prefix.with_suffix("_kind"), "schema_selected".to_string());
            properties.insert(
                prefix.with_suffix("_selector_offset"),
                selector_offset.to_string(),
            );
            properties.insert(prefix.with_suffix("_selector"), selector.to_string());
            insert_selected_value_properties(properties, &prefix.with_suffix("_selected"), value);
        }
        crate::native::CatiaEntitySuffixPayload::ControlE8 => {
            properties.insert(prefix.with_suffix("_kind"), "control_e8".to_string());
        }
        crate::native::CatiaEntitySuffixPayload::ControlE9 => {
            properties.insert(prefix.with_suffix("_kind"), "control_e9".to_string());
        }
        crate::native::CatiaEntitySuffixPayload::Separator37 => {
            properties.insert(prefix.with_suffix("_kind"), "separator_37".to_string());
        }
    }
}

fn insert_selected_value_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySuffixSelectedValue,
) {
    match value {
        crate::native::CatiaEntitySuffixSelectedValue::Atom { value } => {
            properties.insert(prefix.with_suffix("_kind"), "atom".to_string());
            properties.insert(prefix.with_suffix("_value"), value.to_string());
        }
        crate::native::CatiaEntitySuffixSelectedValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            properties.insert(prefix.with_suffix("_kind"), "evaluation".to_string());
            properties.insert(
                prefix.with_suffix("_opcode_offset"),
                opcode_offset.to_string(),
            );
            insert_evaluation_properties(properties, prefix, evaluation);
        }
        crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
            properties.insert(prefix.with_suffix("_kind"), "control_e8".to_string());
        }
        crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
            properties.insert(prefix.with_suffix("_kind"), "separator_37".to_string());
        }
        crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector {
            offset, ordinal, ..
        } => {
            properties.insert(prefix.with_suffix("_kind"), "schema_selector".to_string());
            properties.insert(prefix.with_suffix("_offset"), offset.to_string());
            properties.insert(prefix.with_suffix("_ordinal"), ordinal.to_string());
        }
    }
}

fn insert_schema_selection_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    selection: &crate::native::CatiaEntitySuffixSchemaSelection,
) {
    properties.insert(prefix.with_suffix("_offset"), selection.offset.to_string());
    properties.insert(
        prefix.with_suffix("_ordinal"),
        selection.ordinal.to_string(),
    );
    properties.insert(prefix.with_suffix("_entry"), selection.entry.clone());
    properties.insert(prefix.with_suffix("_name"), selection.name.clone());
    insert_schema_selected_value_properties(
        properties,
        &prefix.with_suffix("_value"),
        &selection.value,
    );
}

fn insert_schema_selected_value_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySuffixSchemaValue,
) {
    match value {
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value } => {
            properties.insert(prefix.with_suffix("_kind"), "atom".to_string());
            properties.insert(prefix.with_suffix("_atom"), value.to_string());
        }
        crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            properties.insert(prefix.with_suffix("_kind"), "evaluation".to_string());
            properties.insert(
                prefix.with_suffix("_opcode_offset"),
                opcode_offset.to_string(),
            );
            insert_evaluation_properties(properties, prefix, evaluation);
        }
        crate::native::CatiaEntitySuffixSchemaValue::ControlE8 => {
            properties.insert(prefix.with_suffix("_kind"), "control_e8".to_string());
        }
        crate::native::CatiaEntitySuffixSchemaValue::Separator37 => {
            properties.insert(prefix.with_suffix("_kind"), "separator_37".to_string());
        }
        crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset,
            ordinal,
            resolution,
        } => {
            properties.insert(prefix.with_suffix("_kind"), "schema_selector".to_string());
            properties.insert(prefix.with_suffix("_offset"), offset.to_string());
            properties.insert(prefix.with_suffix("_ordinal"), ordinal.to_string());
            if let Some(class) = resolution {
                properties.insert(prefix.with_suffix("_entry"), class.entry.clone());
                properties.insert(prefix.with_suffix("_name"), class.name.clone());
            }
        }
    }
}

fn insert_evaluation_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    evaluation: &crate::native::CatiaEntityEvaluation,
) {
    match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            properties.insert(prefix.with_suffix("_evaluation"), "unset".to_string());
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => {
            properties.insert(prefix.with_suffix("_evaluation"), "scalar".to_string());
            properties.insert(
                prefix.with_suffix("_evaluation_bits"),
                format!("{bits:016x}"),
            );
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
    let class_name = first.class_name()?;
    let class_entry = first.class_entry()?;
    let plane = principal_plane(class_name)?;
    declarations
        .iter()
        .all(|record| {
            record.class_name() == Some(class_name)
                && record.class_entry() == Some(class_entry)
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
    (owner_record.class_name() == Some("Sketch")
        && owner_record.class_entry() == Some(owner_class.entry.as_str())
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
        && record.storage_ref().is_none()
        && record.references.is_empty()
        && record.subtype() == PayloadSubtype::Empty
        && record.payload.size == 1
        && record.payload.fields == [PayloadField::Terminator]
}

#[cfg(test)]
mod tests;
