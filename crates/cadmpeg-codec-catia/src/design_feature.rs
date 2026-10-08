// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
use crate::object_graph::PayloadField;

/// Borrowed native indexes shared by feature transfer and parameter ownership.
pub(crate) struct DesignFeatureSources<'native, 'ctx> {
    native: &'native CatiaNative,
    entities: HashMap<&'native str, &'native CatiaEntityRecord>,
    object_records: HashMap<&'native str, &'native CatiaObjectRecord>,
    design_objects: BTreeMap<&'native str, &'native CatiaDesignObject>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'native, 'ctx> DesignFeatureSources<'native, 'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        native: &'native CatiaNative,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "catia_feature_sources")?;
        let entities = storage.with_storage(|| {
            ctx.collect_hash_map(
                native
                    .entity_records
                    .iter()
                    .map(|entity| (entity.id.as_str(), entity)),
                "catia_feature_transfer_entities",
            )
        })?;
        let mut object_records = HashMap::new();
        for graph in ctx.admit_iter(&native.object_graphs, "catia_feature_transfer_graphs")? {
            for record in ctx.admit_iter(&graph.records, "catia_feature_transfer_record_visits")? {
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut object_records,
                        record.id.as_str(),
                        record,
                        "catia_feature_transfer_records",
                    )
                })?;
            }
        }
        let mut design_objects = BTreeMap::new();
        for object in ctx.admit_iter(
            &native.design_objects,
            "catia_feature_transfer_object_visits",
        )? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut design_objects,
                    object.id.as_str(),
                    object,
                    "catia_feature_transfer_objects",
                )
            })?;
        }
        Ok(Self {
            native,
            entities,
            object_records,
            design_objects,
            _storage: storage,
        })
    }
}

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
    #[cfg(test)]
    pub(crate) fn consumed_records(&self) -> impl Iterator<Item = &String> {
        self.principal_plane_records
            .union(&self.sketch_owner_records)
            .chain(self.reference_plane_records.iter())
            .chain(self.native_operation_records.iter())
            .chain(self.native_operation_definition_value_records.iter())
            .chain(self.native_operation_definition_chain_value_records.iter())
            .chain(self.native_operation_range_records.iter())
    }

    /// Tests whether any design-feature transfer consumed an object record.
    pub(crate) fn consumes(
        &self,
        ctx: &DecodeContext<'_>,
        record: &str,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "catia_design_feature_consumed_records";
        for records in [
            &self.principal_plane_records,
            &self.sketch_owner_records,
            &self.reference_plane_records,
            &self.native_operation_records,
            &self.native_operation_definition_value_records,
            &self.native_operation_definition_chain_value_records,
            &self.native_operation_range_records,
        ] {
            if ctx.contains_hash_set(records, record, OPERATION)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Bind parameters to a transferred feature only through their exact
    /// entity-record and object-record ownership chain. The same exact
    /// incidences populate feature-local parameter ordinals.
    pub(crate) fn assign_parameter_owners(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        sources: &DesignFeatureSources<'_, '_>,
    ) -> Result<(), CodecError> {
        let mut resolved_owners = HashMap::new();
        let mut resolved_owner_storage = ctx.reserve_scoped(0, "catia_feature_resolved_owners")?;
        let entities = &sources.entities;
        let object_records = &sources.object_records;
        let design_objects = &sources.design_objects;
        let mut exact_feature_owners = HashMap::new();
        let mut owner_storage = ctx.reserve_scoped(0, "catia_feature_exact_owners")?;

        for parameter in ctx.admit_iter(&mut ir.model.parameters, "catia_feature_visits")? {
            let Some(native_ref) = parameter.native_ref.as_deref() else {
                continue;
            };
            let Some(entity) = ctx.get_hash_map(entities, native_ref, "catia_feature_lookup")?
            else {
                continue;
            };
            let Some(object_record) = ctx.get_hash_map(
                object_records,
                entity.object_record.as_str(),
                "catia_feature_lookup",
            )?
            else {
                continue;
            };
            let Some(design_object) = object_record.design_object.as_deref() else {
                continue;
            };
            let Some(feature_id) = nearest_feature_for_design_object(
                ctx,
                design_object,
                design_objects,
                &self.feature_ids,
                &mut resolved_owners,
                &mut resolved_owner_storage,
            )?
            else {
                continue;
            };
            if parameter.owner.is_none() {
                parameter.owner =
                    Some(feature_id.try_clone_for_decode(ctx, "catia_feature_parameter_owner")?);
            }
            if ctx.equal(
                &parameter.owner.as_ref(),
                &Some(feature_id),
                "catia_feature_parameter_owner_match",
            )? {
                owner_storage.with_storage(|| {
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
                    Ok::<_, CodecError>(())
                })?;
            }
        }

        assign_feature_parameter_ordinals(
            ctx,
            ir,
            entities,
            object_records,
            &exact_feature_owners,
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
        sources: &DesignFeatureSources<'_, '_>,
    ) -> Result<(), CodecError> {
        let (parents, _parent_storage) = ctx
            .with_scoped_storage("catia_feature_parent_map", || {
                self.feature_parents(ctx, ir, sources)
            })?;
        for feature in ctx.admit_iter(&mut ir.model.features, "catia_feature_parent_features")? {
            if let Some(parent) =
                ctx.get_btree_map(&parents, &feature.id, "catia_feature_parent_lookup")?
            {
                feature
                    .dependencies
                    .retain(|dependency| dependency != parent);
            }
        }
        for (child, parent) in ctx.admit_iter(parents, "catia_feature_parent_assignments")? {
            ir.model
                .set_feature_regeneration_parent(ctx, &child, &parent)?;
        }
        Ok(())
    }

    fn feature_parents(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        sources: &DesignFeatureSources<'_, '_>,
    ) -> Result<BTreeMap<FeatureId, FeatureId>, CodecError> {
        let mut resolved_owners = HashMap::new();
        let mut resolved_owner_storage = ctx.reserve_scoped(0, "catia_feature_resolved_owners")?;
        let design_objects = &sources.design_objects;
        let (feature_ordinals, _ordinal_storage) =
            ctx.with_scoped_storage("catia_feature_parent_ordinals", || {
                ctx.collect_hash_map(
                    ir.model
                        .features
                        .iter()
                        .map(|feature| (&feature.id, feature.ordinal)),
                    "catia_feature_parent_ordinals",
                )
            })?;
        let mut parents = BTreeMap::new();
        for feature in ctx.admit_iter(&ir.model.features, "catia_feature_visits")? {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some(object) =
                ctx.get_btree_map(design_objects, native_ref, "catia_feature_lookup")?
            else {
                continue;
            };
            let Some(parent_object) = object.owner_design_object.as_deref() else {
                continue;
            };
            let Some(parent) = nearest_feature_for_design_object(
                ctx,
                parent_object,
                design_objects,
                &self.feature_ids,
                &mut resolved_owners,
                &mut resolved_owner_storage,
            )?
            else {
                continue;
            };
            let Some(parent_ordinal) =
                ctx.get_hash_map(&feature_ordinals, parent, "catia_feature_lookup")?
            else {
                continue;
            };
            if ctx.equal(parent, &feature.id, "catia_feature_parent_match")?
                || *parent_ordinal >= feature.ordinal
            {
                continue;
            }
            let child = feature
                .id
                .try_clone_for_decode(ctx, "catia_feature_parent_child")?;
            let parent = parent.try_clone_for_decode(ctx, "catia_feature_parent_id")?;
            ctx.insert_btree_map(&mut parents, child, parent, "catia_feature_parent_map")?;
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
        sources: &DesignFeatureSources<'_, '_>,
    ) -> Result<(), CodecError> {
        let mut resolved_owners = HashMap::new();
        let mut resolved_owner_storage = ctx.reserve_scoped(0, "catia_feature_resolved_owners")?;
        let mut ordinal_storage = ctx.reserve_scoped(0, "catia_feature_dependency_ordinals")?;
        let mut feature_ordinals = HashMap::new();
        for feature in ctx.admit_iter(
            &ir.model.features,
            "catia_feature_dependency_ordinal_visits",
        )? {
            ordinal_storage.with_storage(|| {
                let id = feature
                    .id
                    .try_clone_for_decode(ctx, "catia_feature_dependency_ordinal_id")?;
                ctx.insert_hash_map(
                    &mut feature_ordinals,
                    id,
                    feature.ordinal,
                    "catia_feature_dependency_ordinals",
                )
            })?;
        }
        for feature in
            ctx.admit_iter(&mut ir.model.features, "catia_feature_dependency_features")?
        {
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some(object) =
                ctx.get_btree_map(&sources.design_objects, native_ref, "catia_feature_lookup")?
            else {
                continue;
            };
            let mut seen_storage = ctx.reserve_scoped(0, "catia_feature_dependency_seen")?;
            let mut seen = HashSet::new();
            for dependency in ctx.admit_iter(
                feature.dependencies.as_slice(),
                "catia_feature_dependency_existing",
            )? {
                seen_storage.with_storage(|| {
                    let id =
                        dependency.try_clone_for_decode(ctx, "catia_feature_dependency_seen_id")?;
                    ctx.insert_hash_set(&mut seen, id, "catia_feature_dependency_seen")
                })?;
            }
            for relation in
                ctx.admit_iter(&object.relations, "catia_feature_dependency_relations")?
            {
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
                    &sources.design_objects,
                    &self.feature_ids,
                    &mut resolved_owners,
                    &mut resolved_owner_storage,
                )?
                else {
                    continue;
                };
                if ctx.equal(target, &feature.id, "catia_feature_dependency_match")?
                    || ctx.contains_hash_set(&seen, target, "catia_feature_dependency_seen")?
                {
                    continue;
                }
                let Some(target_ordinal) =
                    ctx.get_hash_map(&feature_ordinals, target, "catia_feature_lookup")?
                else {
                    continue;
                };
                if *target_ordinal >= feature.ordinal {
                    continue;
                }
                seen_storage.with_storage(|| {
                    let id =
                        target.try_clone_for_decode(ctx, "catia_feature_dependency_seen_id")?;
                    ctx.insert_hash_set(&mut seen, id, "catia_feature_dependency_seen")
                })?;
                let target = target.try_clone_for_decode(ctx, "catia_feature_dependency_target")?;
                feature
                    .dependencies
                    .insert(ctx, target, "catia_feature_dependency_values")?;
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
) -> Result<(), CodecError> {
    drop(ctx.with_scoped_storage("catia_feature_ordinal_tables", || {
        let mut parameters_by_feature = BTreeMap::<FeatureId, Vec<(u64, u64, ParameterId)>>::new();
        for parameter in ctx.admit_iter(&ir.model.parameters, "catia_feature_visits")? {
            let Some(feature_id) =
                ctx.get_hash_map(exact_feature_owners, &parameter.id, "catia_feature_lookup")?
            else {
                continue;
            };
            let Some(entity_id) = parameter.native_ref.as_deref() else {
                continue;
            };
            let Some(entity) = ctx.get_hash_map(entities, entity_id, "catia_feature_lookup")?
            else {
                continue;
            };
            let Some(object_record) = ctx.get_hash_map(
                object_records,
                entity.object_record.as_str(),
                "catia_feature_lookup",
            )?
            else {
                continue;
            };
            if !ctx.contains_key_btree_map(
                &parameters_by_feature,
                feature_id,
                "catia_feature_lookup",
            )? {
                let id =
                    feature_id.try_clone_for_decode(ctx, "catia_feature_parameter_bucket_id")?;
                ctx.insert_btree_map(
                    &mut parameters_by_feature,
                    id,
                    Vec::new(),
                    "catia_feature_parameter_buckets",
                )?;
            }
            let id = parameter
                .id
                .try_clone_for_decode(ctx, "catia_feature_parameter_ordinal_id")?;
            if let Some(parameters) = ctx.get_mut_btree_map(
                &mut parameters_by_feature,
                feature_id,
                "catia_feature_lookup",
            )? {
                ctx.push_vec(
                    parameters,
                    (object_record.byte_offset, entity.byte_offset, id),
                    "catia_feature_parameter_rows",
                )?;
            }
        }

        let mut parameter_ordinals = HashMap::new();
        for (_, parameters) in ctx.admit_iter(&mut parameters_by_feature, "catia_feature_visits")? {
            match parameters.len() {
                0 | 1 => {}
                2 => {
                    if ctx
                        .compare(
                            &parameters[0],
                            &parameters[1],
                            "catia_feature_parameter_order_sort",
                        )?
                        .is_gt()
                    {
                        parameters.swap(0, 1);
                    }
                }
                _ => {
                    ctx.sort_unstable_by(
                        parameters,
                        |value| value,
                        Ord::cmp,
                        "catia_feature_parameter_order_sort",
                    )?;
                }
            }
            for (ordinal, parameter) in ctx
                .admit_iter(parameters.as_slice(), "catia_feature_ordinal_visits")?
                .enumerate()
            {
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

        for parameter in ctx.admit_iter(&mut ir.model.parameters, "catia_feature_visits")? {
            if let Some(ordinal) =
                ctx.get_hash_map(&parameter_ordinals, &parameter.id, "catia_feature_lookup")?
            {
                parameter.ordinal = *ordinal;
            }
        }
        Ok::<_, CodecError>(())
    })?);
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
    drop(
        ctx.with_scoped_storage("catia_document_ordinal_tables", || {
            let mut parameters = Vec::new();
            for parameter in
                ctx.admit_iter(&ir.model.parameters, "catia_document_parameter_visits")?
            {
                if parameter.owner.is_some() {
                    continue;
                }
                let id = parameter
                    .id
                    .try_clone_for_decode(ctx, "catia_document_parameter_sort_id")?;
                ctx.push_vec(
                    &mut parameters,
                    (parameter.ordinal, id),
                    "catia_document_parameter_sort_rows",
                )?;
            }
            match parameters.len() {
                0 | 1 => {}
                2 => {
                    if ctx
                        .compare(
                            &parameters[0],
                            &parameters[1],
                            "catia_document_parameter_order_sort",
                        )?
                        .is_gt()
                    {
                        parameters.swap(0, 1);
                    }
                }
                _ => {
                    ctx.sort_unstable_by(
                        &mut parameters,
                        |value| value,
                        Ord::cmp,
                        "catia_document_parameter_order_sort",
                    )?;
                }
            }

            let mut parameter_ordinals = HashMap::new();
            for (ordinal, (_, parameter)) in ctx
                .admit_iter(parameters, "catia_document_ordinal_visits")?
                .enumerate()
            {
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
            for parameter in ctx.admit_iter(&mut ir.model.parameters, "catia_feature_visits")? {
                if let Some(ordinal) =
                    ctx.get_hash_map(&parameter_ordinals, &parameter.id, "catia_feature_lookup")?
                {
                    parameter.ordinal = *ordinal;
                }
            }
            Ok::<_, CodecError>(())
        })?,
    );
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
    let mut storage = ctx.reserve_scoped(0, "catia_feature_operation_values")?;
    let mut values_by_feature =
        BTreeMap::<&FeatureId, BTreeMap<&str, &cadmpeg_ir::features::DesignParameter>>::new();
    for parameter in ctx.admit_iter(
        &ir.model.parameters,
        "catia_feature_operation_parameter_visits",
    )? {
        let Some(feature_id) =
            ctx.get_hash_map(exact_feature_owners, &parameter.id, "catia_feature_lookup")?
        else {
            continue;
        };
        storage.with_storage(|| {
            let values = ctx
                .entry_btree_map(
                    &mut values_by_feature,
                    feature_id,
                    "catia_feature_operation_values",
                )?
                .or_default();
            ctx.insert_btree_map(
                values,
                parameter.name.as_str(),
                parameter,
                "catia_feature_operation_parameters",
            )
        })?;
    }
    for feature in ctx.admit_iter(
        &mut ir.model.features,
        "catia_feature_operation_feature_visits",
    )? {
        let Some(values) =
            ctx.get_btree_map(&values_by_feature, &feature.id, "catia_feature_lookup")?
        else {
            continue;
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters })
                if parameters.is_empty() =>
            {
                let mut parameters = BTreeMap::new();
                for (name, parameter) in
                    ctx.admit_iter(values, "catia_feature_operation_value_visits")?
                {
                    let (name, name_storage) = ctx.with_scoped_storage(
                        "catia_feature_operation_parameter_name",
                        || -> Result<_, CodecError> {
                            Ok(cadmpeg_core::text::NonBlankString::for_decode(
                                ctx,
                                ctx.copy_retained_text(
                                    name,
                                    "catia_feature_operation_parameter_name",
                                )?,
                                "validate nonblank text",
                            )?)
                        },
                    )?;
                    let Some(name) = name else {
                        continue;
                    };
                    name_storage.commit()?;
                    let expression = ctx.copy_retained_text(
                        &parameter.expression,
                        "catia_feature_operation_expression",
                    )?;
                    ctx.insert_btree_map(
                        &mut parameters,
                        name,
                        expression,
                        "catia_feature_operation_parameters",
                    )?;
                }
                if !parameters.is_empty() {
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
                            parameters,
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
                for (name, parameter) in
                    ctx.admit_iter(values, "catia_feature_operation_value_visits")?
                {
                    if ctx.trim_text(name, "validate nonblank text")?.is_empty() {
                        continue;
                    }
                    let key = ctx.format_retained(
                        format_args!("catia_parameter_{name}"),
                        "catia_feature_source_parameter_key",
                    )?;
                    let key = cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        key,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| CodecError::malformed("CATIA source parameter key is blank"))?;
                    let expression = ctx.copy_retained_text(
                        &parameter.expression,
                        "catia_feature_operation_expression",
                    )?;
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
        .map(|id| id.try_clone_for_decode(ctx, "catia_parameter_scope_id"))
        .transpose()
}

fn normalize_parameter_names(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "catia_parameter_name_indexes")?;
    let mut reserved_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    for parameter in ctx.admit_iter(&ir.model.parameters, "catia_parameter_reserved_visits")? {
        if parameter.name.is_empty() {
            continue;
        }
        storage.with_storage(|| {
            if !ctx.contains_key_hash_map(
                &reserved_by_scope,
                &parameter.owner,
                "catia_parameter_scope_lookup",
            )? {
                let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
                ctx.insert_hash_map(
                    &mut reserved_by_scope,
                    scope,
                    HashSet::new(),
                    "catia_parameter_reserved_scopes",
                )?;
            }
            if let Some(reserved) = ctx.get_mut_hash_map(
                &mut reserved_by_scope,
                &parameter.owner,
                "catia_parameter_scope_lookup",
            )? {
                ctx.insert_string_set(reserved, &parameter.name, "catia_parameter_reserved_names")?;
            }
            Ok::<_, CodecError>(())
        })?;
    }
    let mut used_by_scope = HashMap::<Option<FeatureId>, HashSet<String>>::new();
    let mut next_by_scope = HashMap::<Option<FeatureId>, HashMap<String, u32>>::new();
    for parameter in ctx.admit_iter(&mut ir.model.parameters, "catia_parameter_name_visits")? {
        storage.with_storage(|| {
            if !ctx.contains_key_hash_map(
                &reserved_by_scope,
                &parameter.owner,
                "catia_parameter_scope_lookup",
            )? {
                let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
                ctx.insert_hash_map(
                    &mut reserved_by_scope,
                    scope,
                    HashSet::new(),
                    "catia_parameter_reserved_scopes",
                )?;
            }
            if !ctx.contains_key_hash_map(
                &used_by_scope,
                &parameter.owner,
                "catia_parameter_scope_lookup",
            )? {
                let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
                ctx.insert_hash_map(
                    &mut used_by_scope,
                    scope,
                    HashSet::new(),
                    "catia_parameter_used_scopes",
                )?;
            }
            Ok::<_, CodecError>(())
        })?;
        let Some(reserved) = ctx.get_hash_map(
            &reserved_by_scope,
            &parameter.owner,
            "catia_parameter_scope_lookup",
        )?
        else {
            continue;
        };
        let Some(used) = ctx.get_mut_hash_map(
            &mut used_by_scope,
            &parameter.owner,
            "catia_parameter_scope_lookup",
        )?
        else {
            continue;
        };
        if !parameter.name.is_empty()
            && storage.with_storage(|| {
                ctx.insert_string_set(used, &parameter.name, "catia_parameter_used_names")
            })?
        {
            continue;
        }
        let base = if parameter.name.is_empty() {
            "Parameter"
        } else {
            parameter.name.as_str()
        };
        storage.with_storage(|| -> Result<(), CodecError> {
            if !ctx.contains_key_hash_map(
                &next_by_scope,
                &parameter.owner,
                "catia_parameter_suffix_scope",
            )? {
                let scope = copy_feature_scope(ctx, parameter.owner.as_ref())?;
                ctx.insert_hash_map(
                    &mut next_by_scope,
                    scope,
                    HashMap::new(),
                    "catia_parameter_suffix_scopes",
                )?;
            }
            Ok(())
        })?;
        let Some(next_suffixes) = ctx.get_mut_hash_map(
            &mut next_by_scope,
            &parameter.owner,
            "catia_parameter_suffix_scope",
        )?
        else {
            continue;
        };
        let mut suffix = ctx
            .get_hash_map(next_suffixes, base, "catia_parameter_suffix_lookup")?
            .copied()
            .unwrap_or(1);
        let neutral_name = loop {
            ctx.charge_work(1, "catia_parameter_name_collision")?;
            let (candidate, candidate_storage) =
                ctx.with_scoped_storage("catia_parameter_neutral_name", || {
                    ctx.format_retained(
                        format_args!("{base}#{suffix}"),
                        "catia_parameter_neutral_name",
                    )
                })?;
            suffix = suffix.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_parameter_name_collision", u64::MAX, u64::MAX)
            })?;
            if !ctx.contains_hash_set(reserved, &candidate, "catia_parameter_reserved_lookup")?
                && storage.with_storage(|| {
                    ctx.insert_string_set(used, &candidate, "catia_parameter_used_names")
                })?
            {
                candidate_storage.commit()?;
                break candidate;
            }
        };
        storage.with_storage(|| {
            let key = ctx.copy_retained_text(base, "catia_parameter_suffix_base")?;
            ctx.insert_hash_map(next_suffixes, key, suffix, "catia_parameter_next_suffix")
        })?;
        let source_name = std::mem::replace(&mut parameter.name, neutral_name);
        ctx.insert_btree_map(
            &mut parameter.properties,
            cadmpeg_core::nonblank_literal!("source_name"),
            source_name,
            "catia_parameter_source_property",
        )?;
    }
    Ok(())
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
    design_objects: &BTreeMap<&str, &CatiaDesignObject>,
    feature_ids: &'a HashMap<String, FeatureId>,
    resolved: &mut HashMap<String, Option<&'a FeatureId>>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Option<&'a FeatureId>, CodecError> {
    let mut path_storage = ctx.reserve_scoped(0, "catia_feature_owner_path")?;
    let mut path = Vec::new();
    let mut active = HashSet::new();
    let mut current = Some(start);
    let owner = loop {
        let Some(id) = current else {
            break None;
        };
        ctx.charge_work(1, "catia_feature_owner_chain")?;
        if let Some(owner) =
            ctx.get_hash_map(resolved, id, "catia_feature_resolved_owner_lookup")?
        {
            break *owner;
        }
        if !path_storage
            .with_storage(|| ctx.insert_hash_set(&mut active, id, "catia_feature_owner_path"))?
        {
            break None;
        }
        ctx.push_scoped_vec(&mut path_storage, &mut path, id, "catia_feature_owner_path")?;
        let Some(object) = ctx
            .get_btree_map(design_objects, id, "catia_feature_lookup")?
            .copied()
        else {
            break None;
        };
        if let Some(feature) = ctx.get_hash_map(feature_ids, id, "catia_feature_lookup")? {
            break Some(feature);
        }
        current = object.owner_design_object.as_deref();
    };
    for id in ctx.admit_iter(path, "catia_feature_owner_path_results")? {
        storage.with_storage(|| {
            let key = ctx.copy_retained_text(id, "catia_feature_owner_key")?;
            ctx.insert_hash_map(resolved, key, owner, "catia_feature_resolved_owners")
        })?;
    }
    Ok(owner)
}

/// Transfer exact owner-bound reference history nodes.
pub(crate) fn transfer_design_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    membership_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &mut CadIr,
    sources: &DesignFeatureSources<'_, '_>,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<DesignFeatureTransfer, cadmpeg_core::CodecError> {
    let native = sources.native;
    let records = &sources.object_records;
    let entities = &sources.entities;
    let design_objects = &sources.design_objects;
    let ((native_operation_object_ids, native_operations), _operation_storage) = ctx
        .with_scoped_storage("catia_feature_operation_candidates", || {
            let mut ids = HashSet::new();
            let mut candidates = HashMap::new();
            for object in ctx.admit_iter(
                &native.design_objects,
                "catia_feature_operation_candidate_visits",
            )? {
                if let Some(candidate) = native_operation_candidate(ctx, object, records)? {
                    ctx.insert_hash_set(
                        &mut ids,
                        object.id.as_str(),
                        "catia_feature_operation_object_ids",
                    )?;
                    ctx.insert_hash_map(
                        &mut candidates,
                        object.id.as_str(),
                        candidate,
                        "catia_feature_operation_candidates",
                    )?;
                }
            }
            Ok::<_, CodecError>((ids, candidates))
        })?;
    let (owned_objects, _owned_storage) = ctx
        .with_scoped_storage("catia_feature_operation_groups", || {
            native_operation_owned_objects(ctx, design_objects, &native_operation_object_ids)
        })?;
    let operation_sources = NativeOperationSources {
        object_records: records,
        entities,
        owned_objects: &owned_objects,
    };
    let mut transfer = DesignFeatureTransfer::default();

    for object in ctx.admit_iter(&native.design_objects, "catia_feature_object_visits")? {
        let in_scope = match graph_scope {
            crate::decode::ModelingGraphScope::Unscoped => true,
            crate::decode::ModelingGraphScope::Unresolved => false,
            crate::decode::ModelingGraphScope::Scoped(part) => ctx.equal_bytes(
                part.as_bytes(),
                object.parent.as_bytes(),
                "catia_feature_graph_scope",
            )?,
        };
        if !in_scope {
            continue;
        }
        let (plane_candidate, _plane_storage) = ctx
            .with_scoped_storage("catia_principal_plane_declarations", || {
                principal_plane_candidate(ctx, object, records)
            })?;
        let sketch_owner = sketch_candidate(ctx, object, records)?;
        let reference_plane = reference_plane_candidate(ctx, object, records)?;
        let native_operation = ctx.get_hash_map(
            &native_operations,
            object.id.as_str(),
            "catia_feature_operation_candidate_lookup",
        )?;
        match (
            plane_candidate,
            sketch_owner,
            reference_plane,
            native_operation,
        ) {
            (Some(candidate), None, None, None) => {
                transfer_principal_plane(ctx, membership_storage, ir, &mut transfer, candidate)?;
            }
            (None, Some(owner_record), None, None) => {
                transfer_sketch(
                    ctx,
                    membership_storage,
                    ir,
                    &mut transfer,
                    object,
                    owner_record,
                )?;
            }
            (None, None, Some(candidate), None) => {
                transfer_reference_plane(ctx, membership_storage, ir, &mut transfer, &candidate)?;
            }
            (None, None, None, Some(candidate)) => {
                transfer_native_operation(
                    ctx,
                    membership_storage,
                    ir,
                    &mut transfer,
                    candidate,
                    &operation_sources,
                )?;
            }
            _ => {
                // One object cannot safely occupy two neutral feature identities.
                // Leave all declarations unresolved so the feature-id map cannot
                // overwrite one transfer with another.
            }
        }
    }

    transfer.assign_feature_parents(ctx, ir, sources)?;
    transfer.assign_feature_dependencies(ctx, ir, sources)?;
    Ok(transfer)
}

fn transfer_principal_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    membership_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
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
    let map_feature_id = membership_storage
        .with_storage(|| feature_id.try_clone_for_decode(ctx, "catia_principal_feature_map_id"))?;
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
    let map_key = membership_storage
        .with_storage(|| ctx.copy_retained_text(&object.id, "catia_principal_feature_key"))?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_map(
            &mut transfer.feature_ids,
            map_key,
            map_feature_id,
            "catia_principal_feature_ids",
        )
    })?;
    for record in ctx.admit_iter(candidate.declarations, "catia_principal_record_visits")? {
        let id = membership_storage
            .with_storage(|| ctx.copy_retained_text(&record.id, "catia_principal_record_id"))?;
        membership_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut transfer.principal_plane_records,
                id,
                "catia_principal_records",
            )
        })?;
    }
    Ok(())
}

fn transfer_reference_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    membership_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
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
    let map_feature_id = membership_storage
        .with_storage(|| feature_id.try_clone_for_decode(ctx, "catia_reference_feature_map_id"))?;
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
    let map_key = membership_storage
        .with_storage(|| ctx.copy_retained_text(&object.id, "catia_reference_feature_key"))?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_map(
            &mut transfer.feature_ids,
            map_key,
            map_feature_id,
            "catia_reference_feature_ids",
        )
    })?;
    let record = membership_storage.with_storage(|| {
        ctx.copy_retained_text(&candidate.owner_record.id, "catia_reference_record_id")
    })?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_set(
            &mut transfer.reference_plane_records,
            record,
            "catia_reference_records",
        )
    })?;
    Ok(())
}

