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

pub(super) fn component_path_features<'a>(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT component path producers";
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(source_id) = feature.source_value() else {
            continue;
        };
        if let Some(candidate) = by_source.get_mut(&source_id) {
            *candidate = None;
        } else {
            reserve_component_map(ctx, &mut by_source, OPERATION)?;
            by_source.insert(source_id, Some(feature.id.as_str()));
        }
    }
    let mut result: Vec<String> = Vec::new();
    for component in components {
        ctx.charge_work(1, OPERATION)?;
        let Some(source_id) = View::u32_le_at(&component.type_signature, 4) else {
            continue;
        };
        let Some(Some(feature)) = by_source.get(&source_id) else {
            continue;
        };
        let mut duplicate = false;
        for existing in &result {
            charge_component_text_comparison(ctx, existing, feature, OPERATION)?;
            if existing == feature {
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            let identity = copy_component_text(ctx, feature)?;
            ctx.reserve_vec(&mut result, 1, OPERATION)?;
            result.push(identity);
        }
    }
    Ok(result)
}

pub(super) fn feature_precedes_consumer(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "resolve SLDPRT component path consumer";
    for consumer in features {
        charge_component_text_comparison(ctx, &consumer.id, consumer_ref, OPERATION)?;
        if consumer.id != consumer_ref {
            continue;
        }
        charge_component_text_comparison(ctx, &feature.parent, &consumer.parent, OPERATION)?;
        if feature.parent != consumer.parent {
            return Ok(false);
        }
        return Ok(
            match (
                feature.source_value().filter(|source| *source != 0),
                consumer.source_value().filter(|source| *source != 0),
            ) {
                (Some(feature_source), Some(consumer_source)) => feature_source < consumer_source,
                _ => feature.ordinal < consumer.ordinal,
            },
        );
    }
    Ok(false)
}

pub(super) fn component_path_input_features(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    features: &[crate::records::Feature],
    consumer_ref: &str,
) -> Result<Vec<String>, CodecError> {
    let mut producers = component_path_features(ctx, components, features)?;
    let mut retained = 0;
    for index in 0..producers.len() {
        let mut found = None;
        for feature in features {
            charge_component_text_comparison(
                ctx,
                &feature.id,
                &producers[index],
                "resolve SLDPRT component path inputs",
            )?;
            if feature.id == producers[index] {
                found = Some(feature);
                break;
            }
        }
        if let Some(feature) = found {
            if feature_precedes_consumer(ctx, feature, features, consumer_ref)? {
                producers.swap(retained, index);
                retained += 1;
            }
        }
    }
    producers.truncate(retained);
    Ok(producers)
}

pub(crate) fn surface_selection_producer_features(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    terminal_feature_ref: Option<&str>,
    features: &[crate::records::Feature],
) -> Result<Vec<String>, CodecError> {
    let mut producers = component_path_features(ctx, components, features)?;
    if let Some(terminal) = terminal_feature_ref {
        let mut duplicate = false;
        for producer in &producers {
            charge_component_text_comparison(
                ctx,
                producer,
                terminal,
                "resolve SLDPRT surface producers",
            )?;
            if producer == terminal {
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            let terminal = copy_component_text(ctx, terminal)?;
            ctx.reserve_vec(&mut producers, 1, "resolve SLDPRT surface producers")?;
            producers.push(terminal);
        }
    }
    Ok(producers)
}

fn charge_component_text_comparison(
    ctx: &DecodeContext<'_>,
    left: &str,
    right: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(left.len())
        .checked_add(cadmpeg_core::decode::u64_from_index(right.len()))
        .and_then(|work| work.checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)
}

pub(super) fn component_path_terminal_feature<'a>(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Result<Option<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT component path terminal";
    let mut by_source = HashMap::<u32, Option<&str>>::new();
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(source_id) = feature.source_value() else {
            continue;
        };
        if let Some(candidate) = by_source.get_mut(&source_id) {
            *candidate = None;
        } else {
            reserve_component_map(ctx, &mut by_source, OPERATION)?;
            by_source.insert(source_id, Some(feature.id.as_str()));
        }
    }
    for component in components.iter().rev() {
        ctx.charge_work(1, OPERATION)?;
        let Some(source_id) = View::u32_le_at(&component.type_signature, 4) else {
            continue;
        };
        match by_source.get(&source_id) {
            Some(Some(feature)) => return Ok(Some(copy_component_text(ctx, feature)?)),
            Some(None) => return Ok(None),
            None => {}
        }
    }
    Ok(None)
}

