// SPDX-License-Identifier: Apache-2.0
//! Sketch binding, regeneration order, and feature-output derivation.

use crate::records::FeatureHistory;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, PathRef, PlanarProfileRef, ProfileRef,
    SplitFaceTool,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::sketches::{Sketch, SketchId};
use cadmpeg_ir::topology::Face;
use std::collections::HashMap;

use crate::history::classify::HistoryIndex;

const BIND_OPERATION: &str = "bind SLDPRT feature sketches";

/// A sketch that a native feature record, and the feature it projects to, resolve to.
struct SketchBinding<'s> {
    /// The bound feature, or for an alias, the base feature it repeats.
    index: usize,
    feature_id: FeatureId,
    native_ref: String,
    sketch: &'s SketchId,
    has_profile: bool,
}

impl<'s> SketchBinding<'s> {
    /// Copy the feature's identities into the binding workspace.
    fn new(
        ctx: &DecodeContext<'_>,
        workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        index: usize,
        feature: &cadmpeg_ir::features::Feature,
        native_ref: &str,
        sketch: &'s Sketch,
    ) -> Result<Self, CodecError> {
        workspace.with_storage(|| {
            Ok::<_, CodecError>(Self {
                index,
                feature_id: feature.id.try_clone_for_decode(ctx, BIND_OPERATION)?,
                native_ref: ctx.copy_retained_text(native_ref, BIND_OPERATION)?,
                sketch: &sketch.id,
                has_profile: !sketch.profiles.is_empty(),
            })
        })
    }
}

