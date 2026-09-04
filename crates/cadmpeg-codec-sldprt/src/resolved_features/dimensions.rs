//! Dimensioned sketch geometry and radial circle records.

use super::endpoints::{
    compact_indexed_curve_record_end, marker_profile_curve_role, minor_arc_angles,
};
use super::markers::{
    current_geometry_locus_arc_handle_point, inline_arc_coordinates, marker_native_code,
    sketch_marker_prefix_at,
};
use super::relation_geometry::{
    declared_entity_handle_circular_marker, declared_entity_handle_has_resolved_pair,
    declared_entity_handle_indexed_circle_dimension_center, declared_entity_handle_owner,
    declared_entity_handle_point_dimension_center, declared_entity_handle_point_is_declared_radial,
    declared_slot_handle_dimension_center, direct_point_dimension_center, implicit_circle_marker,
    owned_relation_parameters, DeclaredEntityHandleOwner,
};
use super::relation_loci::{marker_transform_candidates_by_feature, same_dimension_length};
use super::transforms::{
    dimensioned_circle_surface_transforms, dimensioned_circle_transform,
    marker_transforms_with_frame_fallback, quantize,
};
use super::typed_relations::marker_curve_endpoint_markers;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_ANGLE_TOLERANCE};
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::{Angle, FeatureDefinition, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry};
use std::collections::{HashMap, HashSet};

const EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8: f64 = 1.0e-8;

#[derive(Debug, Clone)]
struct DimensionedArcNative {
    center: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    endpoint_refs: Vec<String>,
}

#[derive(Debug, Clone)]
enum DimensionedCurveNative {
    Circle { center: [f64; 2] },
    Arc(DimensionedArcNative),
}

struct DimensionedRelationCarrier<'a> {
    marker: &'a SketchInputEntity,
    curve: Option<DimensionedCurveNative>,
    center: [f64; 2],
    construction: Option<bool>,
}

/// Resolve the construction state carried by a native radial-circle record
/// for one dimension center.  The radial record's role is authoritative; a
/// center/radius match alone is not enough because an ordinary circle and a
/// construction circle can share the same solved center and radius.
fn native_dimensioned_circle_construction_state(
    lanes: &[FeatureInputLane],
    feature: &str,
    center: &SketchInputEntity,
    radius: f64,
) -> Option<bool> {
    if center.feature_ref.as_deref() != Some(feature)
        || center.coordinates_m.is_none()
        || !radius.is_finite()
        || radius <= 0.0
    {
        return None;
    }
    let [cu, cv] = center.coordinates_m?;
    let mut states = Vec::new();
    for lane in lanes {
        if !lane
            .sketch_entities
            .iter()
            .any(|marker| marker.id == center.id && marker.feature_ref.as_deref() == Some(feature))
        {
            continue;
        }
        let mut roster = lane
            .sketch_entities
            .iter()
            .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
            .filter(|marker| marker.coordinates_m.is_some())
            .collect::<Vec<_>>();
        roster.sort_unstable_by_key(|marker| marker.offset);
        for (_, radial_index, construction) in radial_circle_records(&lane.native_payload) {
            let Some(radial) = roster.get(radial_index) else {
                continue;
            };
            let Some([ru, rv]) = radial.coordinates_m else {
                continue;
            };
            if same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, radius) {
                states.push(construction);
            }
        }
    }
    states.sort_unstable();
    states.dedup();
    match states.as_slice() {
        [state] => Some(*state),
        _ => None,
    }
}

fn native_radial_record_for_marker(
    lanes: &[FeatureInputLane],
    feature: &str,
    marker_id: &str,
) -> Option<(usize, bool)> {
    lanes.iter().find_map(|lane| {
        let marker = lane.sketch_entities.iter().find(|marker| {
            marker.id == marker_id && marker.feature_ref.as_deref() == Some(feature)
        })?;
        radial_circle_records(&lane.native_payload)
            .into_iter()
            .find(|(offset, ..)| usize::try_from(marker.offset).ok() == Some(*offset))
            .map(|(_, radial_index, construction)| (radial_index, construction))
            .or_else(|| {
                let offset = usize::try_from(marker.offset).ok()?;
                extended_radial_circle_index(&lane.native_payload, offset)
                    .map(|radial_index| (radial_index, false))
            })
    })
}

impl DimensionedCurveNative {
    fn center(&self) -> [f64; 2] {
        match self {
            Self::Circle { center } | Self::Arc(DimensionedArcNative { center, .. }) => *center,
        }
    }

    fn arc(&self) -> Option<&DimensionedArcNative> {
        match self {
            Self::Circle { .. } => None,
            Self::Arc(arc) => Some(arc),
        }
    }
}

