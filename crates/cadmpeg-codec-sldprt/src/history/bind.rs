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

fn copy_binding_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(value.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "retain SLDPRT sketch binding identity",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    ctx.charge_work(copy_work, "retain SLDPRT sketch binding identity")?;
    let mut copy = String::new();
    crate::text_admission::reserve_retained_string(
        ctx,
        &mut copy,
        value.len(),
        "retain SLDPRT sketch binding identity",
    )?;
    copy.push_str(value);
    Ok(copy)
}

fn copy_binding_feature_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &FeatureId,
) -> Result<FeatureId, cadmpeg_core::CodecError> {
    FeatureId::mint(copy_binding_text(ctx, value.as_str())?)
        .map_err(cadmpeg_core::CodecError::malformed)
}

fn copy_binding_sketch_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &cadmpeg_ir::sketches::SketchId,
) -> Result<cadmpeg_ir::sketches::SketchId, cadmpeg_core::CodecError> {
    cadmpeg_ir::sketches::SketchId::mint(copy_binding_text(ctx, value.as_str())?)
        .map_err(cadmpeg_core::CodecError::malformed)
}

fn push_sketch_binding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bindings: &mut Vec<SketchBinding>,
    index: usize,
    feature_id: &FeatureId,
    native_ref: &str,
    sketch: &cadmpeg_ir::sketches::SketchId,
    has_profile: bool,
) -> Result<(), cadmpeg_core::CodecError> {
    let feature_id = copy_binding_feature_id(ctx, feature_id)?;
    let native_ref = copy_binding_text(ctx, native_ref)?;
    let sketch = copy_binding_sketch_id(ctx, sketch)?;
    ctx.reserve_collection_vec(bindings, 1, "collect SLDPRT sketch bindings")?;
    bindings.push(SketchBinding {
        index,
        feature_id,
        native_ref,
        sketch,
        has_profile,
    });
    Ok(())
}