/// Bind sketch features to the uniquely named sketches they project, and
/// resolve every profile and path reference that names a bound native record.
pub(crate) fn bind_unique_sketch_feature(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &[Sketch],
    histories: &[FeatureHistory],
) -> Result<(), CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "SLDPRT sketch binding workspace")?;
    let mut sketch_features = Vec::new();
    let mut features_by_name = HashMap::<&str, Vec<usize>>::new();
    for (index, feature) in ctx.admit_iter(&*features, BIND_OPERATION)?.enumerate() {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
        ) {
            continue;
        }
        workspace.with_storage(|| ctx.push_vec(&mut sketch_features, index, BIND_OPERATION))?;
        if let Some(name) = feature.name.as_deref() {
            workspace.with_storage(|| {
                ctx.push_hash_group(
                    &mut features_by_name,
                    name,
                    index,
                    BIND_OPERATION,
                    BIND_OPERATION,
                )
            })?;
        }
    }
    let (sketches_by_name, _sketch_index) = ctx.unique_index(
        ctx.admit_iter(sketches, BIND_OPERATION)?
            .filter_map(|sketch| sketch.name.as_deref().map(|name| (name, sketch))),
        BIND_OPERATION,
    )?;

    let mut bindings = Vec::new();
    let mut binding_of_feature = HashMap::new();
    for &index in ctx.admit_iter(&sketch_features, BIND_OPERATION)? {
        let feature = &features[index];
        let Some(name) = feature.name.as_deref() else {
            continue;
        };
        if !ctx
            .get_hash_map(&features_by_name, name, BIND_OPERATION)?
            .is_some_and(|group| group.len() == 1)
        {
            continue;
        }
        let Some(sketch) = ctx
            .get_hash_map(&sketches_by_name, name, BIND_OPERATION)?
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let binding = SketchBinding::new(ctx, &mut workspace, index, feature, native_ref, sketch)?;
        workspace.with_storage(|| {
            ctx.insert_hash_map(
                &mut binding_of_feature,
                index,
                bindings.len(),
                BIND_OPERATION,
            )?;
            ctx.push_vec(&mut bindings, binding, BIND_OPERATION)
        })?;
    }
    if bindings.is_empty() {
        if let ([index], [sketch]) = (sketch_features.as_slice(), sketches) {
            if let Some(native_ref) = features[*index].native_ref.as_deref() {
                let binding = SketchBinding::new(
                    ctx,
                    &mut workspace,
                    *index,
                    &features[*index],
                    native_ref,
                    sketch,
                )?;
                workspace.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut binding_of_feature,
                        *index,
                        bindings.len(),
                        BIND_OPERATION,
                    )?;
                    ctx.push_vec(&mut bindings, binding, BIND_OPERATION)
                })?;
            }
        }
    }

    // A sketch named `base<n>` that is not bound itself is an alias of the one
    // sketch feature named `base` whose native record it repeats.
    let mut native_features = HashMap::new();
    for history in ctx.admit_iter(histories, BIND_OPERATION)? {
        for feature in ctx.admit_iter(&history.features, BIND_OPERATION)? {
            workspace.with_storage(|| {
                ctx.insert_hash_map(
                    &mut native_features,
                    feature.id.as_str(),
                    feature,
                    BIND_OPERATION,
                )
            })?;
        }
    }
    let mut alias_dependencies = Vec::new();
    let mut aliases = Vec::new();
    for &index in ctx.admit_iter(&sketch_features, BIND_OPERATION)? {
        let alias = &features[index];
        if ctx
            .get_hash_map(&binding_of_feature, &index, BIND_OPERATION)?
            .is_some()
            || !matches!(
                alias.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                        | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                    ..
                })
            )
        {
            continue;
        }
        let Some(base_name) = alias
            .name
            .as_deref()
            .map(|name| crate::resolved_features::component_paths::ordinal_suffix_base(ctx, name))
            .transpose()?
            .flatten()
            .filter(|base| !base.is_empty())
        else {
            continue;
        };
        let Some(candidates) = ctx.get_hash_map(&features_by_name, base_name, BIND_OPERATION)?
        else {
            continue;
        };
        let matches = |candidate: &usize| {
            sketch_alias_matches(ctx, &native_features, alias, &features[*candidate])
        };
        let Some(first) =
            ctx.position_by(candidates, |candidate| matches(candidate), BIND_OPERATION)?
        else {
            continue;
        };
        if ctx
            .position_by(
                &candidates[first + 1..],
                |candidate| matches(candidate),
                BIND_OPERATION,
            )?
            .is_some()
        {
            continue;
        }
        let base_index = candidates[first];
        let Some(native_ref) = alias.native_ref.as_deref() else {
            continue;
        };
        workspace.with_storage(|| {
            ctx.push_vec(&mut alias_dependencies, (index, base_index), BIND_OPERATION)
        })?;
        let Some(&base_binding) =
            ctx.get_hash_map(&binding_of_feature, &base_index, BIND_OPERATION)?
        else {
            continue;
        };
        let base_binding: &SketchBinding<'_> = &bindings[base_binding];
        let alias_binding = workspace.with_storage(|| {
            Ok::<_, CodecError>(SketchBinding {
                index: base_index,
                feature_id: features[base_index]
                    .id
                    .try_clone_for_decode(ctx, BIND_OPERATION)?,
                native_ref: ctx.copy_retained_text(native_ref, BIND_OPERATION)?,
                sketch: base_binding.sketch,
                has_profile: base_binding.has_profile,
            })
        })?;
        workspace.with_storage(|| ctx.push_vec(&mut aliases, alias_binding, BIND_OPERATION))?;
    }

    for binding in ctx.admit_iter(&bindings, BIND_OPERATION)? {
        features[binding.index]
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    binding.sketch.try_clone_for_decode(ctx, BIND_OPERATION)?,
                )),
            }));
    }
    for &(index, base_index) in ctx.admit_iter(&alias_dependencies, BIND_OPERATION)? {
        let base = features[base_index]
            .id
            .try_clone_for_decode(ctx, BIND_OPERATION)?;
        features[index]
            .dependencies
            .insert(ctx, base, "bind SLDPRT sketch alias dependency")?;
    }
    workspace.with_storage(|| ctx.append_vec(&mut bindings, &mut aliases, BIND_OPERATION))?;
    let index = BindingIndex::new(ctx, &mut workspace, &bindings)?;
    for feature in ctx.admit_iter(&mut *features, BIND_OPERATION)? {
        let mut bound = Ok(Vec::new());
        feature.evaluation.edit(|definition, _| {
            bound = index.bind(ctx, definition);
        });
        let mut bound = bound?;
        ctx.sort_unstable_by(&mut bound, |position| position, Ord::cmp, BIND_OPERATION)?;
        ctx.dedup_vec(&mut bound, BIND_OPERATION)?;
        for &position in ctx.admit_iter(&bound, BIND_OPERATION)? {
            let dependency = &bindings[position].feature_id;
            if !ctx.contains(feature.dependencies.as_slice(), dependency, BIND_OPERATION)? {
                feature.dependencies.insert(
                    ctx,
                    dependency.try_clone_for_decode(ctx, BIND_OPERATION)?,
                    "bind SLDPRT sketch dependency",
                )?;
            }
        }
    }
    Ok(())
}