fn unique_native_radial_witness(
    lane: &FeatureInputLane,
    center: &SketchInputEntity,
    expected_radius: f64,
) -> bool {
    let Some([cu, cv]) = center.coordinates_m else {
        return false;
    };
    let candidates = lane
        .sketch_entities
        .iter()
        .filter(|candidate| {
            candidate.feature_ref == center.feature_ref
                && candidate.offset > center.offset
                && matches!(
                    candidate.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .filter(|candidate| {
            let Some([ru, rv]) = candidate.coordinates_m else {
                return false;
            };
            let radius = (ru - cu).hypot(rv - cv) * 1000.0;
            radius.is_finite() && same_dimension_length(radius, expected_radius)
        })
        .collect::<Vec<_>>();
    candidates.len() == 1
}

fn dimensioned_arc_native_geometry(
    lanes: &[FeatureInputLane],
    marker: &SketchInputEntity,
    expected_radius: f64,
) -> Option<DimensionedCurveNative> {
    if marker.kind != SketchInputKind::Arc {
        return None;
    }
    let lane = lanes.iter().find(|lane| {
        lane.sketch_entities
            .iter()
            .any(|candidate| candidate.id == marker.id)
    })?;
    let object_markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    let markers_by_id = object_markers
        .iter()
        .map(|candidate| (candidate.id.as_str(), *candidate))
        .collect::<HashMap<_, _>>();
    let endpoints = marker_curve_endpoint_markers(
        &lane.native_payload,
        marker,
        &markers_by_id,
        &object_markers,
    );
    let inline = usize::try_from(marker.offset)
        .ok()
        .and_then(|offset| inline_arc_coordinates(&lane.native_payload, offset));
    let ([center, start, end], endpoint_refs) = if let Some(coordinates) = inline {
        let endpoint_refs = match endpoints.as_slice() {
            [first, second] => vec![first.id.clone(), second.id.clone()],
            _ => Vec::new(),
        };
        (coordinates, endpoint_refs)
    } else if let [first, second] = endpoints.as_slice() {
        (
            [
                marker.coordinates_m?,
                first.coordinates_m?,
                second.coordinates_m?,
            ],
            vec![first.id.clone(), second.id.clone()],
        )
    } else {
        return unique_native_radial_witness(lane, marker, expected_radius).then_some(
            DimensionedCurveNative::Circle {
                center: marker.coordinates_m?,
            },
        );
    };
    let start_radius = (start[0] - center[0]).hypot(start[1] - center[1]);
    let end_radius = (end[0] - center[0]).hypot(end[1] - center[1]);
    let valid = center
        .into_iter()
        .chain(start)
        .chain(end)
        .all(f64::is_finite)
        && start != end
        && start_radius.is_finite()
        && start_radius > 0.0
        && same_dimension_length(start_radius, end_radius)
        && same_dimension_length(start_radius * 1000.0, expected_radius);
    valid.then_some(DimensionedCurveNative::Arc(DimensionedArcNative {
        center,
        start,
        end,
        endpoint_refs,
    }))
}

/// Resolve the duplicate-link arc carrier used by a declared entity handle.
///
/// The coordinate-less handle is a reference identity, not a second curve.
/// Exactly two identical links to one earlier arc marker select that marker;
/// the link local identifier and feature scope must agree.  The arc still
/// needs the normal endpoint or radial-witness validation, so a duplicate
/// link cannot turn an arbitrary relation handle into a circular carrier.
fn unique_linked_declared_entity_handle_arc_carrier<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<(&'a SketchInputEntity, DimensionedCurveNative)> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let mut candidates = lane
        .sketch_entities
        .iter()
        .filter(|handle| {
            handle.feature_ref.as_deref() == Some(feature)
                && handle.offset < operand.offset
                && handle.coordinates_m.is_none()
                && handle.kind == SketchInputKind::LineOrCircle
        })
        .filter_map(|handle| {
            let [first, second] = handle.links.as_slice() else {
                return None;
            };
            if first.entity_ref != second.entity_ref || first.local_id != second.local_id {
                return None;
            }
            let arc = lane.sketch_entities.iter().find(|candidate| {
                candidate.id == first.entity_ref
                    && candidate.feature_ref.as_deref() == Some(feature)
                    && candidate.offset < handle.offset
                    && candidate.local_id == Some(u32::from(first.local_id))
                    && candidate.coordinates_m.is_some()
                    && candidate.kind == SketchInputKind::Arc
            })?;
            let curve = dimensioned_arc_native_geometry(lanes, arc, expected_radius)?;
            Some((arc, curve))
        });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn unique_declared_entity_handle_circular_carrier<'a>(
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Option<(&'a SketchInputEntity, DimensionedCurveNative)> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return None;
    }
    if let Some(carrier) =
        unique_linked_declared_entity_handle_arc_carrier(lanes, feature, operand, expected_radius)
    {
        return Some(carrier);
    }
    if declared_entity_handle_has_resolved_pair(lanes, feature, operand) {
        return None;
    }
    let DeclaredEntityHandleOwner::Unique(lane) = declared_entity_handle_owner(lanes, operand)
    else {
        return None;
    };
    let mut candidates = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(feature))
        .filter(|marker| marker.coordinates_m.is_some())
        .filter(|marker| {
            matches!(
                marker.kind,
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
        })
        .filter_map(|marker| {
            let curve = match marker.kind {
                SketchInputKind::Arc => {
                    dimensioned_arc_native_geometry(lanes, marker, expected_radius)?
                }
                SketchInputKind::LineOrCircle => {
                    if !unique_native_radial_witness(lane, marker, expected_radius) {
                        return None;
                    }
                    DimensionedCurveNative::Circle {
                        center: marker.coordinates_m?,
                    }
                }
                _ => return None,
            };
            Some((marker, curve))
        });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn dimensioned_relation_carrier<'a>(
    lanes: &'a [FeatureInputLane],
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    feature: &str,
    operand: &FeatureInputOperand,
    radius: f64,
) -> Option<DimensionedRelationCarrier<'a>> {
    let explicit = operand
        .entity_ref
        .as_deref()
        .and_then(|id| markers_by_id.get(id).copied());
    let explicit_point_marker = explicit.is_some_and(|marker| {
        matches!(
            marker.kind,
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
    });
    if let Some((marker, center)) = declared_slot_handle_dimension_center(lanes, feature, operand) {
        return Some(DimensionedRelationCarrier {
            marker,
            curve: None,
            center: center.coordinates_m?,
            construction: Some(true),
        });
    }
    if operand.kind == FeatureInputOperandKind::Native(0x836e) {
        let marker = declared_entity_handle_indexed_circle_dimension_center(
            lanes, feature, operand, radius,
        )?;
        return Some(DimensionedRelationCarrier {
            marker,
            curve: None,
            center: marker.coordinates_m?,
            construction: Some(false),
        });
    }
    if matches!(
        operand.kind,
        FeatureInputOperandKind::Native(0x80d4 | 0x80d5)
    ) {
        let marker = declared_entity_handle_point_dimension_center(lanes, feature, operand)?;
        return Some(DimensionedRelationCarrier {
            marker,
            curve: None,
            center: marker.coordinates_m?,
            construction: Some(false),
        });
    }
    let explicit_circular_marker = explicit.is_some_and(|marker| {
        matches!(
            marker.kind,
            SketchInputKind::LineOrCircle | SketchInputKind::Arc
        )
    });
    let explicit_current_arc_handle_point = explicit_point_marker
        && explicit.is_some_and(|marker| {
            let Ok(offset) = usize::try_from(marker.offset) else {
                return false;
            };
            lanes.iter().any(|lane| {
                lane.sketch_entities.iter().any(|candidate| {
                    candidate.id == marker.id && candidate.feature_ref.as_deref() == Some(feature)
                }) && current_geometry_locus_arc_handle_point(&lane.native_payload, offset)
            })
        });
    let declared_owner = declared_entity_handle_owner(lanes, operand);
    let declared = declared_entity_handle_circular_marker(lanes, feature, operand, radius);
    let declared_entity_handle = !matches!(declared_owner, DeclaredEntityHandleOwner::Absent);
    let (marker, encoded_radius, fallback_curve) = if let Some((marker, radius)) = declared {
        (marker, Some(radius), None)
    } else if declared_entity_handle && explicit_current_arc_handle_point {
        (explicit?, None, None)
    } else if declared_entity_handle && !explicit_circular_marker {
        if explicit.is_none() || explicit_point_marker {
            if let Some((marker, curve)) =
                unique_declared_entity_handle_circular_carrier(lanes, feature, operand, radius)
            {
                (marker, None, Some(curve))
            } else if matches!(operand.kind, FeatureInputOperandKind::Native(_))
                && explicit_point_marker
                && !declared_entity_handle_point_is_declared_radial(lanes, feature, operand)
            {
                let marker =
                    declared_entity_handle_point_dimension_center(lanes, feature, operand)?;
                (marker, None, None)
            } else {
                return None;
            }
        } else {
            // A declared handle blocks point-based guessing. An explicit native
            // line-or-circle or arc marker remains a direct geometry carrier.
            return None;
        }
    } else {
        match explicit {
            Some(marker)
                if matches!(
                    marker.kind,
                    SketchInputKind::Point
                        | SketchInputKind::ConstrainedPoint
                        | SketchInputKind::LineOrCircle
                        | SketchInputKind::Arc
                ) =>
            {
                (marker, None, None)
            }
            _ => {
                let (marker, radius) = implicit_circle_marker(
                    lanes,
                    feature,
                    operand.kind,
                    operand.entity_index,
                    radius,
                )?;
                (marker, Some(radius), None)
            }
        }
    };
    let curve = fallback_curve.or_else(|| {
        (marker.kind == SketchInputKind::Arc)
            .then(|| dimensioned_arc_native_geometry(lanes, marker, radius))
            .flatten()
    });
    if marker.kind == SketchInputKind::Arc && curve.is_none() {
        return None;
    }
    if !matches!(
        marker.kind,
        SketchInputKind::Point
            | SketchInputKind::ConstrainedPoint
            | SketchInputKind::LineOrCircle
            | SketchInputKind::Arc
    ) {
        return None;
    }
    if curve
        .as_ref()
        .and_then(DimensionedCurveNative::arc)
        .is_some_and(|arc| {
            !same_dimension_length(
                (arc.start[0] - arc.center[0]).hypot(arc.start[1] - arc.center[1]) * 1000.0,
                radius,
            )
        })
    {
        return None;
    }
    if encoded_radius.is_some_and(|encoded| !same_dimension_length(encoded, radius)) {
        return None;
    }
    let construction = native_dimensioned_circle_construction_state(lanes, feature, marker, radius)
        .or_else(|| direct_point_dimension_center(lanes, feature, operand, radius).map(|_| false))
        .or_else(|| {
            declared_entity_handle.then_some(false).or_else(|| {
                matches!(
                    marker.kind,
                    SketchInputKind::LineOrCircle | SketchInputKind::Arc
                )
                .then_some(false)
            })
        });
    Some(DimensionedRelationCarrier {
        marker,
        center: curve
            .as_ref()
            .map_or(marker.coordinates_m, |curve| Some(curve.center()))?,
        curve,
        construction,
    })
}

fn transformed_dimensioned_arc(
    transform: super::transforms::MarkerTransform,
    arc: &DimensionedArcNative,
    native_to_ir: f64,
    quantum: f64,
) -> Option<(SketchGeometry, Vec<String>)> {
    let transform_point = |[u, v]: [f64; 2]| {
        let point = transform.apply(quantize(
            Point2::new(u * native_to_ir, v * native_to_ir),
            quantum,
        ))?;
        Some(Point2::new(
            point.0 as f64 * quantum,
            point.1 as f64 * quantum,
        ))
    };
    let center = transform_point(arc.center)?;
    let mut start = transform_point(arc.start)?;
    let mut end = transform_point(arc.end)?;
    let radius = (start.u - center.u).hypot(start.v - center.v);
    let end_radius = (end.u - center.u).hypot(end.v - center.v);
    let mut endpoint_refs = arc.endpoint_refs.clone();
    let start_angle = (start.v - center.v).atan2(start.u - center.u);
    let end_angle = (end.v - center.v).atan2(end.u - center.u);
    let (start_angle, end_angle, reversed) = minor_arc_angles(start_angle, end_angle);
    if reversed {
        std::mem::swap(&mut start, &mut end);
        if endpoint_refs.len() == 2 {
            endpoint_refs.swap(0, 1);
        }
    }
    let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
    (radius.is_finite()
        && radius > quantum
        && same_dimension_length(radius, end_radius)
        && sweep > SKETCH_ANGLE_TOLERANCE
        && sweep <= std::f64::consts::PI + SKETCH_ANGLE_TOLERANCE)
        .then_some((
            SketchGeometry::Arc {
                center,
                radius: Length(radius),
                start_angle: Angle(start_angle),
                end_angle: Angle(end_angle),
            },
            endpoint_refs,
        ))
}

/// Materialize dimensioned circular sketch geometry omitted by a selected-profile stream.
pub(crate) fn project_dimensioned_sketch_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    surfaces: &[cadmpeg_ir::geometry::Surface],
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
                sketch: Some(sketch),
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
    let relation_parameter = |relation: &FeatureInputRelationInstance| {
        ownership
            .get(&relation.id)?
            .as_ref()
            .and_then(|parameter| parameters_by_id.get(parameter))
            .copied()
    };
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let marker_transforms =
        marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let transforms = sketches_by_feature
        .iter()
        .filter_map(|(feature, sketch_id)| {
            let circles = lanes
                .iter()
                .flat_map(|lane| &lane.relation_instances)
                .filter(|relation| {
                    relation.feature_ref == *feature
                        && relation.family == FeatureInputRelationFamily::CircleDiameter
                })
                .filter_map(|relation| {
                    let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                        return None;
                    };
                    let parameter = relation_parameter(relation)?;
                    let cadmpeg_ir::features::ParameterValue::Length(value) =
                        parameter.value.as_ref()?
                    else {
                        return None;
                    };
                    let radius = match parameter.display {
                        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                        None => return None,
                    };
                    if !(radius.is_finite() && radius > 0.0) {
                        return None;
                    }
                    let carrier = dimensioned_relation_carrier(
                        lanes,
                        &markers_by_id,
                        relation.feature_ref.as_str(),
                        operand,
                        radius,
                    )?;
                    Some((
                        quantize(
                            Point2::new(
                                carrier.center[0] * NATIVE_TO_IR,
                                carrier.center[1] * NATIVE_TO_IR,
                            ),
                            QUANTUM,
                        ),
                        (radius / QUANTUM).round() as i64,
                    ))
                })
                .collect::<Vec<_>>();
            let candidates = marker_transforms.get(*feature).cloned().unwrap_or_else(|| {
                sketches
                    .iter()
                    .find(|sketch| sketch.id == *sketch_id)
                    .map_or_else(Vec::new, |sketch| {
                        dimensioned_circle_surface_transforms(sketch, surfaces, &circles, QUANTUM)
                    })
            });
            let candidates = sketches
                .iter()
                .find(|sketch| sketch.id == *sketch_id)
                .map_or(candidates.clone(), |sketch| {
                    marker_transforms_with_frame_fallback(&candidates, sketch, QUANTUM)
                });
            dimensioned_circle_transform(&candidates, &circles)
                .map(|transform| ((*feature).to_string(), transform))
        })
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let (Some(sketch), Some(transform)) = (
                sketches_by_feature.get(relation.feature_ref.as_str()),
                transforms.get(relation.feature_ref.as_str()),
            ) else {
                continue;
            };
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let parameter = relation_parameter(relation);
            let Some(cadmpeg_ir::features::ParameterValue::Length(value)) =
                parameter.and_then(|parameter| parameter.value.as_ref())
            else {
                continue;
            };
            let radius = match parameter.and_then(|parameter| parameter.display) {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
                None => continue,
            };
            if !(radius.is_finite() && radius > 0.0) {
                continue;
            }
            let carrier = dimensioned_relation_carrier(
                lanes,
                &markers_by_id,
                relation.feature_ref.as_str(),
                operand,
                radius,
            );
            let Some(carrier) = carrier else {
                continue;
            };
            let Some(construction) = carrier.construction else {
                continue;
            };
            let native = quantize(
                Point2::new(
                    carrier.center[0] * NATIVE_TO_IR,
                    carrier.center[1] * NATIVE_TO_IR,
                ),
                QUANTUM,
            );
            let Some(center) = transform.apply(native) else {
                continue;
            };
            let center = Point2::new(center.0 as f64 * QUANTUM, center.1 as f64 * QUANTUM);
            if entities.iter().any(|entity| {
                entity.sketch == *sketch
                    && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
            }) {
                continue;
            }
            if carrier
                .curve
                .as_ref()
                .and_then(DimensionedCurveNative::arc)
                .is_none()
                && entities.iter().any(|entity| {
                    entity.sketch == *sketch
                        && match &entity.geometry {
                            SketchGeometry::Circle {
                                center: existing,
                                radius: existing_radius,
                            } => {
                                quantize(*existing, QUANTUM) == quantize(center, QUANTUM)
                                    && same_dimension_length(existing_radius.0, radius)
                            }
                            _ => false,
                        }
                })
            {
                continue;
            }
            let (geometry, endpoint_refs) =
                if let Some(arc) = carrier.curve.as_ref().and_then(DimensionedCurveNative::arc) {
                    let Some((geometry, endpoint_refs)) =
                        transformed_dimensioned_arc(*transform, arc, NATIVE_TO_IR, QUANTUM)
                    else {
                        continue;
                    };
                    let SketchGeometry::Arc {
                        radius: arc_radius, ..
                    } = &geometry
                    else {
                        unreachable!("dimensioned arc helper emits an arc");
                    };
                    if !same_dimension_length(arc_radius.0, radius) {
                        continue;
                    }
                    (geometry, endpoint_refs)
                } else {
                    (
                        SketchGeometry::Circle {
                            center,
                            radius: cadmpeg_ir::features::Length(radius),
                        },
                        Vec::new(),
                    )
                };
            entities.push(
                SketchEntity::new(
                    SketchEntityId(format!(
                        "sldprt:model:sketch-entity#dimension:{lane_key}:{}",
                        relation.offset
                    )),
                    sketch.clone(),
                    geometry,
                )
                .with_construction(construction)
                .with_native_ref(Some(carrier.marker.id.clone()))
                .with_geometry_ref(Some(relation.id.clone()))
                .with_endpoint_refs(endpoint_refs),
            );
        }
    }
}

