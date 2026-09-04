// SPDX-License-Identifier: Apache-2.0
//! Sketch binding, regeneration order, and feature-output derivation.

use crate::records::FeatureHistory;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, PathRef, ProfileRef, SplitFaceTool};
use cadmpeg_ir::topology::Face;
use std::collections::HashMap;

use crate::history::classify::is_history_metadata_record;

pub fn bind_unique_sketch_feature(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[FeatureHistory],
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let feature_indices = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
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
        let matches = sketches
            .iter()
            .filter(|sketch| sketch.name.as_deref() == Some(name))
            .collect::<Vec<_>>();
        let [sketch] = matches.as_slice() else {
            continue;
        };
        if let Some(native_ref) = features[*index].native_ref.clone() {
            bindings.push((
                *index,
                features[*index].id.clone(),
                native_ref,
                sketch.id.clone(),
                !sketch.profiles.is_empty(),
            ));
        }
    }
    if bindings.is_empty() {
        if let ([index], [sketch]) = (feature_indices.as_slice(), sketches) {
            if let Some(native_ref) = features[*index].native_ref.clone() {
                bindings.push((
                    *index,
                    features[*index].id.clone(),
                    native_ref,
                    sketch.id.clone(),
                    !sketch.profiles.is_empty(),
                ));
            }
        }
    }
    for (index, _, _, sketch, _) in &bindings {
        features[*index].definition = FeatureDefinition::Sketch {
            sketch: Some(sketch.clone()),
        };
    }
    let mut aliases = Vec::new();
    for index in &feature_indices {
        let FeatureDefinition::Sketch { sketch: None, .. } = &features[*index].definition else {
            continue;
        };
        let Some(base_name) = features[*index]
            .name
            .as_deref()
            .and_then(sketch_alias_base_name)
        else {
            continue;
        };
        let candidates = feature_indices
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
            })
            .collect::<Vec<_>>();
        let [base_index] = candidates.as_slice() else {
            continue;
        };
        let base_index = **base_index;
        let base_dependency = features[base_index].id.clone();
        let Some(native_ref) = features[*index].native_ref.clone() else {
            continue;
        };
        if !features[*index].dependencies.contains(&base_dependency) {
            features[*index].dependencies.push(base_dependency.clone());
        }
        let Some((_, _, _, sketch, has_profile)) = bindings
            .iter()
            .find(|(bound_index, _, _, _, _)| *bound_index == base_index)
        else {
            continue;
        };
        aliases.push((
            base_index,
            base_dependency,
            native_ref,
            sketch.clone(),
            *has_profile,
        ));
    }
    bindings.extend(aliases);
    for feature in features {
        for (_, dependency, native_ref, sketch, has_profile) in &bindings {
            if bind_definition_sketch(
                &mut feature.definition,
                native_ref,
                dependency,
                sketch,
                *has_profile,
            ) && !feature.dependencies.contains(dependency)
            {
                feature.dependencies.push(dependency.clone());
            }
        }
    }
}

pub(crate) fn sketch_alias_base_name(name: &str) -> Option<&str> {
    let (base, suffix) = name.rsplit_once('<')?;
    let ordinal = suffix.strip_suffix('>')?;
    (!base.is_empty() && !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(base)
}

/// Assign stable neutral regeneration ordinals with every structural parent and
/// explicit dependency before its consumer. Native history ordinals retain the
/// independent Keywords serialization order.
pub fn order_features_for_regeneration(features: &mut [cadmpeg_ir::features::Feature]) -> bool {
    let tree_parent_by_child = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::TreeNode { children, .. } =
                &feature.definition
            else {
                return None;
            };
            Some(
                children
                    .iter()
                    .map(|child| (child.clone(), feature.id.clone())),
            )
        })
        .flatten()
        .collect::<HashMap<_, _>>();
    let by_id = features
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let Ok(mut outgoing) = cadmpeg_core::decode::alloc_filled(
        features.len(),
        Vec::<usize>::new(),
        "sldprt feature regeneration adjacency",
    ) else {
        return false;
    };
    let Ok(mut indegree) = cadmpeg_core::decode::alloc_filled(
        features.len(),
        0usize,
        "sldprt feature regeneration indegree",
    ) else {
        return false;
    };
    for (consumer, feature) in features.iter().enumerate() {
        let mut predecessors = feature
            .dependencies
            .iter()
            .collect::<std::collections::HashSet<_>>();
        if let Some(parent) = tree_parent_by_child.get(&feature.id) {
            predecessors.insert(parent);
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
            ready.insert((feature.ordinal, feature.id.clone(), index));
        }
    }
    let mut order = Vec::with_capacity(features.len());
    while let Some(item) = ready.pop_first() {
        let index = item.2;
        order.push(index);
        for &consumer in &outgoing[index] {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                let feature = &features[consumer];
                ready.insert((feature.ordinal, feature.id.clone(), consumer));
            }
        }
    }
    if order.len() != features.len() {
        return false;
    }
    for (ordinal, index) in order.into_iter().enumerate() {
        features[index].ordinal = ordinal as u64;
    }
    true
}

