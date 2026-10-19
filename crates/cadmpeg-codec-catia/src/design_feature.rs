// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
    PrincipalPlane, UnresolvedFamily,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::chain_walk::{ChainStep, ChainStops};
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
    pub(crate) fn consumed_records(&self) -> impl Iterator<Item = &String> {
        self.principal_plane_records
            .union(&self.sketch_owner_records)
            .chain(self.reference_plane_records.iter())
            .chain(self.native_operation_records.iter())
            .chain(self.native_operation_definition_value_records.iter())
            .chain(self.native_operation_definition_chain_value_records.iter())
            .chain(self.native_operation_range_records.iter())
    }

    /// Bind parameters to a transferred feature only through their exact
    /// entity-record and object-record ownership chain. The same exact
    /// incidences populate feature-local parameter ordinals.
    pub(crate) fn assign_parameter_owners(
        &self,
        ir: &mut CadIr,
        native: &CatiaNative,
    ) -> Result<(), CodecError> {
        let entities = native
            .entity_records
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect::<std::collections::HashMap<_, _>>();
        let object_records = native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .map(|record| (record.id.as_str(), record))
            .collect::<std::collections::HashMap<_, _>>();
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<std::collections::HashMap<_, _>>();
        let mut exact_feature_owners = HashMap::new();
        let mut nearest = nearest_features(&design_objects, &self.feature_ids);

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
            let Some(feature_id) = nearest.of(design_object) else {
                continue;
            };
            if parameter.owner.is_none() {
                parameter.owner = Some(feature_id.clone());
            }
            if parameter.owner.as_ref() == Some(feature_id) {
                let parameter_id = parameter.id.clone();
                let feature_id = feature_id.clone();
                exact_feature_owners.insert(parameter_id, feature_id);
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
        assign_native_operation_parameter_values(ir, &exact_feature_owners)?;
        Ok(())
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
    fn assign_feature_parents(
        &self,
        ir: &mut CadIr,
        native: &CatiaNative,
    ) -> Result<(), CodecError> {
        let parents = self.feature_parents(ir, native);
        for feature in &mut ir.model.features {
            feature.dependencies.retain(|dependency| {
                parents
                    .get(&feature.id)
                    .is_none_or(|parent| dependency != parent)
            });
        }
        for (child, parent) in parents {
            ir.model.set_feature_regeneration_parent(&child, &parent)?;
        }
        Ok(())
    }

    fn feature_parents(&self, ir: &CadIr, native: &CatiaNative) -> HashMap<FeatureId, FeatureId> {
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<std::collections::HashMap<_, _>>();
        let feature_ordinals = ir
            .model
            .features
            .iter()
            .map(|feature| (&feature.id, feature.ordinal))
            .collect::<std::collections::HashMap<_, _>>();
        let mut parents = HashMap::new();
        let mut nearest = nearest_features(&design_objects, &self.feature_ids);
        for feature in &ir.model.features {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some(object) = design_objects.get(native_ref) else {
                continue;
            };
            let Some(parent_object) = object.owner_design_object.as_deref() else {
                continue;
            };
            let Some(parent) = nearest.of(parent_object) else {
                continue;
            };
            let Some(parent_ordinal) = feature_ordinals.get(parent) else {
                continue;
            };
            if parent == &feature.id || *parent_ordinal >= feature.ordinal {
                continue;
            }
            let child = feature.id.clone();
            let parent = parent.clone();
            parents.insert(child, parent);
        }
        for feature in cyclic_feature_parents(&parents) {
            parents.remove(&feature);
        }
        parents
    }

    /// Bind exact payload references to earlier transferred features as
    /// structural dependencies. A target may resolve through its complete
    /// owner-design-object chain. Storage selectors, unresolved targets,
    /// self-links, and forward targets do not establish history edges.
    fn assign_feature_dependencies(&self, ir: &mut CadIr, native: &CatiaNative) {
        let design_objects = native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object))
            .collect::<std::collections::HashMap<_, _>>();
        let feature_ordinals = ir
            .model
            .features
            .iter()
            .map(|feature| (&feature.id, feature.ordinal))
            .collect::<std::collections::HashMap<_, _>>();
        let mut dependencies_by_feature = HashMap::<FeatureId, Vec<FeatureId>>::new();
        let mut nearest = nearest_features(&design_objects, &self.feature_ids);

        for feature in &ir.model.features {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some(object) = design_objects.get(native_ref) else {
                continue;
            };
            let mut seen = feature
                .dependencies
                .iter()
                .collect::<std::collections::HashSet<_>>();
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
                let Some(target) = nearest.of(target_object) else {
                    continue;
                };
                if target == &feature.id || seen.contains(target) {
                    continue;
                }
                let Some(target_ordinal) = feature_ordinals.get(target) else {
                    continue;
                };
                if *target_ordinal >= feature.ordinal {
                    continue;
                }
                seen.insert(target);
                if !dependencies_by_feature.contains_key(&feature.id) {
                    let id = feature.id.clone();
                    dependencies_by_feature.insert(id, Vec::new());
                }
                let target = target.clone();
                if let Some(dependencies) = dependencies_by_feature.get_mut(&feature.id) {
                    dependencies.push(target);
                }
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
    let transferred_features = feature_ids
        .values()
        .collect::<std::collections::HashSet<_>>();
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
        if !parameters_by_feature.contains_key(feature_id) {
            let id = feature_id.clone();
            parameters_by_feature.insert(id, Vec::new());
        }
        let id = parameter.id.clone();
        if let Some(parameters) = parameters_by_feature.get_mut(feature_id) {
            parameters.push((object_record.byte_offset, entity.byte_offset, id));
        }
    }

    let mut parameter_ordinals = HashMap::new();
    for parameters in parameters_by_feature.values_mut() {
        parameters.sort_unstable_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
        });
        for (ordinal, parameter) in parameters.iter().enumerate() {
            let Some(ordinal) = u32::try_from(ordinal).ok() else {
                continue;
            };
            let id = parameter.2.clone();
            parameter_ordinals.insert(id, ordinal);
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
    let mut parameters = Vec::new();
    for parameter in ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
    {
        let id = parameter.id.clone();
        parameters.push((parameter.ordinal, id));
    }
    parameters.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

    let mut parameter_ordinals = HashMap::new();
    for (ordinal, (_, parameter)) in parameters.into_iter().enumerate() {
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            continue;
        };
        parameter_ordinals.insert(parameter, ordinal);
    }
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
) -> Result<(), CodecError> {
    let mut values_by_feature =
        HashMap::<FeatureId, BTreeMap<cadmpeg_core::text::NonBlankString, String>>::new();
    for parameter in &ir.model.parameters {
        let Some(feature_id) = exact_feature_owners.get(&parameter.id) else {
            continue;
        };
        let Some(name) = cadmpeg_core::text::NonBlankString::new(parameter.name.clone()) else {
            continue;
        };
        if !values_by_feature.contains_key(feature_id) {
            let id = feature_id.clone();
            values_by_feature.insert(id, BTreeMap::new());
        }
        let expression = parameter.expression.clone();
        if let Some(values) = values_by_feature.get_mut(feature_id) {
            values.insert(name, expression);
        }
    }

    for feature in &mut ir.model.features {
        let Some(values) = values_by_feature.remove(&feature.id) else {
            continue;
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters }) => {
                if parameters.is_empty() {
                    let kind = match kind {
                        cadmpeg_ir::features::NativeFeatureKind::Other(name) => {
                            cadmpeg_ir::features::NativeFeatureKind::Other(name.to_owned())
                        }
                        other => other.clone(),
                    };
                    feature
                        .evaluation
                        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                            kind,
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
                    let key = format!("catia_parameter_{name}");
                    let key = cadmpeg_core::text::NonBlankString::new(key).ok_or_else(|| {
                        CodecError::malformed("CATIA source parameter key is blank")
                    })?;
                    feature.source_properties.insert(key, expression);
                }
            }
            _ => {}
        }
    }
    Ok(())
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
            if !reserved_by_scope.contains_key(&parameter.owner) {
                let scope = parameter.owner.clone();
                reserved_by_scope.insert(scope, HashSet::new());
            }
            let name = parameter.name.clone();
            if let Some(reserved) = reserved_by_scope.get_mut(&parameter.owner) {
                reserved.insert(name);
            }
        }
    }

    let mut used_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    for parameter in &mut ir.model.parameters {
        if !reserved_by_scope.contains_key(&parameter.owner) {
            let scope = parameter.owner.clone();
            reserved_by_scope.insert(scope, HashSet::new());
        }
        if !used_by_scope.contains_key(&parameter.owner) {
            let scope = parameter.owner.clone();
            used_by_scope.insert(scope, HashSet::new());
        }
        let Some(reserved) = reserved_by_scope.get(&parameter.owner) else {
            continue;
        };
        let Some(used) = used_by_scope.get_mut(&parameter.owner) else {
            continue;
        };
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
            suffix += 1;
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

/// Features whose parent chain reaches a cycle.
fn cyclic_feature_parents(parents: &HashMap<FeatureId, FeatureId>) -> Vec<FeatureId> {
    let mut chain_ends = ChainStops::new(|feature: &FeatureId| {
        parents
            .get(feature)
            .map_or(ChainStep::Stop, ChainStep::Next)
    });
    let mut cyclic = Vec::new();
    for feature in parents.keys() {
        if chain_ends.stop(feature).is_none() {
            cyclic.push(feature.clone());
        }
    }
    cyclic
}

/// Walk one exact owner-design-object chain to the first object that
/// `is_stop` accepts.
///
/// A missing object, a chain end or a cycle answers `None`.
pub(crate) fn owner_chain_step<'a>(
    id: &'a str,
    object: Option<&'a CatiaDesignObject>,
    is_stop: impl FnOnce(&str) -> bool,
) -> ChainStep<&'a str> {
    let Some(object) = object else {
        return ChainStep::End;
    };
    if is_stop(id) {
        return ChainStep::Stop;
    }
    object
        .owner_design_object
        .as_deref()
        .filter(|parent| *parent != id)
        .map_or(ChainStep::End, ChainStep::Next)
}

