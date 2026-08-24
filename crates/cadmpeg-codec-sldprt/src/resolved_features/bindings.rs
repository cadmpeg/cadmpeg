//! Pattern, sweep and scalar operand lane binding.

use super::assembly::is_supplemental_config_lane;
use super::axes::{
    canonical_unit_direction, compact_line_reference_directions,
    declared_line_reference_directions, linear_pattern_display_directions,
    typed_linear_pattern_dimensions,
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
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind, SketchInputLink};
use cadmpeg_ir::features::{FeatureDefinition, Length, PathRef, PatternKind, PatternSeed};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchId;
use std::collections::{HashMap, HashSet};

const EPS_BINDINGS_BIND_PATTERN_INPUTS_E12: f64 = 1e-12;
const EPS_BINDINGS_BIND_DETACHED_SPATIAL_RELATION_OBJECTS_E9: f64 = 1e-9;

/// Bind pattern operands carried by adjacent feature-input objects.
pub(crate) fn bind_pattern_inputs(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let model_by_native = model_features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| Some((feature.native_ref.clone()?, index)))
        .collect::<HashMap<_, _>>();
    let mut curve_seed_assignments = Vec::<(usize, cadmpeg_ir::features::FeatureId)>::new();
    let mut curve_path_assignments =
        Vec::<(usize, cadmpeg_ir::features::FeatureId, PathRef)>::new();
    let mut pattern_seed_assignments = Vec::<(usize, cadmpeg_ir::features::FeatureId)>::new();
    let mut linear_direction_assignments = Vec::<(usize, Vector3)>::new();
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
            .map(|native| native.id.clone())
    };

    for lane in lanes {
        let generated_identities = if lane.generated_surface_identities.is_empty() {
            generated_surface_identities(lane)
        } else {
            lane.generated_surface_identities.clone()
        };
        let mut starts = history_features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(offset, _)| *offset);
        for (start_index, (_, feature)) in starts.iter().enumerate() {
            let has_derived_cosmetic_thread_output =
                starts.get(start_index + 1).is_some_and(|(_, candidate)| {
                    candidate.input_class.as_deref() == Some("moDerivedCosmeticThread_c")
                });
            let pattern_object_end = || {
                let next = start_index + 1 + usize::from(has_derived_cosmetic_thread_output);
                starts
                    .get(next)
                    .and_then(|(offset, _)| usize::try_from(*offset).ok())
                    .unwrap_or(lane.native_payload.len())
            };
            if native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                == NativeClassKind::MirrorPattern
            {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                    continue;
                };
                let (needs_plane, needs_seeds) = match &model_features[model_index].definition {
                    FeatureDefinition::Pattern { seeds, pattern, .. } => (
                        matches!(pattern, PatternKind::Unresolved { .. }),
                        seeds.is_empty(),
                    ),
                    _ => continue,
                };
                if !needs_plane && !needs_seeds {
                    continue;
                }
                let start = usize::try_from(starts[start_index].0).ok();
                let end = pattern_object_end();
                let Some(start) = start.filter(|start| *start < end) else {
                    continue;
                };
                let object = &lane.native_payload[start..end];
                if needs_plane {
                    if let Ok(Some((origin, normal, _))) = explicit_reference_plane_frame(object) {
                        mirror_plane_assignments.push((model_index, origin, normal));
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
                            let source =
                                u32::from_le_bytes(component.type_signature[4..8].try_into().ok()?);
                            let mut matches = history_features.iter().filter(|candidate| {
                                candidate
                                    .source_id
                                    .as_deref()
                                    .and_then(|value| value.parse::<u32>().ok())
                                    == Some(source)
                            });
                            let Some(feature) = matches.next() else {
                                continue;
                            };
                            return matches.next().is_none().then(|| feature.id.clone());
                        }
                        None
                    })
                    .filter_map(|native| model_by_native.get(native.as_str()).copied())
                    .filter(|seed_index| *seed_index != model_index)
                    .map(|seed_index| model_features[seed_index].id.clone());
                let mut seeds = Vec::new();
                for seed in seed_candidates {
                    if !seeds.contains(&seed) {
                        seeds.push(seed);
                    }
                }
                if seeds.is_empty() {
                    if has_derived_cosmetic_thread_output {
                        if let Some(seed_index) = derived_cosmetic_thread_seed(feature)
                            .and_then(|native| model_by_native.get(native.as_str()).copied())
                        {
                            mirror_seed_assignments
                                .push((model_index, vec![model_features[seed_index].id.clone()]));
                        }
                    }
                } else {
                    mirror_seed_assignments.push((model_index, seeds));
                }
                continue;
            }
            if feature.input_class.as_deref() == Some("moCirPattern_c") {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                    continue;
                };
                if !matches!(
                    &model_features[model_index].definition,
                    FeatureDefinition::Pattern { seeds, .. } if seeds.is_empty()
                ) {
                    continue;
                }
                let Some(pattern_source) = feature
                    .source_id
                    .as_deref()
                    .and_then(|source| source.parse::<u32>().ok())
                else {
                    continue;
                };
                let Some(start) = usize::try_from(starts[start_index].0)
                    .ok()
                    .filter(|start| *start < pattern_object_end())
                else {
                    continue;
                };
                let end = pattern_object_end();
                let mut seed_candidates = generated_identities
                    .iter()
                    .filter(|identity| {
                        usize::try_from(identity.offset)
                            .ok()
                            .is_some_and(|offset| (start..end).contains(&offset))
                    })
                    .filter(|identity| {
                        identity.components.first().is_some_and(|component| {
                            u32::from_le_bytes(
                                component.type_signature[4..8]
                                    .try_into()
                                    .expect("four-byte pattern source"),
                            ) == pattern_source
                        })
                    })
                    .filter(|identity| {
                        identity.components.last().is_some_and(|component| {
                            u32::from_le_bytes(
                                component.type_signature[4..8]
                                    .try_into()
                                    .expect("four-byte seed source"),
                            ) == identity.feature_source_id
                                && component.local_id == Some(identity.local_identity)
                        })
                    })
                    .filter_map(|identity| {
                        let mut matches = history_features.iter().filter(|candidate| {
                            candidate
                                .source_id
                                .as_deref()
                                .and_then(|source| source.parse::<u32>().ok())
                                == Some(identity.feature_source_id)
                        });
                        let seed = matches.next()?;
                        matches.next().is_none().then(|| seed.id.clone())
                    })
                    .filter_map(|native| model_by_native.get(native.as_str()).copied())
                    .filter(|seed_index| *seed_index != model_index)
                    .map(|seed_index| model_features[seed_index].id.clone())
                    .collect::<Vec<_>>();
                seed_candidates.sort();
                seed_candidates.dedup();
                if let [seed] = seed_candidates.as_slice() {
                    pattern_seed_assignments.push((model_index, seed.clone()));
                }
                continue;
            }
            if feature.input_class.as_deref() == Some("moLPattern_c") {
                let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                    continue;
                };
                let object_start = usize::try_from(starts[start_index].0).ok();
                let end = pattern_object_end();
                if matches!(
                    model_features[model_index].definition,
                    FeatureDefinition::Pattern {
                        pattern: PatternKind::Unresolved {
                            form: Some(cadmpeg_ir::features::PatternForm::Linear)
                        },
                        ..
                    }
                ) {
                    if let Some((spacing, count)) =
                        object_start.filter(|start| *start < end).and_then(|start| {
                            typed_linear_pattern_dimensions(feature, lane, start, end)
                        })
                    {
                        if let FeatureDefinition::Pattern { pattern, .. } =
                            &mut model_features[model_index].definition
                        {
                            *pattern = PatternKind::Linear {
                                direction: None,
                                spacing,
                                count,
                                second: None,
                            };
                        }
                    }
                }
                let (needs_seed, needs_direction) = match &model_features[model_index].definition {
                    FeatureDefinition::Pattern {
                        seeds,
                        pattern: PatternKind::Linear { direction, .. },
                    } => (seeds.is_empty(), direction.is_none()),
                    _ => continue,
                };
                if !needs_seed && !needs_direction {
                    continue;
                }
                if needs_seed {
                    if has_derived_cosmetic_thread_output {
                        if let Some(seed_index) = derived_cosmetic_thread_seed(feature)
                            .and_then(|native| model_by_native.get(native.as_str()).copied())
                        {
                            pattern_seed_assignments
                                .push((model_index, model_features[seed_index].id.clone()));
                        }
                    } else if let Some((_, seed)) = start_index
                        .checked_sub(1)
                        .and_then(|index| starts.get(index))
                    {
                        if let Some(&seed_index) = model_by_native.get(seed.id.as_str()) {
                            pattern_seed_assignments
                                .push((model_index, model_features[seed_index].id.clone()));
                        }
                    }
                }
                if !needs_direction {
                    continue;
                }
                let declarations = lane
                    .classes
                    .iter()
                    .filter(|class| {
                        class.name == "moLineRef_w"
                            && class.offset > starts[start_index].0
                            && usize::try_from(class.offset).is_ok_and(|offset| offset < end)
                    })
                    .collect::<Vec<_>>();
                let mut directions = declarations
                    .iter()
                    .flat_map(|class| {
                        declared_line_reference_directions(&lane.native_payload, class.offset, end)
                    })
                    .collect::<Vec<_>>();
                if let Some(start) = object_start {
                    let excluded_handles = declarations
                        .iter()
                        .filter_map(|class| usize::try_from(class.offset).ok())
                        .flat_map(|offset| [offset + 136, offset + 144])
                        .collect::<Vec<_>>();
                    directions.extend(compact_line_reference_directions(
                        &lane.native_payload,
                        start,
                        end,
                        &excluded_handles,
                    ));
                    if directions.is_empty() {
                        let first_spacing_m = feature
                            .parameters
                            .get("D3")
                            .and_then(|value| {
                                crate::history::parse_positive_dimension_length_mm(value)
                            })
                            .map(|value| value / 1000.0);
                        let second_spacing_m = feature
                            .parameters
                            .get("D4")
                            .and_then(|value| {
                                crate::history::parse_positive_dimension_length_mm(value)
                            })
                            .map(|value| value / 1000.0);
                        directions.extend(linear_pattern_display_directions(
                            &lane.native_payload,
                            start,
                            end,
                            &lane.names,
                            [first_spacing_m, second_spacing_m],
                        ));
                    }
                }
                let mut unique_directions = Vec::new();
                for direction in directions.into_iter().map(canonical_unit_direction) {
                    if !unique_directions.iter().any(|candidate: &Vector3| {
                        let dot = candidate.x * direction.x
                            + candidate.y * direction.y
                            + candidate.z * direction.z;
                        (dot.abs() - 1.0).abs() <= EPS_BINDINGS_BIND_PATTERN_INPUTS_E12
                    }) {
                        unique_directions.push(direction);
                    }
                }
                if matches!(unique_directions.len(), 1 | 2) {
                    linear_direction_assignments.extend(
                        unique_directions
                            .into_iter()
                            .map(|direction| (model_index, direction)),
                    );
                }
                continue;
            }
            if feature.input_class.as_deref() != Some("moCurvePattern_c") {
                continue;
            }
            let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                continue;
            };
            let (needs_seed, needs_path) = match &model_features[model_index].definition {
                FeatureDefinition::Pattern {
                    seeds,
                    pattern: PatternKind::CurveDriven { path, .. },
                    ..
                } => (seeds.is_empty(), path.is_none()),
                _ => continue,
            };
            if needs_seed {
                if let Some((_, seed)) = start_index
                    .checked_sub(1)
                    .and_then(|index| starts.get(index))
                {
                    if let Some(&seed_index) = model_by_native.get(seed.id.as_str()) {
                        curve_seed_assignments
                            .push((model_index, model_features[seed_index].id.clone()));
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
            let FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &model_features[target_index].definition
            else {
                continue;
            };
            curve_path_assignments.push((
                model_index,
                model_features[target_index].id.clone(),
                PathRef::Sketch(sketch.clone()),
            ));
        }
    }
    pattern_seed_assignments.extend(curve_seed_assignments);
    let mut seeds_by_pattern = HashMap::<usize, Vec<cadmpeg_ir::features::FeatureId>>::new();
    for (index, seed) in pattern_seed_assignments {
        let candidates = seeds_by_pattern.entry(index).or_default();
        if !candidates.contains(&seed) {
            candidates.push(seed);
        }
    }
    for (index, candidates) in seeds_by_pattern {
        let [seed] = candidates.as_slice() else {
            continue;
        };
        if !model_features[index].dependencies.contains(seed) {
            model_features[index].dependencies.push(seed.clone());
        }
        if let FeatureDefinition::Pattern { seeds, .. } = &mut model_features[index].definition {
            if seeds.is_empty() {
                seeds.push(PatternSeed::Feature(seed.clone()));
            }
        }
    }
    let mut paths_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, dependency, path) in curve_path_assignments {
        let candidates = paths_by_pattern.entry(index).or_default();
        if !candidates.contains(&(dependency.clone(), path.clone())) {
            candidates.push((dependency, path));
        }
    }
    for (index, candidates) in paths_by_pattern {
        let [(dependency, path)] = candidates.as_slice() else {
            continue;
        };
        if !model_features[index].dependencies.contains(dependency) {
            model_features[index].dependencies.push(dependency.clone());
        }
        if let FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven { path: slot, .. },
            ..
        } = &mut model_features[index].definition
        {
            if slot.is_none() {
                *slot = Some(path.clone());
            }
        }
    }
    let mut linear_directions_by_pattern = HashMap::<usize, Vec<Vector3>>::new();
    for (index, direction) in linear_direction_assignments {
        let candidates = linear_directions_by_pattern.entry(index).or_default();
        if !candidates.contains(&direction) {
            candidates.push(direction);
        }
    }
    for (index, candidates) in linear_directions_by_pattern {
        if let FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                direction, second, ..
            },
            ..
        } = &mut model_features[index].definition
        {
            match candidates.as_slice() {
                [first] if direction.is_none() => *direction = Some(*first),
                [first, second_direction] => {
                    let native = model_features[index]
                        .native_ref
                        .as_deref()
                        .and_then(|native| {
                            history_features.iter().find(|feature| feature.id == native)
                        });
                    let secondary = native.and_then(|feature| {
                        Some(cadmpeg_ir::features::LinearPatternDirection {
                            direction: *second_direction,
                            spacing: Length(feature.parameters.get("D4").and_then(|value| {
                                crate::history::parse_positive_dimension_length_mm(value)
                            })?),
                            count: feature.parameters.get("D2")?.parse::<u32>().ok()?,
                        })
                    });
                    if direction.is_none() && second.is_none() && secondary.is_some() {
                        *direction = Some(*first);
                        *second = secondary;
                    }
                }
                _ => {}
            }
        }
    }
    let mut mirror_planes_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, origin, normal) in mirror_plane_assignments {
        let candidates = mirror_planes_by_pattern.entry(index).or_default();
        if !candidates.contains(&(origin, normal)) {
            candidates.push((origin, normal));
        }
    }
    for (index, candidates) in mirror_planes_by_pattern {
        let [(origin, normal)] = candidates.as_slice() else {
            continue;
        };
        if let FeatureDefinition::Pattern {
            pattern: slot @ PatternKind::Unresolved { .. },
            ..
        } = &mut model_features[index].definition
        {
            *slot = PatternKind::Mirror {
                plane_origin: *origin,
                plane_normal: *normal,
            };
        }
    }
    let mut mirror_seed_sets_by_pattern = HashMap::<usize, Vec<_>>::new();
    for (index, seeds) in mirror_seed_assignments {
        let candidates = mirror_seed_sets_by_pattern.entry(index).or_default();
        if !candidates.contains(&seeds) {
            candidates.push(seeds);
        }
    }
    for (index, candidates) in mirror_seed_sets_by_pattern {
        let [seeds] = candidates.as_slice() else {
            continue;
        };
        for seed in seeds {
            if !model_features[index].dependencies.contains(seed) {
                model_features[index].dependencies.push(seed.clone());
            }
        }
        if let FeatureDefinition::Pattern {
            seeds: seed_slots, ..
        } = &mut model_features[index].definition
        {
            if seed_slots.is_empty() {
                seed_slots.extend(seeds.iter().cloned().map(PatternSeed::Feature));
            }
        }
    }
}

