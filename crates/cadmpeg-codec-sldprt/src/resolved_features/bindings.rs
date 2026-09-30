//! Pattern, sweep and scalar operand lane binding.

use super::assembly::is_supplemental_config_lane;
use super::axes::{
    compact_line_reference_directions, declared_line_reference_directions,
    linear_pattern_display_directions, temporary_axis_reference, typed_linear_pattern_dimensions,
};
use super::component_paths::is_dissected_profile_feature;
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
use super::scalars::feature_object_name;
use super::selections::{
    compact_body_selections, compact_edge_selections, compact_surface_selections,
    coordinate_marker_local_links, generated_surface_identities, marker_local_links,
    mirror_pattern_component_path_at, unique_marker_candidate, COMPACT_EDGE_VECTOR_MARKER,
};
use super::typed_relations::{legacy_terminal_indexed_profile_line, marker_curve_endpoint_markers};
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
use std::collections::{HashMap, HashSet};

const EPS_BINDINGS_BIND_PATTERN_INPUTS_E12: f64 = 1e-12;
const EPS_BINDINGS_BIND_DETACHED_SPATIAL_RELATION_OBJECTS_E9: f64 = 1e-9;

pub(super) fn history_metadata_ids<'a>(
    ctx: &DecodeContext<'_>,
    histories: &'a [crate::records::FeatureHistory],
) -> Result<HashSet<&'a str>, cadmpeg_core::CodecError> {
    let mut ids = HashSet::new();
    for history in histories {
        for feature in &history.features {
            ctx.charge_work(1, "scan SLDPRT history metadata identities")?;
            if is_history_metadata_record(feature, &history.features) && !ids.contains(feature.id.as_str()) {
                reserve_binding_set(ctx, &mut ids)?;
                ids.insert(feature.id.as_str());
            }
        }
    }
    Ok(ids)
}

