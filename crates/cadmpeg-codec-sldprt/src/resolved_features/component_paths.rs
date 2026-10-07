//! Component path resolution and selection value encoding.
use cadmpeg_ir::features::PlanarProfileRef;

use super::operations::feature_inline_operation_fields;
use super::scalars::ObjectNames;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    Feature, FeatureInputComponentPathEntry, FeatureInputEdgeSelection, FeatureInputLane,
    FeatureInputName,
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// History features by native source identifier, indexed once. A source that
/// more than one feature carries names no feature.
pub(crate) struct FeaturesBySource<'a, 'ctx> {
    table: HashMap<u32, Option<&'a Feature>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> FeaturesBySource<'a, 'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        features: impl IntoIterator<Item = &'a Feature>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT component path producers";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut table = HashMap::<u32, Option<&Feature>>::new();
        let mut features = features.into_iter();
        while let Some(feature) = ctx.next_charged(&mut features, OPERATION)? {
            let Some(source) = feature.source_value() else {
                continue;
            };
            if let Some(single) = ctx.get_mut_hash_map(&mut table, &source, OPERATION)? {
                *single = None;
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut table, source, Some(feature), OPERATION)
            })?;
        }
        Ok(Self {
            table,
            _storage: storage,
        })
    }

    /// The feature a component's type signature names: `None` when no feature
    /// carries its source, `Some(None)` when more than one does.
    fn component(
        &self,
        ctx: &DecodeContext<'_>,
        component: &FeatureInputComponentPathEntry,
        operation: &'static str,
    ) -> Result<Option<&Option<&'a Feature>>, CodecError> {
        let Some(source) = View::u32_le_at(&component.type_signature, 4) else {
            return Ok(None);
        };
        Ok(ctx.get_hash_map(&self.table, &source, operation)?)
    }

    /// The unique feature that carries a native source.
    pub(super) fn source(
        &self,
        ctx: &DecodeContext<'_>,
        source: u32,
        operation: &'static str,
    ) -> Result<Option<&'a Feature>, CodecError> {
        Ok(ctx
            .get_hash_map(&self.table, &source, operation)?
            .copied()
            .flatten())
    }

    /// The feature the last resolvable component names, if one feature
    /// carries its source.
    pub(super) fn terminal_feature(
        &self,
        ctx: &DecodeContext<'_>,
        components: &[FeatureInputComponentPathEntry],
    ) -> Result<Option<&'a Feature>, CodecError> {
        const OPERATION: &str = "resolve SLDPRT component path terminal";
        Ok(ctx
            .find_map(
                components.iter().rev(),
                |component| self.component(ctx, component, OPERATION),
                OPERATION,
            )?
            .copied()
            .flatten())
    }

    /// The distinct features the components name, in component order.
    pub(super) fn features(
        &self,
        ctx: &DecodeContext<'_>,
        components: &[FeatureInputComponentPathEntry],
    ) -> Result<Vec<String>, CodecError> {
        const OPERATION: &str = "resolve SLDPRT component path producers";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for component in ctx.admit_iter(components, OPERATION)? {
            let Some(Some(feature)) = self.component(ctx, component, OPERATION)? else {
                continue;
            };
            if !storage
                .with_storage(|| ctx.insert_hash_set(&mut seen, feature.id.as_str(), OPERATION))?
            {
                continue;
            }
            let identity = ctx.copy_retained_text(&feature.id, OPERATION)?;
            ctx.push_vec(&mut result, identity, OPERATION)?;
        }
        Ok(result)
    }

    /// The feature the last resolvable component names.
    pub(super) fn terminal(
        &self,
        ctx: &DecodeContext<'_>,
        components: &[FeatureInputComponentPathEntry],
    ) -> Result<Option<String>, CodecError> {
        const OPERATION: &str = "resolve SLDPRT component path terminal";
        self.terminal_feature(ctx, components)?
            .map(|feature| ctx.copy_retained_text(&feature.id, OPERATION))
            .transpose()
    }
}