fn mirror_plane_from_surface(geometry: &SurfaceGeometry) -> Option<(Point3, Vector3)> {
    match geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => Some((*origin, normal.unit()?)),
        SurfaceGeometry::Transformed { basis, transform } if transform.is_proper_rigid() => {
            let (origin, normal) = mirror_plane_from_surface(basis)?;
            Some((
                transform.apply_point(origin),
                transform.apply_normal(normal)?,
            ))
        }
        _ => None,
    }
}

/// Bind mirror planes selected by persistent feature-local face identity.
pub(crate) fn bind_mirror_surface_planes(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    face_identities: &[(String, u32, u32)],
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) {
    let mirror_native_refs = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                == NativeClassKind::MirrorPattern
        })
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let mut faces_by_identity = HashMap::<(u32, u32), Vec<&str>>::new();
    for (face, source, local) in face_identities {
        let candidates = faces_by_identity.entry((*source, *local)).or_default();
        if !candidates.contains(&face.as_str()) {
            candidates.push(face);
        }
    }
    let faces_by_id = faces
        .iter()
        .map(|face| (face.id.0.as_str(), face))
        .collect::<HashMap<_, _>>();
    let surfaces_by_id = surfaces
        .iter()
        .map(|surface| (surface.id.0.as_str(), surface))
        .collect::<HashMap<_, _>>();

    for feature in features {
        let FeatureDefinition::Pattern {
            pattern: slot @ PatternKind::Unresolved { .. },
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        if !mirror_native_refs.contains(native_ref) {
            continue;
        }
        let mut candidates = Vec::new();
        for selection in lanes
            .iter()
            .filter(|lane| !is_supplemental_config_lane(lane))
            .flat_map(|lane| &lane.surface_selections)
            .filter(|selection| selection.feature_ref == native_ref)
        {
            let Some(component) = selection.components.last() else {
                continue;
            };
            let source = u32::from_le_bytes(
                component.type_signature[4..8]
                    .try_into()
                    .expect("four-byte feature source ID slice"),
            );
            let Some(local) = component.local_id else {
                continue;
            };
            let Some([face_id]) = faces_by_identity.get(&(source, local)).map(Vec::as_slice) else {
                continue;
            };
            let Some(surface) = faces_by_id
                .get(face_id)
                .and_then(|face| surfaces_by_id.get(face.surface.0.as_str()))
            else {
                continue;
            };
            let Some(plane) = mirror_plane_from_surface(&surface.geometry) else {
                continue;
            };
            if !candidates.contains(&plane) {
                candidates.push(plane);
            }
        }
        let [(origin, normal)] = candidates.as_slice() else {
            continue;
        };
        *slot = PatternKind::Mirror {
            plane_origin: *origin,
            plane_normal: *normal,
        };
    }
}

