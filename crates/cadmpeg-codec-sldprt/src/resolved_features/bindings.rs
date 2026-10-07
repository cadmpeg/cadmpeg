//! Pattern, sweep and scalar operand lane binding.

use super::assembly::is_supplemental_config_lane;
use super::axes::{
    compact_line_reference_directions, declared_line_reference_directions,
    linear_pattern_display_directions, temporary_axis_reference, typed_linear_pattern_dimensions,
};
use super::component_paths::is_dissected_profile_feature;
use super::component_paths::FeaturesBySource;
use super::endpoints::{
    alternate_current_indexed_curve_endpoint_indices, compact_indexed_curve_endpoint_indices,
    compact_legacy_curve_endpoint_indices, extended_compact_indexed_curve_endpoint_indices,
    marker_is_selected_construction_line, wide_indexed_curve_endpoint_indices,
};
use super::markers::{
    current_reverse_incidence_endpoint_offsets, linked_profile_point, relation_bindings_scoped,
};
use super::operands::resolve_scalar_operand_markers;
use super::reference_geometry::explicit_reference_plane_frame;
use super::relation_records::{feature_intervals, relation_instances};
use super::scalars::ObjectNames;
use super::selections::{
    compact_body_selections, compact_edge_selections, compact_surface_selections,
    coordinate_marker_local_links, generated_surface_identities, marker_local_links,
    mirror_pattern_component_path_at, unique_marker_candidate, COMPACT_EDGE_VECTOR_MARKER,
};
use super::typed_relations::{legacy_terminal_indexed_profile_line, marker_curve_endpoint_markers};
use super::{classes_within, sorted_classes};
use crate::brep::feature_source::FeatureSourceId;
use crate::classification::{native_object_class, NativeClassKind};
use crate::history::classify::is_history_metadata_record;
use crate::history::literals::parse_positive_angle_rad;
use crate::history::project::pattern::parse_count;
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_core::decode::index_from_u64;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::{
    features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureDirection3, FeatureOperation, FinitePoint3, PathRef,
    },
    units::UnitVector3,
};
use std::collections::{BTreeMap, HashMap, HashSet};

const EPS_BINDINGS_BIND_PATTERN_INPUTS_E12: f64 = 1e-12;
const EPS_BINDINGS_BIND_DETACHED_SPATIAL_RELATION_OBJECTS_E9: f64 = 1e-9;

pub(super) fn history_metadata_ids<'a>(
    ctx: &DecodeContext<'_>,
    histories: &'a [crate::records::FeatureHistory],
) -> Result<HashSet<&'a str>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "scan SLDPRT history metadata identities";
    let mut ids = HashSet::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            if is_history_metadata_record(ctx, feature, &history.features)? {
                ctx.insert_hash_set(&mut ids, feature.id.as_str(), SCALAR_BINDING_INDEX)?;
            }
        }
    }
    Ok(ids)
}

const PATTERN_INPUTS: &str = "index SLDPRT pattern inputs";

/// Appends a candidate to an index's group unless an equal one is present.
fn push_distinct_candidate<T: PartialEq>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    groups: &mut BTreeMap<usize, Vec<T>>,
    index: usize,
    candidate: T,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(group) = ctx.get_mut_btree_map(groups, &index, PATTERN_INPUTS)? {
        if !ctx.any_by(
            &*group,
            |existing| Ok(*existing == candidate),
            PATTERN_INPUTS,
        )? {
            storage.with_storage(|| ctx.push_vec(group, candidate, PATTERN_INPUTS))?;
        }
        return Ok(());
    }
    storage.with_storage(|| {
        ctx.push_btree_group(groups, index, candidate, PATTERN_INPUTS, PATTERN_INPUTS)
    })
}

fn admission_error(ctx: &DecodeContext<'_>, message: &str) -> cadmpeg_core::CodecError {
    match ctx.copy_retained_text(message, "retain SLDPRT pattern admission error") {
        Ok(text) => cadmpeg_core::CodecError::Malformed(text),
        Err(error) => error,
    }
}

