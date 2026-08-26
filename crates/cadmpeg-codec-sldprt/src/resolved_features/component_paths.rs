//! Component path resolution and selection value encoding.

use super::operations::feature_inline_operation_fields;
use super::scalars::feature_object_name;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::FeatureDefinition;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

pub(crate) fn component_path_features(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Vec<String> {
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        let Some(source_id) = feature
            .source_id
            .as_deref()
            .and_then(|id| id.parse::<u32>().ok())
        else {
            continue;
        };
        by_source
            .entry(source_id)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(feature.id.as_str()));
    }
    let mut result = Vec::new();
    for component in components {
        let Some(source_id) = View::u32_le_at(&component.type_signature, 4) else {
            continue;
        };
        if let Some(Some(feature)) = by_source.get(&source_id) {
            if !result.iter().any(|existing| existing == feature) {
                result.push((*feature).to_string());
            }
        }
    }
    result
}

pub(super) fn feature_precedes_consumer(
    feature: &crate::records::Feature,
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> bool {
    features
        .iter()
        .find(|consumer| consumer.id == consumer_ref)
        .is_some_and(|consumer| {
            if feature.parent != consumer.parent {
                return false;
            }
            match (
                feature
                    .source_id
                    .as_deref()
                    .and_then(|source| source.parse::<u32>().ok())
                    .filter(|source| *source != 0),
                consumer
                    .source_id
                    .as_deref()
                    .and_then(|source| source.parse::<u32>().ok())
                    .filter(|source| *source != 0),
            ) {
                (Some(feature_source), Some(consumer_source)) => feature_source < consumer_source,
                _ => feature.ordinal < consumer.ordinal,
            }
        })
}

pub(super) fn component_path_input_features(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Vec<String> {
    component_path_features(components, features)
        .into_iter()
        .filter(|feature_ref| {
            features
                .iter()
                .find(|feature| feature.id == feature_ref.as_str())
                .is_some_and(|feature| feature_precedes_consumer(feature, features, consumer_ref))
        })
        .collect()
}

pub(crate) fn surface_selection_producer_features(
    components: &[FeatureInputComponentPathEntry],
    terminal_feature_ref: Option<&str>,
    features: &[crate::records::Feature],
) -> Vec<String> {
    let mut producers = component_path_features(components, features);
    if let Some(terminal) = terminal_feature_ref {
        if !producers.iter().any(|producer| producer == terminal) {
            producers.push(terminal.to_string());
        }
    }
    producers
}

pub(crate) fn component_path_terminal_feature(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Option<String> {
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        let Some(source_id) = feature
            .source_id
            .as_deref()
            .and_then(|id| id.parse::<u32>().ok())
        else {
            continue;
        };
        by_source
            .entry(source_id)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(feature.id.as_str()));
    }
    for component in components.iter().rev() {
        let source_id =
            View::u32_le_at(&component.type_signature, 4).expect("four-byte component source");
        match by_source.get(&source_id) {
            Some(Some(feature)) => return Some((*feature).to_string()),
            Some(None) => return None,
            None => {}
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
        .find(|feature| feature.id == owner_ref)?
        .source_id
        .as_deref()?
        .parse::<u32>()
        .ok()?;
    let mut by_source = HashMap::<u32, Option<&crate::records::Feature>>::new();
    for feature in features {
        let Some(source_id) = feature
            .source_id
            .as_deref()
            .and_then(|id| id.parse::<u32>().ok())
        else {
            continue;
        };
        by_source
            .entry(source_id)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(*feature));
    }
    let candidate = |component: &'a FeatureInputComponentPathEntry| {
        let source_id = View::u32_le_at(&component.type_signature, 4)?;
        let feature = by_source.get(&source_id)?.as_ref()?;
        (source_id < owner_source).then_some((component, *feature))
    };
    match end {
        ComponentPathEnd::Leading => components.iter().find_map(candidate),
        ComponentPathEnd::Trailing => components.iter().rev().find_map(candidate),
    }
}

pub(crate) fn project_adjacent_extrusion_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    #[derive(PartialEq)]
    enum ProfileVote {
        Missing,
        Unique { profile: String, strength: u8 },
        Ambiguous { strength: u8 },
    }

    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let history_features = histories
        .iter()
        .map(|history| (history.id.as_str(), history.features.as_slice()))
        .collect::<HashMap<_, _>>();
    let neutral_indices = features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| Some((feature.native_ref.clone()?, index)))
        .collect::<HashMap<_, _>>();
    let mut profiles = HashMap::<String, Vec<ProfileVote>>::new();
    for lane in lanes {
        let mut objects = native_features
            .values()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?, *feature)))
            .filter(|(_, feature)| {
                !history_features
                    .get(feature.parent.as_str())
                    .is_some_and(|features| {
                        crate::history::is_history_metadata_record(feature, features)
                    })
            })
            .collect::<Vec<_>>();
        objects.sort_by_key(|(name, _)| name.offset);
        let object_kind = |name: &FeatureInputName, feature: &crate::records::Feature| {
            let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
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
        for (name, feature) in &objects {
            if object_kind(name, feature) == NativeClassKind::Extrusion {
                profiles
                    .entry(feature.id.clone())
                    .or_default()
                    .push(ProfileVote::Missing);
            }
        }
        let mut associations = Vec::new();
        for pair in objects.windows(2) {
            let [(first_name, first), (second_name, second)] = pair else {
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
            associations.extend(association);
        }
        for extrusion_index in 1..objects.len() {
            let (extrusion_name, extrusion) = objects[extrusion_index];
            if object_kind(extrusion_name, extrusion) != NativeClassKind::Extrusion {
                continue;
            }
            let Some(profile) = (0..extrusion_index).rev().find_map(|profile_index| {
                let (profile_name, profile) = objects[profile_index];
                (object_kind(profile_name, profile) == NativeClassKind::ProfileFeature
                    && profile_owns_intervening_sketch_blocks(
                        profile,
                        objects[profile_index + 1..extrusion_index]
                            .iter()
                            .map(|(_, feature)| *feature),
                    ))
                .then_some(profile)
            }) else {
                continue;
            };
            associations.push((profile, extrusion, 2));
        }
        for (profile, extrusion, strength) in associations {
            let Some(vote) = profiles
                .get_mut(&extrusion.id)
                .and_then(|votes| votes.last_mut())
            else {
                continue;
            };
            *vote = match vote {
                ProfileVote::Missing => ProfileVote::Unique {
                    profile: profile.id.clone(),
                    strength,
                },
                ProfileVote::Unique {
                    profile: existing,
                    strength: existing_strength,
                } if existing == &profile.id => ProfileVote::Unique {
                    profile: existing.clone(),
                    strength: (*existing_strength).max(strength),
                },
                ProfileVote::Unique {
                    strength: existing_strength,
                    ..
                }
                | ProfileVote::Ambiguous {
                    strength: existing_strength,
                } if strength > *existing_strength => ProfileVote::Unique {
                    profile: profile.id.clone(),
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
        let Some(&index) = neutral_indices.get(&extrusion) else {
            continue;
        };
        let FeatureDefinition::Extrude {
            profile: neutral_profile,
            ..
        } = &mut features[index].definition
        else {
            continue;
        };
        if !matches!(neutral_profile, cadmpeg_ir::features::ProfileRef::Unresolved(owner) if owner == &extrusion)
        {
            continue;
        }
        if let Some(&profile_index) = neutral_indices.get(profile) {
            let dependency = features[profile_index].id.clone();
            *neutral_profile = cadmpeg_ir::features::ProfileRef::Feature(dependency.clone());
            if !features[index].dependencies.contains(&dependency) {
                features[index].dependencies.push(dependency);
            }
        }
    }
}

pub(crate) fn is_profile_feature_object(feature: &crate::records::Feature) -> bool {
    native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
        == NativeClassKind::ProfileFeature
        || (feature.input_class.is_none()
            && feature.xml_tag.eq_ignore_ascii_case("Sketch")
            && feature
                .source_id
                .as_deref()
                .and_then(|source| source.parse::<u32>().ok())
                .is_some_and(|source| source != 0))
}

pub(crate) fn profile_owns_intervening_sketch_blocks<'a>(
    profile: &crate::records::Feature,
    objects: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> bool {
    let explicit_children = profile
        .properties
        .get("DissectableChildren")
        .map(|encoded| {
            let children = encoded
                .split(',')
                .map(str::trim)
                .map(str::parse::<u32>)
                .collect::<Result<HashSet<_>, _>>()
                .ok()?;
            (!children.is_empty()
                && !children.contains(&0)
                && children.len() == encoded.split(',').count())
            .then_some(children)
        });
    if explicit_children.as_ref().is_some_and(Option::is_none) {
        return false;
    }
    let mut definitions = HashSet::new();
    let mut referenced_definitions = HashSet::new();
    let mut object_ids = HashSet::new();
    let mut instance_count = 0usize;
    for feature in objects {
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind;
        if !matches!(
            kind,
            NativeClassKind::SketchBlockDefinition | NativeClassKind::SketchBlockInstance
        ) {
            return false;
        }
        let Some(source) = feature
            .source_id
            .as_deref()
            .and_then(|source| source.parse::<u32>().ok())
            .filter(|source| *source != 0)
        else {
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
                instance_count += 1;
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
            _ => unreachable!(),
        }
    }
    if let Some(Some(children)) = explicit_children.as_ref() {
        return &definitions == children;
    }
    if definitions.len() != 1 || instance_count == 0 {
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
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[crate::records::FeatureHistory],
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let single_profile_sketches = sketches
        .iter()
        .filter(|sketch| sketch.profiles.len() == 1)
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let resolved = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.id.clone(), sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let planar_features = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::Planar,
                    ..
                }
            )
        })
        .map(|feature| feature.id.clone())
        .collect::<HashSet<_>>();
    let aliases = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::Sketch { sketch: None, .. }
            ) && feature
                .native_ref
                .as_deref()
                .and_then(|native| native_features.get(native))
                .is_some_and(|native| is_dissected_profile_feature(native))
        })
        .filter_map(|feature| {
            let mut candidates = feature
                .dependencies
                .iter()
                .filter(|dependency| planar_features.contains(*dependency));
            let owner = candidates.next()?;
            candidates
                .next()
                .is_none()
                .then(|| (feature.id.clone(), owner.clone()))
        })
        .collect::<HashMap<_, _>>();
    let profile_aliases = aliases
        .iter()
        .filter_map(|(child, owner)| {
            let sketch = resolved.get(owner)?;
            single_profile_sketches
                .contains(sketch)
                .then(|| (child.clone(), (owner.clone(), sketch.clone())))
        })
        .collect::<HashMap<_, _>>();

    for feature in features {
        if aliases.contains_key(&feature.id) {
            feature.definition = FeatureDefinition::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
                children: Vec::new(),
                active_child: None,
            };
            continue;
        }
        let replace = |profile: &mut cadmpeg_ir::features::ProfileRef| {
            let cadmpeg_ir::features::ProfileRef::Feature(child) = profile else {
                return None;
            };
            let (owner, sketch) = profile_aliases.get(child)?;
            let child = child.clone();
            *profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
            Some((child, owner.clone()))
        };
        let replaced = match &mut feature.definition {
            FeatureDefinition::Extrude { profile, .. }
            | FeatureDefinition::Wrap { profile, .. } => replace(profile).into_iter().collect(),
            FeatureDefinition::Rib { construction, .. } => construction
                .profile
                .as_mut()
                .and_then(replace)
                .into_iter()
                .collect(),
            FeatureDefinition::Revolve { construction, .. } => construction
                .profile
                .as_mut()
                .and_then(replace)
                .into_iter()
                .collect(),
            FeatureDefinition::Sweep { section, .. } => section
                .referenced_profile_mut()
                .and_then(replace)
                .into_iter()
                .collect(),
            FeatureDefinition::Loft { sections, .. } => sections
                .iter_mut()
                .filter_map(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => replace(profile),
                    cadmpeg_ir::features::LoftSection::Point(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for (child, owner) in replaced {
            feature
                .dependencies
                .retain(|dependency| dependency != &child);
            if !feature.dependencies.contains(&owner) {
                feature.dependencies.push(owner);
            }
        }
    }
}

fn compact_edge_selection_value(local_edge_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:edge-ids:");
    for (index, edge_id) in local_edge_ids.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        write!(&mut value, "{edge_id}").expect("writing to String cannot fail");
    }
    value
}