pub(crate) fn bind_sweep_adjacent_profiles(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let model_by_native = model_features
        .iter()
        .enumerate()
        .filter_map(|(index, feature)| Some((feature.native_ref.as_deref()?, index)))
        .collect::<HashMap<_, _>>();
    let mut assignments = HashMap::<
        usize,
        Vec<(
            cadmpeg_ir::features::FeatureId,
            SketchId,
            Option<(cadmpeg_ir::features::FeatureId, SketchId)>,
        )>,
    >::new();
    for lane in lanes {
        let mut starts = history_features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, (_, feature)) in starts.iter().enumerate() {
            if feature.input_class.as_deref() != Some("moSweep_c") {
                continue;
            }
            let Some(&model_index) = model_by_native.get(feature.id.as_str()) else {
                continue;
            };
            if !matches!(
                model_features[model_index].definition,
                FeatureDefinition::Sweep {
                    section: cadmpeg_ir::features::SweepSection::Unresolved(_),
                    ..
                }
            ) {
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
            let FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &model_features[profile_index].definition
            else {
                continue;
            };
            let path = index.checked_sub(1).and_then(|path_object_index| {
                let (_, path_feature) = starts[path_object_index];
                if path_feature.input_class.as_deref() != Some("moProfileFeature_c") {
                    return None;
                }
                let path_index = *model_by_native.get(path_feature.id.as_str())?;
                let FeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::Planar,
                    sketch: Some(path),
                    ..
                } = &model_features[path_index].definition
                else {
                    return None;
                };
                Some((model_features[path_index].id.clone(), path.clone()))
            });
            let candidate = (
                model_features[profile_index].id.clone(),
                sketch.clone(),
                path,
            );
            let candidates = assignments.entry(model_index).or_default();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    for (index, candidates) in assignments {
        let [(profile_dependency, sketch, path)] = candidates.as_slice() else {
            continue;
        };
        let mut profile_bound = false;
        if let FeatureDefinition::Sweep {
            section,
            path: path_slot,
            ..
        } = &mut model_features[index].definition
        {
            if matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_)) {
                *section = cadmpeg_ir::features::SweepSection::Profile(
                    cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone()),
                );
                profile_bound = true;
            }
            if let Some((_, path)) = path {
                if path_slot
                    .as_ref()
                    .is_none_or(|existing| matches!(existing, PathRef::Native(_)))
                {
                    *path_slot = Some(PathRef::Sketch(path.clone()));
                }
            }
        }
        if profile_bound
            && !model_features[index]
                .dependencies
                .contains(profile_dependency)
        {
            model_features[index]
                .dependencies
                .push(profile_dependency.clone());
        }
        if let Some((path_dependency, _)) = path {
            if !model_features[index].dependencies.contains(path_dependency) {
                model_features[index]
                    .dependencies
                    .push(path_dependency.clone());
            }
        }
    }
}