/// Bind pattern operands carried by adjacent feature-input objects.
pub(crate) fn bind_pattern_inputs(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, PATTERN_INPUTS)?;
    let metadata_ids = storage.with_storage(|| history_metadata_ids(ctx, histories))?;
    let mut history_features = Vec::new();
    let mut history_by_id = HashMap::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT pattern input candidates")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT pattern input candidates")? {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut history_features,
                    feature,
                    "collect SLDPRT pattern input candidates",
                )
            })?;
            storage.with_storage(|| {
                ctx.entry_hash_map(&mut history_by_id, feature.id.as_str(), PATTERN_INPUTS)
                    .map(|slot| {
                        slot.or_insert(feature);
                    })
            })?;
        }
    }
    let by_source = FeaturesBySource::new(ctx, history_features.iter().copied())?;
    // Cosmetic threads by owning history, in ordinal order.
    let mut cosmetic_threads = BTreeMap::<&str, Vec<&crate::records::Feature>>::new();
    for feature in ctx.admit_iter(&history_features, PATTERN_INPUTS)? {
        if feature.input_class.as_deref() == Some("moCosmeticThread_c") {
            storage.with_storage(|| {
                ctx.push_btree_group(
                    &mut cosmetic_threads,
                    feature.parent.as_str(),
                    *feature,
                    PATTERN_INPUTS,
                    PATTERN_INPUTS,
                )
            })?;
        }
    }
    for (_, threads) in ctx.admit_iter(&mut cosmetic_threads, PATTERN_INPUTS)? {
        ctx.stable_sort_by_key(threads, |thread| thread.ordinal, Ord::cmp, PATTERN_INPUTS)?;
    }
    // The last cosmetic thread of the feature's history before it.
    let derived_cosmetic_thread_seed = |feature: &crate::records::Feature| {
        let Some(threads) =
            ctx.get_btree_map(&cosmetic_threads, feature.parent.as_str(), PATTERN_INPUTS)?
        else {
            return Ok::<_, cadmpeg_core::CodecError>(None);
        };
        let before = ctx.partition_point(
            threads,
            |thread| Ok(thread.ordinal < feature.ordinal),
            PATTERN_INPUTS,
        )?;
        Ok(before
            .checked_sub(1)
            .and_then(|index| threads.get(index))
            .map(|thread| thread.id.as_str()))
    };
    let mut model_by_native = HashMap::new();
    for (index, feature) in ctx
        .admit_iter(&*model_features, PATTERN_INPUTS)?
        .enumerate()
    {
        if let Some(native) = feature.native_ref.as_deref() {
            let native = storage.with_storage(|| {
                ctx.copy_retained_text(native, "retain SLDPRT pattern native identity")
            })?;
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut model_by_native, native, index, PATTERN_INPUTS)
            })?;
        }
    }
    let model_of = |native: &str| {
        Ok::<_, cadmpeg_core::CodecError>(
            ctx.get_hash_map(&model_by_native, native, PATTERN_INPUTS)?
                .copied(),
        )
    };
    let mut seeds_by_pattern = BTreeMap::<usize, Vec<cadmpeg_ir::features::FeatureId>>::new();
    let mut paths_by_pattern =
        BTreeMap::<usize, Vec<(cadmpeg_ir::features::FeatureId, PathRef)>>::new();
    let mut linear_directions_by_pattern = BTreeMap::<usize, Vec<FeatureDirection3>>::new();
    let mut mirror_planes_by_pattern = BTreeMap::<usize, Vec<(Point3, Vector3)>>::new();
    let mut mirror_seed_sets_by_pattern =
        BTreeMap::<usize, Vec<Vec<cadmpeg_ir::features::FeatureId>>>::new();
    let mut circular_axes_by_pattern = BTreeMap::<usize, Vec<(FinitePoint3, UnitVector3)>>::new();
    // Curve-pattern seeds join the pattern seeds after every lane is read.
    let mut curve_seeds = Vec::<(usize, cadmpeg_ir::features::FeatureId)>::new();

    for lane in ctx.admit_iter(lanes, "scan SLDPRT pattern input lanes")? {
        let discovered_identities;
        let generated_identities = if lane.generated_surface_identities.is_empty() {
            discovered_identities = generated_surface_identities(ctx, lane)?;
            &discovered_identities
        } else {
            &lane.generated_surface_identities
        };
        let line_refs = sorted_classes(
            ctx,
            &mut storage,
            lane,
            "moLineRef_w",
            "scan SLDPRT pattern line declarations",
        )?;
        // `lanes::admit` compares every stored name offset with the offset
        // `object_names` read out of `native_payload`, so an admitted name
        // offset is an index of that payload. It is narrowed once here, where
        // that proof holds, and the objects below carry `usize` offsets.
        let mut starts = Vec::new();
        for (offset, feature) in ctx.admit_iter(
            lane_feature_objects(
                ctx,
                history_features.iter().copied(),
                &metadata_ids,
                lane,
                "scan SLDPRT pattern input candidates",
            )?,
            "collect SLDPRT pattern input candidates",
        )? {
            if let Ok(offset) = usize::try_from(offset) {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut starts,
                        (offset, feature),
                        "collect SLDPRT pattern input candidates",
                    )
                })?;
            }
        }
        ctx.sort_unstable_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT pattern input candidates",
        )?;
        for (start_index, &(start, feature)) in ctx
            .admit_iter(&starts, "scan SLDPRT pattern input candidates")?
            .enumerate()
        {
            let has_derived_cosmetic_thread_output =
                starts.get(start_index + 1).is_some_and(|(_, candidate)| {
                    candidate.input_class.as_deref() == Some("moDerivedCosmeticThread_c")
                });
            // The end of this pattern's object is the next object name's
            // offset, or the end of the payload when this is the last one.
            // Both are inside `native_payload`: an object name states the
            // payload bytes at its own offset, and `SldprtNative::load`
            // refuses a name whose offset the payload does not state.
            let end = {
                let next = start_index + 1 + usize::from(has_derived_cosmetic_thread_output);
                match starts.get(next) {
                    Some((offset, _)) => *offset,
                    None => lane.native_payload.len(),
                }
            };
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                == NativeClassKind::MirrorPattern
            {
                let Some(model_index) = model_of(feature.id.as_str())? else {
                    continue;
                };
                let (needs_plane, needs_seeds) =
                    match model_features[model_index].evaluation.definition() {
                        FeatureDefinition::Operation(FeatureOperation::Pattern {
                            seeds,
                            pattern,
                            ..
                        }) => (pattern.is_unresolved(), seeds.is_empty()),
                        _ => continue,
                    };
                if (!needs_plane && !needs_seeds) || start >= end {
                    continue;
                }
                let object = &lane.native_payload[start..end];
                if needs_plane {
                    if let Ok(Some((origin, normal, _))) =
                        explicit_reference_plane_frame(ctx, object)?
                    {
                        push_distinct_candidate(
                            ctx,
                            &mut storage,
                            &mut mirror_planes_by_pattern,
                            model_index,
                            (origin, normal),
                        )?;
                    }
                }
                if !needs_seeds {
                    continue;
                }
                let mut seeds = Vec::<cadmpeg_ir::features::FeatureId>::new();
                let scan_end = object
                    .len()
                    .checked_sub(COMPACT_EDGE_VECTOR_MARKER.len())
                    .unwrap_or_default();
                for offset in
                    ctx.admit_iter(&(0..scan_end), "scan SLDPRT mirror pattern seed paths")?
                {
                    if object.get(offset..offset + COMPACT_EDGE_VECTOR_MARKER.len())
                        != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                    {
                        continue;
                    }
                    let Some(components) = mirror_pattern_component_path_at(ctx, object, offset)?
                    else {
                        continue;
                    };
                    let Some(native) = by_source.terminal_feature(ctx, &components)? else {
                        continue;
                    };
                    let Some(seed_index) = model_of(native.id.as_str())? else {
                        continue;
                    };
                    if seed_index == model_index {
                        continue;
                    }
                    let seed = &model_features[seed_index].id;
                    if !ctx.any_by(
                        &seeds,
                        |existing| {
                            ctx.equal(
                                existing.as_str(),
                                seed.as_str(),
                                "compare SLDPRT mirror pattern seeds",
                            )
                        },
                        "compare SLDPRT mirror pattern seeds",
                    )? {
                        let seed = copy_feature_binding_id(ctx, seed)?;
                        ctx.push_vec(&mut seeds, seed, "collect SLDPRT mirror pattern seeds")?;
                    }
                }
                if seeds.is_empty() {
                    if has_derived_cosmetic_thread_output {
                        if let Some(seed_index) = match derived_cosmetic_thread_seed(feature)? {
                            Some(native) => model_of(native)?,
                            None => None,
                        } {
                            let mut seeds = Vec::new();
                            let seed =
                                copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                            ctx.push_vec(&mut seeds, seed, "collect SLDPRT mirror pattern seeds")?;
                            push_distinct_candidate(
                                ctx,
                                &mut storage,
                                &mut mirror_seed_sets_by_pattern,
                                model_index,
                                seeds,
                            )?;
                        }
                    }
                } else {
                    push_distinct_candidate(
                        ctx,
                        &mut storage,
                        &mut mirror_seed_sets_by_pattern,
                        model_index,
                        seeds,
                    )?;
                }
                continue;
            }
            if feature.input_class.as_deref() == Some("moCirPattern_c") {
                let Some(model_index) = model_of(feature.id.as_str())? else {
                    continue;
                };
                let (needs_seed, needs_axis) = match model_features[model_index]
                    .evaluation
                    .definition()
                {
                    FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern })
                        if matches!(
                            pattern.definition(),
                            PatternTransform::Unresolved {
                                form: Some(cadmpeg_ir::features::patterns::PatternForm::Circular)
                            }
                        ) =>
                    {
                        (seeds.is_empty(), true)
                    }
                    FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                        (seeds.is_empty(), false)
                    }
                    _ => continue,
                };
                if (!needs_seed && !needs_axis) || start >= end {
                    continue;
                }
                if needs_seed {
                    if let Some(pattern_source) = feature.source_value() {
                        let mut seeds = Vec::new();
                        for identity in ctx.admit_iter(
                            generated_identities,
                            "scan SLDPRT pattern input candidates",
                        )? {
                            if !usize::try_from(identity.offset)
                                .ok()
                                .is_some_and(|offset| (start..end).contains(&offset))
                                || !identity.components.first().is_some_and(|component| {
                                    View::u32_le_at(&component.type_signature, 4)
                                        == Some(pattern_source)
                                })
                                || !identity.components.last().is_some_and(|component| {
                                    View::u32_le_at(&component.type_signature, 4)
                                        == Some(identity.feature_source_id.value())
                                        && component.local_id == Some(identity.local_identity)
                                })
                            {
                                continue;
                            }
                            let Some(Some(seed)) = by_source.source(
                                ctx,
                                identity.feature_source_id.value(),
                                "scan SLDPRT pattern input candidates",
                            )?
                            else {
                                continue;
                            };
                            let Some(seed_index) = model_of(seed.id.as_str())? else {
                                continue;
                            };
                            if seed_index != model_index {
                                storage.with_storage(|| {
                                    ctx.push_vec(
                                        &mut seeds,
                                        &model_features[seed_index].id,
                                        "collect SLDPRT pattern input candidates",
                                    )
                                })?;
                            }
                        }
                        let Some((first, rest)) = seeds.split_first() else {
                            continue;
                        };
                        if ctx.all_by(
                            rest,
                            |seed| {
                                ctx.equal(
                                    seed.as_str(),
                                    first.as_str(),
                                    "deduplicate SLDPRT pattern input seeds",
                                )
                            },
                            "deduplicate SLDPRT pattern input seeds",
                        )? {
                            let seed = copy_feature_binding_id(ctx, first)?;
                            push_distinct_candidate(
                                ctx,
                                &mut storage,
                                &mut seeds_by_pattern,
                                model_index,
                                seed,
                            )?;
                        }
                    }
                }
                if needs_axis {
                    if let Some(axis) =
                        temporary_axis_reference(ctx, &lane.native_payload, start, end)?
                    {
                        push_distinct_candidate(
                            ctx,
                            &mut storage,
                            &mut circular_axes_by_pattern,
                            model_index,
                            axis,
                        )?;
                    }
                }
                continue;
            }
            if feature.input_class.as_deref() == Some("moLPattern_c") {
                let Some(model_index) = model_of(feature.id.as_str())? else {
                    continue;
                };
                if matches!(&(model_features[model_index].evaluation.definition()),
                    FeatureDefinition::Operation(FeatureOperation::Pattern {
                        pattern: admitted_pattern,
                        ..
                    }) if matches!(admitted_pattern.definition(), PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Linear) })
                ) && start < end
                {
                    if let Some((spacing, count)) =
                        typed_linear_pattern_dimensions(ctx, feature, lane, start, end)?
                    {
                        let admitted = PatternKind::new(PatternTransform::Linear {
                            direction: None,
                            spacing,
                            count,
                            second: None,
                        })
                        .map_err(|message| admission_error(ctx, message))?;
                        model_features[model_index]
                            .evaluation
                            .edit(|definition, _| {
                                if let FeatureDefinition::Operation(FeatureOperation::Pattern {
                                    pattern,
                                    ..
                                }) = definition
                                {
                                    *pattern = admitted;
                                }
                            });
                    }
                }
                let (needs_seed, needs_direction) = match model_features[model_index]
                    .evaluation
                    .definition()
                {
                    FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
                        let PatternTransform::Linear { direction, .. } = pattern.definition()
                        else {
                            continue;
                        };
                        (seeds.is_empty(), direction.is_none())
                    }
                    _ => continue,
                };
                if !needs_seed && !needs_direction {
                    continue;
                }
                if needs_seed {
                    let seed_native = if has_derived_cosmetic_thread_output {
                        derived_cosmetic_thread_seed(feature)?
                    } else {
                        start_index
                            .checked_sub(1)
                            .and_then(|index| starts.get(index))
                            .map(|(_, seed)| seed.id.as_str())
                    };
                    if let Some(seed_index) = match seed_native {
                        Some(native) => model_of(native)?,
                        None => None,
                    } {
                        let seed = copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                        push_distinct_candidate(
                            ctx,
                            &mut storage,
                            &mut seeds_by_pattern,
                            model_index,
                            seed,
                        )?;
                    }
                }
                if !needs_direction {
                    continue;
                }
                const DIRECTIONS: &str = "collect SLDPRT pattern line directions";
                let declarations = classes_within(
                    ctx,
                    &line_refs,
                    u64_from_index(start).checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(DIRECTIONS, u64::MAX - 1, u64::MAX)
                    })?,
                    u64_from_index(end),
                    "scan SLDPRT pattern line declarations",
                )?;
                let mut directions = Vec::new();
                for class in ctx.admit_iter(declarations, DIRECTIONS)? {
                    let declared = declared_line_reference_directions(
                        ctx,
                        &lane.native_payload,
                        class.offset,
                        end,
                    )?;
                    storage.with_storage(|| {
                        ctx.extend_vec(
                            &mut directions,
                            declared,
                            "merge SLDPRT declared line directions",
                        )
                    })?;
                }
                let mut excluded_handles = Vec::new();
                for class in ctx.admit_iter(declarations, DIRECTIONS)? {
                    if let Ok(offset) = usize::try_from(class.offset) {
                        for handle in [offset + 136, offset + 144] {
                            storage.with_storage(|| {
                                ctx.push_vec(
                                    &mut excluded_handles,
                                    handle,
                                    "collect SLDPRT excluded line handles",
                                )
                            })?;
                        }
                    }
                }
                let compact = compact_line_reference_directions(
                    ctx,
                    &lane.native_payload,
                    start,
                    end,
                    &excluded_handles,
                )?;
                storage.with_storage(|| {
                    ctx.extend_vec(
                        &mut directions,
                        compact,
                        "merge SLDPRT compact line directions",
                    )
                })?;
                if directions.is_empty() {
                    let spacing_m = |name: &str| -> Result<Option<f64>, cadmpeg_core::CodecError> {
                        let Some(value) =
                            ctx.get_btree_map(&feature.parameters, name, DIRECTIONS)?
                        else {
                            return Ok(None);
                        };
                        crate::history::literals::admit_literal(ctx, value, DIRECTIONS)?;
                        Ok(
                            crate::history::literals::parse_positive_dimension_length_mm(value)
                                .map(|value| value.get() / 1000.0),
                        )
                    };
                    let display = linear_pattern_display_directions(
                        ctx,
                        &lane.native_payload,
                        start,
                        end,
                        &lane.names,
                        [spacing_m("D3")?, spacing_m("D4")?],
                    )?;
                    storage.with_storage(|| {
                        ctx.extend_vec(
                            &mut directions,
                            display,
                            "collect SLDPRT pattern display directions",
                        )
                    })?;
                }
                let mut unique_directions = Vec::new();
                for direction in ctx
                    .admit_iter(directions, DIRECTIONS)?
                    .map(FeatureDirection3::from_unit_without_small_components)
                {
                    let parallel = ctx.any_by(
                        &unique_directions,
                        |candidate: &FeatureDirection3| {
                            let candidate = candidate.get();
                            let direction = direction.get();
                            let dot = candidate.x * direction.x
                                + candidate.y * direction.y
                                + candidate.z * direction.z;
                            Ok((dot.abs() - 1.0).abs() <= EPS_BINDINGS_BIND_PATTERN_INPUTS_E12)
                        },
                        DIRECTIONS,
                    )?;
                    if !parallel {
                        storage.with_storage(|| {
                            ctx.push_vec(&mut unique_directions, direction, DIRECTIONS)
                        })?;
                    }
                }
                if matches!(unique_directions.len(), 1 | 2) {
                    for direction in ctx.admit_iter(unique_directions, DIRECTIONS)? {
                        push_distinct_candidate(
                            ctx,
                            &mut storage,
                            &mut linear_directions_by_pattern,
                            model_index,
                            direction,
                        )?;
                    }
                }
                continue;
            }
            if feature.input_class.as_deref() != Some("moCurvePattern_c") {
                continue;
            }
            let Some(model_index) = model_of(feature.id.as_str())? else {
                continue;
            };
            let (needs_seed, needs_path) = match model_features[model_index].evaluation.definition()
            {
                FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
                    let PatternTransform::CurveDriven { path, .. } = pattern.definition() else {
                        continue;
                    };
                    (seeds.is_empty(), path.is_none())
                }
                _ => continue,
            };
            if needs_seed {
                if let Some((_, seed)) = start_index
                    .checked_sub(1)
                    .and_then(|index| starts.get(index))
                {
                    if let Some(seed_index) = model_of(seed.id.as_str())? {
                        let seed = copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                        storage.with_storage(|| {
                            ctx.push_vec(&mut curve_seeds, (model_index, seed), PATTERN_INPUTS)
                        })?;
                    }
                }
            }
            if !needs_path {
                continue;
            }
            let Some((_, target)) = starts.get(start_index + 1) else {
                continue;
            };
            if target.input_class.as_deref() != Some("moProfileFeature_c") {
                continue;
            }
            let Some(target_index) = model_of(target.id.as_str())? else {
                continue;
            };
            let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) = model_features[target_index].evaluation.definition()
            else {
                continue;
            };
            let dependency = copy_feature_binding_id(ctx, &model_features[target_index].id)?;
            let sketch = copy_feature_binding_sketch_id(ctx, sketch)?;
            push_distinct_candidate(
                ctx,
                &mut storage,
                &mut paths_by_pattern,
                model_index,
                (dependency, PathRef::Sketch(sketch)),
            )?;
        }
    }
    for (index, seed) in ctx.admit_iter(curve_seeds, PATTERN_INPUTS)? {
        push_distinct_candidate(ctx, &mut storage, &mut seeds_by_pattern, index, seed)?;
    }
    const DEPENDENCIES: &str = "collect SLDPRT pattern dependencies";
    for (index, candidates) in ctx.admit_iter(seeds_by_pattern, PATTERN_INPUTS)? {
        let Ok([seed]) = <[_; 1]>::try_from(candidates) else {
            continue;
        };
        if !ctx.contains(
            model_features[index].dependencies.as_slice(),
            &seed,
            DEPENDENCIES,
        )? {
            let dependency = copy_feature_binding_id(ctx, &seed)?;
            model_features[index]
                .dependencies
                .insert(ctx, dependency, DEPENDENCIES)?;
        }
        let mut edit_result = Ok(());
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
                definition
            {
                if seeds.is_empty() {
                    edit_result = ctx.push_vec(
                        seeds,
                        PatternSeed::Feature(seed),
                        "collect SLDPRT feature binding assignments",
                    );
                }
            }
        });
        edit_result?;
    }
    for (index, candidates) in ctx.admit_iter(paths_by_pattern, PATTERN_INPUTS)? {
        let Ok([(dependency, path)]) = <[_; 1]>::try_from(candidates) else {
            continue;
        };
        if !ctx.contains(
            model_features[index].dependencies.as_slice(),
            &dependency,
            DEPENDENCIES,
        )? {
            model_features[index]
                .dependencies
                .insert(ctx, dependency, DEPENDENCIES)?;
        }
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                definition
            {
                if let Some(slot) = pattern.curve_path_mut() {
                    if slot.is_none() {
                        *slot = Some(path);
                    }
                }
            }
        });
    }
    for (index, candidates) in ctx.admit_iter(linear_directions_by_pattern, PATTERN_INPUTS)? {
        let native = match model_features[index].native_ref.as_deref() {
            Some(native) => ctx
                .get_hash_map(&history_by_id, native, PATTERN_INPUTS)?
                .copied(),
            None => None,
        };
        let parameters = match native {
            Some(feature) => {
                let spacing = match ctx.get_btree_map(&feature.parameters, "D4", PATTERN_INPUTS)? {
                    Some(value) => {
                        crate::history::literals::admit_literal(ctx, value, PATTERN_INPUTS)?;
                        crate::history::literals::parse_positive_dimension_length_mm(value)
                    }
                    None => None,
                };
                match spacing {
                    Some(spacing) => {
                        match ctx.get_btree_map(&feature.parameters, "D2", PATTERN_INPUTS)? {
                            Some(value) => ctx
                                .parse_text::<u32>(
                                    value,
                                    "parse SLDPRT second linear pattern count",
                                )?
                                .ok()
                                .map(|count| (spacing, count)),
                            None => None,
                        }
                    }
                    None => None,
                }
            }
            None => None,
        };
        let mut edit_result = Ok(());
        model_features[index].evaluation.edit(|definition, _| {
            edit_result = (|| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                    definition
                {
                    let PatternTransform::Linear {
                        direction,
                        spacing,
                        count,
                        second,
                    } = pattern.definition()
                    else {
                        return Ok(());
                    };
                    let mut direction = *direction;
                    let mut second = second.clone();
                    match candidates.as_slice() {
                        [first] if direction.is_none() => direction = Some(*first),
                        [first, second_direction] => {
                            if let (true, true, Some((spacing, count))) =
                                (direction.is_none(), second.is_none(), parameters)
                            {
                                direction = Some(*first);
                                second =
                                    Some(cadmpeg_ir::features::patterns::LinearPatternDirection {
                                        direction: *second_direction,
                                        spacing,
                                        count,
                                    });
                            }
                        }
                        _ => {}
                    }
                    *pattern = PatternKind::new(PatternTransform::Linear {
                        direction,
                        spacing: *spacing,
                        count: *count,
                        second,
                    })
                    .map_err(|message| admission_error(ctx, message))?;
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        edit_result?;
    }
    for (index, candidates) in ctx.admit_iter(mirror_planes_by_pattern, PATTERN_INPUTS)? {
        let [(origin, normal)] = candidates.as_slice() else {
            continue;
        };
        if matches!(model_features[index].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if pattern.is_unresolved())
        {
            let admitted = PatternKind::new(PatternTransform::Mirror {
                plane_origin: admitted_point(*origin)?,
                plane_normal: admitted_direction(*normal)?,
            })
            .map_err(|message| admission_error(ctx, message))?;
            model_features[index].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                    definition
                {
                    *pattern = admitted;
                }
            });
        }
    }
    for (index, candidates) in ctx.admit_iter(mirror_seed_sets_by_pattern, PATTERN_INPUTS)? {
        let Ok([seeds]) = <[_; 1]>::try_from(candidates) else {
            continue;
        };
        for seed in ctx.admit_iter(&seeds, DEPENDENCIES)? {
            if !ctx.contains(
                model_features[index].dependencies.as_slice(),
                seed,
                DEPENDENCIES,
            )? {
                let dependency = copy_feature_binding_id(ctx, seed)?;
                model_features[index]
                    .dependencies
                    .insert(ctx, dependency, DEPENDENCIES)?;
            }
        }
        let mut edit_result = Ok(());
        model_features[index].evaluation.edit(|definition, _| {
            edit_result = (|| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern {
                    seeds: seed_slots,
                    ..
                }) = definition
                {
                    if seed_slots.is_empty() {
                        for seed in ctx.admit_iter(seeds, "collect SLDPRT mirror pattern seeds")? {
                            ctx.push_vec(
                                seed_slots,
                                PatternSeed::Feature(seed),
                                "collect SLDPRT mirror pattern seeds",
                            )?;
                        }
                    }
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        edit_result?;
    }
    for (index, candidates) in ctx.admit_iter(circular_axes_by_pattern, PATTERN_INPUTS)? {
        let [(axis_origin, axis_dir)] = candidates.as_slice() else {
            continue;
        };
        let Some(native_ref) = model_features[index].native_ref.as_deref() else {
            continue;
        };
        let Some(native) = ctx
            .get_hash_map(&history_by_id, native_ref, PATTERN_INPUTS)?
            .copied()
        else {
            continue;
        };
        let angle = match ctx.get_btree_map(&native.parameters, "Angle", PATTERN_INPUTS)? {
            Some(value) => {
                crate::history::literals::admit_literal(ctx, value, PATTERN_INPUTS)?;
                parse_positive_angle_rad(value)
            }
            None => None,
        };
        let Some(angle) = angle else {
            continue;
        };
        let count = match ctx.get_btree_map(&native.parameters, "Count", PATTERN_INPUTS)? {
            Some(value) => {
                crate::history::literals::admit_literal(ctx, value, PATTERN_INPUTS)?;
                parse_count(value)
            }
            None => None,
        };
        let Some(count) = count else {
            continue;
        };
        if !matches!(model_features[index].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if matches!(pattern.definition(), PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Circular) }))
        {
            continue;
        }
        let admitted = PatternKind::new(PatternTransform::Circular {
            axis_origin: *axis_origin,
            axis_dir: FeatureDirection3::from(*axis_dir),
            angle,
            count,
        })
        .map_err(|message| admission_error(ctx, message))?;
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                definition
            {
                *pattern = admitted;
            }
        });
    }
    Ok(())
}

