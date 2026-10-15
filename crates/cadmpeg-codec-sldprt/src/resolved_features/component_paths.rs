//! Component path resolution and selection value encoding.
use cadmpeg_ir::features::PlanarProfileRef;

use super::operations::feature_inline_operation_fields;
use super::scalars::feature_object_name;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::collections::{HashMap, HashSet};

enum ComponentProducer<'a> {
    Missing,
    Unique(&'a str),
    Ambiguous,
}

fn component_producers<'feature>(
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'feature crate::records::Feature>,
) -> HashMap<u32, ComponentProducer<'feature>> {
    let mut by_source = HashMap::new();
    for component in components {
        let Some(source) = View::u32_le_at(component.type_signature.as_ref(), 4) else {
            continue;
        };
        by_source.insert(source, ComponentProducer::Missing);
    }
    if by_source.is_empty() {
        return by_source;
    }
    for feature in features {
        let Some(source) = feature.source_value() else {
            continue;
        };
        let Some(producer) = by_source.get_mut(&source) else {
            continue;
        };
        *producer = match producer {
            ComponentProducer::Missing => ComponentProducer::Unique(feature.id.as_str()),
            ComponentProducer::Unique(_) | ComponentProducer::Ambiguous => {
                ComponentProducer::Ambiguous
            }
        };
    }
    by_source
}

pub(super) fn component_path_features<'a>(
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Vec<String> {
    let by_source = component_producers(components, features);
    let mut result: Vec<String> = Vec::new();
    for component in components {
        let Some(source_id) = View::u32_le_at(component.type_signature.as_ref(), 4) else {
            continue;
        };
        let Some(ComponentProducer::Unique(feature)) = by_source.get(&source_id) else {
            continue;
        };
        if !result.iter().any(|existing| existing == feature) {
            result.push((*feature).to_owned());
        }
    }
    result
}

pub(super) fn component_path_consumer<'a>(
    features: &'a [crate::records::Feature],
    consumer_ref: &str,
) -> Option<&'a crate::records::Feature> {
    features.iter().find(|consumer| consumer.id == consumer_ref)
}

pub(super) fn component_feature_precedes(
    feature: &crate::records::Feature,
    consumer: &crate::records::Feature,
) -> bool {
    if feature.parent != consumer.parent {
        return false;
    }
    match (
        feature.source_value().filter(|source| *source != 0),
        consumer.source_value().filter(|source| *source != 0),
    ) {
        (Some(feature_source), Some(consumer_source)) => feature_source < consumer_source,
        _ => feature.ordinal < consumer.ordinal,
    }
}