pub(crate) fn bind_scalar_operands(
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) {
    let represented_sketches = represented_sketch_features(histories, lanes);
    for lane in lanes {
        for entity in &mut lane.sketch_entities {
            entity.feature_ref = None;
            entity.links.clear();
            entity.link_selector = None;
        }
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.as_str(),
                ))
            })
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|start| start.0);
        for (index, &(start, feature_id)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            for entity in lane
                .sketch_entities
                .iter_mut()
                .filter(|entity| entity.offset > start && entity.offset < end)
            {
                entity.feature_ref = Some(feature_id.to_string());
            }
            for reference in lane
                .references
                .iter_mut()
                .filter(|reference| reference.offset > start && reference.offset < end)
            {
                reference.feature_ref = Some(feature_id.to_string());
            }
            for scalar in lane
                .scalars
                .iter_mut()
                .filter(|scalar| scalar.offset > start && scalar.offset < end)
            {
                scalar.feature_ref = Some(feature_id.to_string());
            }
        }
        bind_detached_legacy_sketch_objects(histories, &represented_sketches, lane);
        let features_by_id = histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| (feature.id.as_str(), feature))
            .collect::<HashMap<_, _>>();
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
            if native_object_class(parent.input_class.as_deref().unwrap_or_default()).kind
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
                scalar.feature_ref = Some((*parent_id).to_string());
            }
        }
        finalize_lane_bindings(histories, lane);
    }
}