/// Whether an alias sketch feature repeats its base's native record.
fn sketch_alias_matches(
    ctx: &DecodeContext<'_>,
    native_features: &HashMap<&str, &crate::records::Feature>,
    alias: &cadmpeg_ir::features::Feature,
    base: &cadmpeg_ir::features::Feature,
) -> Result<bool, CodecError> {
    let native = |feature: &cadmpeg_ir::features::Feature| {
        feature
            .native_ref
            .as_deref()
            .map(|native_ref| ctx.get_hash_map(native_features, native_ref, BIND_OPERATION))
            .transpose()
            .map(|record| record.flatten().copied())
    };
    let (Some(alias), Some(base)) = (native(alias)?, native(base)?) else {
        return Ok(false);
    };
    let compatible_class = ctx.equal(&alias.input_class, &base.input_class, BIND_OPERATION)?
        || (alias.input_class.is_none()
            && base.input_class.as_deref() == Some("moProfileFeature_c")
            && crate::resolved_features::component_paths::is_dissected_profile_feature(
                ctx, alias,
            )?);
    Ok(compatible_class
        && ctx.equal(&alias.xml_tag, &base.xml_tag, BIND_OPERATION)?
        && crate::records::equal_text_maps(
            ctx,
            &alias.parameters,
            &base.parameters,
            BIND_OPERATION,
        )?
        && ctx.equal(&alias.content, &base.content, BIND_OPERATION)?)
}

/// The first binding each profile or path key resolves to, in binding order.
struct BindingIndex<'b, 's> {
    bindings: &'b [SketchBinding<'s>],
    profile_by_native: HashMap<&'b str, usize>,
    profile_by_feature: HashMap<&'b str, usize>,
    path_by_native: HashMap<&'b str, Vec<usize>>,
}