pub(super) fn component_path_input_features(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Vec<String> {
    let mut producers = component_path_features(components, features);
    if producers.is_empty() {
        return producers;
    }
    let Some(consumer) = component_path_consumer(features, consumer_ref) else {
        return Vec::new();
    };
    let mut retained = 0;
    for index in 0..producers.len() {
        let found = features
            .iter()
            .find(|feature| feature.id == producers[index]);
        if let Some(feature) = found {
            if component_feature_precedes(feature, consumer) {
                producers.swap(retained, index);
                retained += 1;
            }
        }
    }
    producers.truncate(retained);
    producers
}

pub(crate) fn surface_selection_producer_features(
    components: &[FeatureInputComponentPathEntry],
    terminal_feature_ref: Option<&str>,
    features: &[crate::records::Feature],
) -> Vec<String> {
    let mut producers = component_path_features(components, features);
    if let Some(terminal) = terminal_feature_ref {
        if !producers.iter().any(|producer| producer == terminal) {
            producers.push(terminal.to_owned());
        }
    }
    producers
}

pub(super) fn component_path_terminal_feature<'a>(
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Option<String> {
    let by_source = component_producers(components, features);
    for component in components.iter().rev() {
        let Some(source_id) = View::u32_le_at(component.type_signature.as_ref(), 4) else {
            continue;
        };
        match by_source.get(&source_id) {
            Some(ComponentProducer::Unique(feature)) => return Some((*feature).to_owned()),
            Some(ComponentProducer::Ambiguous) => return None,
            Some(ComponentProducer::Missing) | None => {}
        }
    }
    None
}

#[derive(Clone, Copy)]
pub(super) enum ComponentPathEnd {
    Leading,
    Trailing,
}

pub(super) fn component_path_feature<'a>(
    components: &'a [FeatureInputComponentPathEntry],
    features: &[&'a crate::records::Feature],
    owner_ref: &str,
    end: ComponentPathEnd,
) -> Option<(
    &'a FeatureInputComponentPathEntry,
    &'a crate::records::Feature,
)> {
    let owner_source = features
        .iter()
        .find(|feature| feature.id == owner_ref)
        .and_then(|feature| feature.source_value())?;
    let candidate = |component: &'a FeatureInputComponentPathEntry| {
        let source_id = View::u32_le_at(component.type_signature.as_ref(), 4)?;
        if source_id >= owner_source {
            return None;
        }
        let mut found = None;
        for feature in features {
            if feature.source_value() != Some(source_id) {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(*feature);
        }
        found.map(|feature| (component, feature))
    };
    match end {
        ComponentPathEnd::Leading => components.iter().find_map(candidate),
        ComponentPathEnd::Trailing => components.iter().rev().find_map(candidate),
    }
}

pub(crate) fn project_adjacent_extrusion_profiles(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    #[derive(PartialEq)]
    enum ProfileVote<'a> {
        Missing,
        Unique { profile: &'a str, strength: u8 },
        Ambiguous { strength: u8 },
    }

    let mut native_features = HashMap::new();
    let mut history_features = HashMap::new();
    for history in histories {
        history_features.insert(history.id.as_str(), history.features.as_slice());
        for feature in &history.features {
            native_features.insert(feature.id.as_str(), feature);
        }
    }
    let mut neutral_indices = HashMap::new();
    for (index, feature) in features.iter().enumerate() {
        if let Some(native) = feature.native_ref.as_deref() {
            neutral_indices.insert(native.to_owned(), index);
        }
    }
    let mut profiles = HashMap::<&str, Vec<ProfileVote<'_>>>::new();
    for lane in lanes {
        let mut objects = native_features
            .values()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?, *feature)))
            .filter(|(_, feature)| {
                !history_features
                    .get(feature.parent.as_str())
                    .is_some_and(|features| {
                        crate::history::classify::is_history_metadata_record(feature, features)
                    })
            })
            .enumerate()
            .collect::<Vec<_>>();
        objects.sort_unstable_by(
            |(left_index, (left_name, _)), (right_index, (right_name, _))| {
                (left_name.offset, *left_index).cmp(&(right_name.offset, *right_index))
            },
        );
        let object_kind = |name: &FeatureInputName, feature: &crate::records::Feature| {
            let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
            if is_profile_feature_object(feature) {
                NativeClassKind::ProfileFeature
            } else if kind == NativeClassKind::Unknown
                && (matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut")
                    || feature_inline_operation_fields(lane, name).is_some())
            {
                NativeClassKind::Extrusion
            } else {
                kind
            }
        };
        let is_dissectable = |feature: &crate::records::Feature| {
            feature.properties.contains_key("DissectableChildren")
                || feature.properties.get("Dissectable").map(String::as_str) == Some("true")
        };
        for (_, (name, feature)) in &objects {
            if object_kind(name, feature) == NativeClassKind::Extrusion {
                profiles
                    .entry(feature.id.as_str())
                    .or_default()
                    .push(ProfileVote::Missing);
            }
        }
        let mut associations = Vec::new();
        for pair in objects.windows(2) {
            let [(_, (first_name, first)), (_, (second_name, second))] = pair else {
                continue;
            };
            let first_kind = object_kind(first_name, first);
            let second_kind = object_kind(second_name, second);
            let association = match (first_kind, second_kind) {
                (NativeClassKind::ProfileFeature, NativeClassKind::Extrusion) => {
                    Some((*first, *second, 0))
                }
                (NativeClassKind::Extrusion, NativeClassKind::ProfileFeature)
                    if is_dissectable(first) || is_dissected_profile_feature(second) =>
                {
                    Some((*second, *first, 1))
                }
                _ => None,
            };
            if let Some(association) = association {
                associations.push(association);
            }
        }
        for extrusion_index in 1..objects.len() {
            let (_, (extrusion_name, extrusion)) = objects[extrusion_index];
            if object_kind(extrusion_name, extrusion) != NativeClassKind::Extrusion {
                continue;
            }
            let mut selected = None;
            for profile_index in (0..extrusion_index).rev() {
                let (_, (profile_name, profile)) = objects[profile_index];
                if object_kind(profile_name, profile) == NativeClassKind::ProfileFeature
                    && profile_owns_intervening_sketch_blocks(
                        profile,
                        objects[profile_index + 1..extrusion_index]
                            .iter()
                            .map(|(_, (_, feature))| *feature),
                    )
                {
                    selected = Some(profile);
                    break;
                }
            }
            if let Some(profile) = selected {
                associations.push((profile, extrusion, 2));
            }
        }
        for (profile, extrusion, strength) in associations {
            let Some(vote) = profiles
                .get_mut(extrusion.id.as_str())
                .and_then(|votes| votes.last_mut())
            else {
                continue;
            };
            *vote = match vote {
                ProfileVote::Missing => ProfileVote::Unique {
                    profile: profile.id.as_str(),
                    strength,
                },
                ProfileVote::Unique {
                    profile: existing,
                    strength: existing_strength,
                } if *existing == profile.id.as_str() => ProfileVote::Unique {
                    profile: existing,
                    strength: (*existing_strength).max(strength),
                },
                ProfileVote::Unique {
                    strength: existing_strength,
                    ..
                }
                | ProfileVote::Ambiguous {
                    strength: existing_strength,
                } if strength > *existing_strength => ProfileVote::Unique {
                    profile: profile.id.as_str(),
                    strength,
                },
                ProfileVote::Unique {
                    strength: existing_strength,
                    ..
                } if strength == *existing_strength => ProfileVote::Ambiguous { strength },
                ProfileVote::Unique { .. } | ProfileVote::Ambiguous { .. } => {
                    continue;
                }
            };
        }
    }
    for (extrusion, votes) in profiles {
        let Some(ProfileVote::Unique { profile, .. }) = votes.first() else {
            continue;
        };
        if !votes
            .iter()
            .all(|vote| matches!(vote, ProfileVote::Unique { profile: candidate, .. } if candidate == profile))
        {
            continue;
        }
        let Some(&index) = neutral_indices.get(extrusion) else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: neutral_profile,
            ..
        }) = features[index].evaluation.definition()
        else {
            continue;
        };
        if !matches!(neutral_profile, cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Unresolved(owner)) if owner == extrusion)
        {
            continue;
        }
        if let Some(&profile_index) = neutral_indices.get(*profile) {
            let reference = features[profile_index].id.clone();
            if !features[index]
                .dependencies
                .contains(&features[profile_index].id)
            {
                features[index].dependencies.insert(
                    ctx,
                    reference.clone(),
                    "collect SLDPRT adjacent profile dependencies",
                )?;
            }
            features[index].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                    definition
                {
                    *profile = cadmpeg_ir::features::ProfileRef::Planar(
                        cadmpeg_ir::features::PlanarProfileRef::Feature(reference),
                    );
                }
            });
        }
    }
    Ok(())
}