fn transfer_sketch(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    membership_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
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
    let map_feature_id = membership_storage.with_storage(|| {
        feature_id.try_clone_for_decode(ctx, "catia_design_sketch_feature_map_id")
    })?;
    let feature_ref = ctx.copy_retained_text(&object.id, "catia_design_sketch_feature_ref")?;
    let source_tag = {
        let mut text = ctx.retained_string(6, "catia_design_sketch_feature_tag")?;
        text.push_str("Sketch");
        text
    };
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
    let map_key = membership_storage
        .with_storage(|| ctx.copy_retained_text(&object.id, "catia_design_sketch_feature_key"))?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_map(
            &mut transfer.feature_ids,
            map_key,
            map_feature_id,
            "catia_design_sketch_feature_ids",
        )
    })?;
    let record = membership_storage.with_storage(|| {
        ctx.copy_retained_text(&owner_record.id, "catia_design_sketch_owner_id")
    })?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_set(
            &mut transfer.sketch_owner_records,
            record,
            "catia_design_sketch_owners",
        )
    })?;
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
    ctx: &DecodeContext<'_>,
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Result<Option<ReferencePlaneCandidate<'a>>, CodecError> {
    let Some(owner_class) = object.owner_class.as_ref() else {
        return Ok(None);
    };
    if !is_admitted_native_reference_plane_class(&owner_class.name) {
        return Ok(None);
    }
    let Some(owner_record_id) = object.owner_record.as_deref() else {
        return Ok(None);
    };
    let Some(owner_record) = ctx
        .get_hash_map(records, owner_record_id, "catia_feature_candidate_lookup")?
        .copied()
    else {
        return Ok(None);
    };
    Ok(
        (owner_record.class_name() == Some(owner_class.name.as_str())
            && ctx.equal(
                &owner_record.class_entry(),
                &Some(owner_class.entry.as_str()),
                "catia_feature_candidate_class_entry",
            )?
            && owner_record.entity_id() == Some(object.owner_entity_id)
            && ctx.equal(
                &owner_record.design_object.as_deref(),
                &object.owner_design_object.as_deref(),
                "catia_feature_candidate_owner",
            )?)
        .then_some(ReferencePlaneCandidate {
            object,
            owner_record,
            kind: owner_class.name.as_str(),
        }),
    )
}

