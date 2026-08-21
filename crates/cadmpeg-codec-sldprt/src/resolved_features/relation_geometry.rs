//! Relation point and solved geometry projection.

use super::curves::slot_curve_and_center_indices;
use super::endpoints::{inferred_point_coordinates_by_index, legacy_undetailed_profile_line};
use super::markers::marker_is_geometry_locus;
use super::names::operand_kind_name;
use super::operands::{
    coordinate_line_endpoints_with_linked_point, linked_coordinate_line_endpoints,
};
use super::relation_loci::{
    line_line_angle, line_line_distance, marker_point_locus,
    marker_transform_candidates_by_feature, point_line_distance_value, profile_loci_by_marker,
    profile_locus_point, relation_constraint_is_inactive, relation_operand_marker,
    same_dimension_angle, same_dimension_length, typed_relation_definition,
    unoriented_line_line_angle,
};
use super::relation_records::{
    circle_dimension_handle_driver, relation_uses_dynamic_operands, relation_uses_solver_points,
};
use super::transforms::{
    marker_entities, quantize, sketch_entity_loci, sketch_frame_marker_transform,
};
use super::typed_relations::{
    current_undetailed_bounded_curve_is_line, marker_curve_endpoint_markers,
    marker_relation_is_inactive, typed_marker_relation_definition_in_sketch,
};
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
    SketchRelationKind,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity, SketchEntityId,
    SketchGeometry, SketchNativeOperand,
};
use std::collections::{HashMap, HashSet};

/// Materialize relation-addressed point geometry omitted from selected profile streams.
pub(crate) fn project_relation_point_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let point_operands = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .flat_map(|relation| {
            let count = match relation.family {
                FeatureInputRelationFamily::PointPointDistance
                | FeatureInputRelationFamily::PointPointHorizontalDistance
                | FeatureInputRelationFamily::PointPointVerticalDistance => 2,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => 0,
            };
            relation
                .operands
                .iter()
                .take(count)
                .filter_map(|operand| operand.entity_ref.as_deref())
        })
        .collect::<HashSet<_>>();
    let curve_operands = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .flat_map(|relation| {
            let first = match relation.family {
                FeatureInputRelationFamily::LineLineDistance
                | FeatureInputRelationFamily::Angle => 0,
                FeatureInputRelationFamily::PointLineDistance => 1,
                _ => relation.operands.len(),
            };
            relation
                .operands
                .iter()
                .skip(first)
                .filter_map(|operand| operand.entity_ref.as_deref())
        })
        .collect::<HashSet<_>>();
    let mut referenced = lanes
        .iter()
        .flat_map(|lane| {
            lane.relation_instances
                .iter()
                .flat_map(|relation| &relation.operands)
                .filter_map(|operand| operand.entity_ref.as_deref())
                .chain(
                    lane.sketch_entities
                        .iter()
                        .filter(|marker| matches!(marker.kind, SketchInputKind::Relation(_)))
                        .map(|marker| marker.id.as_str()),
                )
        })
        .collect::<HashSet<_>>();
    loop {
        let mut linked = Vec::new();
        for marker in markers_by_id.values().copied() {
            let marker_referenced = referenced.contains(marker.id.as_str());
            for link in &marker.links {
                let adjacent = if marker_referenced {
                    Some(link.entity_ref.as_str())
                } else if referenced.contains(link.entity_ref.as_str()) {
                    Some(marker.id.as_str())
                } else {
                    None
                };
                if let Some(id) = adjacent.filter(|id| !referenced.contains(id)) {
                    linked.push(id);
                }
            }
        }
        if linked.is_empty() {
            break;
        }
        referenced.extend(linked);
    }
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for marker in &lane.sketch_entities {
            let qualified_point = point_operands.contains(marker.id.as_str());
            let has_existing_point = entities.iter().any(|entity| {
                (entity.native_ref.as_deref() == Some(marker.id.as_str())
                    || entity.geometry_ref.as_deref() == Some(marker.id.as_str()))
                    && matches!(entity.geometry, SketchGeometry::Point { .. })
            });
            if !referenced.contains(marker.id.as_str())
                || !(qualified_point
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point
                            | SketchInputKind::ConstrainedPoint
                            | SketchInputKind::LineOrCircle
                            | SketchInputKind::Arc
                    )
                    || matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    ))
                || has_existing_point
                || entities.iter().any(|entity| {
                    entity
                        .endpoint_refs
                        .iter()
                        .any(|reference| reference == &marker.id)
                })
            {
                continue;
            }
            let (Some(feature), Some([u, v])) =
                (marker.feature_ref.as_deref(), marker.coordinates_m)
            else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            if sketch.0.contains("sketch#compact:")
                && !marker_is_geometry_locus(&lane.native_payload, marker.offset as usize)
                && !entities.iter().any(|entity| {
                    entity
                        .endpoint_refs
                        .iter()
                        .any(|reference| reference == &marker.id)
                })
            {
                continue;
            }
            let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
            let positions = transforms
                .get(feature)
                .into_iter()
                .flatten()
                .filter_map(|transform| transform.apply(native))
                .collect::<HashSet<_>>();
            let positions = if positions.len() == 1 {
                positions
            } else {
                sketches
                    .iter()
                    .find(|candidate| candidate.id == *sketch)
                    .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                    .and_then(|transform| transform.apply(native))
                    .map(|position| HashSet::from([position]))
                    .unwrap_or(positions)
            };
            if positions.len() != 1 {
                continue;
            }
            let position = positions
                .into_iter()
                .next()
                .expect("one transformed position");
            let position = Point2::new(position.0 as f64 * QUANTUM, position.1 as f64 * QUANTUM);
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#relation-point:{lane_key}:{}",
                    marker.offset
                )),
                sketch: sketch.clone(),
                construction: true,
                native_ref: matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                .then(|| marker.id.clone()),
                geometry_ref: qualified_point.then(|| marker.id.clone()).filter(|_| {
                    matches!(
                        marker.kind,
                        SketchInputKind::LineOrCircle | SketchInputKind::Arc
                    )
                }),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point { position },
            });
        }
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        let marker_roster = lane.sketch_entities.iter().collect::<Vec<_>>();
        for marker in &lane.sketch_entities {
            let marker_offset = usize::try_from(marker.offset).ok();
            let undetailed_arc_line = marker.kind == SketchInputKind::Arc
                && marker_offset.is_some_and(|offset| {
                    current_undetailed_bounded_curve_is_line(&lane.native_payload, offset)
                        || legacy_undetailed_profile_line(&lane.native_payload, offset)
                });
            let self_linked_curve_handle = curve_operands.contains(marker.id.as_str())
                && marker.coordinates_m.is_some()
                && marker.links.iter().any(|link| link.entity_ref == marker.id)
                && marker
                    .links
                    .iter()
                    .filter(|link| link.entity_ref != marker.id)
                    .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()))
                    .filter(|linked| linked.coordinates_m.is_some())
                    .count()
                    == 1;
            let linked_curve_handle = curve_operands.contains(marker.id.as_str())
                && !marker.links.iter().any(|link| link.entity_ref == marker.id)
                && (linked_coordinate_line_endpoints(marker, &markers_by_id).is_some()
                    || coordinate_line_endpoints_with_linked_point(marker, &markers_by_id)
                        .is_some());
            if !referenced.contains(marker.id.as_str())
                || !(marker.kind == SketchInputKind::LineOrCircle
                    || undetailed_arc_line
                    || self_linked_curve_handle
                    || linked_curve_handle)
                || entities
                    .iter()
                    .any(|entity| entity.native_ref.as_deref() == Some(marker.id.as_str()))
            {
                continue;
            }
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let mut endpoints = marker_curve_endpoint_markers(
                &lane.native_payload,
                marker,
                &markers_by_id,
                &marker_roster,
            );
            if endpoints.len() != 2 && linked_curve_handle {
                endpoints = linked_coordinate_line_endpoints(marker, &markers_by_id)
                    .or_else(|| coordinate_line_endpoints_with_linked_point(marker, &markers_by_id))
                    .into_iter()
                    .flatten()
                    .collect();
            }
            if endpoints.len() != 2 {
                endpoints = self_linked_curve_handle
                    .then_some(marker)
                    .into_iter()
                    .chain(
                        marker
                            .links
                            .iter()
                            .filter_map(|link| markers_by_id.get(link.entity_ref.as_str()).copied())
                            .filter(|endpoint| endpoint.id != marker.id)
                            .filter(|endpoint| {
                                endpoint.feature_ref == marker.feature_ref
                                    && endpoint.coordinates_m.is_some()
                                    && entities.iter().any(|entity| {
                                        entity.sketch == *sketch
                                            && matches!(
                                                entity.geometry,
                                                SketchGeometry::Point { .. }
                                            )
                                            && (entity.native_ref.as_deref()
                                                == Some(endpoint.id.as_str())
                                                || entity.geometry_ref.as_deref()
                                                    == Some(endpoint.id.as_str()))
                                    })
                            }),
                    )
                    .collect::<Vec<_>>();
                endpoints.sort_unstable_by_key(|endpoint| endpoint.offset);
                endpoints.dedup_by_key(|endpoint| endpoint.id.as_str());
            }
            let [first_marker, second_marker] = endpoints.as_slice() else {
                continue;
            };
            let (Some(first), Some(second)) =
                (first_marker.coordinates_m, second_marker.coordinates_m)
            else {
                continue;
            };
            let first_native = quantize(
                Point2::new(first[0] * NATIVE_TO_IR, first[1] * NATIVE_TO_IR),
                QUANTUM,
            );
            let second_native = quantize(
                Point2::new(second[0] * NATIVE_TO_IR, second[1] * NATIVE_TO_IR),
                QUANTUM,
            );
            let candidates = transforms
                .get(feature)
                .into_iter()
                .flatten()
                .filter_map(|transform| {
                    Some((
                        transform.apply(first_native)?,
                        transform.apply(second_native)?,
                    ))
                })
                .collect::<HashSet<_>>();
            let candidates = candidates.into_iter().collect::<Vec<_>>();
            let [(start, end)] = candidates.as_slice() else {
                continue;
            };
            if start == end {
                continue;
            }
            let start = Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM);
            let end = Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM);
            let already_present = entities.iter().any(|entity| {
                entity.sketch == *sketch
                    && matches!(&entity.geometry, SketchGeometry::Line { start: existing_start, end: existing_end }
                        if (quantize(*existing_start, QUANTUM) == quantize(start, QUANTUM)
                            && quantize(*existing_end, QUANTUM) == quantize(end, QUANTUM))
                            || (quantize(*existing_start, QUANTUM) == quantize(end, QUANTUM)
                                && quantize(*existing_end, QUANTUM) == quantize(start, QUANTUM)))
            });
            if already_present {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#relation-line:{lane_key}:{}",
                    marker.offset
                )),
                sketch: sketch.clone(),
                construction: true,
                native_ref: (!matches!(marker.kind, SketchInputKind::Relation(_)))
                    .then(|| marker.id.clone()),
                geometry_ref: matches!(marker.kind, SketchInputKind::Relation(_))
                    .then(|| marker.id.clone()),
                endpoint_refs: vec![first_marker.id.clone(), second_marker.id.clone()],
                geometry: SketchGeometry::Line { start, end },
            });
        }
    }
}

