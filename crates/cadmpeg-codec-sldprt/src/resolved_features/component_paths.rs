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

pub(super) fn component_path_features(
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
) -> Vec<String> {
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        let Some(source_id) = feature.source_value() else {
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
                feature.source_value().filter(|source| *source != 0),
                consumer.source_value().filter(|source| *source != 0),
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

pub(super) fn component_path_terminal_feature<'a>(
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Option<String> {
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        let Some(source_id) = feature.source_value() else {
            continue;
        };
        by_source
            .entry(source_id)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(feature.id.as_str()));
    }
    for component in components.iter().rev() {
        let Some(source_id) = View::u32_le_at(&component.type_signature, 4) else {
            continue;
        };
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
        .source_value()?;
    let mut by_source = HashMap::<u32, Option<&crate::records::Feature>>::new();
    for feature in features {
        let Some(source_id) = feature.source_value() else {
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
        reserve_component_map(ctx, &mut history_features)?;
        history_features.insert(history.id.as_str(), history.features.as_slice());
        for feature in &history.features {
            reserve_component_map(ctx, &mut native_features)?;
            native_features.insert(feature.id.as_str(), feature);
        }
    }
    let mut neutral_indices = HashMap::new();
    for (index, feature) in features.iter().enumerate() {
        if let Some(native) = feature.native_ref.as_deref() {
            reserve_component_map(ctx, &mut neutral_indices)?;
            neutral_indices.insert(copy_component_text(ctx, native)?, index);
        }
    }
    let mut profiles = HashMap::<&str, Vec<ProfileVote<'_>>>::new();
    for lane in lanes {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(native_features.len()), "scan SLDPRT adjacent profile objects")?;
        let mut objects = collect_component_vec(ctx, native_features
            .values()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?, *feature)))
            .filter(|(_, feature)| {
                !history_features
                    .get(feature.parent.as_str())
                    .is_some_and(|features| {
                        crate::history::classify::is_history_metadata_record(feature, features)
                    })
            })
            .enumerate())?;
        objects.sort_unstable_by_key(|(index, (name, _))| (name.offset, *index));
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
                reserve_component_map(ctx, &mut profiles)?;
                let votes = profiles.entry(feature.id.as_str()).or_default();
                ctx.reserve_collection_vec(votes, 1, "collect SLDPRT adjacent profile votes")?;
                votes.push(ProfileVote::Missing);
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
                ctx.reserve_collection_vec(&mut associations, 1, "collect SLDPRT adjacent profile associations")?;
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
                ctx.charge_work(1, "match SLDPRT adjacent profile owners")?;
                if object_kind(profile_name, profile) == NativeClassKind::ProfileFeature
                    && profile_owns_intervening_sketch_blocks(ctx, profile, objects[profile_index + 1..extrusion_index].iter().map(|(_, (_, feature))| *feature))? {
                    selected = Some(profile);
                    break;
                }
            }
            if let Some(profile) = selected {
                ctx.reserve_collection_vec(&mut associations, 1, "collect SLDPRT adjacent profile associations")?;
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
                    profile: *existing,
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
        let FeatureDefinition::Operation(FeatureOperation::Extrude { profile: neutral_profile, .. }) = features[index].evaluation.definition() else { continue; };
        if !matches!(neutral_profile, cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Unresolved(owner)) if owner == extrusion) { continue; }
        if let Some(&profile_index) = neutral_indices.get(*profile) {
            let reference = cadmpeg_ir::features::FeatureId::mint(copy_component_text(ctx, features[profile_index].id.as_str())?).map_err(CodecError::malformed)?;
            if !features[index].dependencies.contains(&features[profile_index].id) {
                let dependency = cadmpeg_ir::features::FeatureId::mint(copy_component_text(ctx, features[profile_index].id.as_str())?).map_err(CodecError::malformed)?;
                features[index].dependencies.try_insert_charged(dependency, ctx, "collect SLDPRT adjacent profile dependencies")?;
            }
            features[index].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = definition {
                    *profile = cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Feature(reference));
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
    ctx: &DecodeContext<'_>,
    profile: &crate::records::Feature,
    objects: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Result<bool, CodecError> {
    let explicit_children = if let Some(encoded) = profile.properties.get("DissectableChildren") {
        let mut children = HashSet::new();
        for value in encoded.split(',') {
            ctx.charge_work(1, "parse SLDPRT profile block children")?;
            let Ok(source) = value.trim().parse::<u32>() else { return Ok(false); };
            if source == 0 || children.contains(&source) { return Ok(false); }
            reserve_component_set(ctx, &mut children)?;
            children.insert(source);
        }
        Some(children)
    } else { None };
    let mut definitions = HashSet::new();
    let mut referenced_definitions = HashSet::new();
    let mut object_ids = HashSet::new();
    let mut instance_count = 0usize;
    for feature in objects {
        ctx.charge_work(1, "match SLDPRT profile block ownership")?;
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
        let Some(source) = feature.source_value().filter(|source| *source != 0) else {
            return Ok(false);
        };
        if object_ids.contains(&source) {
            return Ok(false);
        }
        reserve_component_set(ctx, &mut object_ids)?;
        object_ids.insert(source);
        match kind {
            NativeClassKind::SketchBlockDefinition => {
                reserve_component_set(ctx, &mut definitions)?;
                definitions.insert(source);
            }
            NativeClassKind::SketchBlockInstance => {
                instance_count = instance_count.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("count SLDPRT sketch block instances", u64::MAX - 1, u64::MAX))?;
                let Some(definition) = feature
                    .properties
                    .get("BlockDefinition")
                    .and_then(|source| source.parse::<u32>().ok())
                    .filter(|source| *source != 0)
                else {
                    continue;
                };
                reserve_component_set(ctx, &mut referenced_definitions)?;
                referenced_definitions.insert(definition);
            }
            _ => return Ok(false),
        }
    }
    if let Some(children) = explicit_children.as_ref() {
        return Ok(&definitions == children);
    }
    if definitions.len() != 1 || instance_count == 0 {
        return Ok(false);
    }
    Ok(referenced_definitions.is_empty() || referenced_definitions == definitions)
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
            let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) = feature.evaluation.definition()
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
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            )
        })
        .map(|feature| feature.id.clone())
        .collect::<HashSet<_>>();
    let aliases = features
        .iter()
        .filter(|feature| {
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
        let mut definition = feature.evaluation.definition().clone();
        'feature_edit: {
            if aliases.contains_key(&feature.id) {
                definition = FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
                    children: cadmpeg_ir::features::TreeChildren::default(),
                });
                break 'feature_edit;
            }
            let replace = |profile: &mut cadmpeg_ir::features::ProfileRef| {
                let cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Feature(child),
                ) = profile
                else {
                    return None;
                };
                let (owner, sketch) = profile_aliases.get(child)?;
                let child = child.clone();
                *profile = cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Sketch(sketch.clone()),
                );
                Some((child, owner.clone()))
            };
            let replace_planar = |profile: &mut cadmpeg_ir::features::PlanarProfileRef| {
                let PlanarProfileRef::Feature(child) = profile else {
                    return None;
                };
                let (owner, sketch) = profile_aliases.get(child)?;
                let child = child.clone();
                *profile = sketch.clone().into();
                Some((child, owner.clone()))
            };
            let replaced = match &mut definition {
                FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                    replace(profile).into_iter().collect()
                }
                FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) => {
                    replace_planar(profile).into_iter().collect()
                }
                FeatureDefinition::Operation(FeatureOperation::Rib { construction, .. }) => {
                    construction
                        .profile
                        .as_mut()
                        .and_then(replace_planar)
                        .into_iter()
                        .collect()
                }
                FeatureDefinition::Operation(FeatureOperation::Revolve {
                    construction, ..
                }) => construction
                    .profile_mut()
                    .and_then(replace_planar)
                    .into_iter()
                    .collect(),
                FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => shape
                    .referenced_profile_mut()
                    .and_then(replace_planar)
                    .into_iter()
                    .collect(),
                FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) => sections
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
                    feature.dependencies.insert(owner);
                }
            }
        }
        feature.evaluation.set_definition(definition);
    }
}

