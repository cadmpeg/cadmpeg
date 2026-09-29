// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
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
use crate::resource;

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
                parameter.owner = Some(resource::copy_id(
                    ctx,
                    feature_id.as_str(),
                    FeatureId::mint,
                    "catia_feature_parameter_owner",
                )?);
            }
            if parameter.owner.as_ref() == Some(feature_id) {
                let parameter_id = resource::copy_id(
                    ctx,
                    parameter.id.as_str(),
                    ParameterId::mint,
                    "catia_feature_owner_parameter_id",
                )?;
                let feature_id = resource::copy_id(
                    ctx,
                    feature_id.as_str(),
                    FeatureId::mint,
                    "catia_feature_owner_feature_id",
                )?;
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
        assign_native_operation_parameter_values(ctx, ir, &exact_feature_owners)?;
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
            ctx.charge_collection_items(1, "catia_feature_regeneration_parents")?;
            ir.model
                .set_feature_regeneration_parent(child, parent)
                .map_err(cadmpeg_core::CodecError::malformed)?;
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
            let child = resource::copy_id(
                ctx,
                feature.id.as_str(),
                FeatureId::mint,
                "catia_feature_parent_child",
            )?;
            let parent = resource::copy_id(
                ctx,
                parent.as_str(),
                FeatureId::mint,
                "catia_feature_parent_id",
            )?;
            ctx.insert_hash_map(&mut parents, child, parent, "catia_feature_parent_map")?;
        }
        let mut cyclic = Vec::new();
        for feature in parents.keys() {
            if !feature_parent_chain_is_acyclic(ctx, feature, &parents)? {
                let id = resource::copy_id(
                    ctx,
                    feature.as_str(),
                    FeatureId::mint,
                    "catia_feature_cyclic_id",
                )?;
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
                    let id = resource::copy_id(
                        ctx,
                        feature.id.as_str(),
                        FeatureId::mint,
                        "catia_feature_dependency_owner",
                    )?;
                    ctx.insert_hash_map(
                        &mut dependencies_by_feature,
                        id,
                        Vec::new(),
                        "catia_feature_dependency_map",
                    )?;
                }
                let target = resource::copy_id(
                    ctx,
                    target.as_str(),
                    FeatureId::mint,
                    "catia_feature_dependency_target",
                )?;
                if let Some(dependencies) = dependencies_by_feature.get_mut(&feature.id) {
                    ctx.push_vec(dependencies, target, "catia_feature_dependency_values")?;
                }
            }
        }

        for feature in &mut ir.model.features {
            if let Some(dependencies) = dependencies_by_feature.remove(&feature.id) {
                feature
                    .dependencies
                    .try_reserve(dependencies.len())
                    .map_err(|_| {
                        cadmpeg_core::CodecError::ResourceLimit(
                            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                                cadmpeg_core::decode::ResourceDimension::Codec(
                                    "catia_feature_dependency_values",
                                ),
                                cadmpeg_core::decode::u64_from_index(0),
                                cadmpeg_core::decode::u64_from_index(dependencies.len()),
                                "catia_feature_dependency_values",
                            ),
                        )
                    })?;
                feature.dependencies.extend(dependencies);
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
            let id = resource::copy_id(
                ctx,
                feature_id.as_str(),
                FeatureId::mint,
                "catia_feature_parameter_bucket_id",
            )?;
            ctx.insert_hash_map(
                &mut parameters_by_feature,
                id,
                Vec::new(),
                "catia_feature_parameter_buckets",
            )?;
        }
        let id = resource::copy_id(
            ctx,
            parameter.id.as_str(),
            ParameterId::mint,
            "catia_feature_parameter_ordinal_id",
        )?;
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(parameters.len()),
            "catia_feature_parameter_sort",
        )?;
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
            let id = resource::copy_id(
                ctx,
                parameter.2.as_str(),
                ParameterId::mint,
                "catia_feature_ordinal_map_id",
            )?;
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
        let id = resource::copy_id(
            ctx,
            parameter.id.as_str(),
            ParameterId::mint,
            "catia_document_parameter_sort_id",
        )?;
        ctx.push_vec(
            &mut parameters,
            (parameter.ordinal, id),
            "catia_document_parameter_sort_rows",
        )?;
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(parameters.len()),
        "catia_document_parameter_sort",
    )?;
    parameters.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

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

