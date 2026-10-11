// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId, PrincipalPlane,
    UnresolvedFamily,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::ids::neutral_history_id;
use crate::native::{
    CatiaDesignObject, CatiaDesignObjectRelationSource, CatiaNative, CatiaObjectRecord,
};
use crate::object_graph::{PayloadField, PayloadSubtype};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DesignFeatureTransfer {
    pub(crate) feature_ids: HashMap<String, FeatureId>,
    pub(crate) principal_plane_records: HashSet<String>,
    pub(crate) reference_plane_records: HashSet<String>,
    pub(crate) sketch_owner_records: HashSet<String>,
}

impl DesignFeatureTransfer {
    pub(crate) fn consumed_records(&self) -> impl Iterator<Item = &String> {
        self.principal_plane_records
            .union(&self.sketch_owner_records)
            .chain(self.reference_plane_records.iter())
    }

    /// Bind parameters to a transferred feature only through their exact
    /// entity-record and object-record ownership chain. The same exact
    /// incidences populate feature-local parameter ordinals.
    pub(crate) fn assign_parameter_owners(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        native: &CatiaNative,
    ) -> Result<(), CodecError> {
        let entities = ctx.collect_hash_map(
            native
                .entity_records
                .iter()
                .map(|entity| (entity.id.as_str(), entity)),
            "catia_feature_owner_entities",
        )?;
        let object_records = ctx.collect_hash_map(
            native
                .object_graphs
                .iter()
                .flat_map(|graph| &graph.records)
                .map(|record| (record.id.as_str(), record)),
            "catia_feature_owner_records",
        )?;
        let design_objects = ctx.collect_hash_map(
            native
                .design_objects
                .iter()
                .map(|object| (object.id.as_str(), object)),
            "catia_feature_owner_objects",
        )?;
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
                ctx,
                design_object,
                &design_objects,
                &self.feature_ids,
            )?
            else {
                continue;
            };
            if parameter.owner.is_none() {
                parameter.owner =
                    Some(feature_id.try_clone_for_decode(ctx, "catia_feature_parameter_owner")?);
            }
            if parameter.owner.as_ref() == Some(feature_id) {
                let parameter_id = parameter
                    .id
                    .try_clone_for_decode(ctx, "catia_feature_owner_parameter_id")?;
                let feature_id =
                    feature_id.try_clone_for_decode(ctx, "catia_feature_owner_feature_id")?;
                ctx.insert_hash_map(
                    &mut exact_feature_owners,
                    parameter_id,
                    feature_id,
                    "catia_feature_exact_owners",
                )?;
            }
        }

        assign_feature_parameter_ordinals(
            ctx,
            ir,
            &entities,
            &object_records,
            &exact_feature_owners,
            &self.feature_ids,
        )?;
        assign_document_parameter_ordinals(ctx, ir)?;
        normalize_parameter_names(ctx, ir)?;
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
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        native: &CatiaNative,
    ) -> Result<(), CodecError> {
        let parents = self.feature_parents(ctx, ir, native)?;
        for feature in &mut ir.model.features {
            feature.dependencies.retain(|dependency| {
                parents
                    .get(&feature.id)
                    .is_none_or(|parent| dependency != parent)
            });
        }
        for (child, parent) in parents {
            ir.model
                .set_feature_regeneration_parent(ctx, &child, &parent)?;
        }
        Ok(())
    }

    fn feature_parents(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        native: &CatiaNative,
    ) -> Result<HashMap<FeatureId, FeatureId>, CodecError> {
        let design_objects = ctx.collect_hash_map(
            native
                .design_objects
                .iter()
                .map(|object| (object.id.as_str(), object)),
            "catia_feature_parent_objects",
        )?;
        let feature_ordinals = ctx.collect_hash_map(
            ir.model
                .features
                .iter()
                .map(|feature| (&feature.id, feature.ordinal)),
            "catia_feature_parent_ordinals",
        )?;
        let mut parents = HashMap::new();
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
            let Some(parent) = nearest_feature_for_design_object(
                ctx,
                parent_object,
                &design_objects,
                &self.feature_ids,
            )?
            else {
                continue;
            };
            let Some(parent_ordinal) = feature_ordinals.get(parent) else {
                continue;
            };
            if parent == &feature.id || *parent_ordinal >= feature.ordinal {
                continue;
            }
            let child = feature
                .id
                .try_clone_for_decode(ctx, "catia_feature_parent_child")?;
            let parent = parent.try_clone_for_decode(ctx, "catia_feature_parent_id")?;
            ctx.insert_hash_map(&mut parents, child, parent, "catia_feature_parent_map")?;
        }
        let mut cyclic = Vec::new();
        for feature in parents.keys() {
            if !feature_parent_chain_is_acyclic(ctx, feature, &parents)? {
                let id = feature.try_clone_for_decode(ctx, "catia_feature_cyclic_id")?;
                ctx.push_vec(&mut cyclic, id, "catia_feature_cyclic_ids")?;
            }
        }
        for feature in cyclic {
            parents.remove(&feature);
        }
        Ok(parents)
    }

    /// Bind exact payload references to earlier transferred features as
    /// structural dependencies. A target may resolve through its complete
    /// owner-design-object chain. Storage selectors, unresolved targets,
    /// self-links, and forward targets do not establish history edges.
    fn assign_feature_dependencies(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        native: &CatiaNative,
    ) -> Result<(), CodecError> {
        let design_objects = ctx.collect_hash_map(
            native
                .design_objects
                .iter()
                .map(|object| (object.id.as_str(), object)),
            "catia_feature_dependency_objects",
        )?;
        let feature_ordinals = ctx.collect_hash_map(
            ir.model
                .features
                .iter()
                .map(|feature| (&feature.id, feature.ordinal)),
            "catia_feature_dependency_ordinals",
        )?;
        let mut dependencies_by_feature = HashMap::<FeatureId, Vec<FeatureId>>::new();

        for feature in &ir.model.features {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some(object) = design_objects.get(native_ref) else {
                continue;
            };
            let mut seen =
                ctx.collect_hash_set(feature.dependencies.iter(), "catia_feature_dependency_seen")?;
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
                    ctx,
                    target_object,
                    &design_objects,
                    &self.feature_ids,
                )?
                else {
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
                ctx.insert_hash_set(&mut seen, target, "catia_feature_dependency_seen")?;
                if !dependencies_by_feature.contains_key(&feature.id) {
                    let id = feature
                        .id
                        .try_clone_for_decode(ctx, "catia_feature_dependency_owner")?;
                    ctx.insert_hash_map(
                        &mut dependencies_by_feature,
                        id,
                        Vec::new(),
                        "catia_feature_dependency_map",
                    )?;
                }
                let target = target.try_clone_for_decode(ctx, "catia_feature_dependency_target")?;
                if let Some(dependencies) = dependencies_by_feature.get_mut(&feature.id) {
                    ctx.push_vec(dependencies, target, "catia_feature_dependency_values")?;
                }
            }
        }

        for feature in &mut ir.model.features {
            if let Some(dependencies) = dependencies_by_feature.remove(&feature.id) {
                feature.dependencies.reserve_for_decode(
                    ctx,
                    dependencies.len(),
                    "catia_feature_dependency_values",
                )?;
                feature.dependencies.append(
                    ctx,
                    dependencies,
                    "catia_feature_dependency_values",
                )?;
            }
        }
        Ok(())
    }
}