fn compact_edge_selection_value(local_edge_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:edge-ids:");
    for (index, edge_id) in local_edge_ids.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        value.push_str(&edge_id.to_string());
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

fn append_compact_edge_path_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut String,
    selection: &FeatureInputEdgeSelection,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "format SLDPRT compact edge path";
    if selection.components.is_empty() || !selection.references.is_empty() {
        for (index, edge_id) in selection.local_edge_ids.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if index != 0 {
                ctx.reserve_retained_string(value, 1, OPERATION)?;
                value.push(',');
            }
            let digits = edge_id.to_string();
            ctx.reserve_retained_string(value, digits.len(), OPERATION)?;
            value.push_str(&digits);
        }
    } else {
        for (index, component) in selection.components.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if index != 0 {
                ctx.reserve_retained_string(value, 1, OPERATION)?;
                value.push(',');
            }
            if let Some(id) = component.local_id {
                let digits = id.to_string();
                ctx.reserve_retained_string(value, digits.len(), OPERATION)?;
                value.push_str(&digits);
            } else {
                ctx.reserve_retained_string(value, 1, OPERATION)?;
                value.push('_');
            }
        }
    }
    Ok(())
}

pub(super) fn compact_edge_path_value_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &FeatureInputEdgeSelection,
) -> Result<String, cadmpeg_core::CodecError> {
    let mut value = String::new();
    append_compact_edge_path_charged(ctx, &mut value, selection)?;
    Ok(value)
}