#[derive(Clone, Copy)]
pub(super) enum ComponentPathEnd {
    Leading,
    Trailing,
}

pub(super) fn component_path_feature<'a>(
    ctx: &DecodeContext<'_>,
    components: &'a [FeatureInputComponentPathEntry],
    features: &[&'a crate::records::Feature],
    owner_ref: &str,
    end: ComponentPathEnd,
) -> Result<
    Option<(
        &'a FeatureInputComponentPathEntry,
        &'a crate::records::Feature,
    )>,
    CodecError,
> {
    const OPERATION: &str = "resolve SLDPRT component path feature";
    let mut owner = None;
    for feature in features {
        let work = feature
            .id
            .len()
            .checked_add(owner_ref.len())
            .and_then(|size| size.checked_add(1))
            .and_then(|size| u64::try_from(size).ok())
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
        if feature.id == owner_ref {
            owner = feature.source_value();
            break;
        }
    }
    let Some(owner_source) = owner else {
        return Ok(None);
    };
    let candidate = |component: &'a FeatureInputComponentPathEntry| {
        ctx.charge_work(1, OPERATION)?;
        let Some(source_id) = View::u32_le_at(&component.type_signature, 4) else {
            return Ok(None);
        };
        if source_id >= owner_source {
            return Ok(None);
        }
        let mut found = None;
        for feature in features {
            ctx.charge_work(1, OPERATION)?;
            if feature.source_value() != Some(source_id) {
                continue;
            }
            if found.is_some() {
                return Ok(None);
            }
            found = Some(*feature);
        }
        Ok::<_, CodecError>(found.map(|feature| (component, feature)))
    };
    match end {
        ComponentPathEnd::Leading => {
            for component in components {
                if let Some(found) = candidate(component)? {
                    return Ok(Some(found));
                }
            }
        }
        ComponentPathEnd::Trailing => {
            for component in components.iter().rev() {
                if let Some(found) = candidate(component)? {
                    return Ok(Some(found));
                }
            }
        }
    }
    Ok(None)
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
        reserve_component_map(ctx, &mut history_features, "index SLDPRT adjacent profiles")?;
        history_features.insert(history.id.as_str(), history.features.as_slice());
        for feature in &history.features {
            reserve_component_map(ctx, &mut native_features, "index SLDPRT adjacent profiles")?;
            native_features.insert(feature.id.as_str(), feature);
        }
    }
    let mut neutral_indices = HashMap::new();
    for (index, feature) in features.iter().enumerate() {
        if let Some(native) = feature.native_ref.as_deref() {
            reserve_component_map(ctx, &mut neutral_indices, "index SLDPRT adjacent profiles")?;
            neutral_indices.insert(copy_component_text(ctx, native)?, index);
        }
    }
    let mut profiles = HashMap::<&str, Vec<ProfileVote<'_>>>::new();
    for lane in lanes {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(native_features.len()),
            "scan SLDPRT adjacent profile objects",
        )?;
        let mut objects = collect_component_vec(
            ctx,
            native_features
                .values()
                .filter_map(|feature| Some((feature_object_name(feature, lane)?, *feature)))
                .filter(|(_, feature)| {
                    !history_features
                        .get(feature.parent.as_str())
                        .is_some_and(|features| {
                            crate::history::classify::is_history_metadata_record(feature, features)
                        })
                })
                .enumerate(),
        )?;
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
                reserve_component_map(ctx, &mut profiles, "index SLDPRT adjacent profiles")?;
                let votes = profiles.entry(feature.id.as_str()).or_default();
                ctx.reserve_vec(votes, 1, "collect SLDPRT adjacent profile votes")?;
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
                ctx.reserve_vec(
                    &mut associations,
                    1,
                    "collect SLDPRT adjacent profile associations",
                )?;
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
                    && profile_owns_intervening_sketch_blocks(
                        ctx,
                        profile,
                        objects[profile_index + 1..extrusion_index]
                            .iter()
                            .map(|(_, (_, feature))| *feature),
                    )?
                {
                    selected = Some(profile);
                    break;
                }
            }
            if let Some(profile) = selected {
                ctx.reserve_vec(
                    &mut associations,
                    1,
                    "collect SLDPRT adjacent profile associations",
                )?;
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
            let reference = cadmpeg_ir::features::FeatureId::mint(copy_component_text(
                ctx,
                features[profile_index].id.as_str(),
            )?)
            .map_err(CodecError::malformed)?;
            if !features[index]
                .dependencies
                .contains(&features[profile_index].id)
            {
                let dependency = cadmpeg_ir::features::FeatureId::mint(copy_component_text(
                    ctx,
                    features[profile_index].id.as_str(),
                )?)
                .map_err(CodecError::malformed)?;
                features[index].dependencies.insert_for_decode(ctx, dependency, "collect SLDPRT adjacent profile dependencies")?;
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
    ctx: &DecodeContext<'_>,
    profile: &crate::records::Feature,
    objects: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> Result<bool, CodecError> {
    let explicit_children = if let Some(encoded) = profile.properties.get("DissectableChildren") {
        let mut children = HashSet::new();
        for value in encoded.split(',') {
            ctx.charge_work(1, "parse SLDPRT profile block children")?;
            let Ok(source) = value.trim().parse::<u32>() else {
                return Ok(false);
            };
            if source == 0 || children.contains(&source) {
                return Ok(false);
            }
            reserve_component_set(ctx, &mut children, "index SLDPRT profile block ownership")?;
            children.insert(source);
        }
        Some(children)
    } else {
        None
    };
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
        reserve_component_set(ctx, &mut object_ids, "index SLDPRT profile block ownership")?;
        object_ids.insert(source);
        match kind {
            NativeClassKind::SketchBlockDefinition => {
                reserve_component_set(
                    ctx,
                    &mut definitions,
                    "index SLDPRT profile block ownership",
                )?;
                definitions.insert(source);
            }
            NativeClassKind::SketchBlockInstance => {
                instance_count = instance_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count SLDPRT sketch block instances",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
                let Some(definition) = feature
                    .properties
                    .get("BlockDefinition")
                    .and_then(|source| source.parse::<u32>().ok())
                    .filter(|source| *source != 0)
                else {
                    continue;
                };
                reserve_component_set(
                    ctx,
                    &mut referenced_definitions,
                    "index SLDPRT profile block ownership",
                )?;
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
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[crate::records::FeatureHistory],
) -> Result<(), CodecError> {
    const INDEX: &str = "index SLDPRT dissected profiles";
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        reserve_component_map(ctx, &mut native_features, INDEX)?;
        native_features.insert(feature.id.as_str(), feature);
    }
    let mut single_profile_sketches = HashSet::new();
    for sketch in sketches.iter().filter(|sketch| sketch.profiles.len() == 1) {
        reserve_component_set(ctx, &mut single_profile_sketches, INDEX)?;
        single_profile_sketches.insert(&sketch.id);
    }
    let mut resolved = HashMap::new();
    let mut planar_features = HashSet::new();
    for feature in features.iter() {
        ctx.charge_work(1, "classify SLDPRT dissected profiles")?;
        if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) =
            feature.evaluation.definition()
        {
            reserve_component_set(ctx, &mut planar_features, INDEX)?;
            planar_features.insert(copy_dissected_feature_id(ctx, &feature.id)?);
            if let cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)) = sketch {
                reserve_component_map(ctx, &mut resolved, INDEX)?;
                resolved.insert(
                    copy_dissected_feature_id(ctx, &feature.id)?,
                    copy_dissected_sketch_id(ctx, sketch)?,
                );
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(feature.dependencies.len()),
            "resolve SLDPRT dissected profile owner",
        )?;
        let mut candidates = feature
            .dependencies
            .iter()
            .filter(|dependency| planar_features.contains(*dependency));
        let (Some(owner), None) = (candidates.next(), candidates.next()) else {
            continue;
        };
        reserve_component_map(ctx, &mut aliases, INDEX)?;
        aliases.insert(
            copy_dissected_feature_id(ctx, &feature.id)?,
            copy_dissected_feature_id(ctx, owner)?,
        );
    }
    let mut profile_aliases = HashMap::new();
    for (child, owner) in &aliases {
        let Some(sketch) = resolved.get(owner) else {
            continue;
        };
        if single_profile_sketches.contains(sketch) {
            reserve_component_map(ctx, &mut profile_aliases, INDEX)?;
            profile_aliases.insert(
                copy_dissected_feature_id(ctx, child)?,
                (
                    copy_dissected_feature_id(ctx, owner)?,
                    copy_dissected_sketch_id(ctx, sketch)?,
                ),
            );
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
        let replace_planar = |profile: &mut PlanarProfileRef| -> Result<
            Option<(
                cadmpeg_ir::features::FeatureId,
                cadmpeg_ir::features::FeatureId,
            )>,
            CodecError,
        > {
            let PlanarProfileRef::Feature(child) = profile else {
                return Ok(None);
            };
            let Some((owner, sketch)) = profile_aliases.get(child) else {
                return Ok(None);
            };
            let child = copy_dissected_feature_id(ctx, child)?;
            let owner = copy_dissected_feature_id(ctx, owner)?;
            let sketch = copy_dissected_sketch_id(ctx, sketch)?;
            *profile = sketch.into();
            Ok(Some((child, owner)))
        };
        let replace = |profile: &mut cadmpeg_ir::features::ProfileRef| {
            let cadmpeg_ir::features::ProfileRef::Planar(planar) = profile else {
                return Ok(None);
            };
            replace_planar(planar)
        };
        let mut replaced = Ok(Vec::new());
        feature.evaluation.edit(|definition, _| {
            replaced = (|| {
                let mut replacements = Vec::new();
                let single = match definition {
                    FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                        replace(profile)?
                    }
                    FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) => {
                        replace_planar(profile)?
                    }
                    FeatureDefinition::Operation(FeatureOperation::Rib {
                        construction, ..
                    }) => construction
                        .profile
                        .as_mut()
                        .map(replace_planar)
                        .transpose()?
                        .flatten(),
                    FeatureDefinition::Operation(FeatureOperation::Revolve {
                        construction,
                        ..
                    }) => construction
                        .profile_mut()
                        .map(replace_planar)
                        .transpose()?
                        .flatten(),
                    FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => shape
                        .referenced_profile_mut()
                        .map(replace_planar)
                        .transpose()?
                        .flatten(),
                    FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) => {
                        for section in sections {
                            ctx.charge_work(1, "replace SLDPRT dissected loft profile")?;
                            if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                                if let Some(replacement) = replace(profile)? {
                                    ctx.reserve_vec(
                                        &mut replacements,
                                        1,
                                        "collect SLDPRT dissected profile replacements",
                                    )?;
                                    replacements.push(replacement);
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(single) = single {
                    ctx.reserve_vec(
                        &mut replacements,
                        1,
                        "collect SLDPRT dissected profile replacements",
                    )?;
                    replacements.push(single);
                }
                Ok::<_, CodecError>(replacements)
            })();
        });
        for (child, owner) in replaced? {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(feature.dependencies.len()),
                "replace SLDPRT dissected profile dependency",
            )?;
            feature
                .dependencies
                .retain(|dependency| dependency != &child);
            if !feature.dependencies.contains(&owner) {
                feature.dependencies.insert_for_decode(ctx, owner, "collect SLDPRT dissected profile dependencies")?;
            }
        }
    }
    Ok(())
}

fn copy_dissected_feature_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::features::FeatureId,
) -> Result<cadmpeg_ir::features::FeatureId, CodecError> {
    let text = ctx.format_retained(format_args!("{}", id.as_str()), "retain SLDPRT dissected profile identity")?;
    cadmpeg_ir::features::FeatureId::mint(text).map_err(CodecError::malformed)
}

fn copy_dissected_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::sketches::SketchId,
) -> Result<cadmpeg_ir::sketches::SketchId, CodecError> {
    let text = ctx.format_retained(format_args!("{}", id.as_str()), "retain SLDPRT dissected profile identity")?;
    cadmpeg_ir::sketches::SketchId::mint(text).map_err(CodecError::malformed)
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
                ctx.try_reserve_retained_text(value, 1, OPERATION)?;
                value.push(',');
            }
            let digits = edge_id.to_string();
            ctx.try_reserve_retained_text(value, digits.len(), OPERATION)?;
            value.push_str(&digits);
        }
    } else {
        for (index, component) in selection.components.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if index != 0 {
                ctx.try_reserve_retained_text(value, 1, OPERATION)?;
                value.push(',');
            }
            if let Some(id) = component.local_id {
                let digits = id.to_string();
                ctx.try_reserve_retained_text(value, digits.len(), OPERATION)?;
                value.push_str(&digits);
            } else {
                ctx.try_reserve_retained_text(value, 1, OPERATION)?;
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
    ctx.try_reserve_retained_text(&mut value, prefix.len(), OPERATION)?;
    value.push_str(prefix);
    if let [selection] = selections {
        if selection
            .components
            .iter()
            .all(|component| component.local_id.is_some())
        {
            for (index, edge_id) in selection.local_edge_ids.iter().enumerate() {
                ctx.charge_work(1, OPERATION)?;
                if index != 0 {
                    ctx.try_reserve_retained_text(&mut value, 1, OPERATION)?;
                    value.push(',');
                }
                let digits = edge_id.to_string();
                ctx.try_reserve_retained_text(&mut value, digits.len(), OPERATION)?;
                value.push_str(&digits);
            }
            return Ok(value);
        }
    }
    for (index, selection) in selections.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        if index != 0 {
            ctx.try_reserve_retained_text(&mut value, 1, OPERATION)?;
            value.push(';');
        }
        append_compact_edge_path_charged(ctx, &mut value, selection)?;
    }
    Ok(value)
}