fn assign_feature_parameter_ordinals(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    entities: &HashMap<&str, &crate::native::entity_record::CatiaEntityRecord>,
    object_records: &HashMap<&str, &CatiaObjectRecord>,
    exact_feature_owners: &HashMap<ParameterId, FeatureId>,
    feature_ids: &HashMap<String, FeatureId>,
) -> Result<(), CodecError> {
    let transferred_features =
        ctx.collect_hash_set(feature_ids.values(), "catia_feature_transferred_ids")?;
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
            let id = feature_id.try_clone_for_decode(ctx, "catia_feature_parameter_bucket_id")?;
            ctx.insert_hash_map(
                &mut parameters_by_feature,
                id,
                Vec::new(),
                "catia_feature_parameter_buckets",
            )?;
        }
        let id = parameter
            .id
            .try_clone_for_decode(ctx, "catia_feature_parameter_ordinal_id")?;
        if let Some(parameters) = parameters_by_feature.get_mut(feature_id) {
            ctx.push_vec(
                parameters,
                (object_record.byte_offset, entity.byte_offset, id),
                "catia_feature_parameter_rows",
            )?;
        }
    }

    let mut parameter_ordinals = HashMap::new();
    for parameters in parameters_by_feature.values_mut() {
        ctx.sort_unstable_by(
            parameters,
            |left, right| {
                left.0
                    .cmp(&right.0)
                    .then(left.1.cmp(&right.1))
                    .then(left.2.cmp(&right.2))
            },
            |item| item.2.as_str().len(),
            "catia_feature_parameter_order_sort",
        )?;
        for (ordinal, parameter) in parameters.iter().enumerate() {
            let Some(ordinal) = u32::try_from(ordinal).ok() else {
                continue;
            };
            let id = parameter
                .2
                .try_clone_for_decode(ctx, "catia_feature_ordinal_map_id")?;
            ctx.insert_hash_map(
                &mut parameter_ordinals,
                id,
                ordinal,
                "catia_feature_ordinal_map",
            )?;
        }
    }

    for parameter in &mut ir.model.parameters {
        if let Some(ordinal) = parameter_ordinals.get(&parameter.id) {
            parameter.ordinal = *ordinal;
        }
    }
    Ok(())
}