pub(super) fn relation_operand_geometry_ref(
    relation: &FeatureInputRelationInstance,
    operand_index: usize,
) -> String {
    format!("{}:operand:{operand_index}", relation.id)
}

pub(super) fn solver_line_geometry_ref(feature: &str, index: u16) -> String {
    format!("{feature}:solver-line:{index}")
}

pub(super) fn is_solver_line_operand(kind: FeatureInputOperandKind) -> bool {
    matches!(
        kind,
        FeatureInputOperandKind::E1 | FeatureInputOperandKind::Native(0x81e7)
    )
}

pub(super) fn relation_uses_solver_line_operand(
    relation: &FeatureInputRelationInstance,
    index: usize,
) -> bool {
    let Some(operand) = relation.operands.get(index) else {
        return false;
    };
    is_solver_line_operand(operand.kind)
        || (relation_uses_dynamic_operands(relation)
            && matches!(
                (relation.family, index),
                (
                    FeatureInputRelationFamily::LineLineDistance
                        | FeatureInputRelationFamily::Angle,
                    0 | 1
                ) | (FeatureInputRelationFamily::PointLineDistance, 1)
            ))
}

pub(crate) fn project_relation_solved_line_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let parameters_by_id = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();

    for lane in lanes {
        for relation in &lane.relation_instances {
            let [first_operand, second_operand] = relation.operands.as_slice() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let direct_line_reference = |operand: &FeatureInputOperand| {
                let Some(entity_ref) = operand.entity_ref.as_deref() else {
                    return false;
                };
                let mut matches = entities.iter().filter(|entity| {
                    entity.sketch == *sketch
                        && entity.native_ref.as_deref() == Some(entity_ref)
                        && matches!(entity.geometry, SketchGeometry::Line { .. })
                });
                matches.next().is_some() && matches.next().is_none()
            };
            let line_operands = match relation.family {
                FeatureInputRelationFamily::LineLineDistance
                    if relation_uses_solver_line_operand(relation, 0)
                        && relation_uses_solver_line_operand(relation, 1)
                        && first_operand.entity_index != second_operand.entity_index =>
                {
                    [first_operand, second_operand]
                        .into_iter()
                        .filter(|operand| !direct_line_reference(operand))
                        .collect::<Vec<_>>()
                }
                FeatureInputRelationFamily::PointLineDistance
                    if relation_uses_solver_line_operand(relation, 1)
                        && !direct_line_reference(second_operand) =>
                {
                    vec![second_operand]
                }
                FeatureInputRelationFamily::Angle
                    if relation_uses_solver_line_operand(relation, 0)
                        && relation_uses_solver_line_operand(relation, 1)
                        && first_operand.entity_index != second_operand.entity_index =>
                {
                    [first_operand, second_operand]
                        .into_iter()
                        .filter(|operand| !direct_line_reference(operand))
                        .collect::<Vec<_>>()
                }
                _ => continue,
            };
            if line_operands.is_empty() {
                continue;
            }
            let Some(parameter_value) = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|parameter| parameters_by_id.get(parameter))
                .and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let expected = match (relation.family, parameter_value) {
                (
                    FeatureInputRelationFamily::LineLineDistance
                    | FeatureInputRelationFamily::PointLineDistance,
                    cadmpeg_ir::features::ParameterValue::Length(expected),
                ) => expected.0,
                (
                    FeatureInputRelationFamily::Angle,
                    cadmpeg_ir::features::ParameterValue::Angle(expected),
                ) => expected.0,
                _ => continue,
            };
            if !expected.is_finite() || expected < 0.0 {
                continue;
            }
            let mut points = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    marker.feature_ref.as_deref() == Some(relation.feature_ref.as_str())
                        && marker.coordinates_m.is_some()
                        && matches!(
                            marker.kind,
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                })
                .collect::<Vec<_>>();
            points.sort_by_key(|marker| marker.offset);
            let line_markers = |index: u16| {
                let pair = usize::from(index).checked_mul(2)?;
                Some([*points.get(pair)?, *points.get(pair + 1)?])
            };
            let point_marker = (relation.family == FeatureInputRelationFamily::PointLineDistance)
                .then(|| {
                    first_operand
                        .entity_ref
                        .as_deref()
                        .and_then(|id| lane.sketch_entities.iter().find(|marker| marker.id == id))
                        .or_else(|| {
                            relation_operand_marker(relation, 0, sketch, &markers_by_id).and_then(
                                |id| lane.sketch_entities.iter().find(|marker| marker.id == id),
                            )
                        })
                        .or_else(|| {
                            (first_operand.kind == FeatureInputOperandKind::Native(0x81dd))
                                .then(|| {
                                    points.get(usize::from(first_operand.entity_index)).copied()
                                })
                                .flatten()
                        })
                })
                .flatten();
            let point_position = point_marker.and_then(|marker| {
                let resolved = entities
                    .iter()
                    .find(|entity| {
                        entity.sketch == *sketch
                            && entity.native_ref.as_deref() == Some(marker.id.as_str())
                    })
                    .and_then(|entity| match &entity.geometry {
                        SketchGeometry::Point { position } => Some(*position),
                        _ => None,
                    });
                if resolved.is_some() {
                    return resolved;
                }
                let [u, v] = marker.coordinates_m?;
                let native = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let candidates = transforms
                    .get(relation.feature_ref.as_str())
                    .into_iter()
                    .flatten()
                    .filter_map(|transform| transform.apply(native))
                    .collect::<HashSet<_>>();
                let candidates = if candidates.len() == 1 {
                    candidates
                } else {
                    sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                        .and_then(|transform| transform.apply(native))
                        .map(|position| HashSet::from([position]))
                        .unwrap_or(candidates)
                };
                let candidates = candidates.into_iter().collect::<Vec<_>>();
                let [position] = candidates.as_slice() else {
                    return None;
                };
                Some(Point2::new(
                    position.0 as f64 * QUANTUM,
                    position.1 as f64 * QUANTUM,
                ))
            });
            let candidate = |id: &str, start, end| SketchEntity {
                id: SketchEntityId(id.into()),
                sketch: sketch.clone(),
                construction: true,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line { start, end },
            };
            let transformed_line = |markers: [&SketchInputEntity; 2]| {
                let native = markers.map(|marker| {
                    let [u, v] = marker
                        .coordinates_m
                        .expect("coordinate-bearing roster points carry coordinates");
                    quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM)
                });
                let candidates = transforms
                    .get(relation.feature_ref.as_str())
                    .into_iter()
                    .flatten()
                    .filter_map(|transform| {
                        Some((transform.apply(native[0])?, transform.apply(native[1])?))
                    })
                    .filter(|(start, end)| start != end)
                    .fold(Vec::new(), |mut candidates, candidate| {
                        if !candidates.contains(&candidate) {
                            candidates.push(candidate);
                        }
                        candidates
                    });
                let candidates = if relation.family == FeatureInputRelationFamily::PointLineDistance
                {
                    let mut candidates = candidates
                        .into_iter()
                        .filter(|(start, end)| {
                            let line = candidate(
                                "solver-line",
                                Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM),
                                Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM),
                            );
                            point_position.is_some_and(|point| {
                                point_line_distance_value(point, &line).is_some_and(|measured| {
                                    same_dimension_length(measured, expected)
                                })
                            })
                        })
                        .collect::<Vec<_>>();
                    let &(first_start, first_end) = candidates.first()?;
                    let orientation_is_ambiguous = candidates
                        .iter()
                        .any(|(start, end)| *start == first_end && *end == first_start);
                    if candidates.iter().all(|(start, end)| {
                        (*start == first_start && *end == first_end)
                            || (*start == first_end && *end == first_start)
                    }) {
                        let representative = if orientation_is_ambiguous {
                            if first_start <= first_end {
                                (first_start, first_end)
                            } else {
                                (first_end, first_start)
                            }
                        } else {
                            (first_start, first_end)
                        };
                        candidates.truncate(1);
                        candidates[0] = representative;
                    } else {
                        return None;
                    }
                    candidates
                } else {
                    candidates
                };
                let [(start, end)] = candidates.as_slice() else {
                    return None;
                };
                Some((
                    Point2::new(start.0 as f64 * QUANTUM, start.1 as f64 * QUANTUM),
                    Point2::new(end.0 as f64 * QUANTUM, end.1 as f64 * QUANTUM),
                ))
            };
            let mut lines = Vec::with_capacity(line_operands.len());
            for operand in line_operands {
                let Some(markers) = line_markers(operand.entity_index) else {
                    lines.clear();
                    break;
                };
                let Some((start, end)) = transformed_line(markers) else {
                    lines.clear();
                    break;
                };
                lines.push((operand, markers, candidate("solver-line", start, end)));
            }
            if lines.is_empty() {
                continue;
            }
            let valid = match relation.family {
                FeatureInputRelationFamily::LineLineDistance => match lines.as_slice() {
                    [(_, _, first), (_, _, second)] => line_line_distance(first, second)
                        .is_some_and(|measured| same_dimension_length(measured, expected)),
                    _ => false,
                },
                FeatureInputRelationFamily::PointLineDistance => match lines.as_slice() {
                    [(_, _, line)] => point_position.is_some_and(|point| {
                        point_line_distance_value(point, line)
                            .is_some_and(|measured| same_dimension_length(measured, expected))
                    }),
                    _ => false,
                },
                FeatureInputRelationFamily::Angle => match lines.as_slice() {
                    [(_, _, first), (_, _, second)] => {
                        let angle = if relation_uses_dynamic_operands(relation) {
                            unoriented_line_line_angle(first, second)
                        } else {
                            line_line_angle(first, second)
                        };
                        angle.is_some_and(|measured| same_dimension_angle(measured, expected))
                    }
                    _ => false,
                },
                _ => false,
            };
            if !valid {
                if relation.family == FeatureInputRelationFamily::LineLineDistance
                    && relation_uses_dynamic_operands(relation)
                {
                    let generated = lines
                        .iter()
                        .map(|(_, _, line)| line.clone())
                        .collect::<Vec<_>>();
                    if let Some([first, second]) =
                        unique_dynamic_line_pair(expected, sketch, entities, &generated, QUANTUM)
                    {
                        let selected = [first, second];
                        let aliases_match =
                            relation
                                .operands
                                .iter()
                                .zip(selected.iter())
                                .all(|(operand, line)| {
                                    let geometry_ref = solver_line_geometry_ref(
                                        &relation.feature_ref,
                                        operand.entity_index,
                                    );
                                    entities
                                        .iter()
                                        .filter(|entity| {
                                            entity.sketch == *sketch
                                                && entity.geometry_ref.as_deref()
                                                    == Some(geometry_ref.as_str())
                                        })
                                        .all(|entity| {
                                            dynamic_line_geometry_key(entity, QUANTUM)
                                                == dynamic_line_geometry_key(line, QUANTUM)
                                        })
                                });
                        if !aliases_match {
                            continue;
                        }
                        let feature_key = relation
                            .feature_ref
                            .rsplit_once('#')
                            .map_or(relation.feature_ref.as_str(), |(_, key)| key);
                        for (operand, line) in relation.operands.iter().zip(selected) {
                            let geometry_ref = solver_line_geometry_ref(
                                &relation.feature_ref,
                                operand.entity_index,
                            );
                            if entities.iter().any(|entity| {
                                entity.sketch == *sketch
                                    && entity.geometry_ref.as_deref() == Some(geometry_ref.as_str())
                            }) {
                                continue;
                            }
                            entities.push(SketchEntity {
                                id: SketchEntityId(format!(
                                    "sldprt:model:sketch-entity#solver-line:{feature_key}:{}",
                                    operand.entity_index
                                )),
                                construction: true,
                                native_ref: None,
                                geometry_ref: Some(geometry_ref),
                                ..line
                            });
                        }
                        continue;
                    }
                }
                continue;
            }
            let feature_key = relation
                .feature_ref
                .rsplit_once('#')
                .map_or(relation.feature_ref.as_str(), |(_, key)| key);
            for (operand, markers, line) in lines {
                let geometry_ref =
                    solver_line_geometry_ref(&relation.feature_ref, operand.entity_index);
                if entities.iter().any(|entity| {
                    entity.sketch == *sketch
                        && entity.geometry_ref.as_deref() == Some(geometry_ref.as_str())
                }) {
                    continue;
                }
                entities.push(SketchEntity {
                    id: SketchEntityId(format!(
                        "sldprt:model:sketch-entity#solver-line:{feature_key}:{}",
                        operand.entity_index
                    )),
                    geometry_ref: Some(geometry_ref),
                    endpoint_refs: markers.map(|marker| marker.id.clone()).into(),
                    ..line
                });
            }
        }
    }
}