pub(super) fn is_profile_feature_object(feature: &crate::records::Feature) -> bool {
    native_object_class(feature.input_class.as_deref().unwrap_or_default())
        == NativeClassKind::ProfileFeature
        || (feature.input_class.is_none()
            && feature.xml_tag.eq_ignore_ascii_case("Sketch")
            && feature.source_value().is_some_and(|source| source != 0))
}

pub(super) fn profile_owns_intervening_sketch_blocks<'a>(
    profile: &crate::records::Feature,
    objects: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> bool {
    let explicit_children = if let Some(encoded) = profile.properties.get("DissectableChildren") {
        let mut children = HashSet::new();
        for value in encoded.split(',') {
            let Ok(source) = value.trim().parse::<u32>() else {
                return false;
            };
            if source == 0 || !children.insert(source) {
                return false;
            }
        }
        Some(children)
    } else {
        None
    };
    let mut definitions = HashSet::new();
    let mut referenced_definitions = HashSet::new();
    let mut object_ids = HashSet::new();
    let mut has_instance = false;
    for feature in objects {
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
        let Some(source) = feature.source_value().filter(|source| *source != 0) else {
            return false;
        };
        if !object_ids.insert(source) {
            return false;
        }
        match kind {
            NativeClassKind::SketchBlockDefinition => {
                definitions.insert(source);
            }
            NativeClassKind::SketchBlockInstance => {
                has_instance = true;
                let Some(definition) = feature
                    .properties
                    .get("BlockDefinition")
                    .and_then(|source| source.parse::<u32>().ok())
                    .filter(|source| *source != 0)
                else {
                    continue;
                };
                referenced_definitions.insert(definition);
            }
            _ => return false,
        }
    }
    if let Some(children) = explicit_children.as_ref() {
        return &definitions == children;
    }
    if definitions.len() != 1 || !has_instance {
        return false;
    }
    referenced_definitions.is_empty() || referenced_definitions == definitions
}