fn native_operation_candidate<'a>(
    ctx: &DecodeContext<'_>,
    object: &'a CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Result<Option<NativeOperationCandidate<'a>>, CodecError> {
    let Some(owner_record_id) = object.owner_record.as_deref() else {
        return Ok(None);
    };
    let Some(owner_record) = ctx
        .get_hash_map(records, owner_record_id, "catia_feature_candidate_lookup")?
        .copied()
    else {
        return Ok(None);
    };
    // Only a complete separator-form owner declaration states an operation class.
    let Some(owner_class) = object.owner_class.as_ref() else {
        return Ok(None);
    };
    let Ok(kind) = NativeOperationClass::try_from(owner_class.name.as_str()) else {
        return Ok(None);
    };
    Ok(
        (owner_record.class_name() == Some(owner_class.name.as_str())
            && ctx.equal(
                &owner_record.class_entry(),
                &Some(owner_class.entry.as_str()),
                "catia_feature_candidate_class_entry",
            )?
            && owner_record.entity_id() == Some(object.owner_entity_id)
            && ctx.equal(
                &owner_record.design_object.as_deref(),
                &object.owner_design_object.as_deref(),
                "catia_feature_candidate_owner",
            )?)
        .then_some(NativeOperationCandidate {
            object,
            owner_record,
            kind,
        }),
    )
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
    membership_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &mut CadIr,
    transfer: &mut DesignFeatureTransfer,
    candidate: &NativeOperationCandidate<'_>,
    sources: &NativeOperationSources<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut record_storage = ctx.reserve_scoped(0, "catia_feature_property_record_workspace")?;
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
        &mut record_storage,
        object,
        sources.object_records,
        sources.entities,
        ctx.get_hash_map(
            sources.owned_objects,
            object.id.as_str(),
            "catia_feature_operation_group_lookup",
        )?
        .map_or(&[], Vec::as_slice),
    )?;
    let definition = native_operation_definition(ctx, kind, &object.id)?;
    let feature_id = FeatureId::from(neutral_history_id(
        ctx,
        &object.id,
        &cadmpeg_ir::identity_component!("feature"),
    )?);
    ctx.charge_entities(1, "admit CATIA design feature")?;
    let map_feature_id = membership_storage.with_storage(|| {
        feature_id.try_clone_for_decode(ctx, "catia_native_operation_feature_map_id")
    })?;
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
    let map_key = membership_storage.with_storage(|| {
        ctx.copy_retained_text(&object.id, "catia_native_operation_feature_key")
    })?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_map(
            &mut transfer.feature_ids,
            map_key,
            map_feature_id,
            "catia_native_operation_feature_ids",
        )
    })?;
    let owner_id = membership_storage.with_storage(|| {
        ctx.copy_retained_text(
            &candidate.owner_record.id,
            "catia_native_operation_owner_id",
        )
    })?;
    membership_storage.with_storage(|| {
        ctx.insert_hash_set(
            &mut transfer.native_operation_records,
            owner_id,
            "catia_native_operation_records",
        )
    })?;
    transfer.native_operation_definition_value_count += definition_value_count;
    transfer.native_operation_definition_chain_value_count += definition_chain_value_count;
    transfer.native_operation_range_count += range_count;
    for record in ctx.admit_iter(
        definition_value_records,
        "catia_feature_property_record_visits",
    )? {
        let record = membership_storage
            .with_storage(|| ctx.copy_retained_text(record, "catia_feature_transferred_record"))?;
        membership_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut transfer.native_operation_definition_value_records,
                record,
                "catia_native_operation_definition_records",
            )
        })?;
    }
    for record in ctx.admit_iter(
        definition_chain_value_records,
        "catia_feature_property_record_visits",
    )? {
        let record = membership_storage
            .with_storage(|| ctx.copy_retained_text(record, "catia_feature_transferred_record"))?;
        membership_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut transfer.native_operation_definition_chain_value_records,
                record,
                "catia_native_operation_chain_records",
            )
        })?;
    }
    for record in ctx.admit_iter(range_records, "catia_feature_property_record_visits")? {
        let record = membership_storage
            .with_storage(|| ctx.copy_retained_text(record, "catia_feature_transferred_record"))?;
        membership_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut transfer.native_operation_range_records,
                record,
                "catia_native_operation_range_records",
            )
        })?;
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
struct NativeOperationDefinitionProperties<'a> {
    source_properties: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    definition_value_count: usize,
    definition_chain_value_count: usize,
    range_count: usize,
    definition_value_records: BTreeSet<&'a str>,
    definition_chain_value_records: BTreeSet<&'a str>,
    range_records: BTreeSet<&'a str>,
}