fn unique_dynamic_line_pair(
    expected: f64,
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &[SketchEntity],
    generated: &[SketchEntity],
    quantum: f64,
) -> Option<[SketchEntity; 2]> {
    if generated.len() != 2 {
        return None;
    }
    let mut candidates = Vec::<([(i64, i64); 2], SketchEntity)>::new();
    for entity in generated.iter().chain(entities.iter()) {
        if entity.sketch != *sketch || !matches!(entity.geometry, SketchGeometry::Line { .. }) {
            continue;
        }
        let Some(key) = dynamic_line_geometry_key(entity, quantum) else {
            continue;
        };
        if candidates.iter().any(|(candidate, _)| *candidate == key) {
            continue;
        }
        candidates.push((key, entity.clone()));
    }
    let mut matches = Vec::new();
    for (first_index, (first_key, first)) in candidates.iter().enumerate() {
        for (second_key, second) in candidates.iter().skip(first_index + 1) {
            if line_line_distance(first, second)
                .is_some_and(|measured| same_dimension_length(measured, expected))
            {
                let mut pair_key = [*first_key, *second_key];
                pair_key.sort_unstable();
                matches.push((pair_key, [first.clone(), second.clone()]));
            }
        }
    }
    matches.sort_by_key(|(key, _)| *key);
    matches.dedup_by(|(left, _), (right, _)| left == right);
    let [(_, pair)] = matches.as_slice() else {
        return None;
    };
    let first_key = dynamic_line_geometry_key(&generated[0], quantum)?;
    let second_key = dynamic_line_geometry_key(&generated[1], quantum)?;
    if dynamic_line_geometry_key(&pair[0], quantum) == Some(first_key) {
        return Some(pair.clone());
    }
    if dynamic_line_geometry_key(&pair[1], quantum) == Some(first_key) {
        return Some([pair[1].clone(), pair[0].clone()]);
    }
    if dynamic_line_geometry_key(&pair[0], quantum) == Some(second_key) {
        return Some([pair[1].clone(), pair[0].clone()]);
    }
    if dynamic_line_geometry_key(&pair[1], quantum) == Some(second_key) {
        return Some(pair.clone());
    }
    Some(pair.clone())
}