impl<'b, 's> BindingIndex<'b, 's> {
    fn new(
        ctx: &DecodeContext<'_>,
        workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        bindings: &'b [SketchBinding<'s>],
    ) -> Result<Self, CodecError> {
        let mut index = Self {
            bindings,
            profile_by_native: HashMap::new(),
            profile_by_feature: HashMap::new(),
            path_by_native: HashMap::new(),
        };
        for (position, binding) in ctx.admit_iter(bindings, BIND_OPERATION)?.enumerate() {
            workspace.with_storage(|| {
                if binding.has_profile {
                    ctx.entry_hash_map(
                        &mut index.profile_by_native,
                        binding.native_ref.as_str(),
                        BIND_OPERATION,
                    )?
                    .or_insert(position);
                    ctx.entry_hash_map(
                        &mut index.profile_by_feature,
                        binding.feature_id.as_str(),
                        BIND_OPERATION,
                    )?
                    .or_insert(position);
                }
                ctx.push_hash_group(
                    &mut index.path_by_native,
                    binding.native_ref.as_str(),
                    position,
                    BIND_OPERATION,
                    BIND_OPERATION,
                )
            })?;
        }
        Ok(index)
    }

    fn sketch(&self, ctx: &DecodeContext<'_>, position: usize) -> Result<SketchId, CodecError> {
        self.bindings[position]
            .sketch
            .try_clone_for_decode(ctx, BIND_OPERATION)
    }

    fn bind_planar_profile(
        &self,
        ctx: &DecodeContext<'_>,
        profile: &mut PlanarProfileRef,
        bound: &mut Vec<usize>,
    ) -> Result<(), CodecError> {
        let position = match profile {
            PlanarProfileRef::Unresolved(native_ref) | PlanarProfileRef::Native(native_ref) => {
                ctx.get_hash_map(&self.profile_by_native, native_ref.as_str(), BIND_OPERATION)?
            }
            PlanarProfileRef::Feature(feature) => {
                ctx.get_hash_map(&self.profile_by_feature, feature.as_str(), BIND_OPERATION)?
            }
            _ => None,
        };
        if let Some(&position) = position {
            *profile = self.sketch(ctx, position)?.into();
            ctx.push_vec(bound, position, BIND_OPERATION)?;
        }
        Ok(())
    }

    fn bind_profile(
        &self,
        ctx: &DecodeContext<'_>,
        profile: &mut ProfileRef,
        bound: &mut Vec<usize>,
    ) -> Result<(), CodecError> {
        match profile {
            ProfileRef::Planar(profile) => self.bind_planar_profile(ctx, profile, bound),
            _ => Ok(()),
        }
    }

    /// Bind a native path to the `occurrence`-th binding that names it.
    fn bind_path(
        &self,
        ctx: &DecodeContext<'_>,
        path: &mut PathRef,
        occurrence: usize,
        bound: &mut Vec<usize>,
    ) -> Result<(), CodecError> {
        let PathRef::Native(native_ref) = path else {
            return Ok(());
        };
        let Some(&position) = ctx
            .get_hash_map(&self.path_by_native, native_ref.as_str(), BIND_OPERATION)?
            .and_then(|positions| positions.get(occurrence))
        else {
            return Ok(());
        };
        *path = PathRef::Sketch(self.sketch(ctx, position)?);
        ctx.push_vec(bound, position, BIND_OPERATION)
    }

    /// Resolve every reference of `definition` that a binding names, returning
    /// the positions of the bindings used.
    fn bind(
        &self,
        ctx: &DecodeContext<'_>,
        definition: &mut FeatureDefinition,
    ) -> Result<Vec<usize>, CodecError> {
        let mut bound = Vec::new();
        let FeatureDefinition::Operation(operation) = definition else {
            return Ok(bound);
        };
        match operation {
            FeatureOperation::Extrude { profile, .. } => {
                self.bind_profile(ctx, profile, &mut bound)?;
            }
            FeatureOperation::Wrap { profile, .. } => {
                self.bind_planar_profile(ctx, profile, &mut bound)?;
            }
            FeatureOperation::Rib { construction, .. } => {
                if let Some(profile) = construction.profile.as_mut() {
                    self.bind_planar_profile(ctx, profile, &mut bound)?;
                }
            }
            FeatureOperation::Revolve { construction, .. } => {
                if let Some(profile) = construction.profile_mut() {
                    self.bind_planar_profile(ctx, profile, &mut bound)?;
                }
            }
            FeatureOperation::Sweep { shape, path, .. } => {
                if let Some(profile) = shape.referenced_profile_mut() {
                    self.bind_planar_profile(ctx, profile, &mut bound)?;
                }
                if let Some(path) = path.as_mut() {
                    self.bind_path(ctx, path, 0, &mut bound)?;
                }
            }
            FeatureOperation::TrimSurface { tool, .. }
            | FeatureOperation::SplitFace {
                tool: SplitFaceTool::Path(tool),
                ..
            }
            | FeatureOperation::ProjectedCurve { source: tool, .. } => {
                self.bind_path(ctx, tool, 0, &mut bound)?;
            }
            FeatureOperation::CompositeCurve { segments, .. } => {
                // Each binding resolves the first segment still naming it, so the
                // n-th segment naming a record takes the n-th binding for it.
                let mut occurrences = HashMap::<&str, usize>::new();
                let mut workspace = ctx.reserve_scoped(0, BIND_OPERATION)?;
                for segment in ctx.admit_iter(&mut segments[..], BIND_OPERATION)? {
                    let PathRef::Native(native_ref) = &*segment else {
                        continue;
                    };
                    let Some((key, _)) = ctx.get_key_value_hash_map(
                        &self.path_by_native,
                        native_ref.as_str(),
                        BIND_OPERATION,
                    )?
                    else {
                        continue;
                    };
                    let occurrence = workspace.with_storage(|| {
                        let count = ctx
                            .entry_hash_map(&mut occurrences, *key, BIND_OPERATION)?
                            .or_insert(0);
                        let occurrence = *count;
                        *count = occurrence.checked_add(1).ok_or_else(|| {
                            ctx.refuse_codec_limit(BIND_OPERATION, u64::MAX - 1, u64::MAX)
                        })?;
                        Ok::<_, CodecError>(occurrence)
                    })?;
                    self.bind_path(ctx, segment, occurrence, &mut bound)?;
                }
            }
            FeatureOperation::Loft {
                sections, guidance, ..
            } => {
                for section in ctx.admit_iter(&mut sections[..], BIND_OPERATION)? {
                    if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                        self.bind_profile(ctx, profile, &mut bound)?;
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                        for path in ctx.admit_iter(&mut guides[..], BIND_OPERATION)? {
                            self.bind_path(ctx, path, 0, &mut bound)?;
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                        self.bind_path(ctx, centerline, 0, &mut bound)?;
                    }
                }
            }
            _ => {}
        }
        Ok(bound)
    }
}

/// Resolve the references of `definition` that one binding names.
#[cfg(test)]
pub(super) fn bind_definition_sketch(
    ctx: &DecodeContext<'_>,
    definition: &mut FeatureDefinition,
    native_ref: &str,
    feature_ref: &FeatureId,
    sketch: &SketchId,
    has_profile: bool,
) -> Result<bool, CodecError> {
    let bindings = [SketchBinding {
        index: 0,
        feature_id: feature_ref.clone(),
        native_ref: native_ref.to_owned(),
        sketch,
        has_profile,
    }];
    let mut workspace = ctx.reserve_scoped(0, BIND_OPERATION)?;
    let index = BindingIndex::new(ctx, &mut workspace, &bindings)?;
    Ok(!index.bind(ctx, definition)?.is_empty())
}