pub(super) fn finalize_lane_bindings(
    histories: &[crate::records::FeatureHistory],
    lane: &mut FeatureInputLane,
) {
    normalize_indexed_curve_entities(lane);
    let mut marker_ids = HashMap::<(String, u32), Vec<(String, bool)>>::new();
    for entity in &lane.sketch_entities {
        if let (Some(feature), Some(local_id)) = (&entity.feature_ref, entity.local_id) {
            marker_ids
                .entry((feature.clone(), local_id))
                .or_default()
                .push((entity.id.clone(), entity.coordinates_m.is_some()));
        }
    }
    for entity in &mut lane.sketch_entities {
        let Ok(offset) = usize::try_from(entity.offset) else {
            continue;
        };
        let Some((local_ids, selector)) = marker_local_links(&lane.native_payload, offset)
            .map(|(links, selector)| (links.to_vec(), selector))
            .or_else(|| coordinate_marker_local_links(&lane.native_payload, offset))
        else {
            continue;
        };
        let Some(owner) = &entity.feature_ref else {
            continue;
        };
        let links = local_ids
            .into_iter()
            .filter_map(|local_id| {
                let entity_ref = unique_marker_candidate(
                    marker_ids.get(&(owner.clone(), u32::from(local_id)))?,
                )?;
                Some(SketchInputLink {
                    local_id,
                    entity_ref: entity_ref.to_string(),
                })
            })
            .collect::<Vec<_>>();
        if !links.is_empty() {
            entity.links = links;
            entity.link_selector = Some(selector);
        }
    }
    bind_resolved_curve_vertices(lane);
    let entities_by_feature = lane.sketch_entities.iter().fold(
        HashMap::<&str, Vec<&SketchInputEntity>>::new(),
        |mut by_feature, entity| {
            if let Some(feature) = entity.feature_ref.as_deref() {
                by_feature.entry(feature).or_default().push(entity);
            }
            by_feature
        },
    );
    for scalar in &mut lane.scalars {
        let Some(entities) = scalar
            .feature_ref
            .as_deref()
            .and_then(|feature| entities_by_feature.get(feature))
        else {
            continue;
        };
        let resolved = resolve_scalar_operand_markers(entities.iter().copied(), &scalar.operands);
        for (operand, resolved) in scalar.operands.iter_mut().zip(resolved) {
            operand.entity_ref = resolved.map(|entity| entity.id.clone());
        }
    }
    let scalar_owners = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar.feature_ref.clone()))
        .collect::<HashMap<_, _>>();
    for binding in &mut lane.relation_bindings {
        binding.feature_ref = scalar_owners
            .get(binding.scalar_ref.as_str())
            .cloned()
            .flatten();
    }
    let intervals = feature_intervals(histories, lane);
    lane.relation_bindings =
        relation_bindings_scoped(&lane.id, &lane.classes, &lane.scalars, &intervals);
    lane.relation_instances = relation_instances(histories, lane);
    lane.body_selections = compact_body_selections(histories, lane);
    lane.edge_selections = compact_edge_selections(histories, lane);
    lane.surface_selections = compact_surface_selections(histories, lane);
    lane.generated_surface_identities = generated_surface_identities(lane);
}