pub(crate) fn bind_unique_sketch_feature(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    histories: &[FeatureHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut native_features = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, "index SLDPRT native sketch features")?;
        if !native_features.contains_key(feature.id.as_str()) {
            ctx.charge_collection_items(1, "index SLDPRT native sketch features")?;
            native_features.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(
                    "index SLDPRT native sketch features",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
        }
        native_features.insert(feature.id.as_str(), feature);
    }
    let mut feature_indices = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        ctx.charge_work(1, "collect SLDPRT sketch features")?;
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
        ) {
            ctx.reserve_collection_vec(&mut feature_indices, 1, "collect SLDPRT sketch features")?;
            feature_indices.push(index);
        }
    }
    let mut bindings = Vec::new();
    for index in &feature_indices {
        let Some(name) = features[*index].name.as_deref() else {
            continue;
        };
        ctx.charge_work(
            u64::try_from(feature_indices.len()).map_err(|_| {
                ctx.refuse_codec_limit("match SLDPRT sketch feature names", u64::MAX - 1, u64::MAX)
            })?,
            "match SLDPRT sketch feature names",
        )?;
        if feature_indices
            .iter()
            .filter(|other| features[**other].name.as_deref() == Some(name))
            .count()
            != 1
        {
            continue;
        }
        ctx.charge_work(
            u64::try_from(sketches.len()).map_err(|_| {
                ctx.refuse_codec_limit("match SLDPRT sketch names", u64::MAX - 1, u64::MAX)
            })?,
            "match SLDPRT sketch names",
        )?;
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
            push_sketch_binding(
                ctx,
                &mut bindings,
                *index,
                &features[*index].id,
                native_ref,
                &sketch.id,
                !sketch.profiles.is_empty(),
            )?;
        }
    }
    if bindings.is_empty() {
        if let ([index], [sketch]) = (feature_indices.as_slice(), sketches) {
            if let Some(native_ref) = features[*index].native_ref.as_deref() {
                push_sketch_binding(
                    ctx,
                    &mut bindings,
                    *index,
                    &features[*index].id,
                    native_ref,
                    &sketch.id,
                    !sketch.profiles.is_empty(),
                )?;
            }
        }
    }
    for binding in &bindings {
        features[binding.index]
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    copy_binding_sketch_id(ctx, &binding.sketch)?,
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
        ctx.charge_work(
            u64::try_from(feature_indices.len()).map_err(|_| {
                ctx.refuse_codec_limit("match SLDPRT sketch aliases", u64::MAX - 1, u64::MAX)
            })?,
            "match SLDPRT sketch aliases",
        )?;
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
        let base_dependency = copy_binding_feature_id(ctx, &features[base_index].id)?;
        let Some(native_ref) = features[*index].native_ref.as_deref() else {
            continue;
        };
        let native_ref = copy_binding_text(ctx, native_ref)?;
        if !features[*index].dependencies.contains(&base_dependency) {
            features[*index].dependencies.try_insert_charged(
                copy_binding_feature_id(ctx, &base_dependency)?,
                ctx,
                "bind SLDPRT sketch alias dependency",
            )?;
        }
        ctx.charge_work(
            u64::try_from(bindings.len()).map_err(|_| {
                ctx.refuse_codec_limit("match SLDPRT bound sketch aliases", u64::MAX - 1, u64::MAX)
            })?,
            "match SLDPRT bound sketch aliases",
        )?;
        let Some(binding) = bindings.iter().find(|binding| binding.index == base_index) else {
            continue;
        };
        let sketch = copy_binding_sketch_id(ctx, &binding.sketch)?;
        ctx.reserve_collection_vec(&mut aliases, 1, "collect SLDPRT sketch aliases")?;
        aliases.push(SketchBinding {
            index: base_index,
            feature_id: base_dependency,
            native_ref,
            sketch,
            has_profile: binding.has_profile,
        });
    }
    ctx.reserve_precharged_vec(&mut bindings, aliases.len(), "merge SLDPRT sketch aliases")?;
    bindings.extend(aliases);
    for feature in features {
        ctx.charge_work(
            u64::try_from(bindings.len()).map_err(|_| {
                ctx.refuse_codec_limit("bind SLDPRT feature sketches", u64::MAX - 1, u64::MAX)
            })?,
            "bind SLDPRT feature sketches",
        )?;
        let dependencies = &mut feature.dependencies;
        let mut result = Ok(());
        feature.evaluation.edit(|definition, _| {
            result = (|| {
                for binding in &bindings {
                    if bind_definition_sketch(
                        ctx,
                        definition,
                        &binding.native_ref,
                        &binding.feature_id,
                        &binding.sketch,
                        binding.has_profile,
                    )? && !dependencies.contains(&binding.feature_id)
                    {
                        dependencies.try_insert_charged(
                            copy_binding_feature_id(ctx, &binding.feature_id)?,
                            ctx,
                            "bind SLDPRT sketch dependency",
                        )?;
                    }
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        result?;
    }
    Ok(())
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
    assign_regeneration_ordinals(ctx, features, order)?;
    Ok(true)
}

fn add_regeneration_predecessor<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    predecessors: &mut HashSet<&'a FeatureId>,
    predecessor: &'a FeatureId,
) -> Result<(), cadmpeg_core::CodecError> {
    if !predecessors.contains(predecessor) {
        ctx.charge_collection_items(1, "collect SLDPRT feature predecessors")?;
        predecessors.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit(
                "collect SLDPRT feature predecessors",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    }
    predecessors.insert(predecessor);
    Ok(())
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
    for feature in features.iter() {
        let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        for child in children {
            ctx.charge_work(1, "index SLDPRT feature tree parents")?;
            if !tree_parent_by_child.contains_key(child) {
                ctx.charge_collection_items(1, "index SLDPRT feature tree parents")?;
                tree_parent_by_child.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "index SLDPRT feature tree parents",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            }
            tree_parent_by_child.insert(child, &feature.id);
        }
    }
    let mut by_id = HashMap::new();
    for (index, feature) in features.iter().enumerate() {
        ctx.charge_work(1, "index SLDPRT feature regeneration IDs")?;
        if !by_id.contains_key(&feature.id) {
            ctx.charge_collection_items(1, "index SLDPRT feature regeneration IDs")?;
            by_id.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(
                    "index SLDPRT feature regeneration IDs",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
        }
        by_id.insert(&feature.id, index);
    }
    for (consumer, feature) in features.iter().enumerate() {
        let mut predecessors = HashSet::new();
        for predecessor in feature.dependencies.iter() {
            ctx.charge_work(1, "collect SLDPRT feature predecessors")?;
            add_regeneration_predecessor(ctx, &mut predecessors, predecessor)?;
        }
        if let Some(parent) = tree_parent_by_child.get(&feature.id) {
            add_regeneration_predecessor(ctx, &mut predecessors, parent)?;
        }
        if let Some(model) = model {
            let scan_units = u64::try_from(features.len()).map_err(|_| {
                ctx.refuse_codec_limit("scan SLDPRT feature parents", u64::MAX - 1, u64::MAX)
            })?;
            ctx.charge_work(scan_units, "scan SLDPRT feature parents")?;
            if let Some(parent) = model.feature_parent(&feature.id) {
                add_regeneration_predecessor(ctx, &mut predecessors, parent)?;
            }
            for configuration in &model.configurations {
                ctx.charge_work(1, "scan SLDPRT configuration feature dependencies")?;
                if let Some(state) = configuration.feature_states.get(&feature.id) {
                    for dependency in &state.dependencies {
                        ctx.charge_work(1, "scan SLDPRT configuration feature dependencies")?;
                        add_regeneration_predecessor(ctx, &mut predecessors, dependency)?;
                    }
                }
            }
        }
        for predecessor in predecessors {
            ctx.charge_work(1, "build SLDPRT feature regeneration graph")?;
            let Some(&source) = by_id.get(predecessor) else {
                continue;
            };
            ctx.reserve_collection_vec(
                &mut outgoing[source],
                1,
                "collect SLDPRT feature regeneration edges",
            )?;
            outgoing[source].push(consumer);
            indegree[consumer] += 1;
        }
    }
    let mut ready = std::collections::BTreeSet::new();
    for (index, feature) in features.iter().enumerate() {
        if indegree[index] == 0 {
            ctx.charge_collection_items(1, "queue SLDPRT feature regeneration order")?;
            ready.insert((feature.ordinal, &feature.id, index));
        }
    }
    let mut order = Vec::new();
    ctx.reserve_collection_vec(
        &mut order,
        features.len(),
        "collect SLDPRT feature regeneration order",
    )?;
    while let Some(item) = ready.pop_first() {
        ctx.charge_work(1, "sort SLDPRT feature regeneration order")?;
        let index = item.2;
        order.push(index);
        for &consumer in &outgoing[index] {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                let feature = &features[consumer];
                ctx.charge_collection_items(1, "queue SLDPRT feature regeneration order")?;
                ready.insert((feature.ordinal, &feature.id, consumer));
            }
        }
    }
    if order.len() != features.len() {
        return Ok(None);
    }
    Ok(Some(order))
}

fn assign_regeneration_ordinals(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    order: Vec<usize>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (ordinal, index) in order.into_iter().enumerate() {
        features[index].ordinal = u64::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit(
                "number SLDPRT feature regeneration order",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
    }
    Ok(())
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
    assign_regeneration_ordinals(ctx, &mut ir.model.features, order)?;
    Ok(true)
}

/// Bind each decoded face to the body owning it.
fn face_owner_bodies<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &'a [Face],
    shells: &'a [cadmpeg_ir::topology::Shell],
    regions: &'a [cadmpeg_ir::topology::Region],
) -> Result<HashMap<&'a str, &'a BodyId>, cadmpeg_core::CodecError> {
    let mut region_bodies = HashMap::new();
    for region in regions {
        ctx.charge_work(1, "index SLDPRT face owner regions")?;
        if !region_bodies.contains_key(region.id.as_str()) {
            ctx.charge_collection_items(1, "index SLDPRT face owner regions")?;
            region_bodies.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("index SLDPRT face owner regions", u64::MAX - 1, u64::MAX)
            })?;
        }
        region_bodies.insert(region.id.as_str(), &region.body);
    }
    let mut shell_bodies = HashMap::new();
    for shell in shells {
        ctx.charge_work(1, "index SLDPRT face owner shells")?;
        let Some(&body) = region_bodies.get(shell.region.as_str()) else {
            continue;
        };
        if !shell_bodies.contains_key(shell.id.as_str()) {
            ctx.charge_collection_items(1, "index SLDPRT face owner shells")?;
            shell_bodies.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("index SLDPRT face owner shells", u64::MAX - 1, u64::MAX)
            })?;
        }
        shell_bodies.insert(shell.id.as_str(), body);
    }
    let mut owners = HashMap::new();
    for face in faces {
        ctx.charge_work(1, "index SLDPRT face owner bodies")?;
        let Some(&body) = shell_bodies.get(face.shell.as_str()) else {
            continue;
        };
        if !owners.contains_key(face.id.as_str()) {
            ctx.charge_collection_items(1, "index SLDPRT face owner bodies")?;
            owners.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("index SLDPRT face owner bodies", u64::MAX - 1, u64::MAX)
            })?;
        }
        owners.insert(face.id.as_str(), body);
    }
    Ok(owners)
}