/// Admits a pattern axis or plane point read from solved `SolidWorks` geometry.
fn admitted_point(
    point: Point3,
) -> Result<cadmpeg_ir::features::FinitePoint3, cadmpeg_core::CodecError> {
    cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed("SolidWorks pattern point must be finite".into())
    })
}

/// Admits a pattern direction or plane normal read from solved `SolidWorks` geometry.
fn admitted_direction(
    direction: Vector3,
) -> Result<cadmpeg_ir::features::FeatureDirection3, cadmpeg_core::CodecError> {
    cadmpeg_ir::features::FeatureDirection3::new(direction).ok_or_else(|| {
        cadmpeg_core::CodecError::Malformed(
            "SolidWorks pattern direction must have a finite nonzero norm".into(),
        )
    })
}

fn mirror_plane_from_surface(
    geometry: &SolvedSurfaceGeometry,
) -> Option<(cadmpeg_ir::features::FinitePoint3, UnitVector3)> {
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            Some((plane_surface.origin(), *plane_surface.frame().axis()))
        }
        SolvedSurfaceGeometry::Transformed(placed) if placed.transform().is_proper_rigid() => {
            let (origin, normal) = mirror_plane_from_surface(placed.basis())?;
            let transform = placed.transform();
            Some((
                transform.apply_point(origin.get())?,
                transform.apply_unit_normal(normal)?,
            ))
        }
        _ => None,
    }
}