pub(crate) fn is_dissected_profile_feature(feature: &crate::records::Feature) -> bool {
    feature.properties.get("Description") == Some(&feature.name)
        && feature
            .name
            .rsplit_once('<')
            .and_then(|(_, suffix)| suffix.strip_suffix('>'))
            .is_some_and(|ordinal| {
                !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
            })
}

pub(crate) fn project_dissected_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[crate::records::FeatureHistory],
) -> Result<(), CodecError> {
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        native_features.insert(feature.id.as_str(), feature);
    }
    let mut single_profile_sketches = HashSet::new();
    for sketch in sketches.iter().filter(|sketch| sketch.profiles.len() == 1) {
        single_profile_sketches.insert(&sketch.id);
    }
    let mut resolved = HashMap::new();
    let mut planar_features = HashSet::new();
    for feature in features.iter() {
        if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) =
            feature.evaluation.definition()
        {
            planar_features.insert(feature.id.clone());
            if let cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)) = sketch {
                resolved.insert(feature.id.clone(), sketch.clone());
            }
        }
    }
    let mut aliases = HashMap::new();
    for feature in features.iter().filter(|feature| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                    | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                ..
            })
        ) && feature
            .native_ref
            .as_deref()
            .and_then(|native| native_features.get(native))
            .is_some_and(|native| is_dissected_profile_feature(native))
    }) {
        let mut candidates = feature
            .dependencies
            .iter()
            .filter(|dependency| planar_features.contains(*dependency));
        let (Some(owner), None) = (candidates.next(), candidates.next()) else {
            continue;
        };
        aliases.insert(feature.id.clone(), owner.clone());
    }
    let mut profile_aliases = HashMap::new();
    for (child, owner) in &aliases {
        let Some(sketch) = resolved.get(owner) else {
            continue;
        };
        if single_profile_sketches.contains(sketch) {
            profile_aliases.insert(child.clone(), (owner.clone(), sketch.clone()));
        }
    }
    for feature in features {
        if aliases.contains_key(&feature.id) {
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
                    children: cadmpeg_ir::features::TreeChildren::default(),
                }));
            continue;
        }
        let replace_planar = |profile: &mut PlanarProfileRef| -> Option<(
            cadmpeg_ir::features::FeatureId,
            cadmpeg_ir::features::FeatureId,
        )> {
            let PlanarProfileRef::Feature(child) = profile else {
                return None;
            };
            let (owner, sketch) = profile_aliases.get(child)?;
            let child = child.clone();
            let owner = owner.clone();
            *profile = sketch.clone().into();
            Some((child, owner))
        };
        let replace = |profile: &mut cadmpeg_ir::features::ProfileRef| {
            let cadmpeg_ir::features::ProfileRef::Planar(planar) = profile else {
                return None;
            };
            replace_planar(planar)
        };
        let mut replaced = Vec::new();
        feature.evaluation.edit(|definition, _| {
            let single = match definition {
                FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                    replace(profile)
                }
                FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) => {
                    replace_planar(profile)
                }
                FeatureDefinition::Operation(FeatureOperation::Rib { construction, .. }) => {
                    construction.profile.as_mut().and_then(replace_planar)
                }
                FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction, ..
                }) => construction.profile_mut().and_then(replace_planar),
                FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                    shape.referenced_profile_mut().and_then(replace_planar)
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) => {
                    for section in sections {
                        if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                            if let Some(replacement) = replace(profile) {
                                replaced.push(replacement);
                            }
                        }
                    }
                    None
                }
                _ => None,
            };
            if let Some(single) = single {
                replaced.push(single);
            }
        });
        for (child, owner) in replaced {
            feature
                .dependencies
                .retain(|dependency| dependency != &child);
            if !feature.dependencies.contains(&owner) {
                feature.dependencies.insert(
                    ctx,
                    owner,
                    "collect SLDPRT dissected profile dependencies",
                )?;
            }
        }
    }
    Ok(())
}