fn dynamic_line_geometry_key(entity: &SketchEntity, quantum: f64) -> Option<[(i64, i64); 2]> {
    let SketchGeometry::Line { start, end } = &entity.geometry else {
        return None;
    };
    let mut endpoints = [quantize(*start, quantum), quantize(*end, quantum)];
    endpoints.sort_unstable();
    Some(endpoints)
}

pub(crate) fn project_relation_solved_point_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let parameters_by_id = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = profile_loci_by_marker(features, sketches, entities, lanes);

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if !matches!(
                relation.family,
                FeatureInputRelationFamily::PointPointDistance
                    | FeatureInputRelationFamily::PointPointHorizontalDistance
                    | FeatureInputRelationFamily::PointPointVerticalDistance
            ) || relation.operands.len() != 2
            {
                continue;
            }
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|parameter| parameters_by_id.get(parameter))
                .copied();
            let Some(cadmpeg_ir::features::ParameterValue::Length(distance)) =
                parameter.and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            if relation_uses_solver_points(relation) {
                let coordinates_by_index =
                    inferred_point_coordinates_by_index(lane, relation.feature_ref.as_str());
                let mut resolved_positions = Vec::with_capacity(relation.operands.len());
                for (index, operand) in relation.operands.iter().enumerate() {
                    let geometry_ref = relation_operand_geometry_ref(relation, index);
                    if entities
                        .iter()
                        .any(|entity| entity.geometry_ref.as_deref() == Some(geometry_ref.as_str()))
                    {
                        resolved_positions.push(None);
                        continue;
                    }
                    let Some(coordinates) = coordinates_by_index
                        .get(&u32::from(operand.entity_index))
                        .copied()
                    else {
                        resolved_positions.clear();
                        break;
                    };
                    let native = quantize(
                        Point2::new(coordinates[0] * NATIVE_TO_IR, coordinates[1] * NATIVE_TO_IR),
                        QUANTUM,
                    );
                    let mut transformed_positions = transforms
                        .get(relation.feature_ref.as_str())
                        .into_iter()
                        .flatten()
                        .filter_map(|transform| transform.apply(native))
                        .collect::<HashSet<_>>();
                    if transformed_positions.len() != 1 {
                        if let Some(position) = sketches
                            .iter()
                            .find(|candidate| candidate.id == *sketch)
                            .and_then(|sketch| sketch_frame_marker_transform(sketch, QUANTUM))
                            .and_then(|transform| transform.apply(native))
                        {
                            transformed_positions = HashSet::from([position]);
                        }
                    }
                    let transformed_positions =
                        transformed_positions.into_iter().collect::<Vec<_>>();
                    let [position] = transformed_positions.as_slice() else {
                        resolved_positions.clear();
                        break;
                    };
                    resolved_positions.push(Some(*position));
                }
                if resolved_positions.len() != relation.operands.len() {
                    continue;
                }
                for (index, position) in resolved_positions
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, position)| position.map(|position| (index, position)))
                {
                    let geometry_ref = relation_operand_geometry_ref(relation, index);
                    entities.push(SketchEntity {
                        id: SketchEntityId(format!(
                            "sldprt:model:sketch-entity#solver-point:{lane_key}:{}:{index}",
                            relation.offset
                        )),
                        sketch: sketch.clone(),
                        construction: true,
                        native_ref: None,
                        geometry_ref: Some(geometry_ref),
                        endpoint_refs: Vec::new(),
                        geometry: SketchGeometry::Point {
                            position: Point2::new(
                                position.0 as f64 * QUANTUM,
                                position.1 as f64 * QUANTUM,
                            ),
                        },
                    });
                }
                continue;
            }
            let resolved = [0, 1].map(|index| {
                relation.operands[index]
                    .entity_ref
                    .as_deref()
                    .and_then(|marker| marker_point_locus(marker, &markers_by_id, &loci_by_marker))
            });
            let (known, missing_index) = match resolved {
                [Some(known), None] => (known, 1),
                [None, Some(known)] => (known, 0),
                _ => continue,
            };
            let Some(missing_marker) = relation.operands[missing_index]
                .entity_ref
                .as_deref()
                .and_then(|marker| markers_by_id.get(marker).copied())
            else {
                continue;
            };
            if missing_marker.coordinates_m.is_some()
                || !matches!(
                    missing_marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            {
                continue;
            }
            let Some(known_point) = profile_locus_point(&known, entities) else {
                continue;
            };
            let mut candidates = entities
                .iter()
                .filter(|entity| entity.sketch == *sketch)
                .flat_map(sketch_entity_loci)
                .filter_map(|(point, _)| {
                    let measured = match relation.family {
                        FeatureInputRelationFamily::PointPointDistance => {
                            (point.u - known_point.u).hypot(point.v - known_point.v)
                        }
                        FeatureInputRelationFamily::PointPointHorizontalDistance => {
                            (point.u - known_point.u).abs()
                        }
                        FeatureInputRelationFamily::PointPointVerticalDistance => {
                            (point.v - known_point.v).abs()
                        }
                        _ => unreachable!("relation family was filtered above"),
                    };
                    same_dimension_length(measured, distance.0).then_some(quantize(point, QUANTUM))
                })
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            candidates.dedup();
            let [(u, v)] = candidates.as_slice() else {
                continue;
            };
            let geometry_ref = relation_operand_geometry_ref(relation, missing_index);
            if entities
                .iter()
                .any(|entity| entity.geometry_ref.as_deref() == Some(geometry_ref.as_str()))
            {
                continue;
            }
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}:{missing_index}",
                    relation.offset
                )),
                sketch: sketch.clone(),
                construction: true,
                native_ref: None,
                geometry_ref: Some(geometry_ref),
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM),
                },
            });
        }
    }
}