/// Normalize the document scope after feature ownership is known.
///
/// Formula transfer assigns a source-order ordinal before ownership can be
/// established. Feature-owned parameters receive a feature-local ordinal
/// above; document parameters must receive their own contiguous scope instead
/// of retaining gaps left by those feature parameters.
fn assign_document_parameter_ordinals(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    let mut parameters = Vec::new();
    for parameter in ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.owner.is_none())
    {
        let id = parameter
            .id
            .try_clone_for_decode(ctx, "catia_document_parameter_sort_id")?;
        ctx.push_vec(
            &mut parameters,
            (parameter.ordinal, id),
            "catia_document_parameter_sort_rows",
        )?;
    }
    ctx.sort_unstable_by(
        &mut parameters,
        |left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)),
        |item| item.1.as_str().len(),
        "catia_document_parameter_order_sort",
    )?;

    let mut parameter_ordinals = HashMap::new();
    for (ordinal, (_, parameter)) in parameters.into_iter().enumerate() {
        let Some(ordinal) = u32::try_from(ordinal).ok() else {
            continue;
        };
        ctx.insert_hash_map(
            &mut parameter_ordinals,
            parameter,
            ordinal,
            "catia_document_parameter_ordinals",
        )?;
    }
    for parameter in &mut ir.model.parameters {
        if let Some(ordinal) = parameter_ordinals.get(&parameter.id) {
            parameter.ordinal = *ordinal;
        }
    }
    Ok(())
}

fn copy_feature_scope(
    ctx: &DecodeContext<'_>,
    owner: Option<&FeatureId>,
) -> Result<Option<FeatureId>, CodecError> {
    owner
        .map(|id| id.try_clone_for_decode(ctx, "catia_parameter_scope_id"))
        .transpose()
}

/// Give every neutral parameter a unique name within its ownership scope.
///
/// Keep the first source name; suffix later collisions. Reserve every source
/// name before choosing a suffix. Original spelling stays in
/// `properties["source_name"]` when the neutral name changes.
fn normalize_parameter_names(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let mut reserved_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    for parameter in &ir.model.parameters {
        if !parameter.name.is_empty() {
            if !reserved_by_scope.contains_key(&parameter.owner) {
                let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
                ctx.insert_hash_map(
                    &mut reserved_by_scope,
                    scope,
                    HashSet::new(),
                    "catia_parameter_reserved_scopes",
                )?;
            }
            let name = ctx.copy_retained_text(&parameter.name, "catia_parameter_reserved_name")?;
            if let Some(reserved) = reserved_by_scope.get_mut(&parameter.owner) {
                ctx.insert_hash_set(reserved, name, "catia_parameter_reserved_names")?;
            }
        }
    }

    let mut used_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    for parameter in &mut ir.model.parameters {
        if !reserved_by_scope.contains_key(&parameter.owner) {
            let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
            ctx.insert_hash_map(
                &mut reserved_by_scope,
                scope,
                HashSet::new(),
                "catia_parameter_reserved_scopes",
            )?;
        }
        if !used_by_scope.contains_key(&parameter.owner) {
            let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
            ctx.insert_hash_map(
                &mut used_by_scope,
                scope,
                HashSet::new(),
                "catia_parameter_used_scopes",
            )?;
        }
        let Some(reserved) = reserved_by_scope.get(&parameter.owner) else {
            continue;
        };
        let Some(used) = used_by_scope.get_mut(&parameter.owner) else {
            continue;
        };
        let source_name = ctx.copy_retained_text(&parameter.name, "catia_parameter_source_name")?;
        if !source_name.is_empty()
            && ctx.insert_hash_set(
                used,
                ctx.copy_retained_text(&source_name, "catia_parameter_used_name")?,
                "catia_parameter_used_names",
            )?
        {
            continue;
        }

        let base = if source_name.is_empty() {
            "Parameter"
        } else {
            source_name.as_str()
        };
        let mut suffix = 1u32;
        let neutral_name = loop {
            ctx.charge_work(1, "catia_parameter_name_collision")?;
            let candidate = ctx.format_retained(
                format_args!("{base}#{suffix}"),
                "catia_parameter_neutral_name",
            )?;
            suffix = suffix.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_parameter_name_collision", u64::MAX, u64::MAX)
            })?;
            if !reserved.contains(&candidate)
                && ctx.insert_hash_set(
                    used,
                    ctx.copy_retained_text(&candidate, "catia_parameter_used_name")?,
                    "catia_parameter_used_names",
                )?
            {
                break candidate;
            }
        };
        parameter.name = neutral_name;
        ctx.insert_btree_map(
            &mut parameter.properties,
            cadmpeg_core::nonblank_literal!("source_name"),
            source_name,
            "catia_parameter_source_property",
        )?;
    }
    Ok(())
}