/// Bind mirror planes selected by persistent feature-local face identity.
pub(crate) fn bind_mirror_surface_planes(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    face_identities: &[(cadmpeg_ir::ids::FaceId, crate::brep::PersistentFaceIdentity)],
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Result<(), cadmpeg_core::CodecError> {
    const INDEX: &str = "index SLDPRT mirror surface planes";
    let mut storage = ctx.reserve_scoped(0, INDEX)?;
    let mut mirror_native_refs = HashSet::new();
    for history in ctx.admit_iter(histories, INDEX)? {
        for feature in ctx.admit_iter(&history.features, INDEX)? {
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                == NativeClassKind::MirrorPattern
            {
                storage.with_storage(|| {
                    ctx.insert_hash_set(&mut mirror_native_refs, feature.id.as_str(), INDEX)
                })?;
            }
        }
    }
    let mut faces_by_identity = HashMap::<(FeatureSourceId, u32), Vec<&str>>::new();
    for (face, identity) in ctx.admit_iter(face_identities, INDEX)? {
        let key = (identity.feature_source_id, identity.local_id);
        if let Some(candidates) = ctx.get_mut_hash_map(&mut faces_by_identity, &key, INDEX)? {
            if !ctx.any_by(
                &*candidates,
                |candidate| ctx.equal(*candidate, face.as_str(), INDEX),
                "collect SLDPRT mirror face identities",
            )? {
                storage.with_storage(|| {
                    ctx.push_vec(
                        candidates,
                        face.as_str(),
                        "collect SLDPRT mirror face identities",
                    )
                })?;
            }
            continue;
        }
        storage.with_storage(|| {
            ctx.push_hash_group(
                &mut faces_by_identity,
                key,
                face.as_str(),
                INDEX,
                "collect SLDPRT mirror face identities",
            )
        })?;
    }
    let mut faces_by_id = HashMap::new();
    for face in ctx.admit_iter(faces, INDEX)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut faces_by_id, face.id.as_str(), face, INDEX)
        })?;
    }
    let mut surfaces_by_id = HashMap::new();
    for surface in ctx.admit_iter(surfaces, INDEX)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut surfaces_by_id, surface.id.as_str(), surface, INDEX)
        })?;
    }
    let mut selections_by_owner = HashMap::<&str, Vec<_>>::new();
    for lane in ctx.admit_iter(lanes, INDEX)? {
        if is_supplemental_config_lane(lane) {
            continue;
        }
        for selection in ctx.admit_iter(&lane.surface_selections, INDEX)? {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut selections_by_owner,
                    selection.feature_ref.as_str(),
                    selection,
                    INDEX,
                    INDEX,
                )
            })?;
        }
    }

    for feature in ctx.admit_iter(features, "bind SLDPRT mirror surface planes")? {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. })
                if pattern.is_unresolved()
        ) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        if !ctx.contains_hash_set(&mirror_native_refs, native_ref, INDEX)? {
            continue;
        }
        let Some(selections) = ctx.get_hash_map(&selections_by_owner, native_ref, INDEX)? else {
            continue;
        };
        let mut candidates = Vec::new();
        for selection in ctx.admit_iter(selections, "scan SLDPRT mirror plane selections")? {
            let Some(component) = selection.components.last() else {
                continue;
            };
            let Some(source) = View::u32_le_at(&component.type_signature, 4)
                .and_then(|source| FeatureSourceId::try_from(source).ok())
            else {
                continue;
            };
            let Some(local) = component.local_id else {
                continue;
            };
            let Some([face_id]) = ctx
                .get_hash_map(&faces_by_identity, &(source, local), INDEX)?
                .map(Vec::as_slice)
            else {
                continue;
            };
            let Some(face) = ctx.get_hash_map(&faces_by_id, *face_id, INDEX)? else {
                continue;
            };
            let Some(surface) = ctx.get_hash_map(&surfaces_by_id, face.surface.as_str(), INDEX)?
            else {
                continue;
            };
            let Some(plane) =
                mirror_plane_from_surface(surface.geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?)
            else {
                continue;
            };
            if !ctx.any_by(
                &candidates,
                |candidate| Ok(*candidate == plane),
                "collect SLDPRT mirror plane candidates",
            )? {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates,
                        plane,
                        "collect SLDPRT mirror plane candidates",
                    )
                })?;
            }
        }
        let [(origin, normal)] = candidates.as_slice() else {
            continue;
        };
        let Ok(admitted) = PatternKind::new(PatternTransform::Mirror {
            plane_origin: *origin,
            plane_normal: cadmpeg_ir::features::FeatureDirection3::from(*normal),
        }) else {
            continue;
        };
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                definition
            {
                *pattern = admitted;
            }
        });
    }

    Ok(())
}

pub(crate) fn bind_sweep_adjacent_profiles(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const INDEX: &str = "index SLDPRT sweep adjacent profiles";
    let mut storage = ctx.reserve_scoped(0, INDEX)?;
    let metadata_ids = storage.with_storage(|| history_metadata_ids(ctx, histories))?;
    let mut history_features = Vec::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT feature binding candidates")? {
        for feature in
            ctx.admit_iter(&history.features, "scan SLDPRT feature binding candidates")?
        {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut history_features,
                    feature,
                    "collect SLDPRT feature binding candidates",
                )
            })?;
        }
    }
    let mut assignments = BTreeMap::<
        usize,
        Vec<(
            cadmpeg_ir::features::FeatureId,
            SketchId,
            Option<(cadmpeg_ir::features::FeatureId, SketchId)>,
        )>,
    >::new();
    {
        let mut model_by_native = HashMap::new();
        for (index, feature) in ctx.admit_iter(&*model_features, INDEX)?.enumerate() {
            if let Some(native) = feature.native_ref.as_deref() {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut model_by_native, native, index, INDEX)
                })?;
            }
        }
        let model_of = |native: &str| {
            Ok::<_, cadmpeg_core::CodecError>(
                ctx.get_hash_map(&model_by_native, native, INDEX)?.copied(),
            )
        };
        let planar_sketch = |index: usize| match model_features[index].evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) => Some(sketch),
            _ => None,
        };
        for lane in ctx.admit_iter(lanes, "scan SLDPRT sweep adjacent lanes")? {
            let mut starts = lane_feature_objects(
                ctx,
                history_features.iter().copied(),
                &metadata_ids,
                lane,
                "scan SLDPRT feature binding candidates",
            )?;
            ctx.sort_unstable_by(
                &mut starts,
                |value| &value.0,
                Ord::cmp,
                "sort SLDPRT sweep adjacent features",
            )?;
            for (index, (_, feature)) in ctx
                .admit_iter(&starts, "scan SLDPRT sweep adjacent features")?
                .enumerate()
            {
                if feature.input_class.as_deref() != Some("moSweep_c") {
                    continue;
                }
                let Some(model_index) = model_of(feature.id.as_str())? else {
                    continue;
                };
                if !matches!(
                    model_features[model_index].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
                        shape,
                        ..
                    }) if shape.section_is_unresolved())
                {
                    continue;
                }
                let Some((_, profile_feature)) = starts.get(index + 1) else {
                    continue;
                };
                if profile_feature.input_class.as_deref() != Some("moProfileFeature_c") {
                    continue;
                }
                let Some(profile_index) = model_of(profile_feature.id.as_str())? else {
                    continue;
                };
                let Some(sketch) = planar_sketch(profile_index) else {
                    continue;
                };
                let path_feature = index
                    .checked_sub(1)
                    .map(|path_object_index| starts[path_object_index].1)
                    .filter(|path_feature| {
                        path_feature.input_class.as_deref() == Some("moProfileFeature_c")
                    });
                let path = match path_feature {
                    Some(path_feature) => match model_of(path_feature.id.as_str())? {
                        Some(path_index) => planar_sketch(path_index)
                            .map(|path| (&model_features[path_index].id, path)),
                        None => None,
                    },
                    None => None,
                };
                let path = path
                    .map(|(feature, sketch)| {
                        Ok::<_, cadmpeg_core::CodecError>((
                            copy_feature_binding_id(ctx, feature)?,
                            copy_feature_binding_sketch_id(ctx, sketch)?,
                        ))
                    })
                    .transpose()?;
                let candidate = (
                    copy_feature_binding_id(ctx, &model_features[profile_index].id)?,
                    copy_feature_binding_sketch_id(ctx, sketch)?,
                    path,
                );
                push_distinct_candidate(
                    ctx,
                    &mut storage,
                    &mut assignments,
                    model_index,
                    candidate,
                )?;
            }
        }
    }
    const DEPENDENCIES: &str = "collect SLDPRT sweep profile dependencies";
    for (index, candidates) in ctx.admit_iter(assignments, INDEX)? {
        let Ok([(profile_dependency, sketch, path)]) = <[_; 1]>::try_from(candidates) else {
            continue;
        };
        let mut profile_bound = false;
        let mut path_dependency = None;
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,
                path: path_slot,
                ..
            }) = definition
            {
                if shape.section_is_unresolved() {
                    shape.set_referenced_profile(sketch.into());
                    profile_bound = true;
                }
                if let Some((dependency, path)) = path {
                    if path_slot
                        .as_ref()
                        .is_none_or(|existing| matches!(existing, PathRef::Native(_)))
                    {
                        *path_slot = Some(PathRef::Sketch(path));
                    }
                    path_dependency = Some(dependency);
                }
            } else {
                path_dependency = path.map(|(dependency, _)| dependency);
            }
        });
        if profile_bound
            && !ctx.contains(
                model_features[index].dependencies.as_slice(),
                &profile_dependency,
                DEPENDENCIES,
            )?
        {
            model_features[index]
                .dependencies
                .insert(ctx, profile_dependency, DEPENDENCIES)?;
        }
        if let Some(dependency) = path_dependency {
            if !ctx.contains(
                model_features[index].dependencies.as_slice(),
                &dependency,
                DEPENDENCIES,
            )? {
                model_features[index]
                    .dependencies
                    .insert(ctx, dependency, DEPENDENCIES)?;
            }
        }
    }
    Ok(())
}