pub(super) fn implicit_circle_marker<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand_kind: FeatureInputOperandKind,
    index: u16,
    expected_radius: f64,
) -> Option<(&'a SketchInputEntity, f64)> {
    // CircleDiameter selects the semantic family; native operand tags are only
    // carrier kinds and must not narrow the geometric witness search.
    if !matches!(operand_kind, FeatureInputOperandKind::Native(_))
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return None;
    }
    let relation_index = u32::from(index).checked_add(1)?;
    let mut candidates = lanes
        .iter()
        .filter_map(|lane| {
            let relation = lane.sketch_entities.iter().find(|marker| {
                marker.feature_ref.as_deref() == Some(feature)
                    && marker.object_index == Some(relation_index)
                    && marker.kind == SketchInputKind::Relation(SketchRelationKind::Distance)
                    && matches!(marker.links.as_slice(), [first, second]
                        if first.entity_ref == second.entity_ref
                            && first.local_id == second.local_id)
            })?;
            let center_id = relation.links.first()?.entity_ref.as_str();
            let center = lane
                .sketch_entities
                .iter()
                .find(|marker| marker.id == center_id && marker.coordinates_m.is_some())?;
            let radial = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    marker.feature_ref.as_deref() == Some(feature)
                        && marker.offset > center.offset
                        && marker.coordinates_m.is_some()
                })
                .min_by_key(|marker| marker.offset)?;
            let [cu, cv] = center.coordinates_m?;
            let [ru, rv] = radial.coordinates_m?;
            let radius = (ru - cu).hypot(rv - cv) * 1000.0;
            same_dimension_length(radius, expected_radius).then_some((center, radius))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(center, _)| center.id.as_str());
    candidates
        .dedup_by(|left, right| left.0.id == right.0.id && left.1.to_bits() == right.1.to_bits());
    if let [candidate] = candidates.as_slice() {
        return Some(*candidate);
    }

    let mut terminal_pairs = Vec::new();
    for lane in lanes {
        let feature_markers = lane
            .sketch_entities
            .iter()
            .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
            .filter(|marker| marker.coordinates_m.is_some())
            .filter(|marker| {
                matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .collect::<Vec<_>>();
        for radial in feature_markers
            .iter()
            .copied()
            .filter(|marker| marker.local_id.is_none())
        {
            for center in feature_markers
                .iter()
                .copied()
                .filter(|marker| marker.local_id.is_some() && marker.offset < radial.offset)
            {
                let [cu, cv] = center.coordinates_m?;
                let [ru, rv] = radial.coordinates_m?;
                let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                if same_dimension_length(radius, expected_radius) {
                    terminal_pairs.push((center, radius));
                }
            }
        }
    }
    terminal_pairs.sort_by_key(|(center, _)| center.id.as_str());
    terminal_pairs
        .dedup_by(|left, right| left.0.id == right.0.id && left.1.to_bits() == right.1.to_bits());
    if let [candidate] = terminal_pairs.as_slice() {
        return Some(*candidate);
    }

    // Only 83fe defines an ordered center/radial point roster. Other native
    // carriers may use the relation-qualified witness tiers above, but their
    // point-marker order does not identify a circular-dimension pair.
    if operand_kind != FeatureInputOperandKind::Native(0x83fe) {
        return None;
    }

    let mut markers = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.local_id != Some(0))
        .filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset);
    let pair = (markers.len() % 2 == 0)
        .then(|| markers.chunks_exact(2).nth(usize::from(index)))
        .flatten()?;
    let [center, radial] = pair else {
        return None;
    };
    let [cu, cv] = center.coordinates_m?;
    let [ru, rv] = radial.coordinates_m?;
    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
    same_dimension_length(radius, expected_radius).then_some((*center, radius))
}

#[derive(Clone, Copy)]
pub(super) enum DeclaredEntityHandleOwner<'a> {
    Absent,
    Unique(&'a FeatureInputLane),
    Ambiguous,
}

pub(super) fn declared_entity_handle_owner<'a>(
    lanes: &'a [FeatureInputLane],
    operand: &FeatureInputOperand,
) -> DeclaredEntityHandleOwner<'a> {
    let mut owners = lanes.iter().filter_map(|lane| {
        let reference = lane
            .references
            .iter()
            .find(|reference| reference.id == operand.reference_ref)?;
        let class = reference
            .class_ref
            .as_deref()
            .and_then(|id| lane.classes.iter().find(|class| class.id == id))?;
        (class.name == "sgEntHandle").then_some(lane)
    });
    let Some(lane) = owners.next() else {
        return DeclaredEntityHandleOwner::Absent;
    };
    if owners.next().is_some() {
        DeclaredEntityHandleOwner::Ambiguous
    } else {
        DeclaredEntityHandleOwner::Unique(lane)
    }
}

/// Resolve the circular-dimension center carried by a slot handle.
///
/// A slot is an aggregate boundary descriptor, not an independent circle. Its
/// radial dimension handle therefore identifies the slot marker first and a
/// selected center point second. The two exact `sgSlotHandle` reference cells
/// are required so a slot's center cannot be guessed from its radius or from
/// the slot's boundary roster alone.
pub(super) fn declared_slot_handle_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Option<(&'a SketchInputEntity, &'a SketchInputEntity)> {
    let entity_ref = operand.entity_ref.as_deref()?;
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let marker = lane.sketch_entities.iter().find(|marker| {
        marker.id == entity_ref
            && marker.feature_ref.as_deref() == Some(feature)
            && matches!(marker.kind, SketchInputKind::Native(_))
    })?;
    let marker_offset = usize::try_from(marker.offset).ok()?;
    let (_, center_indices) = slot_curve_and_center_indices(&lane.native_payload, marker_offset)?;

    let entity_class = lane
        .references
        .iter()
        .find(|reference| reference.id == operand.reference_ref)
        .and_then(|reference| reference.class_ref.as_deref())
        .and_then(|class_ref| lane.classes.iter().find(|class| class.id == class_ref))
        .filter(|class| class.name == "sgEntHandle")?;
    let mut slot_classes = lane
        .classes
        .iter()
        .filter(|class| {
            class.name == "sgSlotHandle"
                && class.offset > entity_class.offset
                && class.offset < marker.offset
        })
        .collect::<Vec<_>>();
    if slot_classes.len() != 1 {
        return None;
    }
    let slot_class = slot_classes.pop().expect("one slot handle class");
    let class_end = lane
        .classes
        .iter()
        .filter(|class| class.offset > slot_class.offset)
        .map(|class| class.offset)
        .min()
        .unwrap_or_else(|| u64::try_from(lane.native_payload.len()).unwrap_or(u64::MAX))
        .min(marker.offset);
    let class_start = usize::try_from(slot_class.offset).ok()?;
    let class_end = usize::try_from(class_end)
        .ok()?
        .min(lane.native_payload.len());
    if class_start >= class_end {
        return None;
    }
    let reference_indices = (class_start..class_end)
        .filter_map(|offset| {
            let cell_end = offset.checked_add(12)?;
            if cell_end > class_end {
                return None;
            }
            let cell = lane.native_payload.get(offset..cell_end)?;
            if cell.get(..2) != Some(&[0xe7, 0x88])
                || cell.get(4..8) != Some(&[0xff; 4])
                || cell.get(8..12) != Some(&[0; 4])
            {
                return None;
            }
            Some(usize::from(View::u16_le_at(
                &lane.native_payload,
                offset + 2,
            )?))
        })
        .collect::<Vec<_>>();
    if reference_indices.len() != 2 {
        return None;
    }
    let [slot_index, center_index] = reference_indices.as_slice() else {
        unreachable!("two slot-handle references were required above")
    };
    let slot_index = u32::try_from(*slot_index).ok()?;
    let center_index = u32::try_from(*center_index).ok()?;
    if slot_index != u32::from(operand.entity_index) || marker.local_id != Some(slot_index) {
        return None;
    }

    let mut points = lane
        .sketch_entities
        .iter()
        .filter(|candidate| candidate.feature_ref.as_deref() == Some(feature))
        .filter(|candidate| candidate.coordinates_m.is_some())
        .filter(|candidate| {
            matches!(
                candidate.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    points.sort_unstable_by_key(|candidate| candidate.offset);
    let centers = center_indices
        .map(|index| points.get(index).copied())
        .into_iter()
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = centers.as_slice() else {
        unreachable!("slot descriptor has two center indices")
    };
    let center = match (
        first.local_id == Some(center_index),
        second.local_id == Some(center_index),
    ) {
        (true, false) => *first,
        (false, true) => *second,
        _ => return None,
    };
    let coordinates = center.coordinates_m?;
    coordinates
        .into_iter()
        .all(f64::is_finite)
        .then_some((marker, center))
}

/// Resolve the indexed point-pair form of a circular dimension.
///
/// The `6e 83` operand has no explicit sketch marker. Its `sgEntHandle`
/// reference scopes an ordered point roster, while the operand index selects
/// one adjacent center/radial pair. Every pair must carry the indexed
/// center-to-radial object/local join; a radius match cannot establish the
/// carrier on its own.
pub(super) fn declared_entity_handle_indexed_circle_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<&'a SketchInputEntity> {
    if operand.kind != FeatureInputOperandKind::Native(0x836e)
        || operand.entity_ref.is_some()
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let mut markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| {
            marker
                .coordinates_m
                .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
        })
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset);
    let pairs = markers
        .chunks_exact(2)
        .map(|pair| [pair[0], pair[1]])
        .collect::<Vec<_>>();
    if !markers.chunks_exact(2).remainder().is_empty()
        || pairs.iter().any(|[center, radial]| {
            let Some(center_local_id) = center.local_id else {
                return true;
            };
            center_local_id == 0
                || radial.object_index != Some(center_local_id)
                || radial.local_id.is_none_or(|local_id| local_id == 0)
        })
    {
        return None;
    }
    let [center, radial] = *pairs.get(usize::from(operand.entity_index))?;
    let [cu, cv] = center.coordinates_m?;
    let [ru, rv] = radial.coordinates_m?;
    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
    same_dimension_length(radius, expected_radius).then_some(center)
}