/// Assign one regeneration order that satisfies the baseline feature graph and
/// every configuration-local feature graph.
pub fn order_model_features_for_regeneration(ir: &mut cadmpeg_ir::CadIr) -> bool {
    let mut ordering_graph = ir.model.features.clone();
    let by_id = ordering_graph
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<HashMap<_, _>>();
    for feature in &ir.model.features {
        let Some(parent) = ir.model.feature_parent(&feature.id) else {
            continue;
        };
        let target = &mut ordering_graph[by_id[&feature.id]];
        if !target.dependencies.contains(parent) {
            target.dependencies.push(parent.clone());
        }
    }
    for configuration in &ir.model.configurations {
        for (feature_id, state) in &configuration.feature_states {
            let Some(&index) = by_id.get(feature_id) else {
                continue;
            };
            for dependency in &state.dependencies {
                if !ordering_graph[index].dependencies.contains(dependency) {
                    ordering_graph[index].dependencies.push(dependency.clone());
                }
            }
        }
    }
    if !order_features_for_regeneration(&mut ordering_graph) {
        return false;
    }
    let ordinals = ordering_graph
        .into_iter()
        .map(|feature| (feature.id, feature.ordinal))
        .collect::<HashMap<_, _>>();
    for feature in &mut ir.model.features {
        feature.ordinal = ordinals[&feature.id];
    }
    true
}

/// Mutable references to every side an extrusion extent carries.
/// Bind each decoded face to the body owning it.
pub(crate) fn face_owner_bodies(
    faces: &[Face],
    shells: &[cadmpeg_ir::topology::Shell],
    regions: &[cadmpeg_ir::topology::Region],
) -> HashMap<String, cadmpeg_ir::ids::BodyId> {
    let region_bodies = regions
        .iter()
        .map(|region| (region.id.0.as_str(), &region.body))
        .collect::<HashMap<_, _>>();
    let shell_bodies = shells
        .iter()
        .filter_map(|shell| {
            region_bodies
                .get(shell.region.0.as_str())
                .map(|body| (shell.id.0.as_str(), (*body).clone()))
        })
        .collect::<HashMap<_, _>>();
    faces
        .iter()
        .filter_map(|face| {
            shell_bodies
                .get(face.shell.0.as_str())
                .map(|body| (face.id.0.clone(), body.clone()))
        })
        .collect()
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
pub fn derive_feature_outputs(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    face_producers: &[(String, u32)],
    body_modifiers: &[(String, u32)],
    faces: &[Face],
    shells: &[cadmpeg_ir::topology::Shell],
    regions: &[cadmpeg_ir::topology::Region],
) {
    let mut feature_ids_by_ordinal = HashMap::<u32, Option<&str>>::new();
    for history in histories {
        let mut ordinal = 0_u32;
        for record in history
            .features
            .iter()
            .filter(|record| !is_history_metadata_record(record, &history.features))
        {
            ordinal = ordinal.saturating_add(1);
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
            let body = cadmpeg_ir::ids::BodyId(body.clone());
            if !feature.outputs.contains(&body) {
                feature.outputs.push(body);
            }
        }
    }
    if face_producers.is_empty() {
        return;
    }
    let owners = face_owner_bodies(faces, shells, regions);
    let mut produced: HashMap<u32, Vec<cadmpeg_ir::ids::BodyId>> = HashMap::new();
    for (face, source_id) in face_producers {
        let Some(body) = owners.get(face) else {
            continue;
        };
        let bodies = produced.entry(*source_id).or_default();
        if !bodies.contains(body) {
            bodies.push(body.clone());
        }
    }
    for feature in features {
        if !feature.outputs.is_empty() {
            continue;
        }
        let Some(source_id) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| {
                histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|record| record.id == native_ref)
            })
            .and_then(|record| record.source_id.as_deref())
            .and_then(|source_id| source_id.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(bodies) = produced.get(&source_id) {
            feature.outputs.clone_from(bodies);
        }
    }
}

pub(crate) fn bind_definition_sketch(
    definition: &mut FeatureDefinition,
    native_ref: &str,
    feature_ref: &FeatureId,
    sketch: &cadmpeg_ir::sketches::SketchId,
    has_profile: bool,
) -> bool {
    let bind_profile = |profile: &mut ProfileRef| {
        if has_profile
            && (matches!(profile, ProfileRef::Unresolved(owner) if owner == native_ref)
                || matches!(profile, ProfileRef::Native(value) if value == native_ref)
                || matches!(profile, ProfileRef::Feature(value) if value == feature_ref))
        {
            *profile = ProfileRef::Sketch(sketch.clone());
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
        FeatureDefinition::Extrude { profile, .. } | FeatureDefinition::Wrap { profile, .. } => {
            bind_profile(profile)
        }
        FeatureDefinition::Rib { construction, .. } => {
            construction.profile.as_mut().is_some_and(bind_profile)
        }
        FeatureDefinition::Revolve { construction, .. } => {
            construction.profile_mut().is_some_and(bind_profile)
        }
        FeatureDefinition::Sweep { section, path, .. } => {
            section.referenced_profile_mut().is_some_and(bind_profile)
                | path.as_mut().is_some_and(bind_path)
        }
        FeatureDefinition::TrimSurface { tool, .. } => bind_path(tool),
        FeatureDefinition::SplitFace {
            tool: SplitFaceTool::Path(path),
            ..
        } => bind_path(path),
        FeatureDefinition::ProjectedCurve { source, .. } => bind_path(source),
        FeatureDefinition::CompositeCurve { segments, .. } => segments.iter_mut().any(bind_path),
        FeatureDefinition::Loft {
            sections, guides, ..
        } => {
            let mut profile_bound = false;
            for section in sections {
                if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                    profile_bound |= bind_profile(profile);
                }
            }
            let mut guide_bound = false;
            for path in guides {
                guide_bound |= bind_path(path);
            }
            profile_bound || guide_bound
        }
        _ => false,
    }
}