/// Materialize a circle dimension when its point operand already has one
/// neutral point witness in the owning sketch.
///
/// Some selected profile streams omit the circle carrier but retain the
/// dimension's point marker. The point marker is a center witness for this
/// relation family, not sufficient geometry by itself. Use it only after the
/// relation-point projector has established one same-sketch neutral point;
/// ambiguous or missing witnesses remain native.
pub(crate) fn project_relation_point_dimensioned_circles(
    entities: &mut Vec<SketchEntity>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Sketch {
                sketch: Some(sketch),
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
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

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let Some(parameter) = ownership
                .get(&relation.id)
                .and_then(Option::as_ref)
                .and_then(|parameter| parameters_by_id.get(parameter))
            else {
                continue;
            };
            let Some(radius) = radial_dimension_radius(parameter) else {
                continue;
            };
            let marker_id = operand.entity_ref.as_deref().or_else(|| {
                implicit_circle_marker(
                    lanes,
                    relation.feature_ref.as_str(),
                    operand.kind,
                    operand.entity_index,
                    radius,
                )
                .map(|(marker, _)| marker.id.as_str())
            });
            let Some(marker_id) = marker_id else {
                continue;
            };
            let Some(marker) = markers_by_id.get(marker_id).copied() else {
                continue;
            };
            if !matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                continue;
            }
            let centers = entities
                .iter()
                .filter(|entity| {
                    entity.sketch == **sketch
                        && entity.native_ref.as_deref() == Some(marker_id)
                        && matches!(entity.geometry, SketchGeometry::Point { .. })
                })
                .collect::<Vec<_>>();
            let [center_entity] = centers.as_slice() else {
                continue;
            };
            let SketchGeometry::Point { position: center } = center_entity.geometry else {
                continue;
            };
            let construction = native_dimensioned_circle_construction_state(
                lanes,
                relation.feature_ref.as_str(),
                marker,
                radius,
            )
            .or_else(|| {
                let explicit_center = operand.entity_ref.is_some()
                    && (declared_entity_handle_point_dimension_center(
                        lanes,
                        relation.feature_ref.as_str(),
                        operand,
                    )
                    .is_some()
                        || direct_point_dimension_center(
                            lanes,
                            relation.feature_ref.as_str(),
                            operand,
                            radius,
                        )
                        .is_some())
                    && !declared_entity_handle_point_is_declared_radial(
                        lanes,
                        relation.feature_ref.as_str(),
                        operand,
                    );
                explicit_center.then_some(false)
            })
            .or_else(|| lane.native_payload.is_empty().then_some(false));
            let Some(construction) = construction else {
                continue;
            };
            if entities.iter().any(|entity| {
                entity.sketch == **sketch
                    && matches!(&entity.geometry, SketchGeometry::Circle { center: existing, radius: existing_radius }
                        if quantize(*existing, EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8) == quantize(center, EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8)
                            && same_dimension_length(existing_radius.0, radius))
            }) {
                continue;
            }
            entities.push(
                SketchEntity::new(
                    SketchEntityId(format!(
                        "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}",
                        relation.offset
                    )),
                    (*sketch).clone(),
                    SketchGeometry::Circle {
                        center,
                        radius: Length(radius),
                    },
                )
                .with_construction(construction)
                .with_native_ref(Some(marker.id.clone()))
                .with_geometry_ref(Some(relation.id.clone())),
            );
        }
    }
}