/// Bind pattern operands carried by adjacent feature-input objects.
pub(crate) fn bind_pattern_inputs(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let metadata_ids = history_metadata_ids(ctx, histories)?;
    let history_features = collect_feature_binding_vec(ctx, "collect SLDPRT pattern input candidates", "scan SLDPRT pattern input candidates", histories
        .iter()
        .flat_map(|history| &history.features)
        )?;
    let mut model_by_native = HashMap::new();
    for (index, feature) in model_features.iter().enumerate() {
        if let Some(native) = feature.native_ref.as_deref() {
            let native = ctx.format_retained(format_args!("{native}"), "retain SLDPRT pattern native identity")?;
            reserve_feature_binding_map(ctx, &mut model_by_native, "index SLDPRT pattern inputs")?;
            model_by_native.insert(native, index);
        }
    }
    let mut curve_seed_assignments = Vec::<(usize, cadmpeg_ir::features::FeatureId)>::new();
    let mut curve_path_assignments =
        Vec::<(usize, cadmpeg_ir::features::FeatureId, PathRef)>::new();
    let mut pattern_seed_assignments = Vec::<(usize, cadmpeg_ir::features::FeatureId)>::new();
    let mut circular_axis_assignments = Vec::<(usize, FinitePoint3, UnitVector3)>::new();
    let mut linear_direction_assignments = Vec::<(usize, FeatureDirection3)>::new();
    let mut mirror_plane_assignments = Vec::<(usize, Point3, Vector3)>::new();
    let mut mirror_seed_assignments = Vec::<(usize, Vec<cadmpeg_ir::features::FeatureId>)>::new();
    let derived_cosmetic_thread_seed = |feature: &crate::records::Feature| {
        history_features
            .iter()
            .filter(|candidate| {
                candidate.parent == feature.parent
                    && candidate.ordinal < feature.ordinal
                    && candidate.input_class.as_deref() == Some("moCosmeticThread_c")
            })
            .max_by_key(|candidate| candidate.ordinal)
            .map(|native| native.id.as_str())
    };

    for lane in lanes {
        let discovered_identities;
        let generated_identities = if lane.generated_surface_identities.is_empty() {
            discovered_identities = generated_surface_identities(ctx, lane)?;
            &discovered_identities
        } else {
            &lane.generated_surface_identities
        };
        // `lanes::admit` compares every stored name offset with the offset
        // `object_names` read out of `native_payload`, so an admitted name
        // offset is an index of that payload. It is narrowed once here, where
        // that proof holds, and the objects below carry `usize` offsets.
        let mut starts = collect_feature_binding_vec(ctx, "collect SLDPRT pattern input candidates", "scan SLDPRT pattern input candidates", history_features
            .iter()
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
            .filter_map(|feature| {
                let offset = usize::try_from(feature_object_name(feature, lane)?.offset).ok()?;
                Some((offset, *feature))
            })
            )?;
        starts.sort_unstable_by_key(|(offset, _)| *offset);
        for (start_index, (_, feature)) in starts.iter().enumerate() {
            let has_derived_cosmetic_thread_output =
                starts.get(start_index + 1).is_some_and(|(_, candidate)| {
                    candidate.input_class.as_deref() == Some("moDerivedCosmeticThread_c")
                });
            // The end of this pattern's object is the next object name's
            // offset, or the end of the payload when this is the last one.
            // Both are inside `native_payload`: an object name states the
            // payload bytes at its own offset, and `SldprtNative::load`
            // refuses a name whose offset the payload does not state.
            let pattern_object_end = || {
                let next = start_index + 1 + usize::from(has_derived_cosmetic_thread_output);
                match starts.get(next) {
                    Some((offset, _)) => *offset,
                    None => lane.native_payload.len(),
                }
            };
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                == NativeClassKind::MirrorPattern
            {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
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
                if !needs_plane && !needs_seeds {
                    continue;
                }
                let start = Some(starts[start_index].0);
                let end = pattern_object_end();
                let Some(start) = start.filter(|start| *start < end) else {
                    continue;
                };
                let object = &lane.native_payload[start..end];
                if needs_plane {
                    if let Ok(Some((origin, normal, _))) = explicit_reference_plane_frame(object) {
                        push_feature_binding_candidate(ctx, &mut mirror_plane_assignments, (model_index, origin, normal))?;
                    }
                }
                if !needs_seeds {
                    continue;
                }
                let seed_candidates = (0..object
                    .len()
                    .saturating_sub(COMPACT_EDGE_VECTOR_MARKER.len()))
                    .filter(|offset| {
                        object.get(*offset..*offset + COMPACT_EDGE_VECTOR_MARKER.len())
                            == Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                    })
                    .filter_map(|offset| mirror_pattern_component_path_at(object, offset))
                    .filter_map(|components| {
                        for component in components.iter().rev() {
                            let source = View::u32_le_at(&component.type_signature, 4)?;
                            let mut matches = history_features
                                .iter()
                                .filter(|candidate| candidate.source_value() == Some(source));
                            let Some(feature) = matches.next() else {
                                continue;
                            };
                            return matches.next().is_none().then_some(feature.id.as_str());
                        }
                        None
                    })
                    .filter_map(|native| model_by_native.get(native).copied())
                    .filter(|seed_index| *seed_index != model_index)
                    .map(|seed_index| &model_features[seed_index].id);
                let mut seeds = Vec::new();
                for seed in seed_candidates {
                    if !seeds.contains(seed) {
                        let seed = copy_feature_binding_id(ctx, seed)?;
                        push_feature_binding_candidate(ctx, &mut seeds, seed)?;
                    }
                }
                if seeds.is_empty() {
                    if has_derived_cosmetic_thread_output {
                        if let Some(seed_index) = derived_cosmetic_thread_seed(feature)
                            .and_then(|native| model_by_native.get(native).copied())
                        {
                            let mut seeds = Vec::new();
                            let seed = copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                            push_feature_binding_candidate(ctx, &mut seeds, seed)?;
                            push_feature_binding_candidate(ctx, &mut mirror_seed_assignments, (model_index, seeds))?;
                        }
                    }
                } else {
                    push_feature_binding_candidate(ctx, &mut mirror_seed_assignments, (model_index, seeds))?;
                }
                continue;
            }
            if feature.input_class.as_deref() == Some("moCirPattern_c") {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
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
                if !needs_seed && !needs_axis {
                    continue;
                }
                let Some(start) =
                    Some(starts[start_index].0).filter(|start| *start < pattern_object_end())
                else {
                    continue;
                };
                let end = pattern_object_end();
                if needs_seed {
                    if let Some(pattern_source) = feature.source_value() {
                        let seed_candidates = generated_identities
                            .iter()
                            .filter(|identity| {
                                usize::try_from(identity.offset)
                                    .ok()
                                    .is_some_and(|offset| (start..end).contains(&offset))
                            })
                            .filter(|identity| {
                                identity.components.first().is_some_and(|component| {
                                    View::u32_le_at(&component.type_signature, 4)
                                        == Some(pattern_source)
                                })
                            })
                            .filter(|identity| {
                                identity.components.last().is_some_and(|component| {
                                    View::u32_le_at(&component.type_signature, 4)
                                        == Some(identity.feature_source_id.value())
                                        && component.local_id == Some(identity.local_identity)
                                })
                            })
                            .filter_map(|identity| {
                                let mut matches = history_features.iter().filter(|candidate| {
                                    candidate.source_value()
                                        == Some(identity.feature_source_id.value())
                                });
                                let seed = matches.next()?;
                                matches.next().is_none().then_some(seed.id.as_str())
                            })
                            .filter_map(|native| model_by_native.get(native).copied())
                            .filter(|seed_index| *seed_index != model_index)
                            .map(|seed_index| &model_features[seed_index].id);
                        let mut seeds = collect_feature_binding_vec(ctx, "collect SLDPRT pattern input candidates", "scan SLDPRT pattern input candidates", seed_candidates)?;
                        seeds.sort_unstable();
                        seeds.dedup();
                        if let [seed] = seeds.as_slice() {
                            let seed = copy_feature_binding_id(ctx, seed)?;
                            push_feature_binding_candidate(ctx, &mut pattern_seed_assignments, (model_index, seed))?;
                        }
                    }
                }
                if needs_axis {
                    if let Some(axis) = temporary_axis_reference(&lane.native_payload, start, end) {
                        push_feature_binding_candidate(ctx, &mut circular_axis_assignments, (model_index, axis.0, axis.1))?;
                    }
                }
                continue;
            }
            if feature.input_class.as_deref() == Some("moLPattern_c") {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                    continue;
                };
                let object_start = Some(starts[start_index].0);
                let end = pattern_object_end();
                if matches!(&(model_features[model_index].evaluation.definition()),
                    FeatureDefinition::Operation(FeatureOperation::Pattern {
                        pattern: admitted_pattern,
                        ..
                    }) if matches!(admitted_pattern.definition(), PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Linear) })
                ) {
                    if let Some((spacing, count)) =
                        object_start.filter(|start| *start < end).and_then(|start| {
                            typed_linear_pattern_dimensions(
                                feature,
                                lane,
                                start,
                                pattern_object_end(),
                            )
                        })
                    {
                        let admitted = PatternKind::new(PatternTransform::Linear {
                            direction: None, spacing, count, second: None,
                        }).map_err(|message| cadmpeg_core::CodecError::Malformed(message.into()))?;
                        model_features[model_index].evaluation.edit(|definition, _| {
                            if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition {
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
                    if has_derived_cosmetic_thread_output {
                        if let Some(seed_index) = derived_cosmetic_thread_seed(feature)
                            .and_then(|native| model_by_native.get(native).copied())
                        {
                            let seed = copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                            push_feature_binding_candidate(ctx, &mut pattern_seed_assignments, (model_index, seed))?;
                        }
                    } else if let Some((_, seed)) = start_index
                        .checked_sub(1)
                        .and_then(|index| starts.get(index))
                    {
                        if let Some(&seed_index) = model_by_native.get(seed.id.as_str()) {
                            let seed = copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                            push_feature_binding_candidate(ctx, &mut pattern_seed_assignments, (model_index, seed))?;
                        }
                    }
                }
                if !needs_direction {
                    continue;
                }
                let mut declarations = Vec::new();
                for class in &lane.classes {
                    ctx.charge_work(1, "scan SLDPRT pattern line declarations")?;
                    if class.name == "moLineRef_w"
                        && class.offset > u64_from_index(starts[start_index].0)
                        && class.offset < u64_from_index(end)
                    {
                        ctx.reserve_collection_vec(&mut declarations, 1, "collect SLDPRT pattern line declarations")?;
                        declarations.push(class);
                    }
                }
                let mut directions = Vec::new();
                for class in &declarations {
                    let declared = declared_line_reference_directions(
                        ctx,
                        &lane.native_payload,
                        class.offset,
                        end,
                    )?;
                    ctx.reserve_precharged_vec(
                        &mut directions,
                        declared.len(),
                        "merge SLDPRT declared line directions",
                    )?;
                    directions.extend(declared);
                }
                if let Some(start) = object_start {
                    let mut excluded_handles = Vec::new();
                    for class in &declarations {
                        if let Ok(offset) = usize::try_from(class.offset) {
                            ctx.reserve_collection_vec(&mut excluded_handles, 2, "collect SLDPRT excluded line handles")?;
                            excluded_handles.extend([offset + 136, offset + 144]);
                        }
                    }
                    let compact = compact_line_reference_directions(
                        ctx,
                        &lane.native_payload,
                        start,
                        end,
                        &excluded_handles,
                    )?;
                    ctx.reserve_precharged_vec(
                        &mut directions,
                        compact.len(),
                        "merge SLDPRT compact line directions",
                    )?;
                    directions.extend(compact);
                    if directions.is_empty() {
                        let first_spacing_m = feature
                            .parameters
                            .get("D3")
                            .and_then(|value| {
                                crate::history::literals::parse_positive_dimension_length_mm(value)
                            })
                            .map(|value| value.get() / 1000.0);
                        let second_spacing_m = feature
                            .parameters
                            .get("D4")
                            .and_then(|value| {
                                crate::history::literals::parse_positive_dimension_length_mm(value)
                            })
                            .map(|value| value.get() / 1000.0);
                        let display = linear_pattern_display_directions(
                            &lane.native_payload,
                            start,
                            end,
                            &lane.names,
                            [first_spacing_m, second_spacing_m],
                        );
                        ctx.reserve_collection_vec(&mut directions, display.len(), "collect SLDPRT pattern display directions")?;
                        directions.extend(display);
                    }
                }
                let mut unique_directions = Vec::new();
                for direction in directions
                    .into_iter()
                    .map(FeatureDirection3::from_unit_without_small_components)
                {
                    if !unique_directions
                        .iter()
                        .any(|candidate: &FeatureDirection3| {
                            let candidate = candidate.get();
                            let direction = direction.get();
                            let dot = candidate.x * direction.x
                                + candidate.y * direction.y
                                + candidate.z * direction.z;
                            (dot.abs() - 1.0).abs() <= EPS_BINDINGS_BIND_PATTERN_INPUTS_E12
                        })
                    {
                        push_feature_binding_candidate(ctx, &mut unique_directions, direction)?;
                    }
                }
                if matches!(unique_directions.len(), 1 | 2) {
                    ctx.reserve_collection_vec(&mut linear_direction_assignments, unique_directions.len(), "collect SLDPRT pattern direction assignments")?;
                    linear_direction_assignments.extend(unique_directions.into_iter().map(|direction| (model_index, direction)));
                }
                continue;
            }
            if feature.input_class.as_deref() != Some("moCurvePattern_c") {
                continue;
            }
            let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
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
                    if let Some(&seed_index) = model_by_native.get(seed.id.as_str()) {
                        let seed = copy_feature_binding_id(ctx, &model_features[seed_index].id)?;
                        push_feature_binding_candidate(ctx, &mut curve_seed_assignments, (model_index, seed))?;
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
            let Some(&target_index) = model_by_native.get(target.id.as_str()) else {
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
            push_feature_binding_candidate(ctx, &mut curve_path_assignments, (model_index, dependency, PathRef::Sketch(sketch)))?;
        }
    }
    ctx.reserve_precharged_vec(&mut pattern_seed_assignments, curve_seed_assignments.len(), "merge SLDPRT pattern seed assignments")?;
    pattern_seed_assignments.extend(curve_seed_assignments);
    let mut seeds_by_pattern = HashMap::<usize, Vec<cadmpeg_ir::features::FeatureId>>::new();
    for (index, seed) in pattern_seed_assignments {
        reserve_feature_binding_map(ctx, &mut seeds_by_pattern, "index SLDPRT pattern inputs")?;
        let candidates = seeds_by_pattern.entry(index).or_default();
        if !candidates.contains(&seed) {
            push_feature_binding_candidate(ctx, candidates, seed)?;
        }
    }
    for (index, mut candidates) in seeds_by_pattern {
        if candidates.len() != 1 { continue; }
        let Some(seed) = candidates.pop() else { continue; };
        if !model_features[index].dependencies.contains(&seed) {
            let dependency = copy_feature_binding_id(ctx, &seed)?;
            model_features[index].dependencies.try_insert_charged(dependency, ctx, "collect SLDPRT pattern dependencies")?;
        }
        let mut edit_result = Ok(());
        model_features[index].evaluation.edit(|definition, _| {
            edit_result = (|| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition {
                    if seeds.is_empty() {
                        push_feature_binding_candidate(ctx, seeds, PatternSeed::Feature(seed))?;
                    }
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        edit_result?;
    }
    let mut paths_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, dependency, path) in curve_path_assignments {
        reserve_feature_binding_map(ctx, &mut paths_by_pattern, "index SLDPRT pattern inputs")?;
        let candidates = paths_by_pattern.entry(index).or_default();
        if !candidates.iter().any(|(existing_dependency, existing_path)| existing_dependency == &dependency && existing_path == &path) {
            push_feature_binding_candidate(ctx, candidates, (dependency, path))?;
        }
    }
    for (index, mut candidates) in paths_by_pattern {
        if candidates.len() != 1 { continue; }
        let Some((dependency, path)) = candidates.pop() else { continue; };
        if !model_features[index].dependencies.contains(&dependency) {
            model_features[index].dependencies.try_insert_charged(dependency, ctx, "collect SLDPRT pattern dependencies")?;
        }
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition {
                if let Some(slot) = pattern.curve_path_mut() {
                    if slot.is_none() { *slot = Some(path); }
                }
            }
        });
    }
    let mut linear_directions_by_pattern = HashMap::<usize, Vec<FeatureDirection3>>::new();
    for (index, direction) in linear_direction_assignments {
        reserve_feature_binding_map(ctx, &mut linear_directions_by_pattern, "index SLDPRT pattern inputs")?;
        let candidates = linear_directions_by_pattern.entry(index).or_default();
        if !candidates.contains(&direction) { push_feature_binding_candidate(ctx, candidates, direction)?; }
    }
    for (index, candidates) in linear_directions_by_pattern {
        let native = model_features[index].native_ref.as_deref()
            .and_then(|native| history_features.iter().find(|feature| feature.id == native));
        let parameters = native.and_then(|feature| Some((
            feature.parameters.get("D4").and_then(|value| crate::history::literals::parse_positive_dimension_length_mm(value))?,
            feature.parameters.get("D2")?.parse::<u32>().ok()?,
        )));
        let mut edit_result = Ok(());
        model_features[index].evaluation.edit(|definition, _| {
            edit_result = (|| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition {
                    let PatternTransform::Linear { direction, spacing, count, second } = pattern.definition() else { return Ok(()); };
                    let mut direction = *direction;
                    let mut second = second.clone();
                    match candidates.as_slice() {
                        [first] if direction.is_none() => direction = Some(*first),
                        [first, second_direction] => {
                            if let (true, true, Some((spacing, count))) = (direction.is_none(), second.is_none(), parameters) {
                                direction = Some(*first);
                                second = Some(cadmpeg_ir::features::patterns::LinearPatternDirection { direction: *second_direction, spacing, count });
                            }
                        }
                        _ => {}
                    }
                    *pattern = PatternKind::new(PatternTransform::Linear { direction, spacing: *spacing, count: *count, second })
                        .map_err(|message| cadmpeg_core::CodecError::Malformed(message.into()))?;
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        edit_result?;
    }
    let mut mirror_planes_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, origin, normal) in mirror_plane_assignments {
        reserve_feature_binding_map(ctx, &mut mirror_planes_by_pattern, "index SLDPRT pattern inputs")?;
        let candidates = mirror_planes_by_pattern.entry(index).or_default();
        if !candidates.contains(&(origin, normal)) { push_feature_binding_candidate(ctx, candidates, (origin, normal))?; }
    }
    for (index, candidates) in mirror_planes_by_pattern {
        let [(origin, normal)] = candidates.as_slice() else { continue; };
        if matches!(model_features[index].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if pattern.is_unresolved()) {
            let admitted = PatternKind::new(PatternTransform::Mirror { plane_origin: admitted_point(*origin)?, plane_normal: admitted_direction(*normal)? })
                .map_err(|message| cadmpeg_core::CodecError::Malformed(message.into()))?;
            model_features[index].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition { *pattern = admitted; }
            });
        }
    }
    let mut mirror_seed_sets_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, seeds) in mirror_seed_assignments {
        reserve_feature_binding_map(ctx, &mut mirror_seed_sets_by_pattern, "index SLDPRT pattern inputs")?;
        let candidates = mirror_seed_sets_by_pattern.entry(index).or_default();
        if !candidates.contains(&seeds) { push_feature_binding_candidate(ctx, candidates, seeds)?; }
    }
    for (index, mut candidates) in mirror_seed_sets_by_pattern {
        if candidates.len() != 1 { continue; }
        let Some(seeds) = candidates.pop() else { continue; };
        for seed in &seeds {
            if !model_features[index].dependencies.contains(seed) {
                let dependency = copy_feature_binding_id(ctx, seed)?;
                model_features[index].dependencies.try_insert_charged(dependency, ctx, "collect SLDPRT pattern dependencies")?;
            }
        }
        let mut edit_result = Ok(());
        model_features[index].evaluation.edit(|definition, _| {
            edit_result = (|| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds: seed_slots, .. }) = definition {
                    if seed_slots.is_empty() {
                        ctx.reserve_collection_vec(seed_slots, seeds.len(), "collect SLDPRT mirror pattern seeds")?;
                        seed_slots.extend(seeds.into_iter().map(PatternSeed::Feature));
                    }
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        edit_result?;
    }
    let mut circular_axes_by_pattern = HashMap::<usize, Vec<(FinitePoint3, UnitVector3)>>::new();
    for (index, origin, direction) in circular_axis_assignments {
        reserve_feature_binding_map(ctx, &mut circular_axes_by_pattern, "index SLDPRT pattern inputs")?;
        let candidates = circular_axes_by_pattern.entry(index).or_default();
        if !candidates.contains(&(origin, direction)) { push_feature_binding_candidate(ctx, candidates, (origin, direction))?; }
    }
    for (index, candidates) in circular_axes_by_pattern {
        let [(axis_origin, axis_dir)] = candidates.as_slice() else { continue; };
        let Some(native_ref) = model_features[index].native_ref.as_deref() else { continue; };
        let Some(native) = history_features.iter().find(|feature| feature.id == native_ref) else { continue; };
        let Some(angle) = native.parameters.get("Angle").and_then(|value| parse_positive_angle_rad(value)) else { continue; };
        let Some(count) = native.parameters.get("Count").and_then(|value| parse_count(value)) else { continue; };
        if !matches!(model_features[index].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if matches!(pattern.definition(), PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Circular) })) { continue; }
        let admitted = PatternKind::new(PatternTransform::Circular { axis_origin: *axis_origin, axis_dir: FeatureDirection3::from(*axis_dir), angle, count })
            .map_err(|message| cadmpeg_core::CodecError::Malformed(message.into()))?;
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition { *pattern = admitted; }
        });
    }
    Ok(())
}

fn push_feature_binding_candidate<T>(ctx: &DecodeContext<'_>, values: &mut Vec<T>, value: T) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_work(1, "collect SLDPRT feature binding assignments")?;
    ctx.reserve_collection_vec(values, 1, "collect SLDPRT feature binding assignments")?;
    values.push(value);
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
    let mut mirror_native_refs = HashSet::new();
    for feature in histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            native_object_class(feature.input_class.as_deref().unwrap_or_default())
                == NativeClassKind::MirrorPattern
        })
    {
        ctx.charge_work(1, "index SLDPRT mirror surface planes")?;
        ctx.charge_collection_items(1, "index SLDPRT mirror surface planes")?;
        mirror_native_refs.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT mirror surface planes", u64::MAX - 1, u64::MAX))?;
        mirror_native_refs.insert(feature.id.as_str());
    }
    let mut faces_by_identity = HashMap::<(FeatureSourceId, u32), Vec<&str>>::new();
    for (face, identity) in face_identities {
        reserve_feature_binding_map(ctx, &mut faces_by_identity, "index SLDPRT mirror surface planes")?;
        let candidates = faces_by_identity
            .entry((identity.feature_source_id, identity.local_id))
            .or_default();
        if !candidates.contains(&face.as_str()) {
            ctx.reserve_collection_vec(candidates, 1, "collect SLDPRT mirror face identities")?;
            candidates.push(face.as_str());
        }
    }
    let mut faces_by_id = HashMap::new();
    for face in faces {
        reserve_feature_binding_map(ctx, &mut faces_by_id, "index SLDPRT mirror surface planes")?;
        faces_by_id.insert(face.id.as_str(), face);
    }
    let mut surfaces_by_id = HashMap::new();
    for surface in surfaces {
        reserve_feature_binding_map(ctx, &mut surfaces_by_id, "index SLDPRT mirror surface planes")?;
        surfaces_by_id.insert(surface.id.as_str(), surface);
    }

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            edit_result = (|| {
                'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            if !pattern.is_unresolved() {
                break 'feature_edit;
            }
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            if !mirror_native_refs.contains(native_ref) {
                break 'feature_edit;
            }
            let mut candidates = Vec::new();
            for selection in lanes
                .iter()
                .filter(|lane| !is_supplemental_config_lane(lane))
                .flat_map(|lane| &lane.surface_selections)
                .filter(|selection| selection.feature_ref == native_ref)
            {
                ctx.charge_work(1, "scan SLDPRT mirror plane selections")?;
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
                let Some([face_id]) = faces_by_identity.get(&(source, local)).map(Vec::as_slice)
                else {
                    continue;
                };
                let Some(surface) = faces_by_id
                    .get(face_id)
                    .and_then(|face| surfaces_by_id.get(face.surface.as_str()))
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
                if !candidates.contains(&plane) {
                    ctx.reserve_collection_vec(&mut candidates, 1, "collect SLDPRT mirror plane candidates")?;
                    candidates.push(plane);
                }
            }
            let [(origin, normal)] = candidates.as_slice() else {
                break 'feature_edit;
            };
            if let Ok(admitted) = PatternKind::new(PatternTransform::Mirror {
                plane_origin: *origin,
                plane_normal: cadmpeg_ir::features::FeatureDirection3::from(*normal),
            }) {
                *pattern = admitted;
            }
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })();
        });
        edit_result?;
    }

    Ok(())
}