fn feature_parent_chain_is_acyclic(
    ctx: &DecodeContext<'_>,
    feature_id: &FeatureId,
    parents: &HashMap<FeatureId, FeatureId>,
) -> Result<bool, CodecError> {
    let mut current = feature_id;
    let mut traversed = 0usize;
    while let Some(parent) = parents.get(current) {
        ctx.charge_work(1, "catia_feature_parent_chain")?;
        traversed = traversed.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_feature_parent_chain", u64::MAX, u64::MAX)
        })?;
        if traversed > parents.len() {
            return Ok(false);
        }
        current = parent;
    }
    Ok(true)
}

/// Resolve the nearest transferred feature on one exact owner chain.
///
/// Structural groups may sit between a native feature object and another
/// feature object. Stop at the first transferred feature so a nested feature
/// keeps its immediate structural parent. A missing link or a cycle rejects
/// the chain instead of inferring a relationship from field vocabulary.
fn nearest_feature_for_design_object<'a>(
    ctx: &DecodeContext<'_>,
    start: &str,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    feature_ids: &'a HashMap<String, FeatureId>,
) -> Result<Option<&'a FeatureId>, CodecError> {
    let mut current = Some(start);
    let mut traversed = 0usize;

    while let Some(current_id) = current {
        ctx.charge_work(1, "catia_feature_owner_chain")?;
        traversed = traversed.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_feature_owner_chain", u64::MAX, u64::MAX)
        })?;
        if traversed > design_objects.len() {
            return Ok(None);
        }
        let Some(object) = design_objects.get(current_id).copied() else {
            return Ok(None);
        };
        if let Some(feature) = feature_ids.get(current_id) {
            return Ok(Some(feature));
        }
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }

    Ok(None)
}

/// Transfer exact owner-bound reference history nodes.
pub(crate) fn transfer_design_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<DesignFeatureTransfer, cadmpeg_core::CodecError> {
    let records = ctx.collect_hash_map(
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .map(|record| (record.id.as_str(), record)),
        "catia_feature_transfer_records",
    )?;
    let mut transfer = DesignFeatureTransfer::default();

    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.contains(object.parent.as_str()))
    {
        let plane_candidate = principal_plane_candidate(ctx, object, &records)?;
        let sketch_owner = sketch_candidate(object, &records);
        let reference_plane = reference_plane_candidate(object, &records);
        match (plane_candidate, sketch_owner, reference_plane) {
            (Some(candidate), None, None) => {
                transfer_principal_plane(ctx, ir, &mut transfer, candidate)?;
            }
            (None, Some(owner_record), None) => {
                transfer_sketch(ctx, ir, &mut transfer, object, owner_record)?;
            }
            (None, None, Some(candidate)) => {
                transfer_reference_plane(ctx, ir, &mut transfer, &candidate)?;
            }
            _ => {
                // One object cannot safely occupy two neutral feature identities.
                // Leave all declarations unresolved so the feature-id map cannot
                // overwrite one transfer with another.
            }
        }
    }

    transfer.assign_feature_parents(ctx, ir, native)?;
    transfer.assign_feature_dependencies(ctx, ir, native)?;
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
        ctx,
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = feature_id.try_clone_for_decode(ctx, "catia_principal_feature_map_id")?;
    let source_tag =
        ctx.copy_retained_text(candidate.declaration_class, "catia_principal_feature_tag")?;
    let native_ref = ctx.copy_retained_text(&object.id, "catia_principal_feature_ref")?;
    ctx.push_vec(
        &mut ir.model.features,
        Feature {
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
        },
        "catia_principal_features",
    )?;
    let map_key = ctx.copy_retained_text(&object.id, "catia_principal_feature_key")?;
    ctx.insert_hash_map(
        &mut transfer.feature_ids,
        map_key,
        map_feature_id,
        "catia_principal_feature_ids",
    )?;
    for record in candidate.declarations {
        let id = ctx.copy_retained_text(&record.id, "catia_principal_record_id")?;
        ctx.insert_hash_set(
            &mut transfer.principal_plane_records,
            id,
            "catia_principal_records",
        )?;
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
        ctx,
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = feature_id.try_clone_for_decode(ctx, "catia_reference_feature_map_id")?;
    let source_tag = ctx.copy_retained_text(candidate.kind, "catia_reference_feature_tag")?;
    let native_ref = ctx.copy_retained_text(&object.id, "catia_reference_feature_ref")?;
    ctx.push_vec(
        &mut ir.model.features,
        Feature {
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
        },
        "catia_reference_features",
    )?;
    let map_key = ctx.copy_retained_text(&object.id, "catia_reference_feature_key")?;
    ctx.insert_hash_map(
        &mut transfer.feature_ids,
        map_key,
        map_feature_id,
        "catia_reference_feature_ids",
    )?;
    let record = ctx.copy_retained_text(&candidate.owner_record.id, "catia_reference_record_id")?;
    ctx.insert_hash_set(
        &mut transfer.reference_plane_records,
        record,
        "catia_reference_records",
    )?;
    Ok(())
}