/// Retain complete definition-bound values and source-schema `Range` fields
/// on the exact native operation owner chain. A one-definition value carries
/// a source definition and typed suffix payload, while a two-definition value
/// carries its repeated selector, role, and selected payload. Neither
/// production assigns an operation role here. Supported two-definition roles
/// are exposed separately through the typed-parameter transfer.
fn native_operation_definition_properties<'a>(
    ctx: &DecodeContext<'_>,
    membership_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    object: &'a CatiaDesignObject,
    object_records: &HashMap<&'a str, &'a CatiaObjectRecord>,
    entities: &HashMap<&'a str, &'a CatiaEntityRecord>,
    owned_objects: &[&'a CatiaDesignObject],
) -> Result<NativeOperationDefinitionProperties<'a>, CodecError> {
    let mut properties = BTreeMap::new();
    let mut definition_value_count = 0;
    let mut definition_chain_value_count = 0;
    let mut range_count = 0;
    let mut definition_value_records = BTreeSet::new();
    let mut definition_chain_value_records = BTreeSet::new();
    let mut range_records = BTreeSet::new();

    let mut definition_values_storage = ctx.reserve_scoped(0, "catia_feature_definition_values")?;
    let mut definition_chain_values_storage =
        ctx.reserve_scoped(0, "catia_feature_definition_chain_values")?;
    let mut range_intervals_storage = ctx.reserve_scoped(0, "catia_feature_range_intervals")?;
    let mut definition_values = Vec::new();
    let mut definition_chain_values = Vec::new();
    let mut range_intervals = Vec::new();
    for owned in ctx.admit_iter(owned_objects, "catia_feature_operation_object_visits")? {
        if !ctx.equal(
            &owned.parent,
            &object.parent,
            "catia_feature_operation_parent_match",
        )? {
            continue;
        }
        for entity_id in ctx.admit_iter(
            &owned.definition_values,
            "catia_feature_definition_value_visits",
        )? {
            let Some(entity) = ctx
                .get_hash_map(entities, entity_id.as_str(), "catia_feature_lookup")?
                .copied()
            else {
                continue;
            };
            if let Some(value) = entity.definition_value() {
                definition_values_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut definition_values,
                        (entity, value),
                        "catia_feature_definition_values",
                    )
                })?;
            }
        }
        for entity_id in ctx.admit_iter(
            &owned.definition_chain_values,
            "catia_feature_definition_chain_visits",
        )? {
            let Some(entity) = ctx
                .get_hash_map(entities, entity_id.as_str(), "catia_feature_lookup")?
                .copied()
            else {
                continue;
            };
            if let Some(value) = entity.definition_chain_value() {
                definition_chain_values_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut definition_chain_values,
                        (entity, value),
                        "catia_feature_definition_chain_values",
                    )
                })?;
            }
        }
        for field_id in ctx.admit_iter(&owned.fields, "catia_feature_range_field_visits")? {
            let Some(field) = ctx
                .get_hash_map(object_records, field_id.as_str(), "catia_feature_lookup")?
                .copied()
            else {
                continue;
            };
            if !ctx.equal(
                &field.design_object.as_deref(),
                &Some(owned.id.as_str()),
                "catia_feature_range_owner_match",
            )? {
                continue;
            }
            let Some(entity_id) = field.entity_record() else {
                continue;
            };
            let Some(entity) = ctx
                .get_hash_map(entities, entity_id, "catia_feature_lookup")?
                .copied()
            else {
                continue;
            };
            if !ctx.equal(
                &entity.object_record,
                &field.id,
                "catia_feature_range_record_match",
            )? {
                continue;
            }
            if let Some(range) = entity.range_interval.as_ref() {
                range_intervals_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut range_intervals,
                        (entity, range),
                        "catia_feature_range_intervals",
                    )
                })?;
            }
        }
    }

    ctx.sort_unstable_by_key(
        &mut definition_values,
        |value| {
            let entity = value.0;
            (entity.byte_offset, entity.ordinal, entity.id.as_str())
        },
        Ord::cmp,
        "catia_feature_definition_values_sort",
    )?;
    for (ordinal, (entity, value)) in ctx
        .admit_iter(definition_values, "catia_feature_property_value_visits")?
        .enumerate()
    {
        definition_value_count += 1;
        let record = entity.object_record.as_str();
        membership_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut definition_value_records,
                record,
                "catia_feature_definition_records",
            )
        })?;
        let prefix = OrdinalPropertyPrefix {
            label: "catia_definition_value_",
            ordinal,
        };
        let prefix: &dyn std::fmt::Display = &prefix;
        insert_property(
            ctx,
            &mut properties,
            prefix,
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_schema_value_properties(
            ctx,
            &mut properties,
            &property_prefix(prefix, "_definition"),
            &value.definition,
        )?;
        insert_suffix_payload_properties(
            ctx,
            &mut properties,
            &property_prefix(prefix, "_payload"),
            &value.payload,
        )?;
        if let Some(selection) = value.schema_selection.as_ref() {
            insert_schema_selection_properties(
                ctx,
                &mut properties,
                &property_prefix(prefix, "_schema_selection"),
                selection,
            )?;
        }
    }

    drop(definition_values_storage);
    ctx.sort_unstable_by_key(
        &mut definition_chain_values,
        |value| {
            let entity = value.0;
            (entity.byte_offset, entity.ordinal, entity.id.as_str())
        },
        Ord::cmp,
        "catia_feature_definition_chain_values_sort",
    )?;
    for (ordinal, (entity, value)) in ctx
        .admit_iter(
            definition_chain_values,
            "catia_feature_property_value_visits",
        )?
        .enumerate()
    {
        definition_chain_value_count += 1;
        let record = entity.object_record.as_str();
        membership_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut definition_chain_value_records,
                record,
                "catia_feature_chain_records",
            )
        })?;
        let prefix = OrdinalPropertyPrefix {
            label: "catia_definition_chain_value_",
            ordinal,
        };
        let prefix: &dyn std::fmt::Display = &prefix;
        insert_property(
            ctx,
            &mut properties,
            prefix,
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_schema_value_properties(
            ctx,
            &mut properties,
            &property_prefix(prefix, "_selector"),
            &value.selector,
        )?;
        insert_schema_value_properties(
            ctx,
            &mut properties,
            &property_prefix(prefix, "_role"),
            &value.role,
        )?;
        insert_schema_selected_value_properties(
            ctx,
            &mut properties,
            &property_prefix(prefix, "_value"),
            &value.value,
        )?;
    }

    drop(definition_chain_values_storage);
    ctx.sort_unstable_by_key(
        &mut range_intervals,
        |value| {
            let entity = value.0;
            (entity.byte_offset, entity.ordinal, entity.id.as_str())
        },
        Ord::cmp,
        "catia_feature_range_intervals_sort",
    )?;
    ctx.dedup_by(
        &mut range_intervals,
        |(left, _), (right, _)| {
            ctx.equal(&left.id, &right.id, "catia_feature_range_identity_match")
        },
        "catia_feature_range_dedup",
    )?;
    for (ordinal, (entity, range)) in ctx
        .admit_iter(range_intervals, "catia_feature_property_value_visits")?
        .enumerate()
    {
        range_count += 1;
        let record = entity.object_record.as_str();
        membership_storage.with_storage(|| {
            ctx.insert_btree_set(&mut range_records, record, "catia_feature_range_records")
        })?;
        let prefix = OrdinalPropertyPrefix {
            label: "catia_range_",
            ordinal,
        };
        let prefix: &dyn std::fmt::Display = &prefix;
        insert_property(
            ctx,
            &mut properties,
            prefix,
            "_entity",
            format_args!("{}", entity.id),
        )?;
        insert_range_interval_properties(ctx, &mut properties, &prefix, range)?;
    }

    drop(range_intervals_storage);
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