pub(super) fn compact_edge_path_value(selection: &FeatureInputEdgeSelection) -> String {
    if selection.components.is_empty() || !selection.references.is_empty() {
        return selection
            .local_edge_ids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
    }
    selection
        .components
        .iter()
        .map(|component| {
            component
                .local_id
                .map_or_else(|| "_".into(), |id| id.to_string())
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn compact_edge_selection_set_value(
    selections: &[&FeatureInputEdgeSelection],
) -> String {
    if let [selection] = selections {
        if selection
            .components
            .iter()
            .any(|component| component.local_id.is_none())
        {
            return format!(
                "sldprt:feature-input:edge-ids:{}",
                compact_edge_path_value(selection)
            );
        }
        return compact_edge_selection_value(&selection.local_edge_ids);
    }
    let mut value = String::from("sldprt:feature-input:edge-selection-vectors:");
    for (selection_index, selection) in selections.iter().enumerate() {
        if selection_index != 0 {
            value.push(';');
        }
        value.push_str(&compact_edge_path_value(selection));
    }
    value
}

pub(crate) fn compact_body_selection_value(local_body_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:body-ids:");
    for (index, body_id) in local_body_ids.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        write!(&mut value, "{body_id}").expect("writing to String cannot fail");
    }
    value
}

pub(crate) fn is_compact_body_selection_value(value: &str) -> bool {
    value.starts_with("sldprt:feature-input:body-ids:")
}

#[cfg(test)]
mod component_paths_tests;