pub(super) fn component_path_features<'a>(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    features: impl IntoIterator<Item = &'a Feature>,
) -> Result<Vec<String>, CodecError> {
    FeaturesBySource::new(ctx, features)?.features(ctx, components)
}

pub(super) fn feature_precedes_consumer(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    features: &[Feature],
    consumer_ref: &str,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "resolve SLDPRT component path consumer";
    let Some(consumer) = ctx.find_by(
        features,
        |consumer| ctx.equal(consumer.id.as_str(), consumer_ref, OPERATION),
        OPERATION,
    )?
    else {
        return Ok(false);
    };
    precedes(ctx, feature, consumer, OPERATION)
}

/// Whether `feature` is serialized before `consumer` in the same history.
fn precedes(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    consumer: &Feature,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if !ctx.equal(&feature.parent, &consumer.parent, operation)? {
        return Ok(false);
    }
    Ok(
        match (
            feature.source_value().filter(|source| *source != 0),
            consumer.source_value().filter(|source| *source != 0),
        ) {
            (Some(feature_source), Some(consumer_source)) => feature_source < consumer_source,
            _ => feature.ordinal < consumer.ordinal,
        },
    )
}

pub(super) fn component_path_input_features(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    features: &[Feature],
    consumer_ref: &str,
) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT component path inputs";
    let mut producers = component_path_features(ctx, components, features)?;
    let Some(consumer) = ctx.find_by(
        features,
        |consumer| ctx.equal(consumer.id.as_str(), consumer_ref, OPERATION),
        OPERATION,
    )?
    else {
        ctx.clear_vec(&mut producers, OPERATION)?;
        return Ok(producers);
    };
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut first_by_id = HashMap::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        storage.with_storage(|| {
            ctx.entry_hash_map(&mut first_by_id, feature.id.as_str(), OPERATION)
                .map(|slot| {
                    slot.or_insert(feature);
                })
        })?;
    }
    ctx.retain_vec(
        &mut producers,
        |producer| match ctx.get_hash_map(&first_by_id, producer.as_str(), OPERATION)? {
            Some(feature) => precedes(ctx, feature, consumer, OPERATION),
            None => Ok(false),
        },
        OPERATION,
    )?;
    Ok(producers)
}

pub(crate) fn surface_selection_producer_features(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    terminal_feature_ref: Option<&str>,
    features: &[Feature],
) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT surface producers";
    let mut producers = component_path_features(ctx, components, features)?;
    if let Some(terminal) = terminal_feature_ref {
        if !ctx.any_by(
            &producers,
            |producer| ctx.equal(producer.as_str(), terminal, OPERATION),
            OPERATION,
        )? {
            let terminal = ctx.copy_retained_text(terminal, OPERATION)?;
            ctx.push_vec(&mut producers, terminal, OPERATION)?;
        }
    }
    Ok(producers)
}

#[derive(Clone, Copy)]
pub(super) enum ComponentPathEnd {
    Leading,
    Trailing,
}

pub(super) fn component_path_feature<'a>(
    ctx: &DecodeContext<'_>,
    components: &'a [FeatureInputComponentPathEntry],
    features: &[&'a Feature],
    owner_ref: &str,
    end: ComponentPathEnd,
) -> Result<Option<(&'a FeatureInputComponentPathEntry, &'a Feature)>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT component path feature";
    let Some(owner) = ctx.find_by(
        features,
        |feature| ctx.equal(feature.id.as_str(), owner_ref, OPERATION),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let Some(owner_source) = owner.source_value() else {
        return Ok(None);
    };
    let by_source = FeaturesBySource::new(ctx, features.iter().copied())?;
    let candidate = |component: &'a FeatureInputComponentPathEntry| {
        if View::u32_le_at(&component.type_signature, 4).is_none_or(|source| source >= owner_source)
        {
            return Ok(None);
        }
        Ok(by_source
            .component(ctx, component, OPERATION)?
            .copied()
            .flatten()
            .map(|feature| (component, feature)))
    };
    match end {
        ComponentPathEnd::Leading => ctx.find_map(components, candidate, OPERATION),
        ComponentPathEnd::Trailing => ctx.find_map(components.iter().rev(), candidate, OPERATION),
    }
}