/// Expose exact feature-owned parameter expressions without assigning
/// operation-specific roles. The map uses the neutral, scope-unique parameter
/// names assigned by [`normalize_parameter_names`]. A changed name retains its
/// source spelling in the corresponding parameter's `source_name` property.
/// Typed unresolved operation families retain the same expressions in feature
/// source properties because their neutral definitions have no generic
/// parameter map.
fn assign_native_operation_parameter_values(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    exact_feature_owners: &HashMap<ParameterId, FeatureId>,
) -> Result<(), CodecError> {
    let mut values_by_feature =
        HashMap::<FeatureId, BTreeMap<cadmpeg_core::text::NonBlankString, String>>::new();
    for parameter in &ir.model.parameters {
        let Some(feature_id) = exact_feature_owners.get(&parameter.id) else {
            continue;
        };
        let Some(name) = cadmpeg_core::text::NonBlankString::new(
            ctx.copy_retained_text(&parameter.name, "catia_feature_operation_parameter_name")?,
        ) else {
            continue;
        };
        if !values_by_feature.contains_key(feature_id) {
            let id = resource::copy_id(
                ctx,
                feature_id.as_str(),
                FeatureId::mint,
                "catia_feature_operation_owner",
            )?;
            ctx.insert_hash_map(
                &mut values_by_feature,
                id,
                BTreeMap::new(),
                "catia_feature_operation_values",
            )?;
        }
        let expression =
            ctx.copy_retained_text(&parameter.expression, "catia_feature_operation_expression")?;
        if let Some(values) = values_by_feature.get_mut(feature_id) {
            ctx.insert_btree_map(
                values,
                name,
                expression,
                "catia_feature_operation_parameters",
            )?;
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
                            cadmpeg_ir::features::NativeFeatureKind::Other(
                                ctx.copy_retained_text(name, "catia_feature_operation_kind")?,
                            )
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
                    let key = ctx.format_retained(
                        format_args!("catia_parameter_{name}"),
                        "catia_feature_source_parameter_key",
                    )?;
                    let key = cadmpeg_core::text::NonBlankString::new(key).ok_or_else(|| {
                        CodecError::malformed("CATIA source parameter key is blank")
                    })?;
                    ctx.insert_btree_map(
                        &mut feature.source_properties,
                        key,
                        expression,
                        "catia_feature_source_parameters",
                    )?;
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
fn copy_feature_scope(
    ctx: &DecodeContext<'_>,
    owner: Option<&FeatureId>,
) -> Result<Option<FeatureId>, CodecError> {
    owner
        .map(|id| {
            resource::copy_id(
                ctx,
                id.as_str(),
                FeatureId::mint,
                "catia_parameter_scope_id",
            )
        })
        .transpose()
}

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
    let entities = ctx.collect_hash_map(
        native
            .entity_records
            .iter()
            .map(|entity| (entity.id.as_str(), entity)),
        "catia_feature_transfer_entities",
    )?;
    let design_objects = ctx.collect_hash_map(
        native
            .design_objects
            .iter()
            .map(|object| (object.id.as_str(), object)),
        "catia_feature_transfer_objects",
    )?;
    let native_operation_object_ids = ctx.collect_hash_set(
        native
            .design_objects
            .iter()
            .filter(|object| native_operation_candidate(object, &records).is_some())
            .map(|object| object.id.as_str()),
        "catia_feature_operation_object_ids",
    )?;
    let operation_sources = NativeOperationSources {
        object_records: &records,
        entities: &entities,
        design_objects: &design_objects,
        object_ids: &native_operation_object_ids,
    };
    let mut transfer = DesignFeatureTransfer::default();

    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.contains(object.parent.as_str()))
    {
        let plane_candidate = principal_plane_candidate(ctx, object, &records)?;
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
    let map_feature_id = resource::copy_id(
        ctx,
        feature_id.as_str(),
        FeatureId::mint,
        "catia_principal_feature_map_id",
    )?;
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
    let map_feature_id = resource::copy_id(
        ctx,
        feature_id.as_str(),
        FeatureId::mint,
        "catia_reference_feature_map_id",
    )?;
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
    let binding_sketch_id = resource::copy_id(
        ctx,
        sketch_id.as_str(),
        SketchId::mint,
        "catia_design_sketch_binding_id",
    )?;
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
    let map_feature_id = resource::copy_id(
        ctx,
        feature_id.as_str(),
        FeatureId::mint,
        "catia_design_sketch_feature_map_id",
    )?;
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
    design_objects: &'a HashMap<&'a str, &'a CatiaDesignObject>,
    object_ids: &'a HashSet<&'a str>,
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
        ctx,
        object,
        sources.object_records,
        sources.entities,
        sources.design_objects,
        sources.object_ids,
    )?;
    let definition = native_operation_definition(ctx, kind, &object.id)?;
    let feature_id = FeatureId::from(neutral_history_id(
        ctx,
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = resource::copy_id(
        ctx,
        feature_id.as_str(),
        FeatureId::mint,
        "catia_native_operation_feature_map_id",
    )?;
    let source_tag = ctx.copy_retained_text(kind.as_str(), "catia_native_operation_feature_tag")?;
    let native_ref = ctx.copy_retained_text(&object.id, "catia_native_operation_feature_ref")?;
    ctx.push_vec(
        &mut ir.model.features,
        Feature {
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
        },
        "catia_native_operation_features",
    )?;
    let map_key = ctx.copy_retained_text(&object.id, "catia_native_operation_feature_key")?;
    ctx.insert_hash_map(
        &mut transfer.feature_ids,
        map_key,
        map_feature_id,
        "catia_native_operation_feature_ids",
    )?;
    let owner_id = ctx.copy_retained_text(
        &candidate.owner_record.id,
        "catia_native_operation_owner_id",
    )?;
    ctx.insert_hash_set(
        &mut transfer.native_operation_records,
        owner_id,
        "catia_native_operation_records",
    )?;
    transfer.native_operation_definition_value_count += definition_value_count;
    transfer.native_operation_definition_chain_value_count += definition_chain_value_count;
    transfer.native_operation_range_count += range_count;
    for record in definition_value_records {
        ctx.insert_hash_set(
            &mut transfer.native_operation_definition_value_records,
            record,
            "catia_native_operation_definition_records",
        )?;
    }
    for record in definition_chain_value_records {
        ctx.insert_hash_set(
            &mut transfer.native_operation_definition_chain_value_records,
            record,
            "catia_native_operation_chain_records",
        )?;
    }
    for record in range_records {
        ctx.insert_hash_set(
            &mut transfer.native_operation_range_records,
            record,
            "catia_native_operation_range_records",
        )?;
    }
    Ok(())
}

/// Project an admitted CATIA operation class into the neutral family while
/// keeping every unresolved operand explicit. The exact owner declaration
/// proves the family identity; it does not prove profile, axis, extent,
/// result, edge group, pattern seed, pattern axis, pattern angle, pattern
/// count, or operation-specific dependency roles.
fn native_operation_definition(
    ctx: &DecodeContext<'_>,
    kind: NativeOperationClass,
    native_ref: &str,
) -> Result<FeatureDefinition, CodecError> {
    Ok(match kind {
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
                shape: cadmpeg_ir::features::SweepShape::unresolved(Some(
                    ctx.copy_retained_text(native_ref, "catia_feature_sweep_shape_ref")?,
                )),
                path: Some(cadmpeg_ir::features::PathRef::Unresolved(
                    ctx.copy_retained_text(native_ref, "catia_feature_sweep_path_ref")?,
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
    })
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
    ctx: &DecodeContext<'_>,
    object: &CatiaDesignObject,
    object_records: &HashMap<&str, &CatiaObjectRecord>,
    entities: &HashMap<&str, &CatiaEntityRecord>,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    native_operation_object_ids: &HashSet<&str>,
) -> Result<NativeOperationDefinitionProperties, CodecError> {
    let mut properties = BTreeMap::new();
    let mut definition_value_count = 0;
    let mut definition_chain_value_count = 0;
    let mut range_count = 0;
    let mut definition_value_records = HashSet::new();
    let mut definition_chain_value_records = HashSet::new();
    let mut range_records = HashSet::new();

    let mut owned_objects = Vec::new();
    for candidate in design_objects.values() {
        if candidate.parent == object.parent
            && native_operation_owner_chain_reaches(
                ctx,
                candidate.id.as_str(),
                object.id.as_str(),
                design_objects,
                native_operation_object_ids,
            )?
        {
            ctx.push_vec(
                &mut owned_objects,
                *candidate,
                "catia_feature_operation_owned_objects",
            )?;
        }
    }
    let mut definition_values = ctx.collect_vec(
        owned_objects
            .iter()
            .flat_map(|owned| owned.definition_values.iter())
            .filter_map(|entity_id| entities.get(entity_id.as_str()).copied())
            .filter_map(|entity| entity.definition_value().map(|value| (entity, value))),
        "catia_feature_definition_values",
    )?;
    definition_values.sort_unstable_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    for (ordinal, (entity, value)) in definition_values.into_iter().enumerate() {
        definition_value_count += 1;
        let record =
            ctx.copy_retained_text(&entity.object_record, "catia_feature_definition_record")?;
        ctx.insert_hash_set(
            &mut definition_value_records,
            record,
            "catia_feature_definition_records",
        )?;
        let prefix = property_prefix_args(ctx, format_args!("catia_definition_value_{ordinal}"))?;
        insert_property(
            ctx,
            &mut properties,
            prefix.as_str(),
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_schema_value_properties(
            ctx,
            &mut properties,
            &property_prefix(ctx, prefix.as_str(), "_definition")?,
            &value.definition,
        )?;
        insert_suffix_payload_properties(
            ctx,
            &mut properties,
            &property_prefix(ctx, prefix.as_str(), "_payload")?,
            &value.payload,
        )?;
        if let Some(selection) = value.schema_selection.as_ref() {
            insert_schema_selection_properties(
                ctx,
                &mut properties,
                &property_prefix(ctx, prefix.as_str(), "_schema_selection")?,
                selection,
            )?;
        }
    }

    let mut definition_chain_values = ctx.collect_vec(
        owned_objects
            .iter()
            .flat_map(|owned| owned.definition_chain_values.iter())
            .filter_map(|entity_id| entities.get(entity_id.as_str()).copied())
            .filter_map(|entity| entity.definition_chain_value().map(|value| (entity, value))),
        "catia_feature_definition_chain_values",
    )?;
    definition_chain_values.sort_unstable_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    for (ordinal, (entity, value)) in definition_chain_values.into_iter().enumerate() {
        definition_chain_value_count += 1;
        let record = ctx.copy_retained_text(&entity.object_record, "catia_feature_chain_record")?;
        ctx.insert_hash_set(
            &mut definition_chain_value_records,
            record,
            "catia_feature_chain_records",
        )?;
        let prefix =
            property_prefix_args(ctx, format_args!("catia_definition_chain_value_{ordinal}"))?;
        insert_property(
            ctx,
            &mut properties,
            prefix.as_str(),
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_schema_value_properties(
            ctx,
            &mut properties,
            &property_prefix(ctx, prefix.as_str(), "_selector")?,
            &value.selector,
        )?;
        insert_schema_value_properties(
            ctx,
            &mut properties,
            &property_prefix(ctx, prefix.as_str(), "_role")?,
            &value.role,
        )?;
        insert_schema_selected_value_properties(
            ctx,
            &mut properties,
            &property_prefix(ctx, prefix.as_str(), "_value")?,
            &value.value,
        )?;
    }

    let mut range_intervals = ctx.collect_vec(
        owned_objects
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
            }),
        "catia_feature_range_intervals",
    )?;
    range_intervals.sort_unstable_by(|(left, _), (right, _)| {
        left.byte_offset
            .cmp(&right.byte_offset)
            .then(left.ordinal.cmp(&right.ordinal))
            .then(left.id.cmp(&right.id))
    });
    range_intervals.dedup_by(|(left, _), (right, _)| left.id == right.id);
    for (ordinal, (entity, range)) in range_intervals.into_iter().enumerate() {
        range_count += 1;
        let record = ctx.copy_retained_text(&entity.object_record, "catia_feature_range_record")?;
        ctx.insert_hash_set(&mut range_records, record, "catia_feature_range_records")?;
        let prefix = property_prefix_args(ctx, format_args!("catia_range_{ordinal}"))?;
        insert_property(
            ctx,
            &mut properties,
            prefix.as_str(),
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_range_interval_properties(ctx, &mut properties, &prefix, range)?;
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

/// Return whether a design object belongs to one operation's exact structural
/// owner chain. A nearer admitted operation owns the value instead of an
/// outer operation. Missing links and non-reflexive cycles reject the whole
/// chain so a partial owner path cannot invent feature properties.
fn native_operation_owner_chain_reaches(
    ctx: &DecodeContext<'_>,
    design_object_id: &str,
    operation_object_id: &str,
    design_objects: &HashMap<&str, &CatiaDesignObject>,
    native_operation_object_ids: &HashSet<&str>,
) -> Result<bool, CodecError> {
    let mut current = Some(design_object_id);
    let mut traversed = 0usize;
    while let Some(current_id) = current {
        ctx.charge_work(1, "catia_feature_operation_owner_chain")?;
        traversed = traversed.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("catia_feature_operation_owner_chain", u64::MAX, u64::MAX)
        })?;
        if traversed > design_objects.len() {
            return Ok(false);
        }
        if current_id == operation_object_id {
            return Ok(true);
        }
        if native_operation_object_ids.contains(current_id) {
            return Ok(false);
        }
        let Some(object) = design_objects.get(current_id).copied() else {
            return Ok(false);
        };
        current = object
            .owner_design_object
            .as_deref()
            .filter(|parent| *parent != current_id);
    }
    Ok(false)
}

fn property_prefix(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    suffix: &str,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    property_prefix_args(ctx, format_args!("{prefix}{suffix}"))
}

fn property_prefix_args(
    ctx: &DecodeContext<'_>,
    args: std::fmt::Arguments<'_>,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    let key = ctx.format_retained(args, "catia_feature_property_key")?;
    cadmpeg_core::text::NonBlankString::new(key)
        .ok_or_else(|| CodecError::malformed("CATIA feature property key is blank"))
}

fn insert_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &str,
    suffix: &str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let key = property_prefix(ctx, prefix, suffix)?;
    let value = ctx.format_retained(value, "catia_feature_property_value")?;
    ctx.insert_btree_map(properties, key, value, "catia_feature_property_entries")?;
    Ok(())
}

fn insert_schema_value_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySchemaValue,
) -> Result<(), CodecError> {
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_entry",
        format_args!("{}", value.entry),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_ordinal",
        format_args!("{}", value.ordinal),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_offset",
        format_args!("{}", value.offset),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_value",
        format_args!("{}", value.value),
    )?;
    Ok(())
}

fn insert_range_interval_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    range: &CatiaRangeInterval,
) -> Result<(), CodecError> {
    insert_schema_value_properties(
        ctx,
        properties,
        &property_prefix(ctx, prefix.as_str(), "_selector")?,
        &range.range,
    )?;
    match &range.interval.prefix {
        RangeIntervalPrefix::Compact { value, width } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_prefix_kind",
                format_args!("{}", "compact"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_prefix_value",
                format_args!("{value}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_prefix_width",
                format_args!("{width}"),
            )?;
        }
        RangeIntervalPrefix::EscapedWord { word } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_prefix_kind",
                format_args!("{}", "escaped_word"),
            )?;
            insert_property(
                ctx,
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
                ctx,
                properties,
                prefix.as_str(),
                "_slots",
                format_args!("{}", "two"),
            )?;
            insert_range_slot_properties(
                ctx,
                properties,
                &property_prefix(ctx, prefix.as_str(), "_lower")?,
                lower,
            )?;
            insert_range_slot_properties(
                ctx,
                properties,
                &property_prefix(ctx, prefix.as_str(), "_upper")?,
                upper,
            )?;
        }
        None => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_slots",
                format_args!("{}", "none"),
            )?;
        }
    }
    if let Some(nominal) = range.nominal.as_ref() {
        insert_property(
            ctx,
            properties,
            prefix.as_str(),
            "_nominal_kind",
            format_args!("{}", "finite"),
        )?;
        insert_property(
            ctx,
            properties,
            prefix.as_str(),
            "_nominal_framing",
            format_args!("{}", range_nominal_framing_name(nominal.framing)),
        )?;
        insert_property(
            ctx,
            properties,
            prefix.as_str(),
            "_nominal_bits",
            format_args!("{:016x}", nominal.bits),
        )?;
        insert_property(
            ctx,
            properties,
            prefix.as_str(),
            "_nominal_opcode_offset",
            format_args!("{}", nominal.evaluation_opcode_offset),
        )?;
    } else {
        insert_property(
            ctx,
            properties,
            prefix.as_str(),
            "_nominal_kind",
            format_args!("{}", "absent"),
        )?;
    }
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_incoming_payload_reference_count",
        format_args!("{}", range.incoming_references.len()),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_incoming_storage_reference_count",
        format_args!("{}", range.incoming_storage_references.len()),
    )?;
    Ok(())
}