pub(crate) fn bind_sweep_adjacent_profiles(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let metadata_ids = history_metadata_ids(ctx, histories)?;
    let history_features = collect_feature_binding_vec(ctx, "collect SLDPRT feature binding candidates", "scan SLDPRT feature binding candidates", histories
        .iter()
        .flat_map(|history| &history.features)
        )?;
    let mut model_by_native = HashMap::new();
    for (index, feature) in model_features.iter().enumerate() {
        if let Some(native) = feature.native_ref.as_deref() {
            reserve_feature_binding_map(ctx, &mut model_by_native, "index SLDPRT sweep adjacent profiles")?;
            model_by_native.insert(native, index);
        }
    }
    let mut assignments = HashMap::<
        usize,
        Vec<(
            cadmpeg_ir::features::FeatureId,
            SketchId,
            Option<(cadmpeg_ir::features::FeatureId, SketchId)>,
        )>,
    >::new();
    for lane in lanes {
        let mut starts = collect_feature_binding_vec(ctx, "collect SLDPRT feature binding candidates", "scan SLDPRT feature binding candidates", history_features
            .iter()
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            )?;
        starts.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, (_, feature)) in starts.iter().enumerate() {
            if feature.input_class.as_deref() != Some("moSweep_c") {
                continue;
            }
            let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
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
            let Some(&profile_index) = model_by_native.get(profile_feature.id.as_str()) else {
                continue;
            };
            let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) = model_features[profile_index].evaluation.definition()
            else {
                continue;
            };
            let path = index.checked_sub(1).and_then(|path_object_index| {
                let (_, path_feature) = starts[path_object_index];
                if path_feature.input_class.as_deref() != Some("moProfileFeature_c") {
                    return None;
                }
                let path_index = *model_by_native.get(path_feature.id.as_str())?;
                let FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(path)),
                }) = model_features[path_index].evaluation.definition()
                else {
                    return None;
                };
                Some((&model_features[path_index].id, path))
            });
            let path = path.map(|(feature, sketch)| Ok::<_, cadmpeg_core::CodecError>((copy_feature_binding_id(ctx, feature)?, copy_feature_binding_sketch_id(ctx, sketch)?))).transpose()?;
            let candidate = (
                copy_feature_binding_id(ctx, &model_features[profile_index].id)?,
                copy_feature_binding_sketch_id(ctx, sketch)?,
                path,
            );
            reserve_feature_binding_map(ctx, &mut assignments, "index SLDPRT sweep adjacent profiles")?;
            let candidates = assignments.entry(model_index).or_default();
            if !candidates.contains(&candidate) {
                ctx.reserve_collection_vec(candidates, 1, "collect SLDPRT sweep profile assignments")?;
                candidates.push(candidate);
            }
        }
    }
    for (index, mut candidates) in assignments {
        if candidates.len() != 1 { continue; }
        let Some((profile_dependency, sketch, path)) = candidates.pop() else { continue; };
        let mut profile_bound = false;
        let mut path_dependency = None;
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path: path_slot, .. }) = definition {
                if shape.section_is_unresolved() {
                    shape.set_referenced_profile(sketch.into());
                    profile_bound = true;
                }
                if let Some((dependency, path)) = path {
                    if path_slot.as_ref().is_none_or(|existing| matches!(existing, PathRef::Native(_))) {
                        *path_slot = Some(PathRef::Sketch(path));
                    }
                    path_dependency = Some(dependency);
                }
            } else {
                path_dependency = path.map(|(dependency, _)| dependency);
            }
        });
        if profile_bound && !model_features[index].dependencies.contains(&profile_dependency) {
            model_features[index].dependencies.try_insert_charged(profile_dependency, ctx, "collect SLDPRT sweep profile dependencies")?;
        }
        if let Some(dependency) = path_dependency {
            if !model_features[index].dependencies.contains(&dependency) {
                model_features[index].dependencies.try_insert_charged(dependency, ctx, "collect SLDPRT sweep profile dependencies")?;
            }
        }
    }
    Ok(())
}