pub(crate) fn project_adjacent_extrusion_profiles(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const DEPENDENCY: &str = "collect SLDPRT adjacent profile dependencies";
    #[derive(PartialEq)]
    enum ProfileVote<'a> {
        Missing,
        Unique { profile: &'a str, strength: u8 },
        Ambiguous { strength: u8 },
    }
    const INDEX: &str = "index SLDPRT adjacent profiles";

    let mut storage = ctx.reserve_scoped(0, INDEX)?;
    // A repeated native id resolves to its last feature.
    let mut native_features = HashMap::new();
    let mut history_features = HashMap::new();
    for history in ctx.admit_iter(histories, INDEX)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut history_features,
                history.id.as_str(),
                history.features.as_slice(),
                INDEX,
            )
        })?;
        for feature in ctx.admit_iter(&history.features, INDEX)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut native_features, feature.id.as_str(), feature, INDEX)
            })?;
        }
    }
    // Each indexed feature in history order: the decode never depends on
    // the order of a hash table.
    let mut indexed_features = Vec::new();
    for history in ctx.admit_iter(histories, INDEX)? {
        for feature in ctx.admit_iter(&history.features, INDEX)? {
            if ctx
                .get_hash_map(&native_features, feature.id.as_str(), INDEX)?
                .is_some_and(|indexed| std::ptr::eq(*indexed, feature))
            {
                storage.with_storage(|| ctx.push_vec(&mut indexed_features, feature, INDEX))?;
            }
        }
    }
    let mut neutral_indices = HashMap::new();
    for (index, feature) in ctx.admit_iter(&*features, INDEX)?.enumerate() {
        if let Some(native) = feature.native_ref.as_deref() {
            let native = storage.with_storage(|| ctx.copy_retained_text(native, INDEX))?;
            storage
                .with_storage(|| ctx.insert_hash_map(&mut neutral_indices, native, index, INDEX))?;
        }
    }
    let mut profiles = BTreeMap::<&str, Vec<ProfileVote<'_>>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT adjacent profile objects")? {
        let object_names = ObjectNames::new(ctx, lane)?;
        let mut objects = Vec::new();
        for (index, feature) in ctx
            .admit_iter(&indexed_features, "scan SLDPRT adjacent profile objects")?
            .enumerate()
        {
            let Some(name) = object_names.of(ctx, feature)? else {
                continue;
            };
            let metadata = match ctx.get_hash_map(
                &history_features,
                feature.parent.as_str(),
                "find SLDPRT adjacent profile history",
            )? {
                Some(features) => {
                    crate::history::classify::is_history_metadata_record(ctx, feature, features)?
                }
                None => false,
            };
            if metadata {
                continue;
            }
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut objects,
                    (index, (name, *feature)),
                    "collect SLDPRT adjacent profile objects",
                )
            })?;
        }
        ctx.sort_unstable_by_key(
            &mut objects,
            |value| {
                let (left_index, (left_name, _)) = value;
                (left_name.offset, *left_index)
            },
            Ord::cmp,
            "sort SLDPRT component path objects",
        )?;
        let object_kind =
            |name: &FeatureInputName, feature: &Feature| -> Result<NativeClassKind, CodecError> {
                let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
                Ok(if is_profile_feature_object(feature) {
                    NativeClassKind::ProfileFeature
                } else if kind == NativeClassKind::Unknown
                    && (matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut")
                        || feature_inline_operation_fields(ctx, lane, name)?.is_some())
                {
                    NativeClassKind::Extrusion
                } else {
                    kind
                })
            };
        let is_dissectable = |feature: &Feature| -> Result<bool, CodecError> {
            const OPERATION: &str = "find SLDPRT dissectable profile properties";
            Ok(
                ctx.contains_key_btree_map(&feature.properties, "DissectableChildren", OPERATION)?
                    || ctx
                        .get_btree_map(&feature.properties, "Dissectable", OPERATION)?
                        .map(String::as_str)
                        == Some("true"),
            )
        };
        for (_, (name, feature)) in ctx.admit_iter(&objects, INDEX)? {
            if object_kind(name, feature)? == NativeClassKind::Extrusion {
                storage.with_storage(|| {
                    ctx.push_btree_group(
                        &mut profiles,
                        feature.id.as_str(),
                        ProfileVote::Missing,
                        INDEX,
                        "collect SLDPRT adjacent profile votes",
                    )
                })?;
            }
        }
        let mut associations = Vec::new();
        for pair in ctx
            .admit_iter(&objects, "collect SLDPRT adjacent profile associations")?
            .windows(const { crate::nonzero(2) })
        {
            let [(_, (first_name, first)), (_, (second_name, second))] = pair else {
                continue;
            };
            let first_kind = object_kind(first_name, first)?;
            let second_kind = object_kind(second_name, second)?;
            let association = match (first_kind, second_kind) {
                (NativeClassKind::ProfileFeature, NativeClassKind::Extrusion) => {
                    Some((*first, *second, 0))
                }
                (NativeClassKind::Extrusion, NativeClassKind::ProfileFeature)
                    if is_dissectable(first)? || is_dissected_profile_feature(ctx, second)? =>
                {
                    Some((*second, *first, 1))
                }
                _ => None,
            };
            if let Some(association) = association {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut associations,
                        association,
                        "collect SLDPRT adjacent profile associations",
                    )
                })?;
            }
        }
        for (extrusion_index, (_, (extrusion_name, extrusion))) in ctx
            .admit_iter(&objects, "match SLDPRT adjacent profile owners")?
            .enumerate()
        {
            if object_kind(extrusion_name, extrusion)? != NativeClassKind::Extrusion {
                continue;
            }
            // A profile owns the objects between it and the extrusion only
            // when every one of them is a sketch block, so the search stops at
            // the first object that is not.
            let mut selected = None;
            for profile_index in (0..extrusion_index).rev() {
                ctx.charge_work(1, "match SLDPRT adjacent profile owners")?;
                let (_, (profile_name, profile)) = objects[profile_index];
                if object_kind(profile_name, profile)? == NativeClassKind::ProfileFeature
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
                if !is_sketch_block_object(profile) {
                    break;
                }
            }
            if let Some(profile) = selected {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut associations,
                        (profile, *extrusion, 2),
                        "collect SLDPRT adjacent profile associations",
                    )
                })?;
            }
        }
        for (profile, extrusion, strength) in
            ctx.admit_iter(associations, "resolve SLDPRT adjacent profile votes")?
        {
            let Some(vote) = ctx
                .get_mut_btree_map(
                    &mut profiles,
                    extrusion.id.as_str(),
                    "resolve SLDPRT adjacent profile votes",
                )?
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
                } if ctx.equal(*existing, profile.id.as_str(), INDEX)? => ProfileVote::Unique {
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
    for (extrusion, votes) in ctx.admit_iter(&profiles, "apply SLDPRT adjacent profiles")? {
        let Some(ProfileVote::Unique { profile, .. }) = votes.first() else {
            continue;
        };
        if !ctx.all_by(
            votes,
            |vote| match vote {
                ProfileVote::Unique {
                    profile: candidate, ..
                } => ctx.equal(*candidate, *profile, INDEX),
                _ => Ok(false),
            },
            "check SLDPRT adjacent profile votes",
        )? {
            continue;
        }
        let Some(&index) = ctx.get_hash_map(&neutral_indices, *extrusion, INDEX)? else {
            continue;
        };
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: neutral_profile,
            ..
        }) = features[index].evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::ProfileRef::Planar(PlanarProfileRef::Unresolved(owner)) =
            neutral_profile
        else {
            continue;
        };
        if !ctx.equal(owner.as_str(), *extrusion, INDEX)? {
            continue;
        }
        let Some(&profile_index) = ctx.get_hash_map(&neutral_indices, *profile, INDEX)? else {
            continue;
        };
        let reference = features[profile_index]
            .id
            .try_clone_for_decode(ctx, DEPENDENCY)?;
        if !ctx.contains(
            features[index].dependencies.as_slice(),
            &features[profile_index].id,
            DEPENDENCY,
        )? {
            let dependency = features[profile_index]
                .id
                .try_clone_for_decode(ctx, DEPENDENCY)?;
            features[index]
                .dependencies
                .insert(ctx, dependency, DEPENDENCY)?;
        }
        features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                definition
            {
                *profile =
                    cadmpeg_ir::features::ProfileRef::Planar(PlanarProfileRef::Feature(reference));
            }
        });
    }
    Ok(())
}