pub(crate) fn compact_body_selection_value_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    local_body_ids: &[u32],
) -> Result<String, cadmpeg_core::CodecError> {
    const OPERATION: &str = "format SLDPRT compact body selection";
    let prefix = "sldprt:feature-input:body-ids:";
    let mut value = String::new();
    ctx.try_reserve_retained_text(&mut value, prefix.len(), OPERATION)?;
    value.push_str(prefix);
    for (index, body_id) in local_body_ids.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        if index != 0 {
            ctx.try_reserve_retained_text(&mut value, 1, OPERATION)?;
            value.push(',');
        }
        let digits = body_id.to_string();
        ctx.try_reserve_retained_text(&mut value, digits.len(), OPERATION)?;
        value.push_str(&digits);
    }
    Ok(value)
}

pub(crate) fn is_compact_body_selection_value(value: &str) -> bool {
    value.starts_with("sldprt:feature-input:body-ids:")
}

fn copy_component_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(text.len()),
        "retain SLDPRT adjacent profile identity",
    )?;
    ctx.format_retained(format_args!("{text}"), "retain SLDPRT adjacent profile identity")
}

fn reserve_component_map<K: Eq + std::hash::Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn reserve_component_set<T: Eq + std::hash::Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn collect_component_vec<T>(
    ctx: &DecodeContext<'_>,
    items: impl Iterator<Item = T>,
) -> Result<Vec<T>, CodecError> {
    let mut values = Vec::new();
    for item in items {
        ctx.charge_work(1, "collect SLDPRT adjacent profile objects")?;
        ctx.reserve_vec(&mut values, 1, "collect SLDPRT adjacent profile objects")?;
        values.push(item);
    }
    Ok(values)
}

#[cfg(test)]
mod component_paths_tests;