fn point_dimension_marker_matches_operand(
    marker: &SketchInputEntity,
    feature: &str,
    operand: &FeatureInputOperand,
) -> bool {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return false;
    };
    let address = u32::from(operand.entity_index);
    let identity_matches = match operand.kind {
        FeatureInputOperandKind::Native(0x814c) => marker.object_index == Some(address),
        FeatureInputOperandKind::Native(_) => marker.local_id == Some(address),
        _ => false,
    };
    marker.id == entity_ref
        && marker.feature_ref.as_deref() == Some(feature)
        && identity_matches
        && matches!(
            marker.kind,
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
}

/// Resolve the explicit point-center form of a circular dimension.
///
/// A native operand tag carries the point identity in the resolved operand
/// reference. The reference must resolve to a point marker in the unique
/// `sgEntHandle` lane. Native tag `4c 81` uses the marker's feature-local
/// object index as the identity; other native point identities use the local
/// identifier. No radius-based pair or marker-family fallback is valid for
/// this explicit identity form.
pub(super) fn declared_entity_handle_point_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> Option<&'a SketchInputEntity> {
    if !matches!(operand.kind, FeatureInputOperandKind::Native(_)) {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let marker = lane
        .sketch_entities
        .iter()
        .find(|marker| point_dimension_marker_matches_operand(marker, feature, operand))?;
    marker
        .coordinates_m
        .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
        .then_some(marker)
}

/// Resolve a classless direct point identity for a circular dimension.
///
/// Some circular dimensions carry a reference cell and an explicit point
/// marker without an `sgEntHandle` class declaration. The reference kind,
/// feature, object index, marker identity, and marker-local address must all
/// agree. A point that is the radial member of an encoded center/radial pair
/// at the dimension's radius is not a circle center.
pub(super) fn direct_point_dimension_center<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<&'a SketchInputEntity> {
    if !matches!(operand.kind, FeatureInputOperandKind::Native(_))
        || !expected_radius.is_finite()
        || expected_radius <= 0.0
    {
        return None;
    }
    let mut candidates = lanes.iter().filter_map(|lane| {
        let reference = lane
            .references
            .iter()
            .find(|reference| reference.id == operand.reference_ref)?;
        if reference.feature_ref.as_deref() != Some(feature)
            || reference.kind != operand.kind
            || reference.object_index != operand.entity_index
            || reference.class_ref.is_some()
        {
            return None;
        }
        let marker = lane
            .sketch_entities
            .iter()
            .find(|marker| point_dimension_marker_matches_operand(marker, feature, operand))?;
        marker
            .coordinates_m
            .is_some_and(|coordinates| coordinates.into_iter().all(f64::is_finite))
            .then_some((lane, marker))
    });
    let (lane, marker) = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    for [center, radial] in declared_entity_handle_pairs(lane, feature)
        .into_iter()
        .filter(|[_, radial]| radial.id == marker.id)
    {
        let [cu, cv] = center.coordinates_m?;
        let [ru, rv] = radial.coordinates_m?;
        if same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, expected_radius) {
            return None;
        }
    }
    Some(marker)
}

pub(super) fn declared_entity_handle_circular_marker<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<(&'a SketchInputEntity, f64)> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let child_pairs = declared_entity_handle_declared_child_pairs(lane, feature);
    let pairs = declared_entity_handle_pairs(lane, feature);
    // An explicit radial identity is stronger than one feature-scoped child
    // declaration. Resolve it first because an unrelated line or arc child
    // can coexist with the circular-dimension point pair. Multiple child
    // declarations remain ambiguous, even when one point pair also matches.
    if child_pairs.len() <= 1 {
        if let Some(entity_ref) = operand.entity_ref.as_deref() {
            let mut candidates = pairs
                .iter()
                .copied()
                .filter(|[_, radial]| radial.id == entity_ref);
            let candidate = candidates.next();
            if candidates.next().is_some() {
                return None;
            }
            if let Some([center, radial]) = candidate {
                let [cu, cv] = center.coordinates_m?;
                let [ru, rv] = radial.coordinates_m?;
                let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                return same_dimension_length(radius, expected_radius).then_some((center, radius));
            }
        }
    }
    if let [child_pair] = child_pairs.as_slice() {
        // The relation operand identifies the radial child when present. Use
        // that identity to reject a mismatched child. The scoped child
        // declaration already identifies this pair, so unrelated linked
        // pairs in the same feature do not make it ambiguous.
        let operand_identifies_child = operand
            .entity_ref
            .as_deref()
            .is_some_and(|entity_ref| child_pair[1].id == entity_ref);
        if operand.entity_ref.is_some() && !operand_identifies_child {
            return None;
        }
        let [center, radial] = *child_pair;
        let [cu, cv] = center.coordinates_m?;
        let [ru, rv] = radial.coordinates_m?;
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        return same_dimension_length(radius, expected_radius).then_some((center, radius));
    }
    if !child_pairs.is_empty() {
        return None;
    }
    let mut candidates = pairs.into_iter().filter_map(|[center, radial]| {
        let [cu, cv] = center.coordinates_m?;
        let [ru, rv] = radial.coordinates_m?;
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        same_dimension_length(radius, expected_radius).then_some((center, radius))
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(super) fn declared_entity_handle_has_resolved_pair(
    lanes: &[FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> bool {
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return false;
    };
    !declared_entity_handle_pairs(lane, feature).is_empty()
}

/// Test whether an explicit point reference is the radial member of a
/// declared entity-handle pair. Radial identity cannot be reinterpreted as a
/// center by the native point-identity fallback.
pub(super) fn declared_entity_handle_point_is_declared_radial(
    lanes: &[FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
) -> bool {
    let Some(entity_ref) = operand.entity_ref.as_deref() else {
        return false;
    };
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return false;
    };
    declared_entity_handle_pairs(lane, feature)
        .iter()
        .any(|[_, radial]| radial.id == entity_ref)
}

fn declared_entity_handle_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut pairs = declared_entity_handle_linked_pairs(lane, feature);
    pairs.extend(declared_entity_handle_declared_child_pairs(lane, feature));
    pairs.extend(declared_entity_handle_indexed_point_pairs(lane, feature));
    pairs.sort_unstable_by_key(|[center, radial]| (center.offset, radial.offset));
    pairs.dedup_by(|left, right| left[0].id == right[0].id && left[1].id == right[1].id);
    pairs
}

/// Resolve the indexed point form used by a declared entity handle when the
/// radial point carries its own local identifier. The adjacent roster order
/// and the center-to-radial object/local join are both required; a radius
/// match alone is not a carrier identity.
fn declared_entity_handle_indexed_point_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.coordinates_m.is_some())
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset);
    markers
        .windows(2)
        .filter_map(|pair| {
            let [center, radial] = pair else {
                unreachable!("slice windows have the requested length")
            };
            let center_local_id = center.local_id?;
            if center_local_id == 0
                || radial.object_index != Some(center_local_id)
                || radial.local_id.is_none_or(|local_id| local_id == 0)
            {
                return None;
            }
            Some([*center, *radial])
        })
        .collect()
}