pub(super) fn compact_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    let marker = payload.get(offset..offset + LEGACY_SKETCH_MARKER.len());
    if marker != Some(LEGACY_SKETCH_MARKER) && marker != Some(LEGACY_EXTENDED_SKETCH_MARKER) {
        return None;
    }
    let ordinary = matches!(marker_native_code(payload, offset), Some(1 | 2))
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 60..offset + 64) == Some(&1u32.to_le_bytes());
    let construction = marker == Some(LEGACY_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(7)
        && payload.get(offset + 5..offset + 13)
            == Some(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff])
        && marker_profile_curve_role(payload, offset) == Some(2)
        && payload.get(offset + 29..offset + 31) == Some(&0u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00])
        && payload.get(offset + 60..offset + 64) == Some(&0u32.to_le_bytes())
        && payload.get(offset + 72..offset + 76) == Some(&1i32.to_le_bytes())
        && payload.get(offset + 76..offset + 78) == Some(&8u16.to_le_bytes())
        && payload.get(offset + 78..offset + 94)
            == Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        && payload.get(offset + 94..offset + 96) == Some(&[0; 2])
        && sketch_marker_prefix_at(payload, offset.saturating_add(104));
    if !(ordinary || construction)
        || payload.get(offset + 23..offset + 27) != Some(&[0x04, 0x00, 0x02, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || (ordinary && compact_indexed_curve_record_end(payload, offset).is_none())
    {
        return None;
    }
    let first = View::u16_le_at(payload, offset + 56)?;
    let second = View::u16_le_at(payload, offset + 58)?;
    (first == second).then_some(usize::from(first))
}

pub(super) fn compact_legacy_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    (payload.get(offset..offset + LEGACY_SKETCH_MARKER.len()) == Some(LEGACY_SKETCH_MARKER))
        .then(|| compact_radial_circle_index(payload, offset))
        .flatten()
}

