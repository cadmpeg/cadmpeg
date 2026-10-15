// SPDX-License-Identifier: Apache-2.0
//! Sketch binding, regeneration order, and feature-output derivation.

use crate::records::FeatureHistory;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, PathRef, PlanarProfileRef, ProfileRef,
    SplitFaceTool,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::Face;
use std::collections::{HashMap, HashSet};

use crate::history::classify::is_history_metadata_record;

struct SketchBinding {
    index: usize,
    feature_id: FeatureId,
    native_ref: String,
    sketch: cadmpeg_ir::sketches::SketchId,
    has_profile: bool,
}

pub(crate) fn bind_unique_sketch_feature(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[FeatureHistory],
) {
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(feature.id.as_str(), feature);
    }
    let mut feature_indices = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
        ) {
            feature_indices.push(index);
        }
    }
    let mut bindings = Vec::new();
    for index in &feature_indices {
        let Some(name) = features[*index].name.as_deref() else {
            continue;
        };
        if feature_indices
            .iter()
            .filter(|other| features[**other].name.as_deref() == Some(name))
            .count()
            != 1
        {
            continue;
        }
        let mut matches = sketches
            .iter()
            .filter(|sketch| sketch.name.as_deref() == Some(name));
        let Some(sketch) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        if let Some(native_ref) = features[*index].native_ref.as_deref() {
            bindings.push(SketchBinding {
                index: *index,
                feature_id: features[*index].id.clone(),
                native_ref: native_ref.to_owned(),
                sketch: sketch.id.clone(),
                has_profile: !sketch.profiles.is_empty(),
            });
        }
    }
    if bindings.is_empty() {
        if let ([index], [sketch]) = (feature_indices.as_slice(), sketches) {
            if let Some(native_ref) = features[*index].native_ref.as_deref() {
                bindings.push(SketchBinding {
                    index: *index,
                    feature_id: features[*index].id.clone(),
                    native_ref: native_ref.to_owned(),
                    sketch: sketch.id.clone(),
                    has_profile: !sketch.profiles.is_empty(),
                });
            }
        }
    }
    for binding in &bindings {
        features[binding.index]
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    binding.sketch.clone(),
                )),
            }));
    }
    let mut aliases = Vec::new();
    for index in &feature_indices {
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch:
                cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            ..
        }) = features[*index].evaluation.definition()
        else {
            continue;
        };
        let Some(base_name) = features[*index]
            .name
            .as_deref()
            .and_then(sketch_alias_base_name)
        else {
            continue;
        };
        let mut candidates = feature_indices
            .iter()
            .filter(|base_index| {
                let alias_native = features[*index]
                    .native_ref
                    .as_deref()
                    .and_then(|native_ref| native_features.get(native_ref));
                let base_native = features[**base_index]
                    .native_ref
                    .as_deref()
                    .and_then(|native_ref| native_features.get(native_ref));
                features[**base_index].name.as_deref() == Some(base_name)
                    && alias_native.zip(base_native).is_some_and(|(alias, base)| {
                        let compatible_class = alias.input_class == base.input_class
                            || (alias.input_class.is_none()
                                && base.input_class.as_deref() == Some("moProfileFeature_c")
                                && crate::resolved_features::component_paths::is_dissected_profile_feature(
                                    alias,
                                ));
                        alias.xml_tag == base.xml_tag
                            && compatible_class
                            && alias.parameters == base.parameters
                            && alias.content == base.content
                    })
            });
        let Some(base_index) = candidates.next() else {
            continue;
        };
        if candidates.next().is_some() {
            continue;
        }
        let base_index = *base_index;
        let base_dependency = features[base_index].id.clone();
        let Some(native_ref) = features[*index].native_ref.as_deref() else {
            continue;
        };
        let native_ref = native_ref.to_owned();
        features[*index]
            .dependencies
            .insert(base_dependency.clone());
        let Some(binding) = bindings.iter().find(|binding| binding.index == base_index) else {
            continue;
        };
        let sketch = binding.sketch.clone();
        aliases.push(SketchBinding {
            index: base_index,
            feature_id: base_dependency,
            native_ref,
            sketch,
            has_profile: binding.has_profile,
        });
    }
    bindings.extend(aliases);
    for feature in features {
        let dependencies = &mut feature.dependencies;
        feature.evaluation.edit(|definition, _| {
            for binding in &bindings {
                if bind_definition_sketch(
                    definition,
                    &binding.native_ref,
                    &binding.feature_id,
                    &binding.sketch,
                    binding.has_profile,
                ) {
                    dependencies.insert(binding.feature_id.clone());
                }
            }
        });
    }
}

