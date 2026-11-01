// SPDX-License-Identifier: Apache-2.0
//! Transfer of exact CATIA reference history nodes.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane, UnresolvedFamily,
};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::chain_walk::{ChainStep, ChainStops};

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
    let records = (native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record)))
    .collect::<std::collections::HashMap<_, _>>();
    let mut transfer = DesignFeatureTransfer::default();

    for object in native
        .design_objects
        .iter()
        .filter(|object| graph_scope.contains(object.parent.as_str()))
    {
        let plane_candidate = principal_plane_candidate(object, &records);
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
        product_definition: None,
        needs_recompute: false,
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
        product_definition: None,
        needs_recompute: false,
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
        product_definition: None,
        needs_recompute: false,
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