fn insert_range_slot_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    slot: &RangeIntervalSlot,
) -> Result<(), CodecError> {
    match slot {
        RangeIntervalSlot::Binary64 { bits, offset } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "binary64"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_bits",
                format_args!("{bits:016x}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
        }
        RangeIntervalSlot::Unset { offset } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "unset"),
            )?;
            insert_property(
                ctx,
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
    ctx: &DecodeContext<'_>,
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
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_encoding",
                format_args!("{}", evaluation_encoding_name(*encoding)),
            )?;
            insert_evaluation_properties(ctx, properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixPayload::Atom { value } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "atom"),
            )?;
            insert_property(
                ctx,
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
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "schema_selected"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_selector_offset",
                format_args!("{selector_offset}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_selector",
                format_args!("{selector}"),
            )?;
            insert_selected_value_properties(
                ctx,
                properties,
                &property_prefix(ctx, prefix.as_str(), "_selected")?,
                value,
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::ControlE8 => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::ControlE9 => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e9"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::Separator37 => {
            insert_property(
                ctx,
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
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySuffixSelectedValue,
) -> Result<(), CodecError> {
    match value {
        crate::native::CatiaEntitySuffixSelectedValue::Atom { value } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "atom"),
            )?;
            insert_property(
                ctx,
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
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_evaluation_properties(ctx, properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
            insert_property(
                ctx,
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
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "schema_selector"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
            insert_property(
                ctx,
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
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    selection: &crate::native::CatiaEntitySuffixSchemaSelection,
) -> Result<(), CodecError> {
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_offset",
        format_args!("{}", selection.offset),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_ordinal",
        format_args!("{}", selection.ordinal),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_entry",
        format_args!("{}", selection.entry),
    )?;
    insert_property(
        ctx,
        properties,
        prefix.as_str(),
        "_name",
        format_args!("{}", selection.name),
    )?;
    insert_schema_selected_value_properties(
        ctx,
        properties,
        &property_prefix(ctx, prefix.as_str(), "_value")?,
        &selection.value,
    )?;
    Ok(())
}

fn insert_schema_selected_value_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    value: &crate::native::CatiaEntitySuffixSchemaValue,
) -> Result<(), CodecError> {
    match value {
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "atom"),
            )?;
            insert_property(
                ctx,
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
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_evaluation_properties(ctx, properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::ControlE8 => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::Separator37 => {
            insert_property(
                ctx,
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
                ctx,
                properties,
                prefix.as_str(),
                "_kind",
                format_args!("{}", "schema_selector"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_offset",
                format_args!("{offset}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_ordinal",
                format_args!("{ordinal}"),
            )?;
            if let Some(class) = resolution {
                insert_property(
                    ctx,
                    properties,
                    prefix.as_str(),
                    "_entry",
                    format_args!("{}", class.entry),
                )?;
                insert_property(
                    ctx,
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
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &cadmpeg_core::text::NonBlankString,
    evaluation: &crate::native::CatiaEntityEvaluation,
) -> Result<(), CodecError> {
    match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_evaluation",
                format_args!("{}", "unset"),
            )?;
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => {
            insert_property(
                ctx,
                properties,
                prefix.as_str(),
                "_evaluation",
                format_args!("{}", "scalar"),
            )?;
            insert_property(
                ctx,
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