fn copy_output_body_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &str,
) -> Result<BodyId, cadmpeg_core::CodecError> {
    let mut text = String::new();
    crate::text_admission::reserve_retained_string(
        ctx,
        &mut text,
        id.len(),
        "retain SLDPRT feature output body",
    )?;
    text.push_str(id);
    BodyId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
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
pub(crate) fn derive_feature_outputs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    face_producers: &[(String, u32)],
    body_modifiers: &[(String, u32)],
    faces: &[Face],
    shells: &[cadmpeg_ir::topology::Shell],
    regions: &[cadmpeg_ir::topology::Region],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut feature_ids_by_ordinal = HashMap::<u32, Option<&str>>::new();
    for history in histories {
        let mut ordinal = 0_u32;
        for record in &history.features {
            ctx.charge_work(
                u64::try_from(history.features.len()).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "classify SLDPRT body modifier ordinals",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?,
                "classify SLDPRT body modifier ordinals",
            )?;
            if is_history_metadata_record(record, &history.features) {
                continue;
            }
            ctx.charge_work(1, "index SLDPRT body modifier ordinals")?;
            ordinal = ordinal.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "index SLDPRT body modifier ordinals",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
            if !feature_ids_by_ordinal.contains_key(&ordinal) {
                ctx.charge_collection_items(1, "index SLDPRT body modifier ordinals")?;
                feature_ids_by_ordinal.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "index SLDPRT body modifier ordinals",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            }
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
        ctx.charge_work(
            u64::try_from(features.len()).map_err(|_| {
                ctx.refuse_codec_limit("match SLDPRT body modifiers", u64::MAX - 1, u64::MAX)
            })?,
            "match SLDPRT body modifiers",
        )?;
        for feature in features
            .iter_mut()
            .filter(|feature| feature.native_ref.as_deref() == Some(native_ref))
        {
            let body = match copy_output_body_id(ctx, body) {
                Ok(body) => body,
                Err(cadmpeg_core::CodecError::Malformed(_)) => continue,
                Err(error) => return Err(error),
            };
            if !feature.evaluation.outputs().contains(&body) {
                let mut outputs = Vec::new();
                let count = feature
                    .evaluation
                    .outputs()
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "collect SLDPRT body modifier outputs",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                ctx.reserve_collection_vec(
                    &mut outputs,
                    count,
                    "collect SLDPRT body modifier outputs",
                )?;
                for output in feature.evaluation.outputs() {
                    outputs.push(copy_output_body_id(ctx, output.as_str())?);
                }
                outputs.push(body);
                feature.evaluation.set_outputs(
                    cadmpeg_ir::features::DistinctMembers::try_from_charged(outputs, ctx)?,
                );
            }
        }
    }
    if face_producers.is_empty() {
        return Ok(());
    }
    let owners = face_owner_bodies(ctx, faces, shells, regions)?;
    let mut produced: HashMap<u32, Vec<&BodyId>> = HashMap::new();
    for (face, source_id) in face_producers {
        ctx.charge_work(1, "collect SLDPRT produced bodies")?;
        let Some(body) = owners.get(face.as_str()) else {
            continue;
        };
        if !produced.contains_key(source_id) {
            ctx.charge_collection_items(1, "index SLDPRT produced bodies")?;
            produced.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("index SLDPRT produced bodies", u64::MAX - 1, u64::MAX)
            })?;
        }
        let bodies = produced.entry(*source_id).or_default();
        if !bodies.contains(body) {
            ctx.reserve_collection_vec(bodies, 1, "collect SLDPRT produced bodies")?;
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
                ctx.charge_work(1, "match SLDPRT feature output source")?;
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
            let mut outputs = Vec::new();
            ctx.reserve_collection_vec(
                &mut outputs,
                bodies.len(),
                "collect SLDPRT feature outputs",
            )?;
            for body in bodies {
                outputs.push(copy_output_body_id(ctx, body.as_str())?);
            }
            feature.evaluation.set_outputs(
                cadmpeg_ir::features::DistinctMembers::try_from_charged(outputs, ctx)?,
            );
        }
    }
    Ok(())
}