fn copy_feature_binding_id(ctx: &DecodeContext<'_>, id: &cadmpeg_ir::features::FeatureId) -> Result<cadmpeg_ir::features::FeatureId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(format_args!("{}", id.as_str()), "retain SLDPRT feature binding identity")?;
    cadmpeg_ir::features::FeatureId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn copy_feature_binding_sketch_id(ctx: &DecodeContext<'_>, id: &SketchId) -> Result<SketchId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(format_args!("{}", id.as_str()), "retain SLDPRT feature binding identity")?;
    SketchId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn reserve_feature_binding_map<K: Eq + std::hash::Hash, V>(ctx: &DecodeContext<'_>, values: &mut HashMap<K, V>, operation: &'static str) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_work(1, operation)?;
    ctx.charge_collection_items(1, operation)?;
    values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))
}

fn collect_feature_binding_vec<T>(ctx: &DecodeContext<'_>, collection_operation: &'static str, work_operation: &'static str, items: impl Iterator<Item = T>) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let mut values = Vec::new();
    for item in items {
        ctx.charge_work(1, work_operation)?;
        ctx.reserve_collection_vec(&mut values, 1, collection_operation)?;
        values.push(item);
    }
    Ok(values)
}

pub(crate) fn bind_scalar_operands(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let represented_sketches = represented_sketch_features(ctx, histories, lanes)?;
    let metadata_ids = history_metadata_ids(ctx, histories)?;
    for lane in lanes {
        for entity in &mut lane.sketch_entities {
            entity.feature_ref = None;
            entity.links = None;
        }
        let mut starts = collect_binding_vec(ctx, histories
            .iter()
            .flat_map(|history| &history.features)
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.as_str(),
                ))
            })
            )?;
        starts.sort_unstable_by_key(|start| start.0);
        for (index, &(start, feature_id)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            for entity in lane
                .sketch_entities
                .iter_mut()
                .filter(|entity| entity.offset() > start && entity.offset() < end)
            {
                entity.feature_ref = Some(copy_binding_text(ctx, feature_id)?);
            }
            for reference in lane
                .references
                .iter_mut()
                .filter(|reference| reference.offset > start && reference.offset < end)
            {
                reference.feature_ref = Some(copy_binding_text(ctx, feature_id)?);
            }
            for scalar in lane
                .scalars
                .iter_mut()
                .filter(|scalar| scalar.offset > start && scalar.offset < end)
            {
                scalar.feature_ref = Some(copy_binding_text(ctx, feature_id)?);
            }
        }
        bind_detached_legacy_sketch_objects(ctx, histories, &represented_sketches, lane)?;
        let mut features_by_id = HashMap::new();
        for feature in histories.iter().flat_map(|history| &history.features) {
            reserve_binding_map(ctx, &mut features_by_id)?;
            features_by_id.insert(feature.id.as_str(), feature);
        }
        for pair in starts.windows(2) {
            let [(_, parent_id), (child_start, child_id)] = pair else {
                continue;
            };
            let (Some(parent), Some(child)) = (
                features_by_id.get(parent_id).copied(),
                features_by_id.get(child_id).copied(),
            ) else {
                continue;
            };
            if native_object_class(parent.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::Extrusion
                && !matches!(parent.xml_tag.as_str(), "Extrusion" | "Cut")
            {
                continue;
            }
            if !is_dissected_profile_feature(child) {
                continue;
            }
            let child_end = starts
                .iter()
                .find(|(offset, _)| offset > child_start)
                .map_or(u64::MAX, |(offset, _)| *offset);
            for scalar in lane.scalars.iter_mut().filter(|scalar| {
                scalar.offset > *child_start
                    && scalar.offset < child_end
                    && scalar.feature_ref.as_deref() == Some(*child_id)
            }) {
                scalar.feature_ref = Some(copy_binding_text(ctx, parent_id)?);
            }
        }
        finalize_lane_bindings(ctx, histories, lane)?;
    }
    Ok(())
}