/// Resolve the wide child form where a curve marker is followed by its radial
/// point and the point interval declares the curve handle class. The class
/// declaration is scoped to the following marker interval; a radius match
/// alone is not sufficient because the same feature can contain repeated
/// circular construction carriers.
fn declared_entity_handle_declared_child_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut feature_markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .collect::<Vec<_>>();
    feature_markers.sort_unstable_by_key(|marker| marker.offset);
    let mut markers = feature_markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.coordinates_m.is_some()
                && matches!(
                    marker.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset);
    markers
        .windows(2)
        .filter_map(|pair| {
            let [center, radial] = pair else {
                unreachable!("slice windows have the requested length")
            };
            let class_name = match center.kind {
                SketchInputKind::Arc => "sgArcHandle",
                SketchInputKind::LineOrCircle => "sgLineHandle",
                _ => return None,
            };
            if !matches!(
                radial.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                return None;
            }
            let next_marker_offset = feature_markers
                .iter()
                .find(|marker| marker.offset > radial.offset)
                .map_or(u64::MAX, |marker| marker.offset);
            let declared = lane.classes.iter().any(|class| {
                class.name == class_name
                    && class.offset > radial.offset
                    && class.offset < next_marker_offset
            });
            if !declared {
                return None;
            }
            Some([*center, *radial])
        })
        .collect()
}

fn declared_entity_handle_linked_pairs<'a>(
    lane: &'a FeatureInputLane,
    feature: &str,
) -> Vec<[&'a SketchInputEntity; 2]> {
    let mut markers = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.coordinates_m.is_some())
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
                    | SketchInputKind::Arc
            )
        })
        .collect::<Vec<_>>();
    markers.sort_unstable_by_key(|marker| marker.offset);
    markers
        .windows(2)
        .filter_map(|pair| {
            let [center, radial] = pair else {
                unreachable!("slice windows have the requested length")
            };
            if !matches!(
                radial.kind,
                SketchInputKind::Point
                    | SketchInputKind::ConstrainedPoint
                    | SketchInputKind::LineOrCircle
            ) {
                return None;
            }
            let center_local_id = center.local_id?;
            if center_local_id == 0
                || radial.object_index != Some(center_local_id)
                || !matches!(radial.local_id, None | Some(0))
            {
                return None;
            }
            Some([*center, *radial])
        })
        .collect()
}

pub(crate) fn project_relation_bindings(
    constraints: &mut Vec<SketchConstraint>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    features: &[cadmpeg_ir::features::Feature],
    sketch_entities: &[SketchEntity],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let loci_by_marker = profile_loci_by_marker(features, sketches, sketch_entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let relation_parameters = owned_relation_parameters(features, parameters, lanes);
    let parameters_by_id = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    // First-match index over `constraints` by `native_ref`, maintained on every
    // push below so a lookup is O(1) instead of a scan of a growing arena.
    // `or_insert` keeps the earliest index for a duplicated ref, matching the
    // first-match semantics of the `position` scans this replaces.
    let mut constraints_by_native_ref = HashMap::<String, usize>::new();
    for (index, constraint) in constraints.iter().enumerate() {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            constraints_by_native_ref
                .entry(native_ref.to_owned())
                .or_insert(index);
        }
    }
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            let existing = constraints_by_native_ref.get(relation.id.as_str()).copied();
            if existing.is_some_and(|index| {
                !matches!(
                    &constraints[index].definition,
                    SketchConstraintDefinition::Native { .. }
                )
            }) {
                continue;
            }
            let Some(parameter_id) = relation_parameters.get(&relation.id) else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter = parameter_id
                .as_ref()
                .and_then(|parameter| parameters_by_id.get(parameter))
                .copied();
            let parameter_id = parameter.map(|parameter| parameter.id.clone());
            let native_kind = match relation.family {
                FeatureInputRelationFamily::LineLineDistance => "sgLLDist",
                FeatureInputRelationFamily::PointPointDistance => "sgPntPntDist",
                FeatureInputRelationFamily::PointLineDistance => "sgPntLineDist",
                FeatureInputRelationFamily::PointPointHorizontalDistance => "sgPntPntHorDist",
                FeatureInputRelationFamily::PointPointVerticalDistance => "sgPntPntVertDist",
                FeatureInputRelationFamily::Angle => "sgAnglDim",
                FeatureInputRelationFamily::CircleDiameter => "sgCircleDim",
            };
            let mut entities = relation
                .operands
                .iter()
                .filter_map(|operand| operand.entity_ref.as_deref())
                .flat_map(|marker| {
                    marker_entities(marker, &markers_by_id, &loci_by_marker).into_iter()
                })
                .collect::<Vec<_>>();
            entities.sort_by(|left, right| left.0.cmp(&right.0));
            entities.dedup();
            let definition = typed_relation_definition(
                relation,
                parameter,
                sketch,
                sketch_entities,
                &markers_by_id,
                &loci_by_marker,
            )
            .unwrap_or_else(|| SketchConstraintDefinition::Native {
                native_kind: native_kind.into(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities,
                parameter: parameter_id,
                operands: relation
                    .operands
                    .iter()
                    .map(|operand| SketchNativeOperand {
                        native_kind: operand_kind_name(operand.kind),
                        native_field: None,
                        native_role: None,
                        object_index: u32::from(operand.entity_index),
                        native_ref: operand.entity_ref.clone(),
                    })
                    .collect(),
            });
            let active = relation_constraint_is_inactive(parameter, &definition, sketch_entities)
                .then_some(false);
            let projected = SketchConstraint {
                id: SketchConstraintId(format!(
                    "sldprt:model:sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                )),
                sketch: (*sketch).clone(),
                definition,
                name: None,
                driving: None,
                active,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(relation.id.clone()),
            };
            if let Some(index) = existing {
                if !matches!(
                    &projected.definition,
                    SketchConstraintDefinition::Native { .. }
                ) {
                    constraints[index] = projected;
                }
            } else {
                constraints_by_native_ref
                    .entry(relation.id.clone())
                    .or_insert(constraints.len());
                constraints.push(projected);
            }
        }
        for marker in &lane.sketch_entities {
            let existing = constraints_by_native_ref.get(marker.id.as_str()).copied();
            if existing.is_some_and(|index| {
                !matches!(
                    &constraints[index].definition,
                    SketchConstraintDefinition::Native { .. }
                )
            }) {
                continue;
            }
            let Some(sketch) = marker
                .feature_ref
                .as_deref()
                .and_then(|feature| sketches_by_feature.get(feature))
            else {
                continue;
            };
            let Some(definition) = typed_marker_relation_definition_in_sketch(
                marker,
                sketch,
                sketch_entities,
                &markers_by_id,
                &loci_by_marker,
            ) else {
                continue;
            };
            let active =
                marker_relation_is_inactive(marker, &definition, sketch_entities).then_some(false);
            let projected = SketchConstraint {
                id: SketchConstraintId(format!(
                    "sldprt:model:sketch-constraint#marker:{lane_key}:{}",
                    marker.offset
                )),
                sketch: (*sketch).clone(),
                definition,
                name: None,
                driving: None,
                active,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(marker.id.clone()),
            };
            if let Some(index) = existing {
                if !matches!(
                    &projected.definition,
                    SketchConstraintDefinition::Native { .. }
                ) {
                    constraints[index] = projected;
                }
            } else {
                constraints_by_native_ref
                    .entry(marker.id.clone())
                    .or_insert(constraints.len());
                constraints.push(projected);
            }
        }
    }
}

pub(crate) fn owned_relation_parameters(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Option<cadmpeg_ir::features::ParameterId>> {
    let parameters_by_scalar = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter)))
        .collect::<HashMap<_, _>>();
    let mut claimed = HashSet::new();
    let mut owned = HashMap::new();
    for lane in lanes {
        for relation in &lane.relation_instances {
            let Some(scalar) = relation.parameter_scalar_ref.as_deref() else {
                continue;
            };
            let parameter = parameters_by_scalar
                .get(scalar)
                .map(|parameter| parameter.id.clone())
                .or_else(|| {
                    relation_parameter_by_driving_name(relation, lane, features, parameters)
                        .map(|parameter| parameter.id.clone())
                });
            if let Some(parameter) = &parameter {
                claimed.insert(parameter.clone());
            }
            owned.insert(relation.id.clone(), parameter);
        }
    }
    for lane in lanes {
        for relation in &lane.relation_instances {
            if relation.parameter_scalar_ref.is_some() {
                continue;
            }
            let exact_matches = relation
                .scalar_refs
                .iter()
                .filter_map(|scalar| parameters_by_scalar.get(scalar.as_str()).copied())
                .collect::<Vec<_>>();
            if let [parameter] = exact_matches.as_slice() {
                if claimed.insert(parameter.id.clone()) {
                    owned.insert(relation.id.clone(), Some(parameter.id.clone()));
                }
                continue;
            }
            let parameter =
                relation_parameter_by_driving_name(relation, lane, features, parameters)
                    .or_else(|| {
                        circle_dimension_handle_driver(relation, lane).and_then(|scalar| {
                            parameters_by_scalar.get(scalar.id.as_str()).copied()
                        })
                    })
                    .or_else(|| {
                        relation_parameter_by_display_name(relation, lane, features, parameters)
                    });
            let Some(parameter) = parameter else {
                continue;
            };
            if claimed.insert(parameter.id.clone()) {
                owned.insert(relation.id.clone(), Some(parameter.id.clone()));
            }
        }
    }
    owned
}