pub(super) fn is_profile_feature_object(feature: &Feature) -> bool {
    native_object_class(feature.input_class.as_deref().unwrap_or_default())
        == NativeClassKind::ProfileFeature
        || (feature.input_class.is_none()
            && feature.xml_tag.eq_ignore_ascii_case("Sketch")
            && feature.source_value().is_some_and(|source| source != 0))
}

/// Whether an object can lie between a profile and the extrusion it feeds.
fn is_sketch_block_object(feature: &Feature) -> bool {
    matches!(
        native_object_class(feature.input_class.as_deref().unwrap_or_default()),
        NativeClassKind::SketchBlockDefinition | NativeClassKind::SketchBlockInstance
    )
}

pub(super) fn profile_owns_intervening_sketch_blocks<'a>(
    ctx: &DecodeContext<'_>,
    profile: &Feature,
    objects: impl IntoIterator<Item = &'a Feature>,
) -> Result<bool, CodecError> {
    const OWNERSHIP: &str = "index SLDPRT profile block ownership";
    let mut storage = ctx.reserve_scoped(0, OWNERSHIP)?;
    let explicit_children = if let Some(encoded) =
        ctx.get_btree_map(&profile.properties, "DissectableChildren", OWNERSHIP)?
    {
        let mut children = BTreeSet::new();
        let mut characters = encoded.char_indices();
        let mut start = 0;
        loop {
            let end = ctx
                .find_map(
                    &mut characters,
                    |(offset, character)| Ok((character == ',').then_some(offset)),
                    "parse SLDPRT profile block children",
                )?
                .unwrap_or(encoded.len());
            let value =
                ctx.trim_text(&encoded[start..end], "trim SLDPRT profile child identity")?;
            let Ok(source) = ctx.parse_text::<u32>(value, "parse SLDPRT profile child identity")?
            else {
                return Ok(false);
            };
            if source == 0
                || !storage
                    .with_storage(|| ctx.insert_btree_set(&mut children, source, OWNERSHIP))?
            {
                return Ok(false);
            }
            if end == encoded.len() {
                break;
            }
            start = end + 1;
        }
        Some(children)
    } else {
        None
    };
    let mut definitions = BTreeSet::new();
    let mut referenced_definitions = BTreeSet::new();
    let mut object_ids = HashSet::new();
    let mut instance_count = 0usize;
    let mut objects = objects.into_iter();
    while let Some(feature) =
        ctx.next_charged(&mut objects, "match SLDPRT profile block ownership")?
    {
        let kind = native_object_class(feature.input_class.as_deref().unwrap_or_default());
        let Some(source) = feature.source_value().filter(|source| *source != 0) else {
            return Ok(false);
        };
        if !storage.with_storage(|| ctx.insert_hash_set(&mut object_ids, source, OWNERSHIP))? {
            return Ok(false);
        }
        match kind {
            NativeClassKind::SketchBlockDefinition => {
                storage
                    .with_storage(|| ctx.insert_btree_set(&mut definitions, source, OWNERSHIP))?;
            }
            NativeClassKind::SketchBlockInstance => {
                instance_count = instance_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count SLDPRT sketch block instances",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
                let definition =
                    match ctx.get_btree_map(&feature.properties, "BlockDefinition", OWNERSHIP)? {
                        Some(source) => ctx
                            .parse_text::<u32>(
                                source,
                                "parse SLDPRT sketch block definition identity",
                            )?
                            .ok()
                            .filter(|source| *source != 0),
                        None => None,
                    };
                let Some(definition) = definition else {
                    continue;
                };
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut referenced_definitions, definition, OWNERSHIP)
                })?;
            }
            _ => return Ok(false),
        }
    }
    let same = |left: &BTreeSet<u32>, right: &BTreeSet<u32>| {
        Ok::<_, CodecError>(
            left.len() == right.len() && ctx.is_subset_btree_set(left, right, OWNERSHIP)?,
        )
    };
    if let Some(children) = explicit_children.as_ref() {
        return same(&definitions, children);
    }
    if definitions.len() != 1 || instance_count == 0 {
        return Ok(false);
    }
    Ok(referenced_definitions.is_empty() || same(&referenced_definitions, &definitions)?)
}