fn radial_circle_records(payload: &[u8]) -> Vec<(usize, usize, bool)> {
    (0..payload.len().saturating_sub(LEGACY_SKETCH_MARKER.len() - 1))
        .filter_map(|offset| {
            let radial = compact_radial_circle_index(payload, offset)
                .or_else(|| extended_terminal_repeated_radial_circle_index(payload, offset))?;
            Some((
                offset,
                radial,
                marker_profile_curve_role(payload, offset) == Some(2),
            ))
        })
        .collect()
}

pub(super) fn extended_terminal_repeated_radial_circle_index(
    payload: &[u8],
    offset: usize,
) -> Option<usize> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 31)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || payload.get(offset + 56..offset + 58) != payload.get(offset + 58..offset + 60)
        || payload.get(offset + 56..offset + 58) == Some(&[0; 2])
        || payload.get(offset + 60..offset + 64) != Some(&1u32.to_le_bytes())
        || payload.get(offset + 64..offset + 72) != Some(&(-1.0f64).to_le_bytes())
        || payload.get(offset + 72..offset + 76) != Some(&(-1i32).to_le_bytes())
        || payload.get(offset + 78..offset + 94)
            != Some(&[
                0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff,
                0xff, 0xff,
            ])
        || payload.get(offset + 94..offset + 104) != Some(&[0; 10])
        || sketch_marker_prefix_at(payload, offset.checked_add(104)?)
    {
        return None;
    }
    Some(usize::from(View::u16_le_at(payload, offset + 56)?))
}

pub(super) fn terminal_repeated_radial_circle_pairs<'a>(
    radial_index: usize,
    roster: &[&'a SketchInputEntity],
    radius: f64,
) -> Option<Vec<(&'a SketchInputEntity, &'a SketchInputEntity)>> {
    if radial_index != roster.len() || radius <= 0.0 || !radius.is_finite() {
        return None;
    }
    let terminal = *roster.last()?;
    let mut pairs = roster
        .windows(2)
        .filter_map(|window| {
            let [center, radial] = window else {
                unreachable!("two-wide roster window");
            };
            let center_index = center.object_index?;
            let radial_index = radial.object_index?;
            if center_index != radial_index.checked_add(1)? {
                return None;
            }
            let [cu, cv] = center.coordinates_m?;
            let [ru, rv] = radial.coordinates_m?;
            same_dimension_length((ru - cu).hypot(rv - cv), radius).then_some((*center, *radial))
        })
        .collect::<Vec<_>>();
    if pairs.len() < 2 || pairs.last().map(|(_, radial)| radial.id.as_str()) != Some(&terminal.id) {
        return None;
    }
    let mut used = HashSet::new();
    if pairs
        .iter()
        .any(|(center, radial)| !used.insert(&center.id) || !used.insert(&radial.id))
    {
        return None;
    }
    pairs.sort_unstable_by_key(|(center, _)| center.offset);
    Some(pairs)
}

pub(super) fn extended_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    let supported = payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        == Some(LEGACY_EXTENDED_SKETCH_MARKER)
        && marker_native_code(payload, offset) == Some(2)
        && payload.get(offset + 23..offset + 27) == Some(&[0x04, 0x00, 0x02, 0x00])
        && marker_profile_curve_role(payload, offset) == Some(1)
        && payload.get(offset + 29..offset + 31) == Some(&1u16.to_le_bytes())
        && payload.get(offset + 31..offset + 39)
            == Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        && payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 56..offset + 64) == Some(&[0; 8])
        && payload.get(offset + 64..offset + 66) == payload.get(offset + 66..offset + 68)
        && payload.get(offset + 64..offset + 66) != Some(&[0; 2])
        && payload.get(offset + 68..offset + 72) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 80..offset + 84) == Some(&1u32.to_le_bytes());
    supported.then(|| {
        usize::from(View::u16_le_at(payload, offset + 64).expect("guarded two-byte radial index"))
    })
}

pub(super) fn radial_dimension_radius(
    parameter: &cadmpeg_ir::features::DesignParameter,
) -> Option<f64> {
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.0,
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.0 * 0.5,
        None => return None,
    };
    (radius.is_finite() && radius > 0.0).then_some(radius)
}