pub(super) fn bind_definition_sketch(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &mut FeatureDefinition,
    native_ref: &str,
    feature_ref: &FeatureId,
    sketch: &cadmpeg_ir::sketches::SketchId,
    has_profile: bool,
) -> Result<bool, cadmpeg_core::CodecError> {
    let bind_profile = |profile: &mut ProfileRef| {
        if has_profile
            && (matches!(profile, ProfileRef::Planar(PlanarProfileRef::Unresolved(owner)) if owner == native_ref)
                || matches!(profile, ProfileRef::Planar(PlanarProfileRef::Native(value)) if value == native_ref)
                || matches!(profile, ProfileRef::Planar(PlanarProfileRef::Feature(value)) if value == feature_ref))
        {
            *profile = ProfileRef::Planar(PlanarProfileRef::Sketch(copy_binding_sketch_id(
                ctx, sketch,
            )?));
            Ok(true)
        } else {
            Ok(false)
        }
    };
    let bind_planar_profile = |profile: &mut cadmpeg_ir::features::PlanarProfileRef| {
        if has_profile
            && (matches!(profile, PlanarProfileRef::Unresolved(owner) if owner == native_ref)
                || matches!(profile, PlanarProfileRef::Native(value) if value == native_ref)
                || matches!(profile, PlanarProfileRef::Feature(value) if value == feature_ref))
        {
            *profile = copy_binding_sketch_id(ctx, sketch)?.into();
            Ok(true)
        } else {
            Ok(false)
        }
    };
    let bind_path = |path: &mut PathRef| {
        if matches!(path, PathRef::Native(value) if value == native_ref) {
            *path = PathRef::Sketch(copy_binding_sketch_id(ctx, sketch)?);
            Ok(true)
        } else {
            Ok(false)
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
            .map(bind_planar_profile)
            .transpose()
            .map(|bound| bound.unwrap_or(false)),
        FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
            construction
                .profile_mut()
                .map(bind_planar_profile)
                .transpose()
                .map(|bound| bound.unwrap_or(false))
        }
        FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) => {
            let profile_bound = shape
                .referenced_profile_mut()
                .map(bind_planar_profile)
                .transpose()?
                .unwrap_or(false);
            let path_bound = path.as_mut().map(bind_path).transpose()?.unwrap_or(false);
            Ok(profile_bound | path_bound)
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
                if bind_path(segment)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections, guidance, ..
        }) => {
            let mut profile_bound = false;
            for section in sections {
                if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                    profile_bound |= bind_profile(profile)?;
                }
            }
            let mut guide_bound = false;
            match guidance {
                cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                    for path in guides {
                        guide_bound |= bind_path(path)?;
                    }
                }
                cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                    guide_bound = bind_path(centerline)?;
                }
            }
            Ok(profile_bound || guide_bound)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bind_unique_sketch_feature, derive_feature_outputs, order_features_for_regeneration,
        order_model_features_for_regeneration,
    };
    use crate::records::FeatureHistory;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::scalar::Length;

    fn ordering_feature() -> Feature {
        Feature {
            id: FeatureId::mint("synthetic:test:id#ordering")
                .unwrap_or_else(|error| panic!("invalid test ID: {error}")),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::DatumOffsetPlane {
                    reference: None,
                    distance: Length::ZERO,
                },
            )),
            native_ref: None,
        }
    }

    #[test]
    fn feature_regeneration_index_refuses_work_limit() {
        let mut features = [ordering_feature()];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .unwrap_or_else(|error| panic!("test context failed: {error}"));
        let error = order_features_for_regeneration(&ctx, &mut features)
            .expect_err("one feature must exceed the work limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "index SLDPRT feature regeneration IDs"
        ));
    }

    #[test]
    fn model_regeneration_parent_scan_refuses_work_limit() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.features.push(ordering_feature());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .unwrap_or_else(|error| panic!("test context failed: {error}"));
        let error = order_model_features_for_regeneration(&ctx, &mut ir)
            .expect_err("the parent scan must exceed the remaining work limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "scan SLDPRT feature parents"
        ));
    }

    fn feature_output_error(
        limits: impl FnOnce(&mut cadmpeg_core::decode::ResourceLimits),
    ) -> cadmpeg_core::CodecError {
        let histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: Default::default(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![crate::history::tests::feature("native", Some("700"), 0)],
        }];
        let mut projected = crate::history::project::project_features(
            &cadmpeg_test_support::service_decode_context(),
            &histories,
        )
        .unwrap_or_else(|error| panic!("test projection failed: {error}"));
        let body_modifiers = [("sldprt:brep:body#333".to_owned(), 1)];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        limits(&mut policy.limits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .unwrap_or_else(|error| panic!("test context failed: {error}"));
        derive_feature_outputs(
            &ctx,
            &mut projected,
            &histories,
            &[],
            &body_modifiers,
            &[],
            &[],
            &[],
        )
        .expect_err("the feature-output route must refuse the selected limit")
    }

    #[test]
    fn feature_outputs_refuse_collection_limit() {
        let error = feature_output_error(|limits| limits.max_collection_items = 0);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index SLDPRT body modifier ordinals"
        ));
    }

    #[test]
    fn feature_outputs_refuse_retained_limit() {
        let error = feature_output_error(|limits| limits.max_retained_bytes = 0);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain SLDPRT feature output body"
        ));
    }

    #[test]
    fn feature_outputs_refuse_work_limit() {
        let error = feature_output_error(|limits| limits.max_work_units = 0);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "classify SLDPRT body modifier ordinals"
        ));
    }

    fn sketch_binding_error(
        limits: impl FnOnce(&mut cadmpeg_core::decode::ResourceLimits),
    ) -> cadmpeg_core::CodecError {
        let mut neutral = ordering_feature();
        neutral.name = Some("Sketch1".into());
        neutral.native_ref = Some("native".into());
        neutral
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }));
        let mut features = [neutral];
        let sketches = [cadmpeg_ir::sketches::Sketch {
            id: cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch")
                .unwrap_or_else(|error| panic!("invalid test ID: {error}")),
            name: Some("Sketch1".into()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
            profiles: Default::default(),
            native_ref: None,
        }];
        let histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: Default::default(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![crate::history::tests::feature("native", None, 0)],
        }];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        limits(&mut policy.limits);
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .unwrap_or_else(|error| panic!("test context failed: {error}"));
        bind_unique_sketch_feature(&ctx, &mut features, &sketches, &histories)
            .expect_err("the sketch-binding route must refuse the selected limit")
    }

    #[test]
    fn sketch_binding_refuses_collection_limit() {
        let error = sketch_binding_error(|limits| limits.max_collection_items = 0);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index SLDPRT native sketch features"
        ));
    }

    #[test]
    fn sketch_binding_refuses_retained_limit() {
        let error = sketch_binding_error(|limits| limits.max_retained_bytes = 0);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain SLDPRT sketch binding identity"
        ));
    }

    #[test]
    fn sketch_binding_refuses_work_limit() {
        let error = sketch_binding_error(|limits| limits.max_work_units = 0);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "index SLDPRT native sketch features"
        ));
    }
}