fn copy_feature_binding_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::features::FeatureId,
) -> Result<cadmpeg_ir::features::FeatureId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, "retain SLDPRT feature binding identity")
}

fn copy_feature_binding_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &SketchId,
) -> Result<SketchId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, "retain SLDPRT feature binding identity")
}

/// Each non-metadata feature that names an object in the lane, with the
/// object's offset, in feature order.
fn lane_feature_objects<'h>(
    ctx: &DecodeContext<'_>,
    features: impl IntoIterator<Item = &'h crate::records::Feature>,
    metadata_ids: &HashSet<&str>,
    lane: &FeatureInputLane,
    operation: &'static str,
) -> Result<Vec<(u64, &'h crate::records::Feature)>, cadmpeg_core::CodecError> {
    let object_names = ObjectNames::new(ctx, lane)?;
    let mut objects = Vec::new();
    let mut features = features.into_iter();
    while let Some(feature) = ctx.next_charged(&mut features, operation)? {
        if ctx.contains_hash_set(metadata_ids, feature.id.as_str(), operation)? {
            continue;
        }
        if let Some(name) = object_names.of(ctx, feature)? {
            ctx.push_vec(&mut objects, (name.offset, feature), operation)?;
        }
    }
    Ok(objects)
}

/// The owner of the item at `offset`: the last object that starts before it,
/// when the next object starts after it. `starts` is sorted by offset.
fn object_owner<T: Copy>(
    ctx: &DecodeContext<'_>,
    starts: &[(u64, T)],
    offset: u64,
    operation: &'static str,
) -> Result<Option<T>, cadmpeg_core::CodecError> {
    let following = ctx.partition_point(starts, |(start, _)| Ok(*start < offset), operation)?;
    let Some(owner) = following.checked_sub(1).and_then(|index| starts.get(index)) else {
        return Ok(None);
    };
    Ok(starts
        .get(following)
        .is_none_or(|(next, _)| offset < *next)
        .then_some(owner.1))
}

pub(crate) fn bind_scalar_operands(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const CANDIDATES: &str = "collect SLDPRT scalar binding candidates";
    let mut selection_history = super::selections::SelectionHistory::new(ctx, histories)?;
    let represented_sketches = represented_sketch_features(ctx, histories, lanes)?;
    let metadata_ids = history_metadata_ids(ctx, histories)?;
    let mut storage = ctx.reserve_scoped(0, SCALAR_BINDING_INDEX)?;
    let mut history_features = Vec::new();
    let mut features_by_id = HashMap::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT scalar binding candidates")? {
        for feature in ctx.admit_iter(&history.features, "scan SLDPRT scalar binding candidates")? {
            storage.with_storage(|| ctx.push_vec(&mut history_features, feature, CANDIDATES))?;
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut features_by_id,
                    feature.id.as_str(),
                    feature,
                    SCALAR_BINDING_INDEX,
                )
            })?;
        }
    }
    for lane in ctx.admit_iter(lanes, "bind SLDPRT scalar operands")? {
        for entity in ctx.admit_iter(&mut lane.sketch_entities, "bind SLDPRT scalar operands")? {
            entity.feature_ref = None;
            entity.links = None;
        }
        let mut starts = Vec::new();
        for (offset, feature) in ctx.admit_iter(
            lane_feature_objects(
                ctx,
                history_features.iter().copied(),
                &metadata_ids,
                lane,
                "scan SLDPRT scalar binding candidates",
            )?,
            CANDIDATES,
        )? {
            storage.with_storage(|| {
                ctx.push_vec(&mut starts, (offset, feature.id.as_str()), CANDIDATES)
            })?;
        }
        ctx.sort_unstable_by(
            &mut starts,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT scalar operand features",
        )?;
        const OWNER: &str = "bind SLDPRT scalar operand owners";
        for entity in ctx.admit_iter(&mut lane.sketch_entities, OWNER)? {
            if let Some(owner) = object_owner(ctx, &starts, entity.offset(), OWNER)? {
                entity.feature_ref = Some(copy_binding_text(ctx, owner)?);
            }
        }
        for reference in ctx.admit_iter(&mut lane.references, OWNER)? {
            if let Some(owner) = object_owner(ctx, &starts, reference.offset, OWNER)? {
                reference.feature_ref = Some(copy_binding_text(ctx, owner)?);
            }
        }
        for scalar in ctx.admit_iter(&mut lane.scalars, OWNER)? {
            if let Some(owner) = object_owner(ctx, &starts, scalar.offset, OWNER)? {
                scalar.feature_ref = Some(copy_binding_text(ctx, owner)?);
            }
        }
        bind_detached_legacy_sketch_objects(ctx, histories, &represented_sketches, lane)?;
        // A dissected profile that directly follows an extrusion lends its
        // scalars to the extrusion.
        let mut scalars_by_offset = storage.with_storage(|| {
            ctx.collect_vec(0..lane.scalars.len(), "sort SLDPRT scalar offsets")
        })?;
        ctx.stable_sort_by_key(
            &mut scalars_by_offset,
            |index| lane.scalars[*index].offset,
            Ord::cmp,
            "sort SLDPRT scalar offsets",
        )?;
        for pair in ctx
            .admit_iter(&starts, "scan SLDPRT dissected scalar owners")?
            .windows(const { crate::nonzero(2) })
        {
            let [(_, parent_id), (child_start, child_id)] = pair else {
                continue;
            };
            let (Some(parent), Some(child)) = (
                ctx.get_hash_map(&features_by_id, *parent_id, SCALAR_BINDING_INDEX)?
                    .copied(),
                ctx.get_hash_map(&features_by_id, *child_id, SCALAR_BINDING_INDEX)?
                    .copied(),
            ) else {
                continue;
            };
            if native_object_class(parent.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::Extrusion
                && !matches!(parent.xml_tag.as_str(), "Extrusion" | "Cut")
            {
                continue;
            }
            if !is_dissected_profile_feature(ctx, child)? {
                continue;
            }
            let next = ctx.partition_point(
                &starts,
                |(offset, _)| Ok(offset <= child_start),
                "scan SLDPRT dissected scalar owners",
            )?;
            let child_end = starts.get(next).map(|(offset, _)| *offset);
            let first = ctx.partition_point(
                &scalars_by_offset,
                |index| Ok(lane.scalars[*index].offset <= *child_start),
                "scan SLDPRT dissected scalar owners",
            )?;
            let last = match child_end {
                Some(end) => ctx.partition_point(
                    &scalars_by_offset,
                    |index| Ok(lane.scalars[*index].offset < end),
                    "scan SLDPRT dissected scalar owners",
                )?,
                None => scalars_by_offset.len(),
            };
            for &index in ctx.admit_iter(
                scalars_by_offset.get(first..last).unwrap_or_default(),
                "scan SLDPRT dissected scalar owners",
            )? {
                let scalar = &mut lane.scalars[index];
                if ctx.equal(
                    &scalar.feature_ref.as_deref(),
                    &Some(*child_id),
                    "scan SLDPRT dissected scalar owners",
                )? {
                    scalar.feature_ref = Some(copy_binding_text(ctx, parent_id)?);
                }
            }
        }
        finalize_lane_bindings(ctx, histories, &mut selection_history, lane)?;
    }
    Ok(())
}