/// Remove a native circular marker after its exact circle-dimension relation
/// has materialized the same geometry.
///
/// A direct circular operand is an identity carrier for the dimensioned circle,
/// not an additional sketch entity. Remove only a native entity that carries
/// that marker and only when the relation projector left a typed circle linked
/// to the relation. This keeps unresolved, indirect, and non-circular markers
/// native.
fn reconcile_direct_circle_dimension_carriers(
    entities: &mut Vec<SketchEntity>,
    sketches: &mut [Sketch],
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    feature: &str,
    lanes: &[FeatureInputLane],
) {
    let replacements = lanes
        .iter()
        .flat_map(|lane| &lane.relation_instances)
        .filter(|relation| {
            relation.feature_ref == feature
                && relation.family == FeatureInputRelationFamily::CircleDiameter
        })
        .filter_map(|relation| {
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                return None;
            };
            let marker_id = operand.entity_ref.as_deref()?;
            let markers = lanes
                .iter()
                .flat_map(|lane| &lane.sketch_entities)
                .filter(|marker| {
                    marker.id == marker_id && marker.feature_ref.as_deref() == Some(feature)
                })
                .collect::<Vec<_>>();
            let [marker] = markers.as_slice() else {
                return None;
            };
            if !matches!(marker.kind, SketchInputKind::LineOrCircle)
                || !marker
                    .coordinates_m
                    .is_some_and(|[u, v]| u.is_finite() && v.is_finite())
            {
                return None;
            }
            let typed_entities = entities
                .iter()
                .filter(|entity| {
                    entity.sketch == *sketch_id
                        && entity.native_ref.as_deref() == Some(marker.id.as_str())
                        && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
                        && matches!(entity.geometry, SketchGeometry::Circle { .. })
                })
                .collect::<Vec<_>>();
            let [typed_entity] = typed_entities.as_slice() else {
                return None;
            };
            Some((marker.id.clone(), typed_entity.id().clone()))
        })
        .collect::<HashMap<_, _>>();
    if replacements.is_empty() {
        return;
    }
    let removed = entities
        .iter()
        .filter(|entity| {
            entity.sketch == *sketch_id
                && matches!(entity.geometry, SketchGeometry::Native { .. })
                && entity
                    .native_ref
                    .as_deref()
                    .and_then(|native_ref| replacements.get(native_ref))
                    .is_some()
        })
        .filter_map(|entity| {
            let native_ref = entity.native_ref.as_deref()?;
            Some((entity.id().clone(), replacements.get(native_ref)?.clone()))
        })
        .collect::<HashMap<_, _>>();
    if removed.is_empty() {
        return;
    }
    entities.retain(|entity| !removed.contains_key(entity.id()));
    if let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id) {
        for profile in &mut sketch.profiles {
            let usages = std::mem::take(profile);
            let mut present = usages
                .iter()
                .filter(|usage| !removed.contains_key(&usage.entity))
                .map(|usage| usage.entity.clone())
                .collect::<HashSet<_>>();
            let mut updated = Vec::with_capacity(usages.len());
            for usage in usages {
                let Some(replacement) = removed.get(&usage.entity) else {
                    updated.push(usage);
                    continue;
                };
                if present.insert(replacement.clone()) {
                    updated.push(SketchEntityUse {
                        entity: replacement.clone(),
                        reversed: usage.reversed,
                    });
                }
            }
            *profile = updated;
        }
        sketch.profiles.retain(|profile| !profile.is_empty());
    }
}