fn sketch_alias_base_name(name: &str) -> Option<&str> {
    let (base, suffix) = name.rsplit_once('<')?;
    let ordinal = suffix.strip_suffix('>')?;
    (!base.is_empty() && !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(base)
}

/// Assign stable neutral regeneration ordinals with every structural parent and
/// explicit dependency before its consumer. Native history ordinals retain the
/// independent Keywords serialization order.
pub(crate) fn order_features_for_regeneration(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(order) = regeneration_order(ctx, features, None)? else {
        return Ok(false);
    };
    assign_regeneration_ordinals(features, order);
    Ok(true)
}

fn regeneration_order(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    model: Option<&cadmpeg_ir::document::Model>,
) -> Result<Option<Vec<usize>>, cadmpeg_core::CodecError> {
    let mut outgoing = ctx.alloc_filled(
        features.len(),
        Vec::<usize>::new(),
        "sldprt feature regeneration adjacency",
    )?;
    let mut indegree = ctx.alloc_filled(
        features.len(),
        0usize,
        "sldprt feature regeneration indegree",
    )?;
    let mut tree_parent_by_child = HashMap::new();
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        for child in children {
            tree_parent_by_child.insert(child, &feature.id);
        }
    }
    let mut by_id = HashMap::new();
    for (index, feature) in features.iter().enumerate() {
        by_id.insert(&feature.id, index);
    }
    for (consumer, feature) in features.iter().enumerate() {
        let mut predecessors = HashSet::new();
        for predecessor in &feature.dependencies {
            predecessors.insert(predecessor);
        }
        if let Some(parent) = tree_parent_by_child.get(&feature.id) {
            predecessors.insert(parent);
        }
        if let Some(model) = model {
            if let Some(parent) = model.feature_parent(&feature.id) {
                predecessors.insert(parent);
            }
            for configuration in &model.configurations {
                if let Some(state) = configuration.feature_states.get(&feature.id) {
                    for dependency in &state.dependencies {
                        predecessors.insert(dependency);
                    }
                }
            }
        }
        for predecessor in predecessors {
            let Some(&source) = by_id.get(predecessor) else {
                continue;
            };
            outgoing[source].push(consumer);
            indegree[consumer] += 1;
        }
    }
    let mut ready = std::collections::BTreeSet::new();
    for (index, feature) in features.iter().enumerate() {
        if indegree[index] == 0 {
            ready.insert((feature.ordinal, &feature.id, index));
        }
    }
    let mut order = Vec::new();
    while let Some(item) = ready.pop_first() {
        let index = item.2;
        order.push(index);
        for &consumer in &outgoing[index] {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                let feature = &features[consumer];
                ready.insert((feature.ordinal, &feature.id, consumer));
            }
        }
    }
    if order.len() != features.len() {
        return Ok(None);
    }
    Ok(Some(order))
}

fn assign_regeneration_ordinals(features: &mut [cadmpeg_ir::features::Feature], order: Vec<usize>) {
    for (ordinal, index) in order.into_iter().enumerate() {
        features[index].ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
    }
}