/// Group design objects by their nearest native operation, keeping source key order.
fn native_operation_owned_objects<'a>(
    ctx: &DecodeContext<'_>,
    objects: &BTreeMap<&'a str, &'a CatiaDesignObject>,
    operations: &HashSet<&'a str>,
) -> Result<HashMap<&'a str, Vec<&'a CatiaDesignObject>>, CodecError> {
    if operations.is_empty() {
        return Ok(HashMap::new());
    }
    let mut cache_storage = ctx.reserve_scoped(0, "catia_feature_operation_owner_cache")?;
    let mut resolved = HashMap::<&str, Option<&str>>::new();
    let mut groups = HashMap::new();
    for (id, object) in ctx.admit_iter(objects, "catia_feature_operation_owner_objects")? {
        let mut path_storage = ctx.reserve_scoped(0, "catia_feature_operation_owner_path")?;
        let mut path = Vec::new();
        let mut active = HashSet::new();
        let mut current = Some(*id);
        let owner = loop {
            let Some(current_id) = current else {
                break None;
            };
            ctx.charge_work(1, "catia_feature_operation_owner_chain")?;
            if let Some(owner) = ctx.get_hash_map(
                &resolved,
                current_id,
                "catia_feature_operation_owner_lookup",
            )? {
                break *owner;
            }
            if !path_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut active,
                    current_id,
                    "catia_feature_operation_owner_path",
                )
            })? {
                break None;
            }
            ctx.push_scoped_vec(
                &mut path_storage,
                &mut path,
                current_id,
                "catia_feature_operation_owner_path",
            )?;
            if ctx.contains_hash_set(
                operations,
                current_id,
                "catia_feature_operation_owner_lookup",
            )? {
                break Some(current_id);
            }
            let Some(current_object) = ctx
                .get_btree_map(objects, current_id, "catia_feature_lookup")?
                .copied()
            else {
                break None;
            };
            current = current_object.owner_design_object.as_deref();
        };
        for item in ctx.admit_iter(path, "catia_feature_operation_owner_results")? {
            cache_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut resolved,
                    item,
                    owner,
                    "catia_feature_operation_owner_cache",
                )
            })?;
        }
        if let Some(owner) = owner {
            ctx.push_hash_group(
                &mut groups,
                owner,
                *object,
                "catia_feature_operation_groups",
                "catia_feature_operation_group_members",
            )?;
        }
    }
    Ok(groups)
}