fn relation_parameter_by_driving_name<'a>(
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Option<&'a cadmpeg_ir::features::DesignParameter> {
    let owner = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(relation.feature_ref.as_str()))?
        .id
        .clone();
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let mut driving_names = relation
        .parameter_scalar_ref
        .as_deref()
        .into_iter()
        .chain(relation.scalar_refs.iter().map(String::as_str))
        .filter_map(|scalar| scalars.get(scalar))
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
        .filter_map(|scalar| names.get(scalar.name.as_str()).copied())
        .collect::<Vec<_>>();
    driving_names.sort_unstable();
    driving_names.dedup();
    let [name] = driving_names.as_slice() else {
        return None;
    };
    let mut matches = parameters.iter().filter(|parameter| {
        parameter.owner.as_ref() == Some(&owner) && parameter.name.as_str() == *name
    });
    let parameter = matches.next()?;
    matches.next().is_none().then_some(parameter)
}

pub(super) fn relation_parameter_by_display_name<'a>(
    relation: &FeatureInputRelationInstance,
    lane: &FeatureInputLane,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &'a [cadmpeg_ir::features::DesignParameter],
) -> Option<&'a cadmpeg_ir::features::DesignParameter> {
    let owner = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(relation.feature_ref.as_str()))?
        .id
        .clone();
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let owner = &owner;
    let mut matches = relation
        .scalar_refs
        .iter()
        .filter_map(|scalar| scalars.get(scalar.as_str()))
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
        .filter_map(|scalar| names.get(scalar.name.as_str()).copied())
        .flat_map(|name| {
            parameters.iter().filter(move |parameter| {
                parameter.owner.as_ref() == Some(owner) && parameter.name == name
            })
        });
    let first = matches.next()?;
    matches
        .all(|parameter| parameter.id == first.id)
        .then_some(first)
}

#[cfg(test)]
mod relation_geometry_tests {
    use super::*;

    const TEST_LINE_GEOMETRY_QUANTUM: f64 = 1.0 / 100_000_000.0;

    #[test]
    fn solver_point_relation_projects_graph_resolved_operands() {
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue,
        };
        use cadmpeg_ir::sketches::{Sketch, SketchLocus, SketchPlacement};
        use std::collections::BTreeMap;

        let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch.clone()),
            },
            native_ref: Some("feature-native".into()),
        };
        let marker = |id: &str, ordinal: u32, offset: u64, coordinates_m| {
            let mut marker =
                SketchInputEntity::new(id, "lane#test", ordinal, offset, SketchInputKind::Point);
            marker.feature_ref = Some("feature-native".into());
            marker.coordinates_m = coordinates_m;
            marker
        };
        let operand = |offset: u64, entity_index: u16| FeatureInputOperand {
            offset,
            reference_ref: format!("reference-{offset}"),
            kind: FeatureInputOperandKind::Native(0x8100),
            entity_index,
            entity_ref: None,
        };
        let scalar =
            |id: &str, offset: u64, value: f64, operands| crate::records::FeatureInputScalar {
                id: id.into(),
                parent: "lane#test".into(),
                feature_ref: Some("feature-native".into()),
                ordinal: 0,
                offset,
                object_id: 0,
                name: "distance".into(),
                value,
                role: FeatureInputScalarRole::Driving,
                entity_indices: Vec::new(),
                operands,
            };
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane#test".into(),
            ordinal: 0,
            offset: 30,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: vec!["terminal".into()],
            parameter_scalar_ref: Some("terminal".into()),
            display_scalar_ref: None,
            operands: vec![operand(40, 12), operand(52, 13)],
        };
        let lane = FeatureInputLane {
            id: "lane#test".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: vec![
                scalar("center-1", 10, 0.008, vec![operand(11, 13), operand(12, 3)]),
                scalar(
                    "center-2",
                    20,
                    0.0015,
                    vec![operand(21, 13), operand(22, 4)],
                ),
                scalar(
                    "terminal",
                    30,
                    0.007,
                    vec![operand(31, 12), operand(32, 13)],
                ),
            ],
            relation_bindings: Vec::new(),
            relation_instances: vec![relation.clone()],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                marker("origin", 0, 0, Some([0.0, 0.0])),
                marker("negative", 1, 1, Some([-0.007, 0.0])),
                marker("first-center", 2, 2, Some([0.008, 0.0])),
                marker("second-center", 3, 3, Some([0.0015, 0.0])),
            ],
        };
        let parameter = cadmpeg_ir::features::DesignParameter {
            id: ParameterId("distance".into()),
            owner: Some(feature.id.clone()),
            ordinal: 0,
            name: "distance".into(),
            expression: "7mm".into(),
            display: None,
            value: Some(ParameterValue::Length(Length(7.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("terminal".into()),
        };
        let sketches = vec![Sketch {
            id: sketch.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved,
            profiles: Vec::new(),
            native_ref: Some("lane#test".into()),
        }];
        let mut entities = Vec::new();

        project_relation_solved_point_geometry(
            &mut entities,
            &sketches,
            std::slice::from_ref(&feature),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        );

        let mut positions = entities
            .iter()
            .filter_map(|entity| {
                let geometry_ref = entity.geometry_ref.as_deref()?;
                let SketchGeometry::Point { position } = entity.geometry else {
                    return None;
                };
                Some((geometry_ref, position))
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(
            positions.remove("relation:operand:0"),
            Some(Point2::new(-7.0, 0.0))
        );
        assert_eq!(
            positions.remove("relation:operand:1"),
            Some(Point2::new(0.0, 0.0))
        );
        assert!(positions.is_empty());

        let mut constraints = Vec::new();
        project_relation_bindings(
            &mut constraints,
            &sketches,
            std::slice::from_ref(&feature),
            &entities,
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        );
        let [constraint] = constraints.as_slice() else {
            panic!("one solver-point constraint");
        };
        assert!(matches!(
            &constraint.definition,
            SketchConstraintDefinition::DistanceLoci { first, second, .. }
                if first == &SketchLocus::Entity(
                    SketchEntityId("sldprt:model:sketch-entity#solver-point:test:30:0".into())
                ) && second == &SketchLocus::Entity(
                    SketchEntityId("sldprt:model:sketch-entity#solver-point:test:30:1".into())
                )
        ));
    }

    #[test]
    fn dynamic_line_pair_fallback_preserves_the_existing_solver_slot() {
        let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
        let line = |id: &str, start: Point2, end: Point2| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: sketch.clone(),
            construction: true,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        };
        let generated = vec![
            line("roster-4", Point2::new(-13.0, 3.0), Point2::new(0.0, 3.0)),
            line("roster-0", Point2::new(0.0, 0.0), Point2::new(0.0, 13.0)),
        ];
        let existing = vec![line(
            "profile-line",
            Point2::new(-16.0, 3.0),
            Point2::new(-16.0, 7.0),
        )];

        let [first, second] = unique_dynamic_line_pair(
            16.0,
            &sketch,
            &existing,
            &generated,
            TEST_LINE_GEOMETRY_QUANTUM,
        )
        .expect("one existing line pairs with the roster solver line");
        assert!(matches!(
            first.geometry,
            SketchGeometry::Line { start, end }
                if start == Point2::new(-16.0, 3.0) && end == Point2::new(-16.0, 7.0)
        ));
        assert!(matches!(
            second.geometry,
            SketchGeometry::Line { start, end }
                if start == Point2::new(0.0, 0.0) && end == Point2::new(0.0, 13.0)
        ));
    }
}