fn transfer_sketch(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    object: &CatiaDesignObject,
    owner_record: &CatiaObjectRecord,
) -> Result<(), cadmpeg_core::CodecError> {
    let sketch_id =
        match neutral_history_id(ctx, &object.id, &cadmpeg_ir::identity_component!("sketch")) {
            Ok(id) => SketchId::from(id),
            Err(CodecError::Malformed(_)) => return Ok(()),
            Err(error) => return Err(error),
        };
    let feature_id = FeatureId::from(neutral_history_id(
        ctx,
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design sketch")?;
    let binding_sketch_id =
        sketch_id.try_clone_for_decode(ctx, "catia_design_sketch_binding_id")?;
    let sketch_ref = ctx.copy_retained_text(&object.id, "catia_design_sketch_ref")?;
    ctx.push_vec(
        &mut ir.model.sketches,
        Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some(sketch_ref),
        },
        "catia_design_sketches",
    )?;
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id =
        feature_id.try_clone_for_decode(ctx, "catia_design_sketch_feature_map_id")?;
    let feature_ref = ctx.copy_retained_text(&object.id, "catia_design_sketch_feature_ref")?;
    let source_tag = ctx.copy_retained_text("Sketch", "catia_design_sketch_feature_tag")?;
    ctx.push_vec(
        &mut ir.model.features,
        Feature {
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
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                        binding_sketch_id,
                    )),
                }),
            ),
            native_ref: Some(feature_ref),
        },
        "catia_design_sketch_features",
    )?;
    let map_key = ctx.copy_retained_text(&object.id, "catia_design_sketch_feature_key")?;
    ctx.insert_hash_map(
        &mut transfer.feature_ids,
        map_key,
        map_feature_id,
        "catia_design_sketch_feature_ids",
    )?;
    let record = ctx.copy_retained_text(&owner_record.id, "catia_design_sketch_owner_id")?;
    ctx.insert_hash_set(
        &mut transfer.sketch_owner_records,
        record,
        "catia_design_sketch_owners",
    )?;
    Ok(())
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

fn is_admitted_native_reference_plane_class(name: &str) -> bool {
    matches!(name, "GSMPlaneAngle" | "GSMPlaneOffset")
}

struct PrincipalPlaneCandidate<'a> {
    object: &'a CatiaDesignObject,
    declarations: Vec<&'a CatiaObjectRecord>,
    plane: PrincipalPlane,
    declaration_class: &'a str,
}

fn principal_plane_candidate<'a>(
    ctx: &DecodeContext<'_>,
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Result<Option<PrincipalPlaneCandidate<'a>>, CodecError> {
    if object.owner_record.is_none() {
        return Ok(None);
    }
    let Some(declarations) = ctx.collect_options(
        object
            .fields
            .iter()
            .map(|field| records.get(field.as_str()).copied()),
        "catia_principal_plane_declarations",
    )?
    else {
        return Ok(None);
    };
    let Some(first) = declarations.first() else {
        return Ok(None);
    };
    let Some(class_name) = first.class_name() else {
        return Ok(None);
    };
    let Some(class_entry) = first.class_entry() else {
        return Ok(None);
    };
    let Some(plane) = principal_plane(class_name) else {
        return Ok(None);
    };
    Ok(declarations
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
        }))
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