struct OrdinalPropertyPrefix {
    label: &'static str,
    ordinal: usize,
}

impl std::fmt::Display for OrdinalPropertyPrefix {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{}{}", self.label, self.ordinal)
    }
}

struct PropertyPrefix<'a> {
    parent: &'a dyn std::fmt::Display,
    suffix: &'a str,
}

impl std::fmt::Display for PropertyPrefix<'_> {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "{}{}", self.parent, self.suffix)
    }
}

fn property_prefix<'a>(parent: &'a dyn std::fmt::Display, suffix: &'a str) -> PropertyPrefix<'a> {
    PropertyPrefix { parent, suffix }
}

fn insert_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &dyn std::fmt::Display,
    suffix: &str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let key = ctx.format_retained(
        format_args!("{prefix}{suffix}"),
        "catia_feature_property_key",
    )?;
    let key = cadmpeg_core::text::NonBlankString::for_decode(ctx, key, "validate nonblank text")?
        .ok_or_else(|| CodecError::malformed("CATIA feature property key is blank"))?;
    let value = ctx.format_retained(value, "catia_feature_property_value")?;
    ctx.insert_btree_map(properties, key, value, "catia_feature_property_entries")?;
    Ok(())
}

fn insert_schema_value_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &dyn std::fmt::Display,
    value: &crate::native::CatiaEntitySchemaValue,
) -> Result<(), CodecError> {
    insert_property(
        ctx,
        properties,
        prefix,
        "_entry",
        format_args!("{}", value.entry),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_ordinal",
        format_args!("{}", value.ordinal),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_offset",
        format_args!("{}", value.offset),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_value",
        format_args!("{}", value.value),
    )?;
    Ok(())
}