fn append_compact_edge_path(value: &mut String, selection: &FeatureInputEdgeSelection) {
    if selection.components.is_empty() || !selection.references.is_empty() {
        append_joined_ids(value, selection.local_edge_ids.iter());
    } else {
        for (index, component) in selection.components.iter().enumerate() {
            if index != 0 {
                value.push(',');
            }
            if let Some(id) = component.local_id {
                value.push_str(&id.to_string());
            } else {
                value.push('_');
            }
        }
    }
}

fn append_joined_ids(value: &mut String, ids: impl Iterator<Item = impl std::fmt::Display>) {
    for (index, id) in ids.enumerate() {
        if index != 0 {
            value.push(',');
        }
        value.push_str(&id.to_string());
    }
}

pub(super) fn compact_edge_path_value(selection: &FeatureInputEdgeSelection) -> String {
    let mut value = String::new();
    append_compact_edge_path(&mut value, selection);
    value
}

pub(crate) fn compact_edge_selection_set_value(
    selections: &[&FeatureInputEdgeSelection],
) -> String {
    let mut value = String::from(if selections.len() == 1 {
        "sldprt:feature-input:edge-ids:"
    } else {
        "sldprt:feature-input:edge-selection-vectors:"
    });
    if let [selection] = selections {
        if selection
            .components
            .iter()
            .all(|component| component.local_id.is_some())
        {
            append_joined_ids(&mut value, selection.local_edge_ids.iter());
            return value;
        }
    }
    for (index, selection) in selections.iter().enumerate() {
        if index != 0 {
            value.push(';');
        }
        append_compact_edge_path(&mut value, selection);
    }
    value
}

pub(crate) fn compact_body_selection_value(local_body_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:body-ids:");
    append_joined_ids(&mut value, local_body_ids.iter());
    value
}

pub(crate) fn is_compact_body_selection_value(value: &str) -> bool {
    value.starts_with("sldprt:feature-input:body-ids:")
}

#[cfg(test)]
mod component_paths_tests;

#[cfg(test)]
mod tests;