/// The nearest transferred feature on each exact owner chain.
///
/// Structural groups may sit between a native feature object and another
/// feature object. Stop at the first transferred feature so a nested feature
/// keeps its immediate structural parent. A missing link or a cycle rejects
/// the chain instead of inferring a relationship from field vocabulary.
struct NearestFeatures<'a, F> {
    stops: ChainStops<&'a str, F>,
    feature_ids: &'a HashMap<String, FeatureId>,
}

fn nearest_features<'a>(
    design_objects: &'a HashMap<&'a str, &'a CatiaDesignObject>,
    feature_ids: &'a HashMap<String, FeatureId>,
) -> NearestFeatures<'a, impl FnMut(&'a str) -> ChainStep<&'a str>> {
    NearestFeatures {
        stops: ChainStops::new(move |id: &'a str| {
            owner_chain_step(id, design_objects.get(id).copied(), |id| {
                feature_ids.contains_key(id)
            })
        }),
        feature_ids,
    }
}

impl<'a, F: FnMut(&'a str) -> ChainStep<&'a str>> NearestFeatures<'a, F> {
    fn of(&mut self, design_object: &'a str) -> Option<&'a FeatureId> {
        self.stops
            .stop(design_object)
            .and_then(|object| self.feature_ids.get(object))
    }
}

/// Transfer exact owner-bound reference history nodes.
pub(crate) fn transfer_design_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<DesignFeatureTransfer, cadmpeg_core::CodecError> {
    let records = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<std::collections::HashMap<_, _>>();
    let entities = native
        .entity_records
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<std::collections::HashMap<_, _>>();
    let design_objects = native
        .design_objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<std::collections::HashMap<_, _>>();
    let native_operation_object_ids = native
        .design_objects
        .iter()
        .filter(|object| native_operation_candidate(object, &records).is_some())
        .map(|object| object.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let owned_objects =
        native_operation_owned_objects(&design_objects, &native_operation_object_ids);
    let operation_sources = NativeOperationSources {
        object_records: &records,
        entities: &entities,
        owned_objects: &owned_objects,
    };
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
                transfer_principal_plane(ctx, ir, &mut transfer, candidate)?;
            }
            (None, Some(owner_record), None, None) => {
                transfer_sketch(ctx, ir, &mut transfer, object, owner_record)?;
            }
            (None, None, Some(candidate), None) => {
                transfer_reference_plane(ctx, ir, &mut transfer, &candidate)?;
            }
            (None, None, None, Some(candidate)) => {
                transfer_native_operation(ctx, ir, &mut transfer, &candidate, &operation_sources)?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: PrincipalPlaneCandidate<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let object = candidate.object;
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = feature_id.clone();
    let source_tag = candidate.declaration_class.to_owned();
    let native_ref = object.id.clone();
    ir.model.features.push(Feature {
        id: feature_id,
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(source_tag),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: candidate.plane,
            }),
        ),
        native_ref: Some(native_ref),
    });
    let map_key = object.id.clone();
    transfer.feature_ids.insert(map_key, map_feature_id);
    for record in candidate.declarations {
        let id = record.id.clone();
        transfer.principal_plane_records.insert(id);
    }
    Ok(())
}