/// Assign one regeneration order that satisfies the baseline feature graph and
/// every configuration-local feature graph.
pub(crate) fn order_model_features_for_regeneration(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(order) = regeneration_order(ctx, &ir.model.features, Some(&ir.model))? else {
        return Ok(false);
    };
    assign_regeneration_ordinals(&mut ir.model.features, order);
    Ok(true)
}

/// Bind each decoded face to the body owning it.
fn face_owner_bodies<'a>(
    faces: &'a [Face],
    shells: &'a [cadmpeg_ir::topology::Shell],
    regions: &'a [cadmpeg_ir::topology::Region],
) -> HashMap<&'a str, &'a BodyId> {
    let mut region_bodies = HashMap::new();
    for region in regions {
        region_bodies.insert(region.id.as_str(), &region.body);
    }
    let mut shell_bodies = HashMap::new();
    for shell in shells {
        let Some(&body) = region_bodies.get(shell.region.as_str()) else {
            continue;
        };
        shell_bodies.insert(shell.id.as_str(), body);
    }
    let mut owners = HashMap::new();
    for face in faces {
        let Some(&body) = shell_bodies.get(face.shell.as_str()) else {
            continue;
        };
        owners.insert(face.id.as_str(), body);
    }
    owners
}

fn copy_output_body_id(id: &str) -> Result<BodyId, cadmpeg_core::CodecError> {
    BodyId::mint(id.to_owned()).map_err(cadmpeg_core::CodecError::malformed)
}

/// Derive feature output bodies from the producing-feature identity the
/// Parasolid attribute lane binds to each surviving face.
///
/// `face_producers` pairs an emitted face identity with the native source id of
/// the history feature that produced it. A feature outputs every body owning at
/// least one face it produced. Features whose produced faces did not survive
/// regeneration keep an empty output list.
///
/// `body_modifiers` pairs an emitted body identity with its one-based ordinal in
/// the ordered, non-metadata Keywords feature records. A resolved ordinal adds
/// that body to the corresponding feature's outputs. An ordinal that is absent
/// or ambiguous across history records is ignored.
#[derive(Clone, Copy)]
pub(crate) struct FeatureOutputSources<'a> {
    pub face_producers: &'a [(String, u32)],
    pub body_modifiers: &'a [(String, u32)],
}

pub(crate) fn derive_feature_outputs(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    sources: FeatureOutputSources<'_>,
    faces: &[Face],
    shells: &[cadmpeg_ir::topology::Shell],
    regions: &[cadmpeg_ir::topology::Region],
) -> Result<(), cadmpeg_core::CodecError> {
    let FeatureOutputSources {
        face_producers,
        body_modifiers,
    } = sources;
    let mut feature_ids_by_ordinal = HashMap::<u32, Option<&str>>::new();
    for history in histories {
        let mut ordinal = 0_u32;
        for record in &history.features {
            if is_history_metadata_record(record, &history.features) {
                continue;
            }
            ordinal = ordinal.checked_add(1).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "SLDPRT history feature record ordinal exceeds u32",
                )
            })?;
            match feature_ids_by_ordinal.entry(ordinal) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(record.id.as_str()));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
    }
    for (body, ordinal) in body_modifiers {
        let Some(Some(native_ref)) = feature_ids_by_ordinal.get(ordinal) else {
            continue;
        };
        for feature in features
            .iter_mut()
            .filter(|feature| feature.native_ref.as_deref() == Some(native_ref))
        {
            let body = match copy_output_body_id(body) {
                Ok(body) => body,
                Err(cadmpeg_core::CodecError::Malformed(_)) => continue,
                Err(error) => return Err(error),
            };
            feature.evaluation.edit(|_, outputs| {
                outputs.insert(body);
            });
        }
    }
    if face_producers.is_empty() {
        return Ok(());
    }
    let owners = face_owner_bodies(faces, shells, regions);
    let mut produced: HashMap<u32, Vec<&BodyId>> = HashMap::new();
    for (face, source_id) in face_producers {
        let Some(body) = owners.get(face.as_str()) else {
            continue;
        };
        let bodies = produced.entry(*source_id).or_default();
        if !bodies.contains(body) {
            bodies.push(body);
        }
    }
    for feature in features {
        if !feature.evaluation.outputs().is_empty() {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut source_id = None;
        'histories: for history in histories {
            for record in &history.features {
                if record.id == native_ref {
                    source_id = record.source_value();
                    break 'histories;
                }
            }
        }
        let Some(source_id) = source_id else {
            continue;
        };
        if let Some(bodies) = produced.get(&source_id) {
            let outputs: Vec<_> = bodies.iter().copied().cloned().collect();
            feature
                .evaluation
                .set_outputs(cadmpeg_ir::features::DistinctMembers::try_from(outputs)?);
        }
    }
    Ok(())
}