pub(crate) fn compact_edge_selection_set_value_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selections: &[&FeatureInputEdgeSelection],
) -> Result<String, cadmpeg_core::CodecError> {
    const OPERATION: &str = "format SLDPRT compact edge selection";
    let prefix = if selections.len() == 1 {
        "sldprt:feature-input:edge-ids:"
    } else {
        "sldprt:feature-input:edge-selection-vectors:"
    };
    let mut value = String::new();
    ctx.reserve_retained_string(&mut value, prefix.len(), OPERATION)?;
    value.push_str(prefix);
    if let [selection] = selections {
        if selection.components.iter().all(|component| component.local_id.is_some()) {
            for (index, edge_id) in selection.local_edge_ids.iter().enumerate() {
                ctx.charge_work(1, OPERATION)?;
                if index != 0 {
                    ctx.reserve_retained_string(&mut value, 1, OPERATION)?;
                    value.push(',');
                }
                let digits = edge_id.to_string();
                ctx.reserve_retained_string(&mut value, digits.len(), OPERATION)?;
                value.push_str(&digits);
            }
            return Ok(value);
        }
    }
    for (index, selection) in selections.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        if index != 0 {
            ctx.reserve_retained_string(&mut value, 1, OPERATION)?;
            value.push(';');
        }
        append_compact_edge_path_charged(ctx, &mut value, selection)?;
    }
    Ok(value)
}

pub(crate) fn compact_body_selection_value(local_body_ids: &[u32]) -> String {
    let mut value = String::from("sldprt:feature-input:body-ids:");
    for (index, body_id) in local_body_ids.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        value.push_str(&body_id.to_string());
    }
    value
}

pub(crate) fn compact_body_selection_value_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    local_body_ids: &[u32],
) -> Result<String, cadmpeg_core::CodecError> {
    const OPERATION: &str = "format SLDPRT compact body selection";
    let prefix = "sldprt:feature-input:body-ids:";
    let mut value = String::new();
    ctx.reserve_retained_string(&mut value, prefix.len(), OPERATION)?;
    value.push_str(prefix);
    for (index, body_id) in local_body_ids.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        if index != 0 {
            ctx.reserve_retained_string(&mut value, 1, OPERATION)?;
            value.push(',');
        }
        let digits = body_id.to_string();
        ctx.reserve_retained_string(&mut value, digits.len(), OPERATION)?;
        value.push_str(&digits);
    }
    Ok(value)
}

pub(crate) fn is_compact_body_selection_value(value: &str) -> bool {
    value.starts_with("sldprt:feature-input:body-ids:")
}


fn copy_component_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.format_retained(format_args!("{text}"), "retain SLDPRT adjacent profile identity")
}

fn reserve_component_map<K: Eq + std::hash::Hash, V>(ctx: &DecodeContext<'_>, values: &mut HashMap<K, V>) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "index SLDPRT adjacent profiles")?;
    values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT adjacent profiles", u64::MAX - 1, u64::MAX))
}

fn reserve_component_set<T: Eq + std::hash::Hash>(ctx: &DecodeContext<'_>, values: &mut HashSet<T>) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "index SLDPRT profile block ownership")?;
    values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT profile block ownership", u64::MAX - 1, u64::MAX))
}

fn collect_component_vec<T>(ctx: &DecodeContext<'_>, items: impl Iterator<Item = T>) -> Result<Vec<T>, CodecError> {
    let mut values = Vec::new();
    for item in items {
        ctx.charge_work(1, "collect SLDPRT adjacent profile objects")?;
        ctx.reserve_collection_vec(&mut values, 1, "collect SLDPRT adjacent profile objects")?;
        values.push(item);
    }
    Ok(values)
}

#[cfg(test)]
mod component_paths_tests;