fn transfer_reference_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &ReferencePlaneCandidate<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let object = candidate.object;
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = feature_id.clone();
    let source_tag = candidate.kind.to_owned();
    let native_ref = object.id.clone();
    ir.model.features.push(Feature {
        id: feature_id,
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(source_tag),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            }),
        ),
        native_ref: Some(native_ref),
    });
    let map_key = object.id.clone();
    transfer.feature_ids.insert(map_key, map_feature_id);
    let record = candidate.owner_record.id.clone();
    transfer.reference_plane_records.insert(record);
    Ok(())
}

fn transfer_sketch(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    object: &CatiaDesignObject,
    owner_record: &CatiaObjectRecord,
) -> Result<(), cadmpeg_core::CodecError> {
    let sketch_id = match neutral_history_id(&object.id, &cadmpeg_ir::identity_component!("sketch"))
    {
        Ok(id) => SketchId::from(id),
        Err(CodecError::Malformed(_)) => return Ok(()),
        Err(error) => return Err(error),
    };
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design sketch")?;
    let binding_sketch_id = sketch_id.clone();
    let sketch_ref = object.id.clone();
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(sketch_ref),
    });
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = feature_id.clone();
    let feature_ref = object.id.clone();
    let source_tag = "Sketch".to_owned();
    ir.model.features.push(Feature {
        id: feature_id,
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: Some(source_tag),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(binding_sketch_id)),
            }),
        ),
        native_ref: Some(feature_ref),
    });
    let map_key = object.id.clone();
    transfer.feature_ids.insert(map_key, map_feature_id);
    let record = owner_record.id.clone();
    transfer.sketch_owner_records.insert(record);
    Ok(())
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