/// Assign stable neutral regeneration ordinals with every structural parent and
/// explicit dependency before its consumer. Native history ordinals retain the
/// independent Keywords serialization order.
pub(crate) fn order_features_for_regeneration(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some((order, _order_storage)) = regeneration_order(ctx, features, None)? else {
        return Ok(false);
    };
    assign_regeneration_ordinals(ctx, features, order)?;
    Ok(true)
}

/// The structural parents a tree node names for each child: the first and
/// the last tree node, in feature order, listing it.
pub(super) fn tree_parents<'f>(
    ctx: &DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    features: &'f [cadmpeg_ir::features::Feature],
) -> Result<HashMap<&'f FeatureId, (&'f FeatureId, &'f FeatureId)>, CodecError> {
    const OPERATION: &str = "index SLDPRT feature tree parents";
    let mut parents = HashMap::new();
    for feature in ctx.admit_iter(features, "scan SLDPRT regeneration_order values")? {
        let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        for child in ctx.admit_iter(&children[..], OPERATION)? {
            if let Some((_, last)) = ctx.get_mut_hash_map(&mut parents, child, OPERATION)? {
                *last = &feature.id;
                continue;
            }
            scratch.with_storage(|| {
                ctx.insert_hash_map(&mut parents, child, (&feature.id, &feature.id), OPERATION)
            })?;
        }
    }
    Ok(parents)
}

fn regeneration_order<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    model: Option<&cadmpeg_ir::document::Model>,
) -> Result<Option<(Vec<usize>, cadmpeg_core::decode::ScopedReservation<'ctx>)>, CodecError> {
    const PREDECESSORS: &str = "collect SLDPRT feature predecessors";
    let mut scratch = ctx.reserve_scoped(0, "SLDPRT feature regeneration graph")?;
    let (mut outgoing, mut indegree) = scratch.with_storage(|| {
        let outgoing = ctx.collect_indexed_vec(
            features.len(),
            "sldprt feature regeneration adjacency",
            |_| Ok(Vec::<usize>::new()),
        )?;
        let indegree = ctx.alloc_filled(
            features.len(),
            0usize,
            "sldprt feature regeneration indegree",
        )?;
        Ok::<_, CodecError>((outgoing, indegree))
    })?;
    let tree_parents = tree_parents(ctx, &mut scratch, features)?;
    let mut by_id = HashMap::new();
    for (index, feature) in ctx
        .admit_iter(features, "index SLDPRT feature regeneration IDs")?
        .enumerate()
    {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_id,
                &feature.id,
                index,
                "index SLDPRT feature regeneration IDs",
            )
        })?;
    }
    let mut predecessors = Vec::new();
    for (consumer, feature) in ctx
        .admit_iter(features, "scan SLDPRT regeneration_order values")?
        .enumerate()
    {
        predecessors.clear();
        let mut add = |predecessor: &FeatureId| -> Result<(), CodecError> {
            if let Some(&source) = ctx.get_hash_map(&by_id, predecessor, PREDECESSORS)? {
                ctx.push_scoped_vec(&mut scratch, &mut predecessors, source, PREDECESSORS)?;
            }
            Ok(())
        };
        for predecessor in ctx.admit_iter(feature.dependencies.as_slice(), PREDECESSORS)? {
            add(predecessor)?;
        }
        let tree_parent = ctx
            .get_hash_map(&tree_parents, &feature.id, "look up SLDPRT hash key")?
            .copied();
        if let Some((_, last)) = tree_parent {
            add(last)?;
        }
        if let Some(model) = model {
            // The model's structural owner: the first tree node listing the
            // feature, or its regeneration predecessor when no tree owns it.
            match ctx
                .get_hash_map(&tree_parents, &feature.id, "scan SLDPRT feature parents")?
                .copied()
            {
                Some((first, _)) => add(first)?,
                None => {
                    if let Some(parent) = model.feature_regeneration_parent(&feature.id) {
                        add(parent)?;
                    }
                }
            }
            for configuration in ctx.admit_iter(
                &model.configurations,
                "scan SLDPRT configuration feature dependencies",
            )? {
                if let Some(state) = ctx.get_btree_map(
                    &configuration.feature_states,
                    &feature.id,
                    "look up SLDPRT ordered key",
                )? {
                    for dependency in ctx.admit_iter(
                        state.dependencies.as_slice(),
                        "scan SLDPRT configuration feature dependencies",
                    )? {
                        add(dependency)?;
                    }
                }
            }
        }
        ctx.sort_unstable_by(&mut predecessors, |source| source, Ord::cmp, PREDECESSORS)?;
        ctx.dedup_vec(&mut predecessors, PREDECESSORS)?;
        for &source in ctx.admit_iter(&predecessors, "build SLDPRT feature regeneration graph")? {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut outgoing[source],
                    consumer,
                    "collect SLDPRT feature regeneration edges",
                )
            })?;
            indegree[consumer] += 1;
        }
    }
    let mut ready = std::collections::BTreeSet::new();
    for (index, feature) in ctx
        .admit_iter(features, "scan SLDPRT regeneration_order values")?
        .enumerate()
    {
        if indegree[index] == 0 {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut ready,
                    (feature.ordinal, &feature.id, index),
                    "queue SLDPRT feature regeneration order",
                )
            })?;
        }
    }
    let mut order_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
    let mut order = Vec::new();
    order_storage.with_storage(|| {
        ctx.reserve_capacity(
            &mut order,
            features.len(),
            "collect SLDPRT feature regeneration order",
        )
    })?;
    while let Some(item) = ready.pop_first() {
        ctx.charge_work(1, "sort SLDPRT feature regeneration order")?;
        let index = item.2;
        order_storage.with_storage(|| {
            ctx.push_vec(
                &mut order,
                index,
                "collect SLDPRT feature regeneration order",
            )
        })?;
        for &consumer in
            ctx.admit_iter(&outgoing[index], "scan SLDPRT regeneration_order values")?
        {
            indegree[consumer] -= 1;
            if indegree[consumer] == 0 {
                let feature = &features[consumer];
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut ready,
                        (feature.ordinal, &feature.id, consumer),
                        "queue SLDPRT feature regeneration order",
                    )
                })?;
            }
        }
    }
    if order.len() != features.len() {
        return Ok(None);
    }
    Ok(Some((order, order_storage)))
}

