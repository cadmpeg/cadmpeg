// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane, UnresolvedFamily,
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