pub(crate) fn finalize_lane_bindings(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    selection_history: &mut super::selections::SelectionHistory<'_, '_>,
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    const MARKERS: &str = "collect SLDPRT scalar marker candidates";
    normalize_indexed_curve_entities(ctx, lane)?;
    let mut storage = ctx.reserve_scoped(0, SCALAR_BINDING_INDEX)?;
    // The markers of each feature by local id; a link resolves to the unique
    // candidate. Marker ids are copied because the entities are rewritten.
    let mut marker_ids = HashMap::<String, HashMap<u32, Vec<(String, bool)>>>::new();
    for entity in ctx.admit_iter(&lane.sketch_entities, MARKERS)? {
        let (Some(feature), Some(local_id)) = (&entity.feature_ref, entity.local_id()) else {
            continue;
        };
        let id = storage.with_storage(|| copy_binding_text(ctx, entity.id()))?;
        let candidate = (id, entity.coordinates_m.is_some());
        if let Some(by_local) = ctx.get_mut_hash_map(
            &mut marker_ids,
            feature.as_str(),
            "lookup SLDPRT scalar marker group",
        )? {
            storage.with_storage(|| {
                ctx.push_hash_group(by_local, local_id, candidate, SCALAR_BINDING_INDEX, MARKERS)
            })?;
            continue;
        }
        let mut by_local = HashMap::new();
        storage.with_storage(|| {
            ctx.push_hash_group(
                &mut by_local,
                local_id,
                candidate,
                SCALAR_BINDING_INDEX,
                MARKERS,
            )
        })?;
        let key = storage.with_storage(|| copy_binding_text(ctx, feature))?;
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut marker_ids, key, by_local, SCALAR_BINDING_INDEX)
        })?;
    }
    for entity in ctx.admit_iter(
        &mut lane.sketch_entities,
        "decode SLDPRT scalar local links",
    )? {
        let Ok(offset) = usize::try_from(entity.offset()) else {
            continue;
        };
        let local_links = if let Some((local_ids, selector)) =
            marker_local_links(&lane.native_payload, offset)
        {
            let links = storage
                .with_storage(|| ctx.collect_vec(local_ids, "decode SLDPRT scalar local links"))?;
            Some((links, selector))
        } else {
            coordinate_marker_local_links(ctx, &lane.native_payload, offset)?
        };
        let Some((local_ids, selector)) = local_links else {
            continue;
        };
        let Some(owner) = &entity.feature_ref else {
            continue;
        };
        let mut links = Vec::new();
        let by_local = ctx.get_hash_map(
            &marker_ids,
            owner.as_str(),
            "lookup SLDPRT scalar marker group",
        )?;
        for local_id in ctx.admit_iter(local_ids, "collect SLDPRT scalar local links")? {
            let Some(by_local) = by_local else {
                continue;
            };
            let Some(entity_ref) = ctx
                .get_hash_map(
                    by_local,
                    &u32::from(local_id),
                    "lookup SLDPRT scalar marker group",
                )?
                .map(|candidates| unique_marker_candidate(ctx, candidates))
                .transpose()?
                .flatten()
            else {
                continue;
            };
            ctx.push_vec(
                &mut links,
                SketchInputLink {
                    local_id,
                    entity_ref: copy_binding_text(ctx, entity_ref)?,
                },
                "collect SLDPRT scalar local links",
            )?;
        }
        if let Some(links) = crate::records::SketchInputLinks::new(selector, links) {
            entity.links = Some(links);
        }
    }
    bind_resolved_curve_vertices(ctx, lane)?;
    let mut entities_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
    for entity in ctx.admit_iter(
        &lane.sketch_entities,
        "collect SLDPRT scalar owner entities",
    )? {
        if let Some(feature) = entity.feature_ref.as_deref() {
            storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut entities_by_feature,
                    feature,
                    entity,
                    SCALAR_BINDING_INDEX,
                    "collect SLDPRT scalar owner entities",
                )
            })?;
        }
    }
    for scalar in ctx.admit_iter(&mut lane.scalars, "resolve SLDPRT scalar operands")? {
        let Some(feature) = scalar.feature_ref.as_deref() else {
            continue;
        };
        let Some(entities) =
            ctx.get_hash_map(&entities_by_feature, feature, SCALAR_BINDING_INDEX)?
        else {
            continue;
        };
        let resolved = resolve_scalar_operand_markers(ctx, entities, &scalar.operands)?;
        for (operand, resolved) in ctx
            .admit_iter(&mut scalar.operands, "resolve SLDPRT scalar operands")?
            .zip(resolved)
        {
            operand.entity_ref = resolved
                .map(|entity| copy_binding_text(ctx, entity.id()))
                .transpose()?;
        }
    }
    drop(entities_by_feature);
    let mut scalar_owners = HashMap::new();
    for scalar in ctx.admit_iter(&lane.scalars, SCALAR_BINDING_INDEX)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut scalar_owners,
                scalar.id.as_str(),
                scalar.feature_ref.as_deref(),
                SCALAR_BINDING_INDEX,
            )
        })?;
    }
    for binding in ctx.admit_iter(&mut lane.relation_bindings, SCALAR_BINDING_INDEX)? {
        binding.feature_ref = ctx
            .get_hash_map(
                &scalar_owners,
                binding.scalar_ref.as_str(),
                SCALAR_BINDING_INDEX,
            )?
            .copied()
            .flatten()
            .map(|owner| copy_binding_text(ctx, owner))
            .transpose()?;
    }
    drop(scalar_owners);
    let (intervals, _intervals_storage) = ctx
        .with_scoped_storage("SLDPRT binding feature intervals", || {
            feature_intervals(ctx, histories, lane)
        })?;
    lane.relation_bindings =
        relation_bindings_scoped(ctx, &lane.id, &lane.classes, &lane.scalars, &intervals)?;
    lane.relation_instances = relation_instances(ctx, histories, lane)?;
    lane.body_selections = compact_body_selections(ctx, histories, lane)?;
    let history_features = selection_history.for_lane(ctx, lane)?;
    lane.edge_selections = compact_edge_selections(ctx, histories, history_features, lane)?;
    let identities = generated_surface_identities(ctx, lane)?;
    lane.surface_selections =
        compact_surface_selections(ctx, histories, history_features, lane, &identities)?;
    lane.generated_surface_identities = identities;
    Ok(())
}

fn represented_sketch_features(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<HashSet<String>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "scan SLDPRT represented sketch objects";
    let metadata_ids = history_metadata_ids(ctx, histories)?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut features = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            storage.with_storage(|| ctx.push_vec(&mut features, feature, OPERATION))?;
        }
    }
    let mut represented = HashSet::new();
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        if is_supplemental_config_lane(lane) {
            continue;
        }
        let mut objects = lane_feature_objects(
            ctx,
            features.iter().copied(),
            &metadata_ids,
            lane,
            OPERATION,
        )?;
        ctx.sort_unstable_by(
            &mut objects,
            |value| &value.0,
            Ord::cmp,
            "sort SLDPRT represented sketch objects",
        )?;
        // The offsets of the lane's markers that carry coordinates.
        let mut placed = Vec::new();
        for entity in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
            if entity.coordinates_m.is_some() {
                storage.with_storage(|| ctx.push_vec(&mut placed, entity.offset(), OPERATION))?;
            }
        }
        ctx.sort_unstable_by(&mut placed, |offset| offset, Ord::cmp, OPERATION)?;
        for (index, &(start, feature)) in ctx.admit_iter(&objects, OPERATION)?.enumerate() {
            if feature.xml_tag != "Sketch" {
                continue;
            }
            let end = objects.get(index + 1).map(|next| next.0);
            let following =
                ctx.partition_point(&placed, |offset| Ok(*offset <= start), OPERATION)?;
            if placed
                .get(following)
                .is_some_and(|offset| end.is_none_or(|end| *offset < end))
            {
                ctx.insert_string_set(&mut represented, &feature.id, SCALAR_BINDING_INDEX)?;
            }
        }
    }
    Ok(represented)
}

pub(crate) fn bind_unresolved_detached_sketch_objects(
    ctx: &DecodeContext<'_>,
    model_features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut selection_history = super::selections::SelectionHistory::new(ctx, histories)?;
    let mut storage = ctx.reserve_scoped(0, SCALAR_BINDING_INDEX)?;
    let mut unresolved = HashSet::new();
    for feature in ctx.admit_iter(model_features, SCALAR_BINDING_INDEX)? {
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                    | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                ..
            })
        ) {
            if let Some(native) = feature.native_ref.as_deref() {
                storage.with_storage(|| {
                    ctx.insert_hash_set(&mut unresolved, native, SCALAR_BINDING_INDEX)
                })?;
            }
        }
    }
    let mut represented = HashSet::new();
    for history in ctx.admit_iter(histories, SCALAR_BINDING_INDEX)? {
        for feature in ctx.admit_iter(&history.features, SCALAR_BINDING_INDEX)? {
            if feature.xml_tag == "Sketch"
                && !ctx.contains_hash_set(&unresolved, feature.id.as_str(), SCALAR_BINDING_INDEX)?
            {
                storage.with_storage(|| {
                    ctx.insert_string_set(&mut represented, &feature.id, SCALAR_BINDING_INDEX)
                })?;
            }
        }
    }
    for lane in ctx.admit_iter(lanes, SCALAR_BINDING_INDEX)? {
        if !is_supplemental_config_lane(lane) {
            continue;
        }
        bind_detached_legacy_sketch_objects(ctx, histories, &represented, lane)?;
        finalize_lane_bindings(ctx, histories, &mut selection_history, lane)?;
    }
    Ok(())
}

/// Assigns every marker, reference and scalar of the lane in `start..end`
/// to `owner`.
fn assign_object_range(
    ctx: &DecodeContext<'_>,
    lane: &mut FeatureInputLane,
    range: impl Fn(u64) -> bool,
    owner: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "bind SLDPRT detached sketch objects";
    for entity in ctx.admit_iter(&mut lane.sketch_entities, OPERATION)? {
        if range(entity.offset()) {
            entity.feature_ref = Some(copy_binding_text(ctx, owner)?);
        }
    }
    for reference in ctx.admit_iter(&mut lane.references, OPERATION)? {
        if range(reference.offset) {
            reference.feature_ref = Some(copy_binding_text(ctx, owner)?);
        }
    }
    for scalar in ctx.admit_iter(&mut lane.scalars, OPERATION)? {
        if range(scalar.offset) {
            scalar.feature_ref = Some(copy_binding_text(ctx, owner)?);
        }
    }
    Ok(())
}