pub(crate) fn finalize_lane_bindings(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    normalize_indexed_curve_entities(ctx, lane)?;
    let mut marker_ids = HashMap::<String, HashMap<u32, Vec<(String, bool)>>>::new();
    for entity in &lane.sketch_entities {
        if let (Some(feature), Some(local_id)) = (&entity.feature_ref, entity.local_id()) {
            if !marker_ids.contains_key(feature.as_str()) {
                reserve_binding_map(ctx, &mut marker_ids)?;
                marker_ids.insert(copy_binding_text(ctx, feature)?, HashMap::new());
            }
            if let Some(by_local) = marker_ids.get_mut(feature.as_str()) {
                reserve_binding_map(ctx, by_local)?;
                let candidates = by_local.entry(local_id).or_default();
                ctx.reserve_collection_vec(candidates, 1, "collect SLDPRT scalar marker candidates")?;
                candidates.push((copy_binding_text(ctx, entity.id())?, entity.coordinates_m.is_some()));
            }
        }
    }
    for entity in &mut lane.sketch_entities {
        let Ok(offset) = usize::try_from(entity.offset()) else {
            continue;
        };
        ctx.charge_work(128, "decode SLDPRT scalar local links")?;
        let local_links = if let Some((local_ids, selector)) = marker_local_links(&lane.native_payload, offset) {
            let mut links = Vec::new();
            ctx.reserve_collection_vec(&mut links, local_ids.len(), "decode SLDPRT scalar local links")?;
            links.extend(local_ids);
            Some((links, selector))
        } else {
            coordinate_marker_local_links(ctx, &lane.native_payload, offset)?
        };
        let Some((local_ids, selector)) = local_links else { continue; };
        let Some(owner) = &entity.feature_ref else {
            continue;
        };
        let mut links = Vec::new();
        for local_id in local_ids {
            let Some(entity_ref) = marker_ids.get(owner.as_str())
                .and_then(|by_local| by_local.get(&u32::from(local_id)))
                .and_then(|candidates| unique_marker_candidate(candidates)) else {
                continue;
            };
            ctx.reserve_collection_vec(&mut links, 1, "collect SLDPRT scalar local links")?;
            links.push(SketchInputLink { local_id, entity_ref: copy_binding_text(ctx, entity_ref)? });
        }
        if let Some(links) = crate::records::SketchInputLinks::new(selector, links) {
            entity.links = Some(links);
        }
    }
    bind_resolved_curve_vertices(ctx, lane)?;
    let mut entities_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
    for entity in &lane.sketch_entities {
        if let Some(feature) = entity.feature_ref.as_deref() {
            reserve_binding_map(ctx, &mut entities_by_feature)?;
            let entities = entities_by_feature.entry(feature).or_default();
            ctx.reserve_collection_vec(entities, 1, "collect SLDPRT scalar owner entities")?;
            entities.push(entity);
        }
    }
    for scalar in &mut lane.scalars {
        let Some(entities) = scalar
            .feature_ref
            .as_deref()
            .and_then(|feature| entities_by_feature.get(feature))
        else {
            continue;
        };
        let resolved = resolve_scalar_operand_markers(ctx, entities.iter().copied(), &scalar.operands)?;
        for (operand, resolved) in scalar.operands.iter_mut().zip(resolved) {
            operand.entity_ref = resolved.map(|entity| copy_binding_text(ctx, entity.id())).transpose()?;
        }
    }
    let mut scalar_owners = HashMap::new();
    for scalar in &lane.scalars {
        reserve_binding_map(ctx, &mut scalar_owners)?;
        scalar_owners.insert(scalar.id.as_str(), scalar.feature_ref.as_deref());
    }
    for binding in &mut lane.relation_bindings {
        binding.feature_ref = scalar_owners.get(binding.scalar_ref.as_str()).copied().flatten()
            .map(|owner| copy_binding_text(ctx, owner)).transpose()?;
    }
    let intervals = feature_intervals(ctx, histories, lane)?;
    lane.relation_bindings =
        relation_bindings_scoped(ctx, &lane.id, &lane.classes, &lane.scalars, &intervals)?;
    lane.relation_instances = relation_instances(ctx, histories, lane)?;
    lane.body_selections = compact_body_selections(ctx, histories, lane)?;
    lane.edge_selections = compact_edge_selections(ctx, histories, lane)?;
    lane.surface_selections = compact_surface_selections(ctx, histories, lane)?;
    lane.generated_surface_identities = generated_surface_identities(ctx, lane)?;
    Ok(())
}