fn is_admitted_native_reference_plane_class(name: &str) -> bool {
    matches!(name, "GSMPlaneAngle" | "GSMPlaneOffset")
}

struct NativeOperationSources<'a> {
    object_records: &'a HashMap<&'a str, &'a CatiaObjectRecord>,
    entities: &'a HashMap<&'a str, &'a CatiaEntityRecord>,
    owned_objects: &'a HashMap<&'a str, Vec<&'a CatiaDesignObject>>,
}

fn transfer_native_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &NativeOperationCandidate<'_>,
    sources: &NativeOperationSources<'_>,
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
        sources.object_records,
        sources.entities,
        sources.owned_objects,
    )?;
    let definition = native_operation_definition(kind, &object.id);
    let feature_id = FeatureId::from(neutral_history_id(
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = feature_id.clone();
    let source_tag = kind.as_str().to_owned();
    let native_ref = object.id.clone();
    ir.model.features.push(Feature {
        id: feature_id,
        ordinal: object.first_field_byte_offset,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties,
        source_tag: Some(source_tag),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native_ref),
    });
    let map_key = object.id.clone();
    transfer.feature_ids.insert(map_key, map_feature_id);
    let owner_id = candidate.owner_record.id.clone();
    transfer.native_operation_records.insert(owner_id);
    transfer.native_operation_definition_value_count += definition_value_count;
    transfer.native_operation_definition_chain_value_count += definition_chain_value_count;
    transfer.native_operation_range_count += range_count;
    for record in definition_value_records {
        transfer
            .native_operation_definition_value_records
            .insert(record);
    }
    for record in definition_chain_value_records {
        transfer
            .native_operation_definition_chain_value_records
            .insert(record);
    }
    for record in range_records {
        transfer.native_operation_range_records.insert(record);
    }
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
                shape: cadmpeg_ir::features::SweepShape::unresolved(Some(native_ref.to_owned())),
                path: Some(cadmpeg_ir::features::PathRef::Unresolved(
                    native_ref.to_owned(),
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
    owned_objects: &HashMap<&str, Vec<&CatiaDesignObject>>,
) -> Result<NativeOperationDefinitionProperties, CodecError> {
    let mut properties = BTreeMap::new();
    let mut definition_value_count = 0;
    let mut definition_chain_value_count = 0;
    let mut range_count = 0;
    let mut definition_value_records = HashSet::new();
    let mut definition_chain_value_records = HashSet::new();
    let mut range_records = HashSet::new();

    let owned_objects = owned_objects
        .get(object.id.as_str())
        .into_iter()
        .flatten()
        .copied()
        .filter(|candidate| candidate.parent == object.parent)
        .collect::<Vec<_>>();
    let mut definition_values = owned_objects
        .iter()
        .flat_map(|owned| owned.definition_values.iter())
        .filter_map(|entity_id| entities.get(entity_id.as_str()).copied())
        .filter_map(|entity| entity.definition_value().map(|value| (entity, value)))
        .collect::<Vec<_>>();
    definition_values.sort_unstable_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    for (ordinal, (entity, value)) in definition_values.into_iter().enumerate() {
        definition_value_count += 1;
        let record = entity.object_record.clone();
        definition_value_records.insert(record);
        let prefix = property_prefix_args(format_args!("catia_definition_value_{ordinal}"))?;
        insert_property(
            &mut properties,
            prefix.as_str(),
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_schema_value_properties(
            &mut properties,
            &property_prefix(prefix.as_str(), "_definition")?,
            &value.definition,
        )?;
        insert_suffix_payload_properties(
            &mut properties,
            &property_prefix(prefix.as_str(), "_payload")?,
            &value.payload,
        )?;
        if let Some(selection) = value.schema_selection.as_ref() {
            insert_schema_selection_properties(
                &mut properties,
                &property_prefix(prefix.as_str(), "_schema_selection")?,
                selection,
            )?;
        }
    }

    let mut definition_chain_values = owned_objects
        .iter()
        .flat_map(|owned| owned.definition_chain_values.iter())
        .filter_map(|entity_id| entities.get(entity_id.as_str()).copied())
        .filter_map(|entity| entity.definition_chain_value().map(|value| (entity, value)))
        .collect::<Vec<_>>();
    definition_chain_values.sort_unstable_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    for (ordinal, (entity, value)) in definition_chain_values.into_iter().enumerate() {
        definition_chain_value_count += 1;
        let record = entity.object_record.clone();
        definition_chain_value_records.insert(record);
        let prefix = property_prefix_args(format_args!("catia_definition_chain_value_{ordinal}"))?;
        insert_property(
            &mut properties,
            prefix.as_str(),
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_schema_value_properties(
            &mut properties,
            &property_prefix(prefix.as_str(), "_selector")?,
            &value.selector,
        )?;
        insert_schema_value_properties(
            &mut properties,
            &property_prefix(prefix.as_str(), "_role")?,
            &value.role,
        )?;
        insert_schema_selected_value_properties(
            &mut properties,
            &property_prefix(prefix.as_str(), "_value")?,
            &value.value,
        )?;
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
    range_intervals.sort_unstable_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    range_intervals.dedup_by(|(left, _), (right, _)| left.id == right.id);
    for (ordinal, (entity, range)) in range_intervals.into_iter().enumerate() {
        range_count += 1;
        let record = entity.object_record.clone();
        range_records.insert(record);
        let prefix = property_prefix_args(format_args!("catia_range_{ordinal}"))?;
        insert_property(
            &mut properties,
            prefix.as_str(),
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_range_interval_properties(&mut properties, &prefix, range)?;
    }

    Ok(NativeOperationDefinitionProperties {
        source_properties: properties,
        definition_value_count,
        definition_chain_value_count,
        range_count,
        definition_value_records,
        definition_chain_value_records,
        range_records,
    })
}

/// Group design objects under the operation whose exact structural owner
/// chain they belong to. The nearest admitted operation on the chain owns the
/// object instead of an outer operation. Missing links and non-reflexive cycles
/// reject the whole chain so a partial owner path cannot invent feature
/// properties.
fn native_operation_owned_objects<'a>(
    design_objects: &HashMap<&'a str, &'a CatiaDesignObject>,
    native_operation_object_ids: &HashSet<&str>,
) -> HashMap<&'a str, Vec<&'a CatiaDesignObject>> {
    let mut operations = ChainStops::new(|id: &'a str| {
        owner_chain_step(id, design_objects.get(id).copied(), |id| {
            native_operation_object_ids.contains(id)
        })
    });
    let mut owned = HashMap::<&str, Vec<&CatiaDesignObject>>::new();
    for candidate in design_objects.values().copied() {
        if let Some(operation) = operations.stop(candidate.id.as_str()) {
            owned.entry(operation).or_default().push(candidate);
        }
    }
    owned
}

fn property_prefix(
    prefix: &str,
    suffix: &str,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    property_prefix_args(format_args!("{prefix}{suffix}"))
}

fn property_prefix_args(
    args: std::fmt::Arguments<'_>,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    cadmpeg_core::text::NonBlankString::new(args.to_string())
        .ok_or_else(|| CodecError::malformed("CATIA feature property key is blank"))
}

fn insert_property(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &str,
    suffix: &str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let key = property_prefix(prefix, suffix)?;
    properties.insert(key, value.to_string());
    Ok(())
}

fn insert_schema_value_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySchemaValue,
) -> Result<(), CodecError> {
    insert_property(
        properties,
        prefix.as_str(),
        "_entry",
        format_args!("{}", value.entry),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_ordinal",
        format_args!("{}", value.ordinal),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_offset",
        format_args!("{}", value.offset),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_value",
        format_args!("{}", value.value),
    )?;
    Ok(())
}

fn insert_range_interval_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    range: &CatiaRangeInterval,
) -> Result<(), CodecError> {
    insert_schema_value_properties(
        properties,
        &property_prefix(prefix.as_str(), "_selector")?,
        &range.range,
    )?;
    match &range.interval.prefix {
        RangeIntervalPrefix::Compact { value, width } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_prefix_kind",
                format_args!("{}", "compact"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_prefix_value",
                format_args!("{value}"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_prefix_width",
                format_args!("{width}"),
            )?;
        }
        RangeIntervalPrefix::EscapedWord { word } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_prefix_kind",
                format_args!("{}", "escaped_word"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_prefix_word",
                format_args!("{word}"),
            )?;
        }
    }
    match &range.interval.slots {
        Some([lower, upper]) => {
            insert_property(
                properties,
                prefix.as_str(),
                "_slots",
                format_args!("{}", "two"),
            )?;
            insert_range_slot_properties(
                properties,
                &property_prefix(prefix.as_str(), "_lower")?,
                lower,
            )?;
            insert_range_slot_properties(
                properties,
                &property_prefix(prefix.as_str(), "_upper")?,
                upper,
            )?;
        }
        None => {
            insert_property(
                properties,
                prefix.as_str(),
                "_slots",
                format_args!("{}", "none"),
            )?;
        }
    }
    if let Some(nominal) = range.nominal.as_ref() {
        insert_property(
            properties,
            prefix.as_str(),
            "_nominal_kind",
            format_args!("{}", "finite"),
        )?;
        insert_property(
            properties,
            prefix.as_str(),
            "_nominal_framing",
            format_args!("{}", range_nominal_framing_name(nominal.framing)),
        )?;
        insert_property(
            properties,
            prefix.as_str(),
            "_nominal_bits",
            format_args!("{:016x}", nominal.bits),
        )?;
        insert_property(
            properties,
            prefix.as_str(),
            "_nominal_opcode_offset",
            format_args!("{}", nominal.evaluation_opcode_offset),
        )?;
    } else {
        insert_property(
            properties,
            prefix.as_str(),
            "_nominal_kind",
            format_args!("{}", "absent"),
        )?;
    }
    insert_property(
        properties,
        prefix.as_str(),
        "_incoming_payload_reference_count",
        format_args!("{}", range.incoming_references.len()),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_incoming_storage_reference_count",
        format_args!("{}", range.incoming_storage_references.len()),
    )?;
    Ok(())
}

fn insert_range_slot_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    slot: &RangeIntervalSlot,
) -> Result<(), CodecError> {
    match slot {
        RangeIntervalSlot::Binary64 { bits, offset } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "binary64"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_bits",
                format_args!("{bits:016x}"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
        }
        RangeIntervalSlot::Unset { offset } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "unset"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
        }
    }
    Ok(())
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
) -> Result<(), CodecError> {
    match payload {
        crate::native::CatiaEntitySuffixPayload::Evaluation {
            opcode_offset,
            evaluation,
            encoding,
        } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_encoding",
                format_args!("{}", evaluation_encoding_name(*encoding)),
            )?;
            insert_evaluation_properties(properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixPayload::Atom { value } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "atom"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_value",
                format_args!("{value}"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset,
            selector,
            value,
        } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "schema_selected"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_selector_offset",
                format_args!("{selector_offset}"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_selector",
                format_args!("{selector}"),
            )?;
            insert_selected_value_properties(
                properties,
                &property_prefix(prefix.as_str(), "_selected")?,
                value,
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::ControlE8 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::ControlE9 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e9"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::Separator37 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "separator_37"),
            )?;
        }
    }
    Ok(())
}