fn assign_regeneration_ordinals(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    order: Vec<usize>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (ordinal, index) in ctx
        .admit_iter(&order, "scan SLDPRT regeneration ordinal order")?
        .copied()
        .enumerate()
    {
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
    let Some((order, _order_storage)) =
        regeneration_order(ctx, &ir.model.features, Some(&ir.model))?
    else {
        return Ok(false);
    };
    assign_regeneration_ordinals(ctx, &mut ir.model.features, order)?;
    Ok(true)
}

/// Bind each decoded face to the body owning it.
fn face_owner_bodies<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    faces: &'a [Face],
    shells: &'a [cadmpeg_ir::topology::Shell],
    regions: &'a [cadmpeg_ir::topology::Region],
) -> Result<
    (
        HashMap<&'a str, &'a BodyId>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "index SLDPRT face owner bodies")?;
    let mut region_bodies = HashMap::new();
    for region in ctx.admit_iter(regions, "index SLDPRT face owner regions")? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut region_bodies,
                region.id.as_str(),
                &region.body,
                "index SLDPRT face owner regions",
            )
        })?;
    }
    let mut shell_bodies = HashMap::new();
    for shell in ctx.admit_iter(shells, "index SLDPRT face owner shells")? {
        let Some(&body) = ctx.get_hash_map(
            &region_bodies,
            shell.region.as_str(),
            "look up SLDPRT hash key",
        )?
        else {
            continue;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut shell_bodies,
                shell.id.as_str(),
                body,
                "index SLDPRT face owner shells",
            )
        })?;
    }
    let mut owners = HashMap::new();
    for face in ctx.admit_iter(faces, "index SLDPRT face owner bodies")? {
        let Some(&body) = ctx.get_hash_map(
            &shell_bodies,
            face.shell.as_str(),
            "look up SLDPRT hash key",
        )?
        else {
            continue;
        };
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut owners,
                face.id.as_str(),
                body,
                "index SLDPRT face owner bodies",
            )
        })?;
    }
    Ok((owners, storage))
}