fn represented_sketch_features(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<HashSet<String>, cadmpeg_core::CodecError> {
    let metadata_ids = history_metadata_ids(ctx, histories)?;
    let features = collect_binding_vec(ctx, histories
        .iter()
        .flat_map(|history| &history.features)
        )?;
    let mut represented = HashSet::new();
    for lane in lanes {
        if is_supplemental_config_lane(lane) {
            continue;
        }
        let mut objects = collect_binding_vec(ctx, features
            .iter()
            .filter(|feature| !metadata_ids.contains(feature.id.as_str()))
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            )?;
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, &(start, feature)) in objects.iter().enumerate() {
            if feature.xml_tag != "Sketch" {
                continue;
            }
            let end = objects.get(index + 1).map_or(u64::MAX, |next| next.0);
            if lane.sketch_entities.iter().any(|entity| {
                entity.offset() > start && entity.offset() < end && entity.coordinates_m.is_some()
            }) {
                reserve_binding_set(ctx, &mut represented)?;
                represented.insert(copy_binding_text(ctx, &feature.id)?);
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
    let mut unresolved = HashSet::new();
    for feature in model_features {
        if matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved | cadmpeg_ir::features::SketchFeatureBinding::Planar(None), ..
        })) {
            if let Some(native) = feature.native_ref.as_deref() {
                reserve_binding_set(ctx, &mut unresolved)?;
                unresolved.insert(native);
            }
        }
    }
    let mut represented = HashSet::new();
    for feature in histories.iter().flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag == "Sketch" && !unresolved.contains(feature.id.as_str())) {
        reserve_binding_set(ctx, &mut represented)?;
        represented.insert(copy_binding_text(ctx, &feature.id)?);
    }
    for lane in lanes
        .iter_mut()
        .filter(|lane| is_supplemental_config_lane(lane))
    {
        bind_detached_legacy_sketch_objects(ctx, histories, &represented, lane)?;
        finalize_lane_bindings(ctx, histories, lane)?;
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

    if !is_supplemental_config_lane(lane) {
        return Ok(());
    }
    let limit = lane
        .classes
        .iter()
        .find(|class| class.name == "moFeatureDimHandle_c")
        .map_or_else(
            || u64_from_index(lane.native_payload.len()),
            |class| class.offset,
        );
    let relation_bindings = bind_detached_spatial_relation_objects(ctx, histories, represented, lane)?;
    let markers = collect_binding_vec(ctx, lane
        .sketch_entities
        .iter()
        .filter(|entity| entity.offset() < limit)
        .filter(|entity| {
            relation_bindings
                .iter()
                .all(|(start, end, _)| entity.offset() < *start || entity.offset() >= *end)
        })
        .map(crate::records::SketchInputEntity::offset)
        )?;
    let Some(&first) = markers.first() else {
        return Ok(());
    };
    let mut starts = ctx.alloc_filled(1, first, "collect SLDPRT detached sketch starts")?;
    for pair in markers.windows(2) {
        if pair[1] >= pair[0] && pair[1] - pair[0] >= OBJECT_GAP {
            ctx.reserve_collection_vec(&mut starts, 1, "collect SLDPRT detached sketch starts")?;
            starts.push(pair[1]);
        }
    }

    let mut owners = collect_binding_vec(ctx, histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag == "Sketch")
        .filter(|feature| {
            native_object_class(feature.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::OriginProfileFeature
        })
        .filter(|feature| !represented.contains(&feature.id))
        .filter(|feature| {
            relation_bindings
                .iter()
                .all(|(_, _, owner)| owner != &feature.id)
        })
        .filter_map(|feature| Some((feature.source_value()?, feature)))
        )?;
    owners.sort_unstable_by_key(|(source, _)| *source);
    if starts.len() != owners.len() {
        return Ok(());
    }

    for (index, (&start, (_, owner))) in starts.iter().zip(owners).enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(limit);
        for entity in lane
            .sketch_entities
            .iter_mut()
            .filter(|entity| entity.offset() >= start && entity.offset() < end)
        {
            entity.feature_ref = Some(copy_binding_text(ctx, &owner.id)?);
        }
        for reference in lane
            .references
            .iter_mut()
            .filter(|reference| reference.offset >= start && reference.offset < end)
        {
            reference.feature_ref = Some(copy_binding_text(ctx, &owner.id)?);
        }
        for scalar in lane
            .scalars
            .iter_mut()
            .filter(|scalar| scalar.offset >= start && scalar.offset < end)
        {
            scalar.feature_ref = Some(copy_binding_text(ctx, &owner.id)?);
        }
    }
    Ok(())
}