fn represented_sketch_features(
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> HashSet<String> {
    let features = histories
        .iter()
        .flat_map(|history| &history.features)
        .collect::<Vec<_>>();
    let mut represented = HashSet::new();
    for lane in lanes {
        if is_supplemental_config_lane(lane) {
            continue;
        }
        let mut objects = features
            .iter()
            .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, *feature)))
            .collect::<Vec<_>>();
        objects.sort_unstable_by_key(|(offset, _)| *offset);
        for (index, &(start, feature)) in objects.iter().enumerate() {
            if feature.xml_tag != "Sketch" {
                continue;
            }
            let end = objects.get(index + 1).map_or(u64::MAX, |next| next.0);
            if lane.sketch_entities.iter().any(|entity| {
                entity.offset > start && entity.offset < end && entity.coordinates_m.is_some()
            }) {
                represented.insert(feature.id.clone());
            }
        }
    }
    represented
}

pub(crate) fn bind_unresolved_detached_sketch_objects(
    model_features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) {
    let unresolved = model_features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch { sketch: None, .. } => feature.native_ref.clone(),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let represented = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag == "Sketch" && !unresolved.contains(&feature.id))
        .map(|feature| feature.id.clone())
        .collect::<HashSet<_>>();
    for lane in lanes
        .iter_mut()
        .filter(|lane| is_supplemental_config_lane(lane))
    {
        bind_detached_legacy_sketch_objects(histories, &represented, lane);
        finalize_lane_bindings(histories, lane);
    }
}