/// Materialize marker-only circles whose radial witnesses have exact radial
/// dimensions, including repeated circles constrained to the same radius.
pub(crate) fn project_marker_dimensioned_circles(
    entities: &mut Vec<SketchEntity>,
    sketches: &mut [Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let radial_records_by_lane = lanes
        .iter()
        .map(|lane| {
            (
                lane.id.as_str(),
                radial_circle_records(&lane.native_payload),
            )
        })
        .collect::<HashMap<_, _>>();
    'feature: for feature in features {
        let (
            Some(native_ref),
            FeatureDefinition::Sketch {
                sketch: Some(sketch_id),
            },
        ) = (feature.native_ref.as_deref(), &feature.definition)
        else {
            continue;
        };
        reconcile_direct_circle_dimension_carriers(
            entities, sketches, sketch_id, native_ref, lanes,
        );
        let radial_dimensions = parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
            .filter_map(|parameter| {
                radial_dimension_radius(parameter).map(|radius| (parameter, radius))
            })
            .collect::<Vec<_>>();
        if radial_dimensions.is_empty() {
            continue;
        }
        let owned_lanes = lanes
            .iter()
            .filter(|lane| {
                lane.sketch_entities
                    .iter()
                    .any(|marker| marker.feature_ref.as_deref() == Some(native_ref))
            })
            .collect::<Vec<_>>();
        let markers = owned_lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
            .filter(|marker| marker.coordinates_m.is_some())
            .collect::<Vec<_>>();
        let native_carriers = entities
            .iter()
            .filter(|entity| entity.sketch == *sketch_id)
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Native { .. }))
            .collect::<Vec<_>>();
        let has_resolved_curves = entities.iter().any(|entity| {
            entity.sketch == *sketch_id
                && matches!(
                    entity.geometry,
                    SketchGeometry::Line { .. }
                        | SketchGeometry::Arc { .. }
                        | SketchGeometry::Circle { .. }
                        | SketchGeometry::Ellipse { .. }
                        | SketchGeometry::Nurbs { .. }
                )
        });
        let circle_only_carrier = match native_carriers.as_slice() {
            [carrier] if !has_resolved_curves => {
                carrier.native_ref.as_ref().and_then(|reference| {
                    native_radial_record_for_marker(lanes, native_ref, reference).map(
                        |(radial_index, construction)| {
                            (
                                carrier.id().clone(),
                                reference.clone(),
                                radial_index,
                                construction,
                            )
                        },
                    )
                })
            }
            _ => None,
        };
        if let Some((carrier_id, carrier_ref, radial_index, carrier_construction)) =
            circle_only_carrier
        {
            let mut roster = markers
                .iter()
                .copied()
                .filter(|marker| {
                    matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
                })
                .collect::<Vec<_>>();
            roster.sort_unstable_by_key(|marker| marker.offset);
            let centers = roster
                .iter()
                .enumerate()
                .filter_map(|(center_index, center)| {
                    let [cu, cv] = center.coordinates_m?;
                    let later = &roster[center_index + 1..];
                    let mut matched_radials = HashSet::new();
                    let one_to_one = later.len() == radial_dimensions.len()
                        && radial_dimensions.iter().all(|(_, radius)| {
                            let matches = later
                                .iter()
                                .enumerate()
                                .filter_map(|(index, radial)| {
                                    let [ru, rv] = radial.coordinates_m?;
                                    same_dimension_length(
                                        (ru - cu).hypot(rv - cv) * NATIVE_TO_IR,
                                        *radius,
                                    )
                                    .then_some(index)
                                })
                                .collect::<Vec<_>>();
                            let [index] = matches.as_slice() else {
                                return false;
                            };
                            matched_radials.insert(*index)
                        });
                    (one_to_one && matched_radials.len() == later.len())
                        .then_some((center_index, *center))
                })
                .collect::<Vec<_>>();
            if let [(_, center_marker)] = centers.as_slice() {
                let [cu, cv] = center_marker
                    .coordinates_m
                    .expect("coordinate markers carry coordinates");
                let radii = radial_dimensions
                    .iter()
                    .map(|(_, radius)| *radius)
                    .collect::<Vec<_>>();
                let carrier_radius = roster
                    .get(radial_index)
                    .and_then(|radial| radial.coordinates_m)
                    .map(|[ru, rv]| (ru - cu).hypot(rv - cv) * NATIVE_TO_IR);
                let native_center =
                    quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                let centers = transforms
                    .get(native_ref)
                    .into_iter()
                    .flatten()
                    .filter_map(|transform| transform.apply(native_center))
                    .collect::<HashSet<_>>();
                if let [center] = centers.into_iter().collect::<Vec<_>>().as_slice() {
                    let center = Point2::new(center.0 as f64 * QUANTUM, center.1 as f64 * QUANTUM);
                    let removed = carrier_id;
                    entities.retain(|entity| entity.id() != &removed);
                    let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                    else {
                        continue;
                    };
                    for profile in &mut sketch.profiles {
                        profile.retain(|usage| usage.entity != removed);
                    }
                    sketch.profiles.retain(|profile| !profile.is_empty());
                    let feature_key = feature
                        .id
                        .0
                        .rsplit_once('#')
                        .map_or(feature.id.as_str(), |(_, key)| key);
                    for (index, ((parameter, _), radius)) in
                        radial_dimensions.iter().copied().zip(radii).enumerate()
                    {
                        let entity_id = SketchEntityId(format!(
                            "sldprt:model:sketch-entity#radial-roster:{feature_key}:{index}"
                        ));
                        entities.push(
                            SketchEntity::new(
                                entity_id.clone(),
                                sketch_id.clone(),
                                SketchGeometry::Circle {
                                    center,
                                    radius: Length(radius),
                                },
                            )
                            .with_construction(
                                carrier_construction
                                    && carrier_radius.is_some_and(|carrier| {
                                        same_dimension_length(carrier, radius)
                                    }),
                            )
                            .with_native_ref(
                                carrier_radius
                                    .is_some_and(|carrier| same_dimension_length(carrier, radius))
                                    .then(|| carrier_ref.clone()),
                            )
                            .with_geometry_ref(parameter.native_ref.clone()),
                        );
                        sketch.profiles.push(vec![SketchEntityUse {
                            entity: entity_id,
                            reversed: false,
                        }]);
                    }
                    continue;
                }
            }
            continue 'feature;
        }
        let radial_records = owned_lanes
            .iter()
            .flat_map(|lane| {
                let range = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .map(|marker| marker.offset as usize)
                    .collect::<Vec<_>>();
                let start = range.iter().min().copied().unwrap_or(0);
                let end = range.iter().max().copied().unwrap_or(0);
                radial_records_by_lane
                    .get(lane.id.as_str())
                    .into_iter()
                    .flatten()
                    .filter(move |(offset, ..)| *offset >= start && *offset <= end)
                    .map(move |record| (*lane, *record))
            })
            .filter(|(lane, (offset, ..))| {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let carrier_ref = format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}");
                entities.iter().any(|entity| {
                    entity.sketch == *sketch_id
                        && entity.native_ref.as_deref() == Some(carrier_ref.as_str())
                        && matches!(entity.geometry, SketchGeometry::Native { .. })
                })
            })
            .collect::<Vec<_>>();
        let repeated_radial_sets = radial_records
            .iter()
            .flat_map(|(lane, (offset, radial_index, construction))| {
                if *construction {
                    return Vec::new();
                }
                let mut roster = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .filter(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>();
                roster.sort_unstable_by_key(|marker| marker.offset);
                radial_dimensions
                    .iter()
                    .filter_map(|(parameter, radius)| {
                        let pairs = terminal_repeated_radial_circle_pairs(
                            *radial_index,
                            &roster,
                            *radius / NATIVE_TO_IR,
                        )?;
                        Some((*lane, *offset, *parameter, *radius, pairs))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if let [(lane, offset, parameter, radius, pairs)] = repeated_radial_sets.as_slice() {
            let transformed = pairs
                .iter()
                .filter_map(|(center, _)| {
                    let [cu, cv] = center.coordinates_m?;
                    let native =
                        quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                    let centers = transforms
                        .get(native_ref)
                        .into_iter()
                        .flatten()
                        .filter_map(|transform| transform.apply(native))
                        .collect::<HashSet<_>>();
                    let centers = centers.into_iter().collect::<Vec<_>>();
                    let [(u, v)] = centers.as_slice() else {
                        return None;
                    };
                    Some(Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM))
                })
                .collect::<Vec<_>>();
            if transformed.len() == pairs.len() {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let carrier_ref = format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}");
                let pair_radial_object_indices = pairs
                    .iter()
                    .filter_map(|(_, radial)| radial.object_index)
                    .collect::<HashSet<_>>();
                let consumed_carrier_refs = radial_records_by_lane
                    .get(lane.id.as_str())
                    .into_iter()
                    .flatten()
                    .filter(|(candidate_offset, candidate_radial_index, construction)| {
                        !*construction
                            && lane.sketch_entities.iter().any(|marker| {
                                marker.feature_ref.as_deref() == Some(native_ref)
                                    && marker.offset == *candidate_offset as u64
                            })
                            && (*candidate_offset == *offset
                                || pair_radial_object_indices.contains(
                                    &u32::try_from(*candidate_radial_index).unwrap_or(u32::MAX),
                                ))
                    })
                    .map(|(candidate_offset, ..)| {
                        format!("sldprt:feature-input:sketch-entity#{lane_key}:{candidate_offset}")
                    })
                    .collect::<HashSet<_>>();
                let removed = entities
                    .iter()
                    .filter(|entity| {
                        entity.sketch == *sketch_id
                            && entity.native_ref.as_deref().is_some_and(|native_ref| {
                                consumed_carrier_refs.contains(native_ref)
                            })
                    })
                    .map(|entity| entity.id().clone())
                    .collect::<HashSet<_>>();
                entities.retain(|entity| !removed.contains(entity.id()));
                let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                else {
                    continue;
                };
                for profile in &mut sketch.profiles {
                    profile.retain(|usage| !removed.contains(&usage.entity));
                }
                sketch.profiles.retain(|profile| !profile.is_empty());
                for (index, center) in transformed.into_iter().enumerate() {
                    let entity_id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#repeated-radial-circle:{lane_key}:{offset}:{index}"
                    ));
                    entities.push(
                        SketchEntity::new(
                            entity_id.clone(),
                            sketch_id.clone(),
                            SketchGeometry::Circle {
                                center,
                                radius: Length(*radius),
                            },
                        )
                        .with_native_ref((index == pairs.len() - 1).then(|| carrier_ref.clone()))
                        .with_geometry_ref(parameter.native_ref.clone()),
                    );
                    sketch.profiles.push(vec![SketchEntityUse {
                        entity: entity_id,
                        reversed: false,
                    }]);
                }
                continue 'feature;
            }
        }
        if !radial_records.is_empty() {
            let radial_record_count = radial_records.len();
            let mut resolved = Vec::with_capacity(radial_records.len());
            for (lane, (offset, radial_index, construction)) in radial_records {
                let mut roster = lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .filter(|marker| marker.coordinates_m.is_some())
                    .collect::<Vec<_>>();
                roster.sort_unstable_by_key(|marker| marker.offset);
                let Some(radial) = roster.get(radial_index).copied() else {
                    continue;
                };
                let [ru, rv] = radial
                    .coordinates_m
                    .expect("coordinate markers carry coordinates");
                let mut candidates = markers
                    .iter()
                    .copied()
                    .filter(|marker| marker.id != radial.id)
                    .filter_map(|marker| {
                        let [cu, cv] = marker.coordinates_m?;
                        let radius = (ru - cu).hypot(rv - cv) * NATIVE_TO_IR;
                        let parameters = radial_dimensions
                            .iter()
                            .filter(|(_, candidate)| same_dimension_length(*candidate, radius))
                            .collect::<Vec<_>>();
                        let [(parameter, radius)] = parameters.as_slice() else {
                            return None;
                        };
                        Some((
                            quantize(Point2::new(cu, cv), QUANTUM),
                            marker,
                            *parameter,
                            *radius,
                        ))
                    })
                    .collect::<Vec<_>>();
                candidates.sort_unstable_by_key(|(center, marker, _, _)| (*center, marker.offset));
                candidates.dedup_by_key(|(center, _, _, _)| *center);
                let [(center, marker, parameter, radius)] = candidates.as_slice() else {
                    continue;
                };
                resolved.push((
                    lane,
                    offset,
                    construction,
                    *center,
                    *marker,
                    *parameter,
                    *radius,
                ));
            }
            if resolved.len() == radial_record_count {
                let transformed = resolved
                    .iter()
                    .filter_map(|record| {
                        let native = quantize(
                            Point2::new(
                                record.3 .0 as f64 * QUANTUM * NATIVE_TO_IR,
                                record.3 .1 as f64 * QUANTUM * NATIVE_TO_IR,
                            ),
                            QUANTUM,
                        );
                        let centers = transforms
                            .get(native_ref)
                            .into_iter()
                            .flatten()
                            .filter_map(|transform| transform.apply(native))
                            .collect::<HashSet<_>>();
                        let centers = centers.into_iter().collect::<Vec<_>>();
                        let [(u, v)] = centers.as_slice() else {
                            return None;
                        };
                        Some((
                            record,
                            Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM),
                        ))
                    })
                    .collect::<Vec<_>>();
                if transformed.len() == resolved.len() {
                    let carrier_refs = resolved
                        .iter()
                        .map(|(lane, offset, ..)| {
                            format!(
                                "sldprt:feature-input:sketch-entity#{}:{offset}",
                                lane.id
                                    .rsplit_once('#')
                                    .map_or(lane.id.as_str(), |(_, key)| key)
                            )
                        })
                        .collect::<HashSet<_>>();
                    let center_refs = resolved
                        .iter()
                        .map(|record| record.4.id.as_str())
                        .collect::<HashSet<_>>();
                    let removed = entities
                        .iter()
                        .filter(|entity| {
                            entity.sketch == *sketch_id
                                && entity.native_ref.as_deref().is_some_and(|reference| {
                                    carrier_refs.contains(reference)
                                        || (center_refs.contains(reference)
                                            && !matches!(
                                                entity.geometry,
                                                SketchGeometry::Point { .. }
                                            ))
                                })
                        })
                        .map(|entity| entity.id().clone())
                        .collect::<HashSet<_>>();
                    entities.retain(|entity| !removed.contains(entity.id()));
                    let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                    else {
                        continue;
                    };
                    for profile in &mut sketch.profiles {
                        profile.retain(|usage| !removed.contains(&usage.entity));
                    }
                    sketch.profiles.retain(|profile| !profile.is_empty());
                    for (record, center) in transformed {
                        let lane_key = record
                            .0
                            .id
                            .rsplit_once('#')
                            .map_or(record.0.id.as_str(), |(_, key)| key);
                        let entity_id = SketchEntityId(format!(
                            "sldprt:model:sketch-entity#radial-circle:{lane_key}:{}",
                            record.1
                        ));
                        entities.push(
                            SketchEntity::new(
                                entity_id.clone(),
                                sketch_id.clone(),
                                SketchGeometry::Circle {
                                    center,
                                    radius: Length(record.6),
                                },
                            )
                            .with_construction(record.2)
                            .with_native_ref(Some(format!(
                                "sldprt:feature-input:sketch-entity#{lane_key}:{}",
                                record.1
                            )))
                            .with_geometry_ref(record.5.native_ref.clone()),
                        );
                        if !record.2 {
                            sketch.profiles.push(vec![SketchEntityUse {
                                entity: entity_id,
                                reversed: false,
                            }]);
                        }
                    }
                    continue;
                }
            }
        }
        let centers = markers
            .iter()
            .copied()
            .filter(|marker| marker.kind == SketchInputKind::LineOrCircle)
            .collect::<Vec<_>>();
        let radial = markers
            .iter()
            .copied()
            .filter(|marker| {
                matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            })
            .collect::<Vec<_>>();
        let [center] = centers.as_slice() else {
            continue;
        };
        let center_marker = *center;
        if radial.len() != radial_dimensions.len() {
            continue;
        }
        let [cu, cv] = center
            .coordinates_m
            .expect("coordinate markers carry coordinates");
        let matches = radial_dimensions
            .iter()
            .map(|(_, radius)| {
                radial
                    .iter()
                    .enumerate()
                    .filter_map(|(index, marker)| {
                        let [u, v] = marker
                            .coordinates_m
                            .expect("coordinate markers carry coordinates");
                        same_dimension_length((u - cu).hypot(v - cv) * NATIVE_TO_IR, *radius)
                            .then_some(index)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if matches.iter().any(|matches| matches.len() != 1)
            || matches
                .iter()
                .map(|matches| matches[0])
                .collect::<HashSet<_>>()
                .len()
                != radial.len()
        {
            continue;
        }
        let native_center = quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
        let centers = transforms
            .get(native_ref)
            .into_iter()
            .flatten()
            .filter_map(|transform| transform.apply(native_center))
            .collect::<HashSet<_>>();
        let centers = centers.into_iter().collect::<Vec<_>>();
        let [(u, v)] = centers.as_slice() else {
            continue;
        };
        let center = Point2::new(*u as f64 * QUANTUM, *v as f64 * QUANTUM);
        let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id) else {
            continue;
        };
        for (parameter, radius) in radial_dimensions {
            let Some(construction) = native_dimensioned_circle_construction_state(
                lanes,
                native_ref,
                center_marker,
                radius,
            ) else {
                continue;
            };
            if entities.iter().any(|entity| {
                entity.sketch == *sketch_id
                    && matches!(&entity.geometry, SketchGeometry::Circle { center: existing, radius: existing_radius }
                        if quantize(*existing, QUANTUM) == quantize(center, QUANTUM)
                            && same_dimension_length(existing_radius.0, radius))
            }) {
                continue;
            }
            let feature_key = feature
                .id
                .0
                .rsplit_once('#')
                .map_or(feature.id.as_str(), |(_, key)| key);
            let entity_id = SketchEntityId(format!(
                "sldprt:model:sketch-entity#marker-circle:{}:{}",
                feature_key, parameter.ordinal
            ));
            entities.push(
                SketchEntity::new(
                    entity_id.clone(),
                    sketch_id.clone(),
                    SketchGeometry::Circle {
                        center,
                        radius: Length(radius),
                    },
                )
                .with_construction(construction)
                .with_geometry_ref(parameter.native_ref.clone()),
            );
            sketch.profiles.push(vec![SketchEntityUse {
                entity: entity_id,
                reversed: false,
            }]);
        }
    }
}

#[cfg(test)]
mod dimensions_tests;