fn insert_selected_value_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySuffixSelectedValue,
) -> Result<(), CodecError> {
    match value {
        crate::native::CatiaEntitySuffixSelectedValue::Atom { value } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "atom"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_value",
                format_args!("{value}"),
            )?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_evaluation_properties(properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "separator_37"),
            )?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector {
            offset, ordinal, ..
        } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "schema_selector"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_ordinal",
                format_args!("{ordinal}"),
            )?;
        }
    }
    Ok(())
}

fn insert_schema_selection_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    selection: &crate::native::CatiaEntitySuffixSchemaSelection,
) -> Result<(), CodecError> {
    insert_property(
        properties,
        prefix.as_str(),
        "_offset",
        format_args!("{}", selection.offset),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_ordinal",
        format_args!("{}", selection.ordinal),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_entry",
        format_args!("{}", selection.entry),
    )?;
    insert_property(
        properties,
        prefix.as_str(),
        "_name",
        format_args!("{}", selection.name),
    )?;
    insert_schema_selected_value_properties(
        properties,
        &property_prefix(prefix.as_str(), "_value")?,
        &selection.value,
    )?;
    Ok(())
}

fn insert_schema_selected_value_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySuffixSchemaValue,
) -> Result<(), CodecError> {
    match value {
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "atom"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_atom",
                format_args!("{value}"),
            )?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_evaluation_properties(properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::ControlE8 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::Separator37 => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "separator_37"),
            )?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset,
            ordinal,
            resolution,
        } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "schema_selector"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_ordinal",
                format_args!("{ordinal}"),
            )?;
            if let Some(class) = resolution {
                insert_property(
                    properties,
                    prefix.as_str(),
                    "_entry",
                    format_args!("{}", class.entry),
                )?;
                insert_property(
                    properties,
                    prefix.as_str(),
                    "_name",
                    format_args!("{}", class.name),
                )?;
            }
        }
    }
    Ok(())
}

fn insert_evaluation_properties(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    evaluation: &crate::native::CatiaEntityEvaluation,
) -> Result<(), CodecError> {
    match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            insert_property(
                properties,
                prefix.as_str(),
                "_evaluation",
                format_args!("{}", "unset"),
            )?;
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => {
            insert_property(
                properties,
                prefix.as_str(),
                "_evaluation",
                format_args!("{}", "scalar"),
            )?;
            insert_property(
                properties,
                prefix.as_str(),
                "_evaluation_bits",
                format_args!("{bits:016x}"),
            )?;
        }
    }
    Ok(())
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