pub(super) fn bind_definition_sketch(
    definition: &mut FeatureDefinition,
    native_ref: &str,
    feature_ref: &FeatureId,
    sketch: &cadmpeg_ir::sketches::SketchId,
    has_profile: bool,
) -> bool {
    let bind_profile = |profile: &mut ProfileRef| {
        if has_profile
            && (matches!(profile, ProfileRef::Planar(PlanarProfileRef::Unresolved(owner)) if owner == native_ref)
                || matches!(profile, ProfileRef::Planar(PlanarProfileRef::Native(value)) if value == native_ref)
                || matches!(profile, ProfileRef::Planar(PlanarProfileRef::Feature(value)) if value == feature_ref))
        {
            *profile = ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.clone()));
            true
        } else {
            false
        }
    };
    let bind_planar_profile = |profile: &mut cadmpeg_ir::features::PlanarProfileRef| {
        if has_profile
            && (matches!(profile, PlanarProfileRef::Unresolved(owner) if owner == native_ref)
                || matches!(profile, PlanarProfileRef::Native(value) if value == native_ref)
                || matches!(profile, PlanarProfileRef::Feature(value) if value == feature_ref))
        {
            *profile = sketch.clone().into();
            true
        } else {
            false
        }
    };
    let bind_path = |path: &mut PathRef| {
        if matches!(path, PathRef::Native(value) if value == native_ref) {
            *path = PathRef::Sketch(sketch.clone());
            true
        } else {
            false
        }
    };
    match definition {
        FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
            bind_profile(profile)
        }
        FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) => {
            bind_planar_profile(profile)
        }
        FeatureDefinition::Operation(FeatureOperation::Rib { construction, .. }) => construction
            .profile
            .as_mut()
            .is_some_and(bind_planar_profile),
        FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
            construction.profile_mut().is_some_and(bind_planar_profile)
        }
        FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) => {
            let profile_bound = shape
                .referenced_profile_mut()
                .is_some_and(bind_planar_profile);
            let path_bound = path.as_mut().is_some_and(bind_path);
            profile_bound | path_bound
        }
        FeatureDefinition::Operation(FeatureOperation::TrimSurface { tool, .. }) => bind_path(tool),
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            tool: SplitFaceTool::Path(path),
            ..
        }) => bind_path(path),
        FeatureDefinition::Operation(FeatureOperation::ProjectedCurve { source, .. }) => {
            bind_path(source)
        }
        FeatureDefinition::Operation(FeatureOperation::CompositeCurve { segments, .. }) => {
            for segment in segments {
                if bind_path(segment) {
                    return true;
                }
            }
            false
        }
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections, guidance, ..
        }) => {
            let mut profile_bound = false;
            for section in sections {
                if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                    profile_bound |= bind_profile(profile);
                }
            }
            let mut guide_bound = false;
            match guidance {
                cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                    for path in guides {
                        guide_bound |= bind_path(path);
                    }
                }
                cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                    guide_bound = bind_path(centerline);
                }
            }
            profile_bound || guide_bound
        }
        _ => false,
    }
}