pub(super) fn bind_detached_legacy_sketch_objects(
    histories: &[crate::records::FeatureHistory],
    represented: &HashSet<String>,
    lane: &mut FeatureInputLane,
) {
    const OBJECT_GAP: u64 = 4096;

    if !is_supplemental_config_lane(lane) {
        return;
    }
    let limit = lane
        .classes
        .iter()
        .find(|class| class.name == "moFeatureDimHandle_c")
        .map_or_else(
            || u64::try_from(lane.native_payload.len()).unwrap_or(u64::MAX),
            |class| class.offset,
        );
    let relation_bindings = bind_detached_spatial_relation_objects(histories, represented, lane);
    let markers = lane
        .sketch_entities
        .iter()
        .filter(|entity| entity.offset < limit)
        .filter(|entity| {
            relation_bindings
                .iter()
                .all(|(start, end, _)| entity.offset < *start || entity.offset >= *end)
        })
        .map(|entity| entity.offset)
        .collect::<Vec<_>>();
    let Some(&first) = markers.first() else {
        return;
    };
    let mut starts = vec![first];
    starts.extend(
        markers
            .windows(2)
            .filter_map(|pair| (pair[1].saturating_sub(pair[0]) >= OBJECT_GAP).then_some(pair[1])),
    );

    let mut owners = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag == "Sketch")
        .filter(|feature| {
            native_object_class(feature.input_class.as_deref().unwrap_or_default()).kind
                != NativeClassKind::OriginProfileFeature
        })
        .filter(|feature| !represented.contains(&feature.id))
        .filter(|feature| {
            relation_bindings
                .iter()
                .all(|(_, _, owner)| owner != &feature.id)
        })
        .filter_map(|feature| Some((feature.source_id.as_deref()?.parse::<u32>().ok()?, feature)))
        .collect::<Vec<_>>();
    owners.sort_unstable_by_key(|(source, _)| *source);
    if starts.len() != owners.len() {
        return;
    }

    for (index, (&start, (_, owner))) in starts.iter().zip(owners).enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(limit);
        for entity in lane
            .sketch_entities
            .iter_mut()
            .filter(|entity| entity.offset >= start && entity.offset < end)
        {
            entity.feature_ref = Some(owner.id.clone());
        }
        for reference in lane
            .references
            .iter_mut()
            .filter(|reference| reference.offset >= start && reference.offset < end)
        {
            reference.feature_ref = Some(owner.id.clone());
        }
        for scalar in lane
            .scalars
            .iter_mut()
            .filter(|scalar| scalar.offset >= start && scalar.offset < end)
        {
            scalar.feature_ref = Some(owner.id.clone());
        }
    }
}