fn insert_range_interval_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &dyn std::fmt::Display,
    range: &CatiaRangeInterval,
) -> Result<(), CodecError> {
    insert_schema_value_properties(
        ctx,
        properties,
        &property_prefix(prefix, "_selector"),
        &range.range,
    )?;
    match &range.interval.prefix {
        RangeIntervalPrefix::Compact { value, width } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_prefix_kind",
                format_args!("{}", "compact"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_prefix_value",
                format_args!("{value}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_prefix_width",
                format_args!("{width}"),
            )?;
        }
        RangeIntervalPrefix::EscapedWord { word } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_prefix_kind",
                format_args!("{}", "escaped_word"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_prefix_word",
                format_args!("{word}"),
            )?;
        }
    }
    match &range.interval.slots {
        Some([lower, upper]) => {
            insert_property(ctx, properties, prefix, "_slots", format_args!("{}", "two"))?;
            insert_range_slot_properties(
                ctx,
                properties,
                &property_prefix(prefix, "_lower"),
                lower,
            )?;
            insert_range_slot_properties(
                ctx,
                properties,
                &property_prefix(prefix, "_upper"),
                upper,
            )?;
        }
        None => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_slots",
                format_args!("{}", "none"),
            )?;
        }
    }
    if let Some(nominal) = range.nominal.as_ref() {
        insert_property(
            ctx,
            properties,
            prefix,
            "_nominal_kind",
            format_args!("{}", "finite"),
        )?;
        insert_property(
            ctx,
            properties,
            prefix,
            "_nominal_framing",
            format_args!("{}", range_nominal_framing_name(nominal.framing)),
        )?;
        insert_property(
            ctx,
            properties,
            prefix,
            "_nominal_bits",
            format_args!("{:016x}", nominal.bits),
        )?;
        insert_property(
            ctx,
            properties,
            prefix,
            "_nominal_opcode_offset",
            format_args!("{}", nominal.evaluation_opcode_offset),
        )?;
    } else {
        insert_property(
            ctx,
            properties,
            prefix,
            "_nominal_kind",
            format_args!("{}", "absent"),
        )?;
    }
    insert_property(
        ctx,
        properties,
        prefix,
        "_incoming_payload_reference_count",
        format_args!("{}", range.incoming_references.len()),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_incoming_storage_reference_count",
        format_args!("{}", range.incoming_storage_references.len()),
    )?;
    Ok(())
}