pub(super) fn spatial_relation_manager_ranges_charged(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
) -> Result<Vec<(u64, u64)>, cadmpeg_core::CodecError> {
    let scan_steps = lane.classes.len().checked_mul(lane.classes.len()).ok_or_else(|| {
        ctx.refuse_codec_limit("scan SLDPRT spatial relation classes", u64::MAX - 1, u64::MAX)
    })?;
    ctx.charge_work(
        u64::try_from(scan_steps).map_err(|_| {
            ctx.refuse_codec_limit("scan SLDPRT spatial relation classes", u64::MAX - 1, u64::MAX)
        })?,
        "scan SLDPRT spatial relation classes",
    )?;
    let mut ranges = Vec::new();
    for range in spatial_relation_manager_candidates(lane) {
        ctx.reserve_collection_vec(&mut ranges, 1, "collect SLDPRT spatial relation ranges")?;
        ranges.push(range);
    }
    ranges.sort_unstable();
    ranges.dedup();
    Ok(ranges)
}

fn spatial_relation_manager_candidates(
    lane: &FeatureInputLane,
) -> impl Iterator<Item = (u64, u64)> + '_ {
    lane
        .classes
        .iter()
        .filter(|class| class.name == "sg3DPlaneHandle")
        .filter_map(|plane| {
            let start = lane
                .classes
                .iter()
                .filter(|class| class.name == "moRelMgr_c" && class.offset < plane.offset)
                .max_by_key(|class| class.offset)?
                .offset;
            let end = lane
                .classes
                .iter()
                .filter(|class| class.name == "suObList" && class.offset > plane.offset)
                .min_by_key(|class| class.offset)?
                .offset;
            (start < plane.offset && plane.offset < end).then_some((start, end))
        })
}