pub(super) fn spatial_relation_manager_ranges(lane: &FeatureInputLane) -> Vec<(u64, u64)> {
    let mut ranges = lane
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
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

fn bind_detached_spatial_relation_objects(
    histories: &[crate::records::FeatureHistory],
    represented: &HashSet<String>,
    lane: &mut FeatureInputLane,
) -> Vec<(u64, u64, String)> {
    let ranges = spatial_relation_manager_ranges(lane);
    if ranges.is_empty() {
        return Vec::new();
    }
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let is_dimension_name = |name: &str| {
        name.strip_prefix('D').is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    let owners = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.input_class.as_deref() == Some("mo3DProfileFeature_c"))
        .filter(|feature| !represented.contains(&feature.id))
        .filter_map(|feature| {
            let dimensions = feature
                .parameters
                .iter()
                .filter(|(name, _)| is_dimension_name(name))
                .map(|(name, value)| {
                    Some((
                        name.as_str(),
                        crate::history::parse_dimension_length_mm(value)?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            (dimensions.len() >= 3).then_some((feature, dimensions))
        })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for &(start, end) in &ranges {
        let scalars = lane
            .scalars
            .iter()
            .filter(|scalar| scalar.offset > start && scalar.offset < end)
            .filter(|scalar| scalar.role != crate::records::FeatureInputScalarRole::Display)
            .filter_map(|scalar| Some((names.get(scalar.name.as_str()).copied()?, scalar.value)))
            .filter(|(name, _)| is_dimension_name(name))
            .collect::<Vec<_>>();
        let scalar_names = scalars
            .iter()
            .map(|(name, _)| *name)
            .collect::<HashSet<_>>();
        for (owner, dimensions) in &owners {
            let dimension_names = dimensions
                .iter()
                .map(|(name, _)| *name)
                .collect::<HashSet<_>>();
            if scalar_names != dimension_names {
                continue;
            }
            let exact = dimensions.iter().all(|(name, expected_mm)| {
                scalars.iter().any(|(candidate, value_m)| {
                    candidate == name
                        && (value_m * 1000.0 - expected_mm).abs()
                            <= expected_mm.abs().max(1.0)
                                * EPS_BINDINGS_BIND_DETACHED_SPATIAL_RELATION_OBJECTS_E9
                })
            });
            if exact {
                candidates.push((start, end, owner.id.clone()));
            }
        }
    }
    let bound = candidates
        .iter()
        .filter(|(start, end, owner)| {
            candidates
                .iter()
                .filter(|(candidate_start, candidate_end, _)| {
                    candidate_start == start && candidate_end == end
                })
                .count()
                == 1
                && candidates
                    .iter()
                    .filter(|(_, _, candidate_owner)| candidate_owner == owner)
                    .count()
                    == 1
        })
        .cloned()
        .collect::<Vec<_>>();
    for (start, end, owner) in &bound {
        for entity in lane
            .sketch_entities
            .iter_mut()
            .filter(|entity| entity.offset > *start && entity.offset < *end)
        {
            entity.feature_ref = Some(owner.clone());
        }
        for reference in lane
            .references
            .iter_mut()
            .filter(|reference| reference.offset > *start && reference.offset < *end)
        {
            reference.feature_ref = Some(owner.clone());
        }
        for scalar in lane
            .scalars
            .iter_mut()
            .filter(|scalar| scalar.offset > *start && scalar.offset < *end)
        {
            scalar.feature_ref = Some(owner.clone());
        }
    }
    bound
}

pub(super) fn normalize_indexed_curve_entities(lane: &mut FeatureInputLane) {
    let terminal_lines = {
        let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
        markers
            .iter()
            .copied()
            .filter(|curve| {
                legacy_terminal_indexed_profile_line(&lane.native_payload, curve, &markers)
            })
            .map(|curve| curve.id.clone())
            .collect::<HashSet<_>>()
    };
    for marker in &mut lane.sketch_entities {
        if terminal_lines.contains(&marker.id) {
            marker.kind = SketchInputKind::LineOrCircle;
        }
    }
    let endpoints = lane
        .sketch_entities
        .iter()
        .filter_map(|curve| {
            let feature = curve.feature_ref.as_ref()?;
            let offset = usize::try_from(curve.offset).ok()?;
            let indices = wide_indexed_curve_endpoint_indices(&lane.native_payload, offset)
                .or_else(|| compact_indexed_curve_endpoint_indices(&lane.native_payload, offset))
                .or_else(|| {
                    extended_compact_indexed_curve_endpoint_indices(&lane.native_payload, offset)
                })
                .or_else(|| compact_legacy_curve_endpoint_indices(&lane.native_payload, offset))
                .or_else(|| {
                    alternate_current_indexed_curve_endpoint_indices(&lane.native_payload, offset)
                })?;
            Some(indices.map(|index| (feature.clone(), index)))
        })
        .flatten()
        .collect::<HashSet<_>>();
    let linked_endpoint_coordinates = {
        let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
        lane.sketch_entities
            .iter()
            .filter_map(|curve| {
                current_reverse_incidence_endpoint_offsets(&lane.native_payload, curve, &markers)
            })
            .flatten()
            .filter_map(|offset| {
                let native_offset = usize::try_from(offset).ok()?;
                let (coordinates, _) = linked_profile_point(&lane.native_payload, native_offset)?;
                Some((offset, coordinates))
            })
            .collect::<HashMap<_, _>>()
    };
    for marker in &mut lane.sketch_entities {
        let Some(key) = marker.feature_ref.clone().zip(marker.object_index) else {
            continue;
        };
        if marker.coordinates_m.is_none() {
            marker.coordinates_m = linked_endpoint_coordinates.get(&marker.offset).copied();
        }
        if (endpoints.contains(&key) || linked_endpoint_coordinates.contains_key(&marker.offset))
            && marker.coordinates_m.is_some()
        {
            marker.kind = SketchInputKind::Point;
        }
    }
}

pub(super) fn bind_resolved_curve_vertices(lane: &mut FeatureInputLane) {
    let selected_axis_endpoints = {
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
        markers
            .iter()
            .copied()
            .filter(|curve| {
                usize::try_from(curve.offset).ok().is_some_and(|offset| {
                    marker_is_selected_construction_line(&lane.native_payload, offset)
                })
            })
            .flat_map(|curve| {
                marker_curve_endpoint_markers(&lane.native_payload, curve, &markers_by_id, &markers)
            })
            .filter(|marker| marker.coordinates_m.is_some())
            .map(|marker| marker.id.clone())
            .collect::<HashSet<_>>()
    };
    for marker in &mut lane.sketch_entities {
        if selected_axis_endpoints.contains(marker.id.as_str()) {
            marker.kind = SketchInputKind::Point;
        }
    }
    loop {
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
        let mut resolved_curves = HashSet::new();
        let mut resolved_endpoints = HashSet::new();
        for curve in markers.iter().copied().filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        }) {
            let endpoints = marker_curve_endpoint_markers(
                &lane.native_payload,
                curve,
                &markers_by_id,
                &markers,
            );
            if endpoints.len() == 2 {
                resolved_curves.insert(curve.id.clone());
            }
            resolved_endpoints.extend(
                endpoints
                    .into_iter()
                    .filter(|marker| marker.coordinates_m.is_some())
                    .map(|marker| marker.id.clone()),
            );
        }
        let mut changed = false;
        for marker in &mut lane.sketch_entities {
            if marker.kind != SketchInputKind::Point
                && resolved_endpoints.contains(marker.id.as_str())
                && !resolved_curves.contains(marker.id.as_str())
            {
                marker.kind = SketchInputKind::Point;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod bindings_tests;