fn insert_range_slot_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &dyn std::fmt::Display,
    slot: &RangeIntervalSlot,
) -> Result<(), CodecError> {
    match slot {
        RangeIntervalSlot::Binary64 { bits, offset } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "binary64"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_bits",
                format_args!("{bits:016x}"),
            )?;
            insert_property(ctx, properties, prefix, "_offset", format_args!("{offset}"))?;
        }
        RangeIntervalSlot::Unset { offset } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "unset"),
            )?;
            insert_property(ctx, properties, prefix, "_offset", format_args!("{offset}"))?;
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
    prefix: &dyn std::fmt::Display,
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
                prefix,
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_encoding",
                format_args!("{}", evaluation_encoding_name(*encoding)),
            )?;
            insert_evaluation_properties(ctx, properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixPayload::Atom { value } => {
            insert_property(ctx, properties, prefix, "_kind", format_args!("{}", "atom"))?;
            insert_property(ctx, properties, prefix, "_value", format_args!("{value}"))?;
        }
        crate::native::CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset,
            selector,
            value,
        } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "schema_selected"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_selector_offset",
                format_args!("{selector_offset}"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_selector",
                format_args!("{selector}"),
            )?;
            insert_selected_value_properties(
                ctx,
                properties,
                &property_prefix(prefix, "_selected"),
                value,
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::ControlE8 => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::ControlE9 => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "control_e9"),
            )?;
        }
        crate::native::CatiaEntitySuffixPayload::Separator37 => {
            insert_property(
                ctx,
                properties,
                prefix,
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
    prefix: &dyn std::fmt::Display,
    value: &crate::native::CatiaEntitySuffixSelectedValue,
) -> Result<(), CodecError> {
    match value {
        crate::native::CatiaEntitySuffixSelectedValue::Atom { value } => {
            insert_property(ctx, properties, prefix, "_kind", format_args!("{}", "atom"))?;
            insert_property(ctx, properties, prefix, "_value", format_args!("{value}"))?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_evaluation_properties(ctx, properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
            insert_property(
                ctx,
                properties,
                prefix,
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
                prefix,
                "_kind",
                format_args!("{}", "schema_selector"),
            )?;
            insert_property(ctx, properties, prefix, "_offset", format_args!("{offset}"))?;
            insert_property(
                ctx,
                properties,
                prefix,
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
    prefix: &dyn std::fmt::Display,
    selection: &crate::native::CatiaEntitySuffixSchemaSelection,
) -> Result<(), CodecError> {
    insert_property(
        ctx,
        properties,
        prefix,
        "_offset",
        format_args!("{}", selection.offset),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_ordinal",
        format_args!("{}", selection.ordinal),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_entry",
        format_args!("{}", selection.entry),
    )?;
    insert_property(
        ctx,
        properties,
        prefix,
        "_name",
        format_args!("{}", selection.name),
    )?;
    insert_schema_selected_value_properties(
        ctx,
        properties,
        &property_prefix(prefix, "_value"),
        &selection.value,
    )?;
    Ok(())
}

fn insert_schema_selected_value_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    prefix: &dyn std::fmt::Display,
    value: &crate::native::CatiaEntitySuffixSchemaValue,
) -> Result<(), CodecError> {
    match value {
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value } => {
            insert_property(ctx, properties, prefix, "_kind", format_args!("{}", "atom"))?;
            insert_property(ctx, properties, prefix, "_atom", format_args!("{value}"))?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "evaluation"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_opcode_offset",
                format_args!("{opcode_offset}"),
            )?;
            insert_evaluation_properties(ctx, properties, prefix, evaluation)?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::ControlE8 => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_kind",
                format_args!("{}", "control_e8"),
            )?;
        }
        crate::native::CatiaEntitySuffixSchemaValue::Separator37 => {
            insert_property(
                ctx,
                properties,
                prefix,
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
                prefix,
                "_kind",
                format_args!("{}", "schema_selector"),
            )?;
            insert_property(ctx, properties, prefix, "_offset", format_args!("{offset}"))?;
            insert_property(
                ctx,
                properties,
                prefix,
                "_ordinal",
                format_args!("{ordinal}"),
            )?;
            if let Some(class) = resolution {
                insert_property(
                    ctx,
                    properties,
                    prefix,
                    "_entry",
                    format_args!("{}", class.entry),
                )?;
                insert_property(
                    ctx,
                    properties,
                    prefix,
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
    prefix: &dyn std::fmt::Display,
    evaluation: &crate::native::CatiaEntityEvaluation,
) -> Result<(), CodecError> {
    match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_evaluation",
                format_args!("{}", "unset"),
            )?;
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => {
            insert_property(
                ctx,
                properties,
                prefix,
                "_evaluation",
                format_args!("{}", "scalar"),
            )?;
            insert_property(
                ctx,
                properties,
                prefix,
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
    let Some(declarations) = ctx.collect_fallible_options(
        object.fields.iter().map(|field| {
            Ok::<_, CodecError>(
                ctx.get_hash_map(records, field.as_str(), "catia_feature_candidate_lookup")?
                    .copied(),
            )
        }),
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
    Ok(ctx
        .all_by(
            &declarations,
            |record| {
                Ok(record.class_name() == Some(class_name)
                    && ctx.equal(
                        &record.class_entry(),
                        &Some(class_entry),
                        "catia_principal_declaration_class_entry",
                    )?
                    && complete_empty_declaration(ctx, record, &object.id, object.owner_entity_id)?)
            },
            "catia_principal_declaration_checks",
        )?
        .then_some(PrincipalPlaneCandidate {
            object,
            declarations,
            plane,
            declaration_class: class_name,
        }))
}

fn sketch_candidate<'a>(
    ctx: &DecodeContext<'_>,
    object: &CatiaDesignObject,
    records: &HashMap<&str, &'a CatiaObjectRecord>,
) -> Result<Option<&'a CatiaObjectRecord>, CodecError> {
    let Some(owner_class) = object.owner_class.as_ref() else {
        return Ok(None);
    };
    if owner_class.name != "Sketch" {
        return Ok(None);
    }
    let Some(owner_record_id) = object.owner_record.as_deref() else {
        return Ok(None);
    };
    let Some(owner_record) = ctx
        .get_hash_map(records, owner_record_id, "catia_feature_candidate_lookup")?
        .copied()
    else {
        return Ok(None);
    };
    Ok((owner_record.class_name() == Some("Sketch")
        && ctx.equal(
            &owner_record.class_entry(),
            &Some(owner_class.entry.as_str()),
            "catia_feature_candidate_class_entry",
        )?
        && ctx.equal(
            &owner_record.design_object.as_deref(),
            &object.owner_design_object.as_deref(),
            "catia_feature_candidate_owner",
        )?)
    .then_some(owner_record))
}

fn principal_plane(class_name: &str) -> Option<PrincipalPlane> {
    match class_name {
        "xy-plane" => Some(PrincipalPlane::Top),
        "yz-plane" => Some(PrincipalPlane::Right),
        "zx-plane" => Some(PrincipalPlane::Front),
        _ => None,
    }
}

fn complete_empty_declaration(
    ctx: &DecodeContext<'_>,
    record: &CatiaObjectRecord,
    design_object: &str,
    owner_entity_id: u32,
) -> Result<bool, CodecError> {
    Ok(ctx.equal(
        &record.design_object.as_deref(),
        &Some(design_object),
        "catia_principal_declaration_owner",
    )? && record.owner_entity_id() == Some(owner_entity_id)
        && record.storage_ref().is_none()
        && record.references.is_empty()
        && record.payload.size == 1
        && matches!(record.payload.fields.as_slice(), [PayloadField::Terminator]))
}

#[cfg(test)]
mod tests;