/// A body identity from its emitted text, or `None` when the text is not an
/// identity. The validation scan is admitted with the copy.
fn output_body_id(ctx: &DecodeContext<'_>, id: &str) -> Result<Option<BodyId>, CodecError> {
    const OPERATION: &str = "retain SLDPRT feature output body";
    let text = ctx.copy_retained_text(id, OPERATION)?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(text.len()), OPERATION)?;
    match BodyId::mint(text) {
        Ok(body) => Ok(Some(body)),
        Err(_) => Ok(None),
    }
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    sources: FeatureOutputSources<'_>,
    faces: &[Face],
    shells: &[cadmpeg_ir::topology::Shell],
    regions: &[cadmpeg_ir::topology::Region],
) -> Result<(), cadmpeg_core::CodecError> {
    const MODIFIERS: &str = "match SLDPRT body modifiers";
    const OUTPUTS: &str = "collect SLDPRT body modifier outputs";
    let FeatureOutputSources {
        face_producers,
        body_modifiers,
    } = sources;
    let mut scratch = ctx.reserve_scoped(0, "index SLDPRT body modifier ordinals")?;
    let mut feature_ids_by_ordinal = HashMap::<u32, Option<&str>>::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT derive_feature_outputs values")? {
        let index = HistoryIndex::new(ctx, &history.features)?;
        let mut ordinal = 0_u32;
        for record in ctx.admit_iter(&history.features, "classify SLDPRT body modifier ordinals")? {
            if index.is_metadata(ctx, record)? {
                continue;
            }
            ordinal = ordinal.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "index SLDPRT body modifier ordinals",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
            if let Some(previous) = ctx.get_mut_hash_map(
                &mut feature_ids_by_ordinal,
                &ordinal,
                "index SLDPRT body modifier ordinals",
            )? {
                *previous = None;
                continue;
            }
            scratch.with_storage(|| {
                ctx.insert_hash_map(
                    &mut feature_ids_by_ordinal,
                    ordinal,
                    Some(record.id.as_str()),
                    "index SLDPRT body modifier ordinals",
                )
            })?;
        }
    }
    if !body_modifiers.is_empty() {
        // Each modified body, paired with every projected feature of its record.
        let mut features_by_native = HashMap::new();
        for (position, feature) in ctx.admit_iter(&*features, MODIFIERS)?.enumerate() {
            let Some(native) = feature.native_ref.as_deref() else {
                continue;
            };
            scratch.with_storage(|| {
                ctx.push_hash_group(
                    &mut features_by_native,
                    native,
                    position,
                    MODIFIERS,
                    MODIFIERS,
                )
            })?;
        }
        let mut modified = Vec::new();
        for (body, ordinal) in
            ctx.admit_iter(body_modifiers, "scan SLDPRT body_modifiers values")?
        {
            let Some(Some(native_ref)) =
                ctx.get_hash_map(&feature_ids_by_ordinal, ordinal, MODIFIERS)?
            else {
                continue;
            };
            let Some(positions) = ctx.get_hash_map(&features_by_native, *native_ref, MODIFIERS)?
            else {
                continue;
            };
            for &position in ctx.admit_iter(positions, MODIFIERS)? {
                scratch.with_storage(|| {
                    ctx.push_vec(&mut modified, (position, body.as_str()), MODIFIERS)
                })?;
            }
        }
        drop(features_by_native);
        for (position, body) in ctx.admit_iter(modified, MODIFIERS)? {
            let Some(body) = output_body_id(ctx, body)? else {
                continue;
            };
            let feature = &mut features[position];
            if ctx.contains(feature.evaluation.outputs(), &body, OUTPUTS)? {
                continue;
            }
            let mut inserted = Ok(false);
            feature.evaluation.edit(|_, outputs| {
                inserted = outputs.insert(ctx, body, OUTPUTS);
            });
            inserted?;
        }
    }
    if face_producers.is_empty() {
        return Ok(());
    }
    let (owners, _owners_storage) = face_owner_bodies(ctx, faces, shells, regions)?;
    let mut produced: HashMap<u32, Vec<&BodyId>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for (face, source_id) in ctx.admit_iter(face_producers, "collect SLDPRT produced bodies")? {
        let Some(&body) = ctx.get_hash_map(&owners, face.as_str(), "look up SLDPRT hash key")?
        else {
            continue;
        };
        if ctx.contains_hash_set(&seen, &(*source_id, body), "collect SLDPRT produced bodies")? {
            continue;
        }
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut seen,
                (*source_id, body),
                "collect SLDPRT produced bodies",
            )?;
            ctx.push_hash_group(
                &mut produced,
                *source_id,
                body,
                "index SLDPRT produced bodies",
                "collect SLDPRT produced bodies",
            )
        })?;
    }
    // The source identifier of the first native record bearing each identity.
    let mut sources_by_record = HashMap::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT feature output histories")? {
        for record in ctx.admit_iter(&history.features, "match SLDPRT feature output source")? {
            if ctx.contains_key_hash_map(
                &sources_by_record,
                record.id.as_str(),
                "match SLDPRT feature output source",
            )? {
                continue;
            }
            scratch.with_storage(|| {
                ctx.insert_hash_map(
                    &mut sources_by_record,
                    record.id.as_str(),
                    record.source_value(),
                    "match SLDPRT feature output source",
                )
            })?;
        }
    }
    for feature in ctx.admit_iter(features, "derive SLDPRT feature outputs")? {
        if !feature.evaluation.outputs().is_empty() {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(&Some(source_id)) = ctx.get_hash_map(
            &sources_by_record,
            native_ref,
            "match SLDPRT feature output source",
        )?
        else {
            continue;
        };
        let Some(bodies) =
            ctx.get_hash_map(&produced, &source_id, "look up SLDPRT produced bodies")?
        else {
            continue;
        };
        let mut outputs = Vec::new();
        ctx.reserve_capacity(&mut outputs, bodies.len(), "collect SLDPRT feature outputs")?;
        for body in ctx.admit_iter(bodies, "scan SLDPRT bodies values")? {
            outputs.push(body.try_clone_for_decode(ctx, "retain SLDPRT feature output body")?);
        }
        feature
            .evaluation
            .set_outputs(cadmpeg_ir::features::DistinctMembers::try_from(
                outputs, ctx,
            )?);
    }
    Ok(())
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
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
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
        // Admit adjacency fills and key copies before the selected graph gate.
        let error =
            crate::test_support::work_refusal_at("index SLDPRT feature regeneration IDs", |ctx| {
                order_features_for_regeneration(ctx, &mut [ordering_feature()])
            });
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "index SLDPRT feature regeneration IDs"
        ));
    }

    #[test]
    fn model_regeneration_parent_scan_refuses_work_limit() {
        // Admit adjacency fills and key copies before the selected graph gate.
        let error = crate::test_support::work_refusal_at("scan SLDPRT feature parents", |ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.features.push(ordering_feature());
            order_model_features_for_regeneration(ctx, &mut ir)
        });
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
            properties: std::collections::BTreeMap::default(),
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
            crate::history::bind::FeatureOutputSources {
                face_producers: &[],
                body_modifiers: &body_modifiers,
            },
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
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain SLDPRT feature output body",
            |cap| {
                Err::<(), cadmpeg_core::CodecError>(feature_output_error(|limits| {
                    limits.max_retained_bytes = cap;
                }))
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain SLDPRT feature output body"
        ));
    }

    #[test]
    fn feature_outputs_refuse_work_limit() {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "classify SLDPRT body modifier ordinals",
            |cap| {
                Err::<(), cadmpeg_core::CodecError>(feature_output_error(|limits| {
                    limits.max_work_units = cap;
                }))
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "classify SLDPRT body modifier ordinals"
        ));
    }

    fn sketch_binding_refusal(dimension: ResourceDimension) {
        let mut neutral = ordering_feature();
        neutral.name = Some("Sketch1".into());
        neutral.native_ref = Some("native".into());
        neutral
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }));
        let sketches = [cadmpeg_ir::sketches::Sketch {
            id: cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch")
                .unwrap_or_else(|error| panic!("invalid test ID: {error}")),
            name: Some("Sketch1".into()),
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        }];
        let histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: std::collections::BTreeMap::default(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![crate::history::tests::feature("native", None, 0)],
        }];
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            dimension,
            "bind SLDPRT feature sketches",
            |cap| {
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                    ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap
                    }
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    _ => panic!("sketch binding dimension"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)?;
                let mut features = [neutral.clone()];
                let result = bind_unique_sketch_feature(&ctx, &mut features, &sketches, &histories);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            },
        );
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn sketch_binding_refuses_collection_limit() {
        sketch_binding_refusal(ResourceDimension::CollectionItems);
    }

    #[test]
    fn sketch_binding_refuses_retained_limit() {
        sketch_binding_refusal(ResourceDimension::RetainedBytes);
    }

    #[test]
    fn sketch_binding_refuses_scoped_limit() {
        sketch_binding_refusal(ResourceDimension::MaterializedBytes);
    }

    #[test]
    fn sketch_binding_refuses_work_limit() {
        sketch_binding_refusal(ResourceDimension::WorkUnits);
    }
}