pub(super) fn bind_detached_legacy_sketch_objects(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    represented: &HashSet<String>,
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    const OBJECT_GAP: u64 = 4096;
    const OPERATION: &str = "collect SLDPRT detached sketch starts";

    if !is_supplemental_config_lane(lane) {
        return Ok(());
    }
    let limit = ctx
        .find_by(
            &lane.classes,
            |class| Ok(class.name == "moFeatureDimHandle_c"),
            "find SLDPRT feature dimension handle",
        )?
        .map_or_else(
            || u64_from_index(lane.native_payload.len()),
            |class| class.offset,
        );
    let relation_bindings =
        bind_detached_spatial_relation_objects(ctx, histories, represented, lane)?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut markers = Vec::new();
    for entity in ctx.admit_iter(&lane.sketch_entities, OPERATION)? {
        let offset = entity.offset();
        if offset >= limit {
            continue;
        }
        let inside = ctx.any_by(
            &relation_bindings,
            |(start, end, _)| Ok(*start <= offset && offset < *end),
            OPERATION,
        )?;
        if !inside {
            storage.with_storage(|| ctx.push_vec(&mut markers, offset, OPERATION))?;
        }
    }
    let Some(&first) = markers.first() else {
        return Ok(());
    };
    let mut starts = storage.with_storage(|| ctx.alloc_filled(1, first, OPERATION))?;
    for pair in ctx
        .admit_iter(&markers, OPERATION)?
        .windows(const { crate::nonzero(2) })
    {
        if pair[1] >= pair[0] && pair[1] - pair[0] >= OBJECT_GAP {
            storage.with_storage(|| ctx.push_vec(&mut starts, pair[1], OPERATION))?;
        }
    }

    let mut owners = Vec::new();
    for history in ctx.admit_iter(histories, "collect SLDPRT detached sketch owners")? {
        for feature in ctx.admit_iter(&history.features, "collect SLDPRT detached sketch owners")? {
            if feature.xml_tag != "Sketch"
                || native_object_class(feature.input_class.as_deref().unwrap_or_default())
                    == NativeClassKind::OriginProfileFeature
                || ctx.contains_hash_set(represented, feature.id.as_str(), SCALAR_BINDING_INDEX)?
                || ctx.any_by(
                    &relation_bindings,
                    |(_, _, owner)| ctx.equal(owner.as_str(), feature.id.as_str(), OPERATION),
                    "collect SLDPRT detached sketch owners",
                )?
            {
                continue;
            }
            if let Some(source) = feature.source_value() {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut owners,
                        (source, feature),
                        "collect SLDPRT detached sketch owners",
                    )
                })?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut owners,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT detached sketch owners",
    )?;
    if starts.len() != owners.len() {
        return Ok(());
    }

    for (index, (&start, (_, owner))) in
        ctx.admit_iter(&starts, OPERATION)?.zip(&owners).enumerate()
    {
        let end = starts.get(index + 1).copied().unwrap_or(limit);
        assign_object_range(
            ctx,
            lane,
            |offset| offset >= start && offset < end,
            &owner.id,
        )?;
    }
    Ok(())
}

pub(super) fn spatial_relation_manager_ranges_charged(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<Vec<(u64, u64)>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "scan SLDPRT spatial relation classes";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let managers = sorted_classes(ctx, &mut storage, lane, "moRelMgr_c", OPERATION)?;
    let lists = sorted_classes(ctx, &mut storage, lane, "suObList", OPERATION)?;
    let mut ranges = Vec::new();
    for plane in ctx.admit_iter(&lane.classes, OPERATION)? {
        if plane.name != "sg3DPlaneHandle" {
            continue;
        }
        // The last manager before the plane and the first list after it.
        let before = ctx.partition_point(
            &managers,
            |class| Ok(class.offset < plane.offset),
            OPERATION,
        )?;
        let Some(start) = before.checked_sub(1).and_then(|index| managers.get(index)) else {
            continue;
        };
        let after =
            ctx.partition_point(&lists, |class| Ok(class.offset <= plane.offset), OPERATION)?;
        let Some(end) = lists.get(after) else {
            continue;
        };
        ctx.push_vec(
            &mut ranges,
            (start.offset, end.offset),
            "collect SLDPRT spatial relation ranges",
        )?;
    }
    ctx.sort_unstable_by(
        &mut ranges,
        |value| value,
        Ord::cmp,
        "sort SLDPRT spatial relation ranges",
    )?;
    ctx.dedup_vec(&mut ranges, "deduplicate SLDPRT spatial relation ranges")?;
    Ok(ranges)
}

/// Whether a name is `D` followed by decimal digits.
fn is_dimension_name(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(suffix) = name.strip_prefix('D') else {
        return Ok(false);
    };
    Ok(!suffix.is_empty()
        && ctx.all_by(
            suffix.bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "check SLDPRT dimension name",
        )?)
}

fn bind_detached_spatial_relation_objects(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    represented: &HashSet<String>,
    lane: &mut FeatureInputLane,
) -> Result<Vec<(u64, u64, String)>, cadmpeg_core::CodecError> {
    const OWNERS: &str = "scan SLDPRT spatial sketch owners";
    const MATCH: &str = "match SLDPRT spatial sketch names";
    let ranges = spatial_relation_manager_ranges_charged(ctx, lane)?;
    if ranges.is_empty() {
        return Ok(Vec::new());
    }
    let mut storage = ctx.reserve_scoped(0, SCALAR_BINDING_INDEX)?;
    let mut names = HashMap::new();
    for name in ctx.admit_iter(&lane.names, SCALAR_BINDING_INDEX)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut names,
                name.id.as_str(),
                name.value.as_str(),
                SCALAR_BINDING_INDEX,
            )
        })?;
    }
    // Each unrepresented spatial sketch owner with its dimensions by name.
    let mut owners = Vec::new();
    for history in ctx.admit_iter(histories, OWNERS)? {
        'owner: for feature in ctx.admit_iter(&history.features, OWNERS)? {
            if feature.input_class.as_deref() != Some("mo3DProfileFeature_c")
                || ctx.contains_hash_set(represented, feature.id.as_str(), SCALAR_BINDING_INDEX)?
            {
                continue;
            }
            let mut dimensions = BTreeMap::new();
            for (name, value) in ctx.admit_iter(&feature.parameters, OWNERS)? {
                if !is_dimension_name(ctx, name.as_str())? {
                    continue;
                }
                crate::history::literals::admit_literal(ctx, value, OWNERS)?;
                let Some(value) = crate::history::literals::parse_dimension_length_mm(value) else {
                    continue 'owner;
                };
                storage.with_storage(|| {
                    ctx.insert_btree_map(&mut dimensions, name.as_str(), value, OWNERS)
                })?;
            }
            if dimensions.len() >= 3 {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut owners,
                        (feature, dimensions),
                        "collect SLDPRT spatial sketch owners",
                    )
                })?;
            }
        }
    }
    let mut candidates = Vec::new();
    for &(start, end) in ctx.admit_iter(&ranges, MATCH)? {
        // The range's dimension scalars by name, with every value per name.
        let mut scalars = BTreeMap::<&str, Vec<f64>>::new();
        for scalar in ctx.admit_iter(&lane.scalars, MATCH)? {
            if scalar.offset <= start
                || scalar.offset >= end
                || scalar.role == crate::records::FeatureInputScalarRole::Display
            {
                continue;
            }
            let Some(name) = ctx
                .get_hash_map(&names, scalar.name.as_str(), SCALAR_BINDING_INDEX)?
                .copied()
            else {
                continue;
            };
            if is_dimension_name(ctx, name)? {
                storage.with_storage(|| {
                    ctx.push_btree_group(&mut scalars, name, scalar.value.get(), MATCH, MATCH)
                })?;
            }
        }
        for (owner, dimensions) in ctx.admit_iter(&owners, MATCH)? {
            // The owner and the range name the same dimensions, and each owner
            // dimension has a scalar of its value.
            if dimensions.len() != scalars.len() {
                continue;
            }
            let exact = ctx.all_by(
                dimensions,
                |(name, expected_mm)| {
                    let Some(values) = ctx.get_btree_map(&scalars, *name, MATCH)? else {
                        return Ok(false);
                    };
                    ctx.any_by(
                        values,
                        |value_m| {
                            Ok((value_m * 1000.0 - expected_mm.get()).abs()
                                <= expected_mm.get().abs().max(1.0)
                                    * EPS_BINDINGS_BIND_DETACHED_SPATIAL_RELATION_OBJECTS_E9)
                        },
                        MATCH,
                    )
                },
                MATCH,
            )?;
            if exact {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates,
                        (start, end, owner.id.as_str()),
                        "collect SLDPRT spatial sketch candidates",
                    )
                })?;
            }
        }
    }
    // A range binds when exactly one owner matches it and that owner matches
    // no other range.
    const DISAMBIGUATE: &str = "disambiguate SLDPRT spatial sketch ranges";
    let mut range_counts = BTreeMap::<(u64, u64), usize>::new();
    let mut owner_counts = HashMap::<&str, usize>::new();
    for &(start, end, owner) in ctx.admit_iter(&candidates, DISAMBIGUATE)? {
        let count = storage.with_storage(|| {
            ctx.entry_btree_map(&mut range_counts, (start, end), DISAMBIGUATE)
                .map(|entry| entry.or_insert(0_usize))
        })?;
        *count += 1;
        let count = storage.with_storage(|| {
            ctx.entry_hash_map(&mut owner_counts, owner, DISAMBIGUATE)
                .map(|entry| entry.or_insert(0_usize))
        })?;
        *count += 1;
    }
    let mut bound = Vec::new();
    for &(start, end, owner) in ctx.admit_iter(&candidates, DISAMBIGUATE)? {
        if ctx.get_btree_map(&range_counts, &(start, end), DISAMBIGUATE)? == Some(&1)
            && ctx.get_hash_map(&owner_counts, owner, DISAMBIGUATE)? == Some(&1)
        {
            ctx.push_vec(
                &mut bound,
                (start, end, copy_binding_text(ctx, owner)?),
                "collect SLDPRT spatial sketch bindings",
            )?;
        }
    }
    drop((range_counts, owner_counts, candidates, owners));
    for (start, end, owner) in ctx.admit_iter(&bound, "bind SLDPRT spatial sketch objects")? {
        assign_object_range(ctx, lane, |offset| offset > *start && offset < *end, owner)?;
    }
    Ok(bound)
}