/// The name before a trailing `<digits>` ordinal, when the name ends in one.
pub(crate) fn ordinal_suffix_base<'a>(
    ctx: &DecodeContext<'_>,
    name: &'a str,
) -> Result<Option<&'a str>, CodecError> {
    let Some(head) = name.strip_suffix('>') else {
        return Ok(None);
    };
    let Some((base, ordinal)) = ctx.rsplit_once(head, "<", "split SLDPRT name ordinal")? else {
        return Ok(None);
    };
    Ok((!ordinal.is_empty()
        && ctx.all_by(
            ordinal.as_bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "check SLDPRT name ordinal digits",
        )?)
    .then_some(base))
}

/// A profile feature split out of a sketch: its description repeats its
/// name, and the name carries a `<digits>` ordinal.
pub(crate) fn is_dissected_profile_feature(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
) -> Result<bool, CodecError> {
    let Some(description) = ctx.get_btree_map(
        &feature.properties,
        "Description",
        "find SLDPRT dissected profile description",
    )?
    else {
        return Ok(false);
    };
    Ok(ctx.equal(
        description,
        &feature.name,
        "compare SLDPRT dissected profile description",
    )? && ordinal_suffix_base(ctx, &feature.name)?.is_some())
}

pub(crate) fn project_dissected_sketches(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[crate::records::FeatureHistory],
) -> Result<(), CodecError> {
    const DEPENDENCY: &str = "replace SLDPRT dissected profile dependency";
    const INDEX: &str = "index SLDPRT dissected profiles";
    const IDENTITY: &str = "retain SLDPRT dissected profile identity";
    let mut storage = ctx.reserve_scoped(0, INDEX)?;
    let mut native_features = HashMap::new();
    for history in ctx.admit_iter(histories, INDEX)? {
        for feature in ctx.admit_iter(&history.features, INDEX)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut native_features, feature.id.as_str(), feature, INDEX)
            })?;
        }
    }
    let mut single_profile_sketches = HashMap::new();
    for sketch in ctx.admit_iter(sketches, INDEX)? {
        if sketch.profiles.len() == 1 {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut single_profile_sketches,
                    sketch.id.as_str(),
                    &sketch.id,
                    INDEX,
                )
            })?;
        }
    }
    // Planar sketch features by id, with the sketch each one resolved to.
    let mut planar_features = HashMap::<&str, Option<&str>>::new();
    for feature in ctx.admit_iter(&*features, "classify SLDPRT dissected profiles")? {
        if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) =
            feature.evaluation.definition()
        {
            let resolved = match sketch {
                cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)) => {
                    Some(sketch.as_str())
                }
                _ => None,
            };
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut planar_features, feature.id.as_str(), resolved, INDEX)
            })?;
        }
    }
    // A dissected profile is an alias of the one planar sketch it depends on;
    // a repeated child id keeps its last owner.
    let mut aliases = HashMap::<&str, &FeatureId>::new();
    let mut alias_order = Vec::<&str>::new();
    for feature in ctx.admit_iter(&*features, "resolve SLDPRT dissected profile owner")? {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                    | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                ..
            })
        ) {
            continue;
        }
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(native) = ctx.get_hash_map(&native_features, native, INDEX)? else {
            continue;
        };
        if !is_dissected_profile_feature(ctx, native)? {
            continue;
        }
        let mut dependencies = feature.dependencies.as_slice().iter();
        let is_planar = |dependency: &&FeatureId| {
            ctx.contains_key_hash_map(&planar_features, dependency.as_str(), INDEX)
        };
        let Some(owner) = ctx.find_by(
            &mut dependencies,
            is_planar,
            "resolve SLDPRT dissected profile owner",
        )?
        else {
            continue;
        };
        if ctx.any_by(
            &mut dependencies,
            |dependency| is_planar(&dependency),
            "resolve SLDPRT dissected profile owner",
        )? {
            continue;
        }
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut aliases, feature.id.as_str(), owner, INDEX)
        })?;
        storage.with_storage(|| ctx.push_vec(&mut alias_order, feature.id.as_str(), INDEX))?;
    }
    let mut alias_children = HashSet::<String>::new();
    let mut profile_aliases = HashMap::<String, (FeatureId, cadmpeg_ir::sketches::SketchId)>::new();
    for child in ctx.admit_iter(&alias_order, INDEX)? {
        let child_id = storage.with_storage(|| ctx.copy_retained_text(child, IDENTITY))?;
        storage.with_storage(|| ctx.insert_hash_set(&mut alias_children, child_id, INDEX))?;
        let Some(owner) = ctx.get_hash_map(&aliases, *child, INDEX)?.copied() else {
            continue;
        };
        let Some(Some(sketch)) = ctx.get_hash_map(&planar_features, owner.as_str(), INDEX)? else {
            continue;
        };
        let Some(sketch) = ctx
            .get_hash_map(&single_profile_sketches, *sketch, INDEX)?
            .copied()
        else {
            continue;
        };
        let child_id = storage.with_storage(|| ctx.copy_retained_text(child, IDENTITY))?;
        let owner = storage.with_storage(|| owner.try_clone_for_decode(ctx, IDENTITY))?;
        let sketch = storage.with_storage(|| sketch.try_clone_for_decode(ctx, IDENTITY))?;
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut profile_aliases, child_id, (owner, sketch), INDEX)
        })?;
    }
    drop((native_features, planar_features, aliases, alias_order));
    for feature in ctx.admit_iter(features, "replace SLDPRT dissected profiles")? {
        if ctx.contains_hash_set(&alias_children, feature.id.as_str(), INDEX)? {
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: cadmpeg_ir::features::FeatureTreeNodeRole::DissectedProfile,
                    children: cadmpeg_ir::features::TreeChildren::default(),
                }));
            continue;
        }
        let replace_planar =
            |profile: &mut PlanarProfileRef| -> Result<Option<(FeatureId, FeatureId)>, CodecError> {
                let PlanarProfileRef::Feature(child) = profile else {
                    return Ok(None);
                };
                let Some((owner, sketch)) =
                    ctx.get_hash_map(&profile_aliases, child.as_str(), INDEX)?
                else {
                    return Ok(None);
                };
                let child = child.try_clone_for_decode(ctx, IDENTITY)?;
                let owner = owner.try_clone_for_decode(ctx, IDENTITY)?;
                let sketch = sketch.try_clone_for_decode(ctx, IDENTITY)?;
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
                        for section in
                            ctx.admit_iter(sections, "replace SLDPRT dissected loft profile")?
                        {
                            if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                                if let Some(replacement) = replace(profile)? {
                                    ctx.push_vec(
                                        &mut replacements,
                                        replacement,
                                        "collect SLDPRT dissected profile replacements",
                                    )?;
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(single) = single {
                    ctx.push_vec(
                        &mut replacements,
                        single,
                        "collect SLDPRT dissected profile replacements",
                    )?;
                }
                Ok::<_, CodecError>(replacements)
            })();
        });
        for (child, owner) in
            ctx.admit_iter(replaced?, "replace SLDPRT dissected profile dependency")?
        {
            if let Some(position) = ctx.position_by(
                feature.dependencies.as_slice(),
                |dependency| ctx.equal(dependency, &child, DEPENDENCY),
                DEPENDENCY,
            )? {
                // Members are distinct, so only the matched position leaves.
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(feature.dependencies.as_slice().len()),
                    DEPENDENCY,
                )?;
                let mut index = 0usize;
                feature.dependencies.retain(|_| {
                    let keep = index != position;
                    index += 1;
                    keep
                });
            }
            if !ctx.contains(feature.dependencies.as_slice(), &owner, DEPENDENCY)? {
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

fn append_local_id(
    ctx: &DecodeContext<'_>,
    value: &mut String,
    id: Option<u32>,
    operation: &'static str,
) -> Result<(), CodecError> {
    match id {
        Some(id) => ctx.append_formatted_retained(value, format_args!("{id}"), operation),
        None => ctx.push_retained_char(value, '_', operation),
    }
}

fn append_compact_edge_path_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &mut String,
    selection: &FeatureInputEdgeSelection,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "format SLDPRT compact edge path";
    if selection.components.is_empty() || !selection.references.is_empty() {
        for (index, edge_id) in ctx
            .admit_iter(&selection.local_edge_ids, OPERATION)?
            .enumerate()
        {
            if index != 0 {
                ctx.push_retained_char(
                    value,
                    ',',
                    "format SLDPRT compact edge local id separator",
                )?;
            }
            append_local_id(ctx, value, Some(*edge_id), OPERATION)?;
        }
    } else {
        for (index, component) in ctx
            .admit_iter(&selection.components, OPERATION)?
            .enumerate()
        {
            if index != 0 {
                ctx.push_retained_char(
                    value,
                    ',',
                    "format SLDPRT compact edge component separator",
                )?;
            }
            match component.local_id {
                Some(id) => append_local_id(ctx, value, Some(id), OPERATION)?,
                None => ctx.push_retained_char(
                    value,
                    '_',
                    "format SLDPRT compact edge absent component",
                )?,
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
    ctx.append_retained(&mut value, prefix, OPERATION)?;
    if let [selection] = selections {
        if ctx.all_by(
            &selection.components,
            |component| Ok(component.local_id.is_some()),
            OPERATION,
        )? {
            for (index, edge_id) in ctx
                .admit_iter(&selection.local_edge_ids, OPERATION)?
                .enumerate()
            {
                if index != 0 {
                    ctx.push_retained_char(
                        &mut value,
                        ',',
                        "format SLDPRT compact edge set local id separator",
                    )?;
                }
                append_local_id(ctx, &mut value, Some(*edge_id), OPERATION)?;
            }
            return Ok(value);
        }
    }
    for (index, selection) in ctx.admit_iter(selections, OPERATION)?.enumerate() {
        if index != 0 {
            ctx.push_retained_char(
                &mut value,
                ';',
                "format SLDPRT compact edge selection separator",
            )?;
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
    let mut value = String::new();
    ctx.append_retained(&mut value, "sldprt:feature-input:body-ids:", OPERATION)?;
    for (index, body_id) in ctx.admit_iter(local_body_ids, OPERATION)?.enumerate() {
        if index != 0 {
            ctx.push_retained_char(
                &mut value,
                ',',
                "format SLDPRT compact body local id separator",
            )?;
        }
        append_local_id(ctx, &mut value, Some(*body_id), OPERATION)?;
    }
    Ok(value)
}

pub(crate) fn is_compact_body_selection_value(value: &str) -> bool {
    value.starts_with("sldprt:feature-input:body-ids:")
}

#[cfg(test)]
mod component_paths_tests;