fn bind_detached_spatial_relation_objects(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    represented: &HashSet<String>,
    lane: &mut FeatureInputLane,
) -> Result<Vec<(u64, u64, String)>, cadmpeg_core::CodecError> {
    let ranges = spatial_relation_manager_ranges_charged(ctx, lane)?;
    if ranges.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = HashMap::new();
    for name in &lane.names {
        reserve_binding_map(ctx, &mut names)?;
        names.insert(name.id.as_str(), name.value.as_str());
    }
    let is_dimension_name = |name: &str| {
        name.strip_prefix('D').is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    let mut owners = Vec::new();
    'owner: for feature in histories.iter().flat_map(|history| &history.features)
        .filter(|feature| feature.input_class.as_deref() == Some("mo3DProfileFeature_c"))
        .filter(|feature| !represented.contains(&feature.id))
    {
        ctx.charge_work(1, "scan SLDPRT spatial sketch owners")?;
        let mut dimensions = Vec::new();
        for (name, value) in feature.parameters.iter().filter(|(name, _)| is_dimension_name(name.as_str())) {
            let Some(value) = crate::history::literals::parse_dimension_length_mm(value) else {
                continue 'owner;
            };
            ctx.reserve_collection_vec(&mut dimensions, 1, "collect SLDPRT spatial sketch dimensions")?;
            dimensions.push((name.as_str(), value));
        }
        if dimensions.len() >= 3 {
            ctx.reserve_collection_vec(&mut owners, 1, "collect SLDPRT spatial sketch owners")?;
            owners.push((feature, dimensions));
        }
    }
    let mut candidates = Vec::new();
    for &(start, end) in &ranges {
        let scalars = collect_binding_vec(ctx, lane.scalars.iter()
            .filter(|scalar| scalar.offset > start && scalar.offset < end)
            .filter(|scalar| scalar.role != crate::records::FeatureInputScalarRole::Display)
            .filter_map(|scalar| Some((names.get(scalar.name.as_str()).copied()?, scalar.value.get())))
            .filter(|(name, _)| is_dimension_name(name)))?;
        for (owner, dimensions) in &owners {
            ctx.charge_work(1, "match SLDPRT spatial sketch owners")?;
            let steps = dimensions.len().checked_mul(scalars.len()).and_then(|count| count.checked_mul(2))
                .ok_or_else(|| ctx.refuse_codec_limit("match SLDPRT spatial sketch names", u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(u64_from_index(steps), "match SLDPRT spatial sketch names")?;
            if !scalars.iter().all(|(name, _)| dimensions.iter().any(|(candidate, _)| candidate == name))
                || !dimensions.iter().all(|(name, _)| scalars.iter().any(|(candidate, _)| candidate == name)) {
                continue;
            }
            let mut exact = true;
            for (name, expected_mm) in dimensions {
                ctx.charge_work(u64_from_index(scalars.len()), "match SLDPRT spatial sketch dimensions")?;
                if !scalars.iter().any(|(candidate, value_m)| candidate == name
                    && (value_m * 1000.0 - expected_mm.get()).abs()
                        <= expected_mm.get().abs().max(1.0) * EPS_BINDINGS_BIND_DETACHED_SPATIAL_RELATION_OBJECTS_E9) {
                    exact = false;
                    break;
                }
            }
            if exact {
                ctx.reserve_collection_vec(&mut candidates, 1, "collect SLDPRT spatial sketch candidates")?;
                candidates.push((start, end, owner.id.as_str()));
            }
        }
    }
    let mut bound = Vec::new();
    for &(start, end, owner) in &candidates {
        ctx.charge_work(u64_from_index(candidates.len()), "disambiguate SLDPRT spatial sketch ranges")?;
        if candidates.iter().filter(|(candidate_start, candidate_end, _)| *candidate_start == start && *candidate_end == end).count() != 1 {
            continue;
        }
        ctx.charge_work(u64_from_index(candidates.len()), "disambiguate SLDPRT spatial sketch owners")?;
        if candidates.iter().filter(|(_, _, candidate_owner)| *candidate_owner == owner).count() == 1 {
            ctx.reserve_collection_vec(&mut bound, 1, "collect SLDPRT spatial sketch bindings")?;
            bound.push((start, end, copy_binding_text(ctx, owner)?));
        }
    }
    for (start, end, owner) in &bound {
        for entity in lane.sketch_entities.iter_mut().filter(|entity| entity.offset() > *start && entity.offset() < *end) {
            entity.feature_ref = Some(copy_binding_text(ctx, owner)?);
        }
        for reference in lane.references.iter_mut().filter(|reference| reference.offset > *start && reference.offset < *end) {
            reference.feature_ref = Some(copy_binding_text(ctx, owner)?);
        }
        for scalar in lane.scalars.iter_mut().filter(|scalar| scalar.offset > *start && scalar.offset < *end) {
            scalar.feature_ref = Some(copy_binding_text(ctx, owner)?);
        }
    }
    Ok(bound)
}

pub(super) fn normalize_indexed_curve_entities(
    ctx: &DecodeContext<'_>,
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    let terminal_lines = {
        let markers = collect_binding_vec(ctx, lane.sketch_entities.iter())?;
        let mut terminal = HashSet::new();
        for curve in &markers {
            ctx.charge_work(u64_from_index(markers.len()), "scan SLDPRT terminal profile lines")?;
            if legacy_terminal_indexed_profile_line(&lane.native_payload, curve, &markers) {
                reserve_binding_set(ctx, &mut terminal)?;
                terminal.insert(copy_binding_text(ctx, curve.id())?);
            }
        }
        terminal
    };
    for marker in &mut lane.sketch_entities {
        if terminal_lines.contains(marker.id()) {
            marker.reclassify(SketchInputKind::LineOrCircle);
        }
    }
    let mut endpoints = HashMap::<String, HashSet<u32>>::new();
    for curve in &lane.sketch_entities {
        ctx.charge_work(1, "scan SLDPRT indexed curve endpoints")?;
        let Some(feature) = curve.feature_ref.as_deref() else { continue; };
        let Some(offset) = index_from_u64(curve.offset()) else { continue; };
        let Some(indices) = wide_indexed_curve_endpoint_indices(&lane.native_payload, offset)
            .or_else(|| compact_indexed_curve_endpoint_indices(&lane.native_payload, offset))
            .or_else(|| extended_compact_indexed_curve_endpoint_indices(&lane.native_payload, offset))
            .or_else(|| compact_legacy_curve_endpoint_indices(&lane.native_payload, offset))
            .or_else(|| alternate_current_indexed_curve_endpoint_indices(&lane.native_payload, offset)) else { continue; };
        if !endpoints.contains_key(feature) {
            reserve_binding_map(ctx, &mut endpoints)?;
            endpoints.insert(copy_binding_text(ctx, feature)?, HashSet::new());
        }
        if let Some(by_index) = endpoints.get_mut(feature) {
            for index in indices {
                reserve_binding_set(ctx, by_index)?;
                by_index.insert(index);
            }
        }
    }
    let linked_endpoint_coordinates = {
        let markers = collect_binding_vec(ctx, lane.sketch_entities.iter())?;
        let mut coordinates = HashMap::new();
        for curve in &lane.sketch_entities {
            ctx.charge_work(u64_from_index(markers.len()), "scan SLDPRT reverse incidence endpoints")?;
            let Some(offsets) = current_reverse_incidence_endpoint_offsets(&lane.native_payload, curve, &markers) else { continue; };
            for offset in offsets {
                let Ok(native_offset) = usize::try_from(offset) else { continue; };
                let Some((point, _)) = linked_profile_point(&lane.native_payload, native_offset) else { continue; };
                reserve_binding_map(ctx, &mut coordinates)?;
                coordinates.insert(offset, point);
            }
        }
        coordinates
    };
    for marker in &mut lane.sketch_entities {
        let Some((feature, index)) = marker.feature_ref.as_deref().zip(marker.object_index()) else { continue; };
        if marker.coordinates_m.is_none() {
            marker.coordinates_m = linked_endpoint_coordinates.get(&marker.offset()).copied();
        }
        if (endpoints.get(feature).is_some_and(|indices| indices.contains(&index))
            || linked_endpoint_coordinates.contains_key(&marker.offset())) && marker.coordinates_m.is_some() {
            marker.reclassify(SketchInputKind::Point);
        }
    }
    Ok(())
}

fn bind_resolved_curve_vertices(
    ctx: &DecodeContext<'_>,
    lane: &mut FeatureInputLane,
) -> Result<(), cadmpeg_core::CodecError> {
    let selected_axis_endpoints = {
        let mut markers_by_id = HashMap::new();
        for marker in &lane.sketch_entities {
            reserve_binding_map(ctx, &mut markers_by_id)?;
            markers_by_id.insert(marker.id(), marker);
        }
        let markers = collect_binding_vec(ctx, lane.sketch_entities.iter())?;
        let mut selected = HashSet::new();
        for curve in markers.iter().copied().filter(|curve| {
            index_from_u64(curve.offset()).is_some_and(|offset| marker_is_selected_construction_line(&lane.native_payload, offset))
        }) {
            ctx.charge_work(u64_from_index(markers.len()), "scan SLDPRT selected curve endpoints")?;
            for marker in marker_curve_endpoint_markers(&lane.native_payload, curve, &markers_by_id, &markers)
                .into_iter().filter(|marker| marker.coordinates_m.is_some()) {
                reserve_binding_set(ctx, &mut selected)?;
                selected.insert(copy_binding_text(ctx, marker.id())?);
            }
        }
        selected
    };
    for marker in &mut lane.sketch_entities {
        if selected_axis_endpoints.contains(marker.id()) {
            marker.reclassify(SketchInputKind::Point);
        }
    }
    loop {
        let mut markers_by_id = HashMap::new();
        for marker in &lane.sketch_entities {
            reserve_binding_map(ctx, &mut markers_by_id)?;
            markers_by_id.insert(marker.id(), marker);
        }
        let markers = collect_binding_vec(ctx, lane.sketch_entities.iter())?;
        let mut resolved_curves = HashSet::new();
        let mut resolved_endpoints = HashSet::new();
        for curve in markers.iter().copied().filter(|marker| matches!(marker.kind(), SketchInputKind::LineOrCircle | SketchInputKind::Arc)) {
            ctx.charge_work(u64_from_index(markers.len()), "resolve SLDPRT curve endpoints")?;
            let endpoints = marker_curve_endpoint_markers(&lane.native_payload, curve, &markers_by_id, &markers);
            if endpoints.len() == 2 {
                reserve_binding_set(ctx, &mut resolved_curves)?;
                resolved_curves.insert(copy_binding_text(ctx, curve.id())?);
            }
            for marker in endpoints.into_iter().filter(|marker| marker.coordinates_m.is_some()) {
                reserve_binding_set(ctx, &mut resolved_endpoints)?;
                resolved_endpoints.insert(copy_binding_text(ctx, marker.id())?);
            }
        }
        let mut changed = false;
        for marker in &mut lane.sketch_entities {
            if marker.kind() != SketchInputKind::Point && resolved_endpoints.contains(marker.id()) && !resolved_curves.contains(marker.id()) {
                marker.reclassify(SketchInputKind::Point);
                changed = true;
            }
        }
        if !changed { break; }
    }
    Ok(())
}

fn copy_binding_text(ctx: &DecodeContext<'_>, text: &str) -> Result<String, cadmpeg_core::CodecError> {
    ctx.format_retained(format_args!("{text}"), "retain SLDPRT scalar binding identity")
}

fn reserve_binding_map<K: Eq + std::hash::Hash, V>(ctx: &DecodeContext<'_>, values: &mut HashMap<K, V>) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_collection_items(1, "index SLDPRT scalar bindings")?;
    values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT scalar bindings", u64::MAX - 1, u64::MAX))
}

fn reserve_binding_set<T: Eq + std::hash::Hash>(ctx: &DecodeContext<'_>, values: &mut HashSet<T>) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_collection_items(1, "index SLDPRT scalar bindings")?;
    values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("index SLDPRT scalar bindings", u64::MAX - 1, u64::MAX))
}

fn collect_binding_vec<T>(ctx: &DecodeContext<'_>, items: impl Iterator<Item = T>) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let mut values = Vec::new();
    for item in items {
        ctx.charge_work(1, "scan SLDPRT scalar binding candidates")?;
        ctx.reserve_collection_vec(&mut values, 1, "collect SLDPRT scalar binding candidates")?;
        values.push(item);
    }
    Ok(values)
}

#[cfg(test)]
mod bindings_tests;