pub(super) fn normalize_indexed_curve_entities(
    ctx: &DecodeContext<'_>,
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    const TERMINAL: &str = "scan SLDPRT terminal profile lines";
    const ENDPOINTS: &str = "scan SLDPRT indexed curve endpoints";
    const REVERSE: &str = "scan SLDPRT reverse incidence endpoints";
    let mut storage = ctx.reserve_scoped(0, SCALAR_BINDING_INDEX)?;
    let terminal_lines = {
        let markers =
            storage.with_storage(|| ctx.collect_vec(lane.sketch_entities.iter(), TERMINAL))?;
        let mut terminal = Vec::new();
        for curve in ctx.admit_iter(&markers, TERMINAL)? {
            let is_terminal =
                legacy_terminal_indexed_profile_line(ctx, &lane.native_payload, curve, &markers)?;
            storage.with_storage(|| ctx.push_vec(&mut terminal, is_terminal, TERMINAL))?;
        }
        terminal
    };
    for (marker, terminal) in ctx
        .admit_iter(&mut lane.sketch_entities, TERMINAL)?
        .zip(terminal_lines)
    {
        if terminal {
            marker.reclassify(SketchInputKind::LineOrCircle);
        }
    }
    let updates = {
        let mut endpoints = HashMap::<&str, HashSet<u32>>::new();
        for curve in ctx.admit_iter(&lane.sketch_entities, ENDPOINTS)? {
            let Some(feature) = curve.feature_ref.as_deref() else {
                continue;
            };
            let Some(offset) = index_from_u64(curve.offset()) else {
                continue;
            };
            let Some(indices) = wide_indexed_curve_endpoint_indices(&lane.native_payload, offset)
                .or_else(|| compact_indexed_curve_endpoint_indices(&lane.native_payload, offset))
                .or_else(|| {
                    extended_compact_indexed_curve_endpoint_indices(&lane.native_payload, offset)
                })
                .or_else(|| compact_legacy_curve_endpoint_indices(&lane.native_payload, offset))
                .or_else(|| {
                    alternate_current_indexed_curve_endpoint_indices(&lane.native_payload, offset)
                })
            else {
                continue;
            };
            if !ctx.contains_key_hash_map(&endpoints, feature, ENDPOINTS)? {
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut endpoints, feature, HashSet::new(), ENDPOINTS)
                })?;
            }
            let Some(by_index) = ctx.get_mut_hash_map(&mut endpoints, feature, ENDPOINTS)? else {
                continue;
            };
            for index in indices {
                storage
                    .with_storage(|| ctx.insert_hash_set(by_index, index, SCALAR_BINDING_INDEX))?;
            }
        }
        let markers =
            storage.with_storage(|| ctx.collect_vec(lane.sketch_entities.iter(), REVERSE))?;
        let mut coordinates = HashMap::new();
        for curve in ctx.admit_iter(&markers, REVERSE)? {
            let Some(offsets) = current_reverse_incidence_endpoint_offsets(
                ctx,
                &lane.native_payload,
                curve,
                &markers,
            )?
            else {
                continue;
            };
            for offset in offsets {
                let Ok(native_offset) = usize::try_from(offset) else {
                    continue;
                };
                let Some((point, _)) = linked_profile_point(&lane.native_payload, native_offset)
                else {
                    continue;
                };
                storage.with_storage(|| {
                    ctx.insert_hash_map(&mut coordinates, offset, point, SCALAR_BINDING_INDEX)
                })?;
            }
        }
        // Each marker's linked coordinates and whether it becomes a point.
        let mut updates = Vec::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, ENDPOINTS)? {
            let update = match marker.feature_ref.as_deref().zip(marker.object_index()) {
                Some((feature, index)) => {
                    let linked = ctx
                        .get_hash_map(&coordinates, &marker.offset(), SCALAR_BINDING_INDEX)?
                        .copied();
                    let indexed = match ctx.get_hash_map(&endpoints, feature, ENDPOINTS)? {
                        Some(indices) => ctx.contains_hash_set(indices, &index, ENDPOINTS)?,
                        None => false,
                    };
                    Some((linked, indexed || linked.is_some()))
                }
                None => None,
            };
            storage.with_storage(|| ctx.push_vec(&mut updates, update, ENDPOINTS))?;
        }
        updates
    };
    for (marker, update) in ctx
        .admit_iter(&mut lane.sketch_entities, ENDPOINTS)?
        .zip(updates)
    {
        let Some((linked, endpoint)) = update else {
            continue;
        };
        if marker.coordinates_m.is_none() {
            marker.coordinates_m = linked;
        }
        if endpoint && marker.coordinates_m.is_some() {
            marker.reclassify(SketchInputKind::Point);
        }
    }
    Ok(())
}

/// A lane's markers by id and in lane order.
fn lane_marker_index<'e>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    entities: &'e [SketchInputEntity],
) -> Result<
    (
        HashMap<&'e str, &'e SketchInputEntity>,
        Vec<&'e SketchInputEntity>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut markers_by_id = HashMap::new();
    for marker in ctx.admit_iter(entities, SCALAR_BINDING_INDEX)? {
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut markers_by_id,
                marker.id(),
                marker,
                SCALAR_BINDING_INDEX,
            )
        })?;
    }
    let markers =
        storage.with_storage(|| ctx.collect_vec(entities.iter(), SCALAR_BINDING_INDEX))?;
    Ok((markers_by_id, markers))
}

fn bind_resolved_curve_vertices(
    ctx: &DecodeContext<'_>,
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    const SELECTED: &str = "scan SLDPRT selected curve endpoints";
    const RESOLVE: &str = "resolve SLDPRT curve endpoints";
    let prefixes = crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
        ctx,
        &lane.native_payload,
    )?;
    let mut storage = ctx.reserve_scoped(0, SCALAR_BINDING_INDEX)?;
    let selected_axis_endpoints = {
        let (markers_by_id, markers) = lane_marker_index(ctx, &mut storage, &lane.sketch_entities)?;
        let geometry =
            crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                ctx,
                &markers,
                std::rc::Rc::clone(&prefixes),
            )?;
        let mut selected = HashSet::new();
        for curve in ctx.admit_iter(&markers, SELECTED)? {
            if !index_from_u64(curve.offset()).is_some_and(|offset| {
                marker_is_selected_construction_line(&lane.native_payload, offset)
            }) {
                continue;
            }
            for marker in ctx.admit_iter(
                marker_curve_endpoint_markers(
                    ctx,
                    &lane.native_payload,
                    curve,
                    &markers_by_id,
                    &markers,
                    &geometry,
                )?,
                SELECTED,
            )? {
                if marker.coordinates_m.is_some() {
                    storage.with_storage(|| {
                        ctx.insert_string_set(&mut selected, marker.id(), SCALAR_BINDING_INDEX)
                    })?;
                }
            }
        }
        selected
    };
    for marker in ctx.admit_iter(&mut lane.sketch_entities, SELECTED)? {
        if ctx.contains_hash_set(&selected_axis_endpoints, marker.id(), SELECTED)? {
            marker.reclassify(SketchInputKind::Point);
        }
    }
    loop {
        let mut round = ctx.reserve_scoped(0, RESOLVE)?;
        let (resolved_curves, resolved_endpoints) = {
            let (markers_by_id, markers) =
                lane_marker_index(ctx, &mut round, &lane.sketch_entities)?;
            let geometry =
                crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
                    ctx,
                    &markers,
                    std::rc::Rc::clone(&prefixes),
                )?;
            let mut resolved_curves = HashSet::new();
            let mut resolved_endpoints = HashSet::new();
            for curve in ctx.admit_iter(&markers, RESOLVE)? {
                if !matches!(
                    curve.kind(),
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                ) {
                    continue;
                }
                let endpoints = marker_curve_endpoint_markers(
                    ctx,
                    &lane.native_payload,
                    curve,
                    &markers_by_id,
                    &markers,
                    &geometry,
                )?;
                if endpoints.len() == 2 {
                    round.with_storage(|| {
                        ctx.insert_string_set(
                            &mut resolved_curves,
                            curve.id(),
                            SCALAR_BINDING_INDEX,
                        )
                    })?;
                }
                for marker in ctx.admit_iter(endpoints, RESOLVE)? {
                    if marker.coordinates_m.is_some() {
                        round.with_storage(|| {
                            ctx.insert_string_set(
                                &mut resolved_endpoints,
                                marker.id(),
                                SCALAR_BINDING_INDEX,
                            )
                        })?;
                    }
                }
            }
            (resolved_curves, resolved_endpoints)
        };
        let mut changed = false;
        for marker in ctx.admit_iter(&mut lane.sketch_entities, RESOLVE)? {
            if marker.kind() != SketchInputKind::Point
                && ctx.contains_hash_set(&resolved_endpoints, marker.id(), RESOLVE)?
                && !ctx.contains_hash_set(&resolved_curves, marker.id(), RESOLVE)?
            {
                marker.reclassify(SketchInputKind::Point);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

fn copy_binding_text(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.copy_retained_text(text, "retain SLDPRT scalar binding identity")
}

const SCALAR_BINDING_INDEX: &str = "index SLDPRT scalar bindings";

#[cfg(test)]
mod bindings_tests;
