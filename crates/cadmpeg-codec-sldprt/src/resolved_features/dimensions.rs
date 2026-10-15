//! Dimensioned sketch geometry and radial circle records.

use super::grid::GridCoordinate;

use super::endpoints::{
    compact_indexed_curve_record_end, marker_profile_curve_role, minor_arc_angles,
};
use super::grid::quantize;
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
    marker_transforms_with_frame_fallback,
};
use super::typed_relations::marker_curve_endpoint_markers;
use super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_ANGLE_TOLERANCE};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
};
use cadmpeg_ir::{
    features::{FeatureDefinition, FeatureOperation},
    scalar::{Angle, Length},
};
use std::collections::{HashMap, HashSet};

const EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8: f64 = 1.0e-8;

#[derive(Debug, Clone)]
struct DimensionedArcNative {
    center: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    endpoints: Option<[String; 2]>,
}

#[derive(Debug, Clone)]
enum DimensionedCurveNative {
    Circle { center: [f64; 2] },
    Arc(DimensionedArcNative),
}

struct DimensionedRelationCarrier<'a> {
    marker: &'a SketchInputEntity,
    geometry: DimensionedCarrierGeometry,
    construction: Option<bool>,
}

enum DimensionedCarrierGeometry {
    Center([f64; 2]),
    Curve(DimensionedCurveNative),
}

impl DimensionedRelationCarrier<'_> {
    fn center(&self) -> [f64; 2] {
        match &self.geometry {
            DimensionedCarrierGeometry::Center(center) => *center,
            DimensionedCarrierGeometry::Curve(curve) => curve.center(),
        }
    }

    fn curve(&self) -> Option<&DimensionedCurveNative> {
        match &self.geometry {
            DimensionedCarrierGeometry::Center(_) => None,
            DimensionedCarrierGeometry::Curve(curve) => Some(curve),
        }
    }
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
    if center.feature_ref.as_deref() != Some(feature) || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let [cu, cv] = center
        .coordinates_m
        .map(cadmpeg_ir::units::FiniteVector::get)?;
    let mut state = None;
    for lane in lanes {
        if !lane.sketch_entities.iter().any(|marker| {
            marker.id() == center.id() && marker.feature_ref.as_deref() == Some(feature)
        }) {
            continue;
        }
        let mut roster = Vec::new();
        for marker in &lane.sketch_entities {
            if marker.feature_ref.as_deref() != Some(feature) || marker.coordinates_m.is_none() {
                continue;
            }
            roster.push(marker);
        }
        roster.sort_unstable_by_key(|left| left.offset());
        let records = radial_circle_records(&lane.native_payload);
        for (_, radial_index, construction) in records {
            let Some(radial) = roster.get(radial_index) else {
                continue;
            };
            let Some([ru, rv]) = radial
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                continue;
            };
            if same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, radius) {
                if state.is_some_and(|previous| previous != construction) {
                    return None;
                }
                state = Some(construction);
            }
        }
    }
    state
}

fn native_radial_record_for_marker(
    lanes: &[FeatureInputLane],
    feature: &str,
    marker_id: &str,
) -> Option<(usize, bool)> {
    lanes.iter().find_map(|lane| {
        let marker = lane.sketch_entities.iter().find(|marker| {
            marker.id() == marker_id && marker.feature_ref.as_deref() == Some(feature)
        })?;
        let offset = usize::try_from(marker.offset()).ok()?;
        compact_radial_circle_index(&lane.native_payload, offset)
            .or_else(|| {
                extended_terminal_repeated_radial_circle_index(&lane.native_payload, offset)
            })
            .map(|radial_index| {
                (
                    radial_index,
                    marker_profile_curve_role(&lane.native_payload, offset) == Some(2),
                )
            })
            .or_else(|| {
                let offset = usize::try_from(marker.offset()).ok()?;
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
    let Some([cu, cv]) = center
        .coordinates_m
        .map(cadmpeg_ir::units::FiniteVector::get)
    else {
        return false;
    };
    let candidate_count = lane
        .sketch_entities
        .iter()
        .filter(|candidate| {
            candidate.feature_ref == center.feature_ref
                && candidate.offset() > center.offset()
                && matches!(
                    candidate.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
        })
        .filter(|candidate| {
            let Some([ru, rv]) = candidate
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return false;
            };
            let radius = (ru - cu).hypot(rv - cv) * 1000.0;
            radius.is_finite() && same_dimension_length(radius, expected_radius)
        })
        .take(2)
        .count();
    candidate_count == 1
}

fn dimensioned_arc_native_geometry(
    lanes: &[FeatureInputLane],
    marker: &SketchInputEntity,
    expected_radius: f64,
) -> Option<DimensionedCurveNative> {
    if marker.kind() != SketchInputKind::Arc {
        return None;
    }
    let mut found = None;
    for lane in lanes {
        if lane
            .sketch_entities
            .iter()
            .any(|candidate| candidate.id() == marker.id())
        {
            found = Some(lane);
            break;
        }
    }
    let lane = found?;
    let mut object_markers = Vec::new();
    let mut markers_by_id = HashMap::<&str, &SketchInputEntity>::new();
    for candidate in &lane.sketch_entities {
        object_markers.push(candidate);
        markers_by_id.contains_key(candidate.id());
        markers_by_id.insert(candidate.id(), candidate);
    }
    let endpoints = marker_curve_endpoint_markers(
        &lane.native_payload,
        marker,
        &markers_by_id,
        &object_markers,
    );
    let inline = usize::try_from(marker.offset())
        .ok()
        .and_then(|offset| inline_arc_coordinates(&lane.native_payload, offset))
        .map(|coordinates| coordinates.map(cadmpeg_ir::units::FiniteVector::get));
    let [center, start, end] = if let Some(coordinates) = inline {
        coordinates
    } else if let [first, second] = endpoints.as_slice() {
        match (
            marker.coordinates_m,
            first.coordinates_m,
            second.coordinates_m,
        ) {
            (Some(center), Some(start), Some(end)) => [center.get(), start.get(), end.get()],
            _ => return None,
        }
    } else {
        // The endpoint search does not establish a bounded arc. Keep the
        // existing exact radial-witness fallback.
        if unique_native_radial_witness(lane, marker, expected_radius) {
            let center = marker
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)?;
            return Some(DimensionedCurveNative::Circle { center });
        }
        return None;
    };
    let endpoint_pair = if let [first, second] = endpoints.as_slice() {
        let first = first.id().to_string();
        let second = second.id().to_string();
        Some([first, second])
    } else {
        None
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
        endpoints: endpoint_pair,
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
    let mut unique = None;
    for handle in &lane.sketch_entities {
        if handle.feature_ref.as_deref() != Some(feature)
            || handle.offset() >= operand.offset
            || handle.coordinates_m.is_some()
            || handle.kind() != SketchInputKind::LineOrCircle
        {
            continue;
        }
        let [first, second] = handle.links() else {
            continue;
        };
        if first.entity_ref != second.entity_ref || first.local_id != second.local_id {
            continue;
        }
        let Some(arc) = lane.sketch_entities.iter().find(|candidate| {
            candidate.id() == first.entity_ref
                && candidate.feature_ref.as_deref() == Some(feature)
                && candidate.offset() < handle.offset()
                && candidate.local_id() == Some(u32::from(first.local_id))
                && candidate.coordinates_m.is_some()
                && candidate.kind() == SketchInputKind::Arc
        }) else {
            continue;
        };
        let Some(curve) = dimensioned_arc_native_geometry(lanes, arc, expected_radius) else {
            continue;
        };
        if unique.is_some() {
            return None;
        }
        unique = Some((arc, curve));
    }
    unique
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
    let mut unique = None;
    for marker in &lane.sketch_entities {
        if marker.feature_ref.as_deref() != Some(feature) {
            continue;
        }
        let Some(center) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let curve = match marker.kind() {
            SketchInputKind::Arc => {
                let Some(curve) = dimensioned_arc_native_geometry(lanes, marker, expected_radius)
                else {
                    continue;
                };
                curve
            }
            SketchInputKind::LineOrCircle => {
                if !unique_native_radial_witness(lane, marker, expected_radius) {
                    continue;
                }
                DimensionedCurveNative::Circle { center }
            }
            _ => continue,
        };
        if unique.is_some() {
            return None;
        }
        unique = Some((marker, curve));
    }
    unique
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
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
    });
    if let Some((marker, center)) = declared_slot_handle_dimension_center(lanes, feature, operand) {
        let center = center
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)?;
        return Some(DimensionedRelationCarrier {
            marker,
            geometry: DimensionedCarrierGeometry::Center(center),
            construction: Some(true),
        });
    }
    if operand.kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_836E) {
        let marker = declared_entity_handle_indexed_circle_dimension_center(
            lanes, feature, operand, radius,
        )?;
        let center = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)?;
        return Some(DimensionedRelationCarrier {
            marker,
            geometry: DimensionedCarrierGeometry::Center(center),
            construction: Some(false),
        });
    }
    if matches!(
        operand.kind,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D4 | NativeOperandTag::TAG_80D5)
    ) {
        let marker = declared_entity_handle_point_dimension_center(lanes, feature, operand)?;
        let center = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)?;
        return Some(DimensionedRelationCarrier {
            marker,
            geometry: DimensionedCarrierGeometry::Center(center),
            construction: Some(false),
        });
    }
    let explicit_circular_marker = explicit.is_some_and(|marker| {
        matches!(
            marker.kind(),
            SketchInputKind::LineOrCircle | SketchInputKind::Arc
        )
    });
    let mut explicit_current_arc_handle_point = false;
    if let Some((marker, offset)) = explicit
        .filter(|_| explicit_point_marker)
        .and_then(|marker| {
            usize::try_from(marker.offset())
                .ok()
                .map(|offset| (marker, offset))
        })
    {
        for lane in lanes {
            if lane.sketch_entities.iter().any(|candidate| {
                candidate.id() == marker.id() && candidate.feature_ref.as_deref() == Some(feature)
            }) && current_geometry_locus_arc_handle_point(&lane.native_payload, offset)
            {
                explicit_current_arc_handle_point = true;
                break;
            }
        }
    }
    let declared_owner = declared_entity_handle_owner(lanes, operand);
    let declared = declared_entity_handle_circular_marker(lanes, feature, operand, radius);
    let declared_entity_handle = !matches!(declared_owner, DeclaredEntityHandleOwner::Absent);
    let (marker, encoded_radius, fallback_curve) = if let Some((marker, radius)) = declared {
        (marker, Some(radius), None)
    } else if declared_entity_handle && explicit_current_arc_handle_point {
        let marker = explicit?;
        (marker, None, None)
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
                    marker.kind(),
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
    let curve = if fallback_curve.is_some() {
        fallback_curve
    } else if marker.kind() == SketchInputKind::Arc {
        dimensioned_arc_native_geometry(lanes, marker, radius)
    } else {
        None
    };
    if marker.kind() == SketchInputKind::Arc && curve.is_none() {
        return None;
    }
    if !matches!(
        marker.kind(),
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
    let mut construction =
        native_dimensioned_circle_construction_state(lanes, feature, marker, radius);
    if construction.is_none() {
        construction =
            direct_point_dimension_center(lanes, feature, operand, radius).map(|_| false);
    }
    let construction = construction.or_else(|| {
        declared_entity_handle.then_some(false).or_else(|| {
            matches!(
                marker.kind(),
                SketchInputKind::LineOrCircle | SketchInputKind::Arc
            )
            .then_some(false)
        })
    });
    let geometry = match curve {
        Some(curve) => DimensionedCarrierGeometry::Curve(curve),
        None => {
            let center = marker
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)?;
            DimensionedCarrierGeometry::Center(center)
        }
    };
    Some(DimensionedRelationCarrier {
        marker,
        geometry,
        construction,
    })
}

/// A dimensioned arc's sketch geometry beside the radius it was built from.
struct DimensionedArcGeometry {
    geometry: SketchGeometry,
    radius: f64,
    endpoint_refs: Vec<String>,
}

fn transformed_dimensioned_arc(
    transform: super::transforms::MarkerTransform,
    arc: &DimensionedArcNative,
    native_to_ir: f64,
    quantum: f64,
) -> Option<DimensionedArcGeometry> {
    let geometry = (|| {
        let transform_point = |[u, v]: [f64; 2]| {
            let point = transform.apply(quantize(
                Point2::new(u * native_to_ir, v * native_to_ir),
                quantum,
            ))?;
            Some(Point2::new(
                cadmpeg_core::convert::f64_from_i64(point.0)? * quantum,
                cadmpeg_core::convert::f64_from_i64(point.1)? * quantum,
            ))
        };
        let center = transform_point(arc.center)?;
        let mut start = transform_point(arc.start)?;
        let mut end = transform_point(arc.end)?;
        let radius = (start.u - center.u).hypot(start.v - center.v);
        let end_radius = (end.u - center.u).hypot(end.v - center.v);
        let start_angle = (start.v - center.v).atan2(start.u - center.u);
        let end_angle = (end.v - center.v).atan2(end.u - center.u);
        let (start_angle, end_angle, reversed) = minor_arc_angles(start_angle, end_angle);
        if reversed {
            std::mem::swap(&mut start, &mut end);
        }
        let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
        (radius.is_finite()
            && radius > quantum
            && same_dimension_length(radius, end_radius)
            && sweep > SKETCH_ANGLE_TOLERANCE
            && sweep <= std::f64::consts::PI + SKETCH_ANGLE_TOLERANCE)
            .then_some((
                SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                    center,
                    radius: Length::new(radius)?,
                    start_angle: Angle::new(start_angle)?,
                    end_angle: Angle::new(end_angle)?,
                })
                .ok()?,
                radius,
                reversed,
            ))
    })();
    let (geometry, radius, reversed) = geometry?;
    let mut endpoint_refs = Vec::new();
    if let Some(endpoints) = &arc.endpoints {
        let order = if reversed { [1, 0] } else { [0, 1] };
        for index in order {
            let text = endpoints[index].clone();
            endpoint_refs.push(text);
        }
    }
    Some(DimensionedArcGeometry {
        geometry,
        radius,
        endpoint_refs,
    })
}

/// Materialize dimensioned circular sketch geometry omitted by a selected-profile stream.
pub(crate) fn project_dimensioned_sketch_geometry(
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let mut sketches_by_feature = HashMap::<&str, _>::new();
    for feature in features {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        sketches_by_feature.insert(native, sketch);
    }
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let mut parameters_by_id = HashMap::<&cadmpeg_ir::features::ParameterId, _>::new();
    let mut parameter_key_bytes = 0usize;
    for parameter in parameters {
        parameter_key_bytes = parameter_key_bytes.max(parameter.id.as_str().len());
        parameters_by_id.insert(&parameter.id, parameter);
    }
    let relation_parameter = |relation: &FeatureInputRelationInstance| {
        ownership
            .get(&relation.id)?
            .as_ref()
            .and_then(|parameter| parameters_by_id.get(parameter))
            .copied()
    };
    let mut markers_by_id = HashMap::<&str, _>::new();
    for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
        markers_by_id.insert(marker.id(), marker);
    }
    let marker_transforms =
        marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let mut transforms = HashMap::<&str, _>::new();
    for (feature, sketch_id) in &sketches_by_feature {
        let mut circles = Vec::new();
        for relation in lanes.iter().flat_map(|lane| &lane.relation_instances) {
            if relation.feature_ref != *feature
                || relation.family != FeatureInputRelationFamily::CircleDiameter
            {
                continue;
            }
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let Some(parameter) = relation_parameter(relation) else {
                continue;
            };
            let Some(cadmpeg_ir::features::ParameterValue::Length(value)) =
                parameter.value.as_ref()
            else {
                continue;
            };
            let radius = match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
                None => continue,
            };
            if !(radius.is_finite() && radius > 0.0) {
                continue;
            }
            let Some(carrier) = dimensioned_relation_carrier(
                lanes,
                &markers_by_id,
                relation.feature_ref.as_str(),
                operand,
                radius,
            ) else {
                continue;
            };
            circles.push((
                quantize(
                    Point2::new(
                        carrier.center()[0] * NATIVE_TO_IR,
                        carrier.center()[1] * NATIVE_TO_IR,
                    ),
                    QUANTUM,
                ),
                GridCoordinate::new(radius, QUANTUM),
            ));
        }
        let candidates = if let Some(existing) = marker_transforms.get(*feature) {
            let mut candidates = Vec::new();
            candidates.extend_from_slice(existing);
            candidates
        } else if let Some(sketch) = sketches.iter().find(|sketch| sketch.id == **sketch_id) {
            dimensioned_circle_surface_transforms(sketch, surfaces, &circles, QUANTUM)
        } else {
            Vec::new()
        };
        let candidates =
            if let Some(sketch) = sketches.iter().find(|sketch| sketch.id == **sketch_id) {
                marker_transforms_with_frame_fallback(candidates, sketch, QUANTUM)
            } else {
                candidates
            };
        let Some(transform) = dimensioned_circle_transform(&candidates, &circles) else {
            continue;
        };
        transforms.insert(*feature, transform);
    }
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
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
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
                    carrier.center()[0] * NATIVE_TO_IR,
                    carrier.center()[1] * NATIVE_TO_IR,
                ),
                QUANTUM,
            );
            let Some(center) = transform.apply(native) else {
                continue;
            };
            let (Some(center_u), Some(center_v)) = (
                cadmpeg_core::convert::f64_from_i64(center.0),
                cadmpeg_core::convert::f64_from_i64(center.1),
            ) else {
                continue;
            };
            let center = Point2::new(center_u * QUANTUM, center_v * QUANTUM);
            if entities.iter().any(|entity| {
                entity.sketch == **sketch
                    && entity.geometry_ref.as_deref() == Some(relation.id.as_str())
            }) {
                continue;
            }
            if carrier
                .curve()
                .and_then(DimensionedCurveNative::arc)
                .is_none()
                && entities.iter().any(|entity| {
                    entity.sketch == **sketch
                        && match entity.geometry.definition() {
                            SketchGeometryDefinition::Circle {
                                center: existing,
                                radius: existing_radius,
                            } => {
                                quantize(existing.get(), QUANTUM) == quantize(center, QUANTUM)
                                    && same_dimension_length(existing_radius.get(), radius)
                            }
                            _ => false,
                        }
                })
            {
                continue;
            }
            let (geometry, endpoint_refs) = if let Some(arc) =
                carrier.curve().and_then(DimensionedCurveNative::arc)
            {
                let Some(arc) = transformed_dimensioned_arc(*transform, arc, NATIVE_TO_IR, QUANTUM)
                else {
                    continue;
                };
                if !same_dimension_length(arc.radius, radius) {
                    continue;
                }
                (arc.geometry, arc.endpoint_refs)
            } else {
                (
                    match SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                        center,
                        radius: cadmpeg_ir::scalar::Length::new(radius).ok_or_else(|| {
                            cadmpeg_core::CodecError::Malformed(
                                "SolidWorks projected length must be finite".into(),
                            )
                        })?,
                    }) {
                        Ok(geometry) => geometry,
                        Err(_) => continue,
                    },
                    Vec::new(),
                )
            };
            let entity_text = format!(
                "sldprt:model:sketch-entity#dimension:{lane_key}:{}",
                relation.offset
            );
            let Ok(entity_id) = SketchEntityId::mint(entity_text) else {
                continue;
            };
            let sketch_text = sketch.as_str().to_owned();
            let Ok(sketch_id) = cadmpeg_ir::sketches::SketchId::mint(sketch_text) else {
                continue;
            };
            let native_ref = carrier.marker.id().to_string();
            let geometry_ref = relation.id.clone();
            entities.push(
                SketchEntity::new(entity_id, sketch_id, geometry)
                    .with_construction(construction)
                    .with_native_ref(Some(native_ref))
                    .with_geometry_ref(Some(geometry_ref))
                    .with_endpoint_refs(endpoint_refs),
            );
        }
    }

    Ok(())
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
) -> Result<(), cadmpeg_core::CodecError> {
    let mut sketches_by_feature = HashMap::<&str, _>::new();
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        sketches_by_feature.contains_key(native_ref);
        sketches_by_feature.insert(native_ref, sketch);
    }
    let ownership = owned_relation_parameters(features, parameters, lanes);
    let mut parameters_by_id = HashMap::<&cadmpeg_ir::features::ParameterId, _>::new();
    for parameter in parameters {
        parameters_by_id.contains_key(&parameter.id);
        parameters_by_id.insert(&parameter.id, parameter);
    }
    let mut markers_by_id = HashMap::<&str, _>::new();
    for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
        markers_by_id.contains_key(marker.id());
        markers_by_id.insert(marker.id(), marker);
    }

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
            let Some(parameter_id) = ownership.get(&relation.id).and_then(Option::as_ref) else {
                continue;
            };
            let Some(parameter) = parameters_by_id.get(parameter_id) else {
                continue;
            };
            let Some(radius) = radial_dimension_radius(parameter) else {
                continue;
            };
            let marker_id = if let Some(reference) = operand.entity_ref.as_deref() {
                Some(reference)
            } else {
                implicit_circle_marker(
                    lanes,
                    relation.feature_ref.as_str(),
                    operand.kind,
                    operand.entity_index,
                    radius,
                )
                .map(|(marker, _)| marker.id())
            };
            let Some(marker_id) = marker_id else {
                continue;
            };
            let Some(marker) = markers_by_id.get(marker_id).copied() else {
                continue;
            };
            if !matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                continue;
            }
            let mut centers = entities.iter().filter(|entity| {
                entity.sketch == **sketch
                    && entity.native_ref.as_deref() == Some(marker_id)
                    && matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    )
            });
            let (Some(center_entity), None) = (centers.next(), centers.next()) else {
                continue;
            };
            let SketchGeometryDefinition::Point { position: center } =
                *center_entity.geometry.definition()
            else {
                continue;
            };
            let mut construction = native_dimensioned_circle_construction_state(
                lanes,
                relation.feature_ref.as_str(),
                marker,
                radius,
            );
            if construction.is_none() {
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
                construction = explicit_center.then_some(false);
            }
            let construction =
                construction.or_else(|| lane.native_payload.is_empty().then_some(false));
            let Some(construction) = construction else {
                continue;
            };
            if entities.iter().any(|entity| {
                entity.sketch == **sketch
                    && matches!(entity.geometry.definition(), SketchGeometryDefinition::Circle { center: existing, radius: existing_radius }
                        if quantize(existing.get(), EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8) == quantize(center.get(), EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8)
                            && same_dimension_length(existing_radius.get(), radius))
            }) {
                continue;
            }
            let entity_id = format!(
                "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}",
                relation.offset
            );
            let Ok(entity_id) = SketchEntityId::mint(entity_id) else {
                continue;
            };
            let Some(geometry) = cadmpeg_ir::scalar::PositiveLength::try_from(
                Length::new(radius).ok_or_else(|| {
                    cadmpeg_core::CodecError::Malformed(
                        "SolidWorks projected length must be finite".into(),
                    )
                })?,
            )
            .ok()
            .and_then(|radius| {
                SketchGeometry::from_parts(SketchGeometryDefinition::Circle { center, radius }).ok()
            }) else {
                continue;
            };
            let sketch_id = sketch.as_str().to_owned();
            let Ok(sketch_id) = cadmpeg_ir::sketches::SketchId::mint(sketch_id) else {
                continue;
            };
            let native_ref = marker.id().to_string();
            let geometry_ref = relation.id.clone();
            entities.push(
                SketchEntity::new(entity_id, sketch_id, geometry)
                    .with_construction(construction)
                    .with_native_ref(Some(native_ref))
                    .with_geometry_ref(Some(geometry_ref)),
            );
        }
    }

    Ok(())
}

fn compact_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
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
        && offset
            .checked_add(104)
            .is_some_and(|at| sketch_marker_prefix_at(payload, at));
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
    // Three five-byte prefix comparisons precede every fixed-layout probe.
    let mut records = Vec::new();
    let Some(scan_len) = payload.len().checked_sub(LEGACY_SKETCH_MARKER.len() - 1) else {
        return records;
    };
    for offset in 0..scan_len {
        if !sketch_marker_prefix_at(payload, offset) {
            continue;
        }
        let Some(radial) = compact_radial_circle_index(payload, offset)
            .or_else(|| extended_terminal_repeated_radial_circle_index(payload, offset))
        else {
            continue;
        };
        records.push((
            offset,
            radial,
            marker_profile_curve_role(payload, offset) == Some(2),
        ));
    }
    records
}

fn extended_terminal_repeated_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
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

fn terminal_repeated_radial_circle_pairs<'a>(
    radial_index: usize,
    roster: &[&'a SketchInputEntity],
    radius: f64,
) -> Option<Vec<(&'a SketchInputEntity, &'a SketchInputEntity)>> {
    if radial_index != roster.len() || radius <= 0.0 || !radius.is_finite() {
        return None;
    }
    let terminal = roster.last().copied()?;
    let mut pairs = (roster
        .iter()
        .zip(roster.iter().skip(1))
        .filter_map(|(center, radial)| {
            let center_index = center.object_index()?;
            let radial_index = radial.object_index()?;
            if center_index != radial_index.checked_add(1)? {
                return None;
            }
            let [cu, cv] = center.coordinates_m?.get();
            let [ru, rv] = radial.coordinates_m?.get();
            same_dimension_length((ru - cu).hypot(rv - cv), radius).then_some((*center, *radial))
        }))
    .collect::<Vec<_>>();
    if pairs.len() < 2 || pairs.last().map(|(_, radial)| radial.id()) != Some(terminal.id()) {
        return None;
    }
    let mut used = HashSet::new();
    for (center, radial) in &pairs {
        if !used.insert(center.id()) || !used.insert(radial.id()) {
            return None;
        }
    }
    pairs.sort_unstable_by_key(|(left, _)| left.offset());
    Some(pairs)
}

pub(super) fn extended_radial_circle_index(payload: &[u8], offset: usize) -> Option<usize> {
    let index = View::u16_le_at(payload, offset + 64)?;
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
    supported.then_some(usize::from(index))
}

fn radial_dimension_radius(parameter: &cadmpeg_ir::features::DesignParameter) -> Option<f64> {
    let cadmpeg_ir::features::ParameterValue::Length(value) = parameter.value.as_ref()? else {
        return None;
    };
    let radius = match parameter.display {
        Some(cadmpeg_ir::features::DimensionDisplay::Radius) => value.get(),
        Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => value.get() * 0.5,
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
    let mut replacements = HashMap::<&str, &SketchEntityId>::new();
    for relation in lanes.iter().flat_map(|lane| &lane.relation_instances) {
        if relation.feature_ref != feature
            || relation.family != FeatureInputRelationFamily::CircleDiameter
        {
            continue;
        }
        let ([operand] | [_, operand]) = relation.operands.as_slice() else {
            continue;
        };
        let Some(marker_id) = operand.entity_ref.as_deref() else {
            continue;
        };
        let mut candidate = None;
        let mut ambiguous = false;
        for marker in lanes.iter().flat_map(|lane| &lane.sketch_entities) {
            if marker.id() != marker_id || marker.feature_ref.as_deref() != Some(feature) {
                continue;
            }
            if candidate.is_some() {
                ambiguous = true;
                break;
            }
            candidate = Some(marker);
        }
        let Some(marker) = candidate.filter(|_| !ambiguous) else {
            continue;
        };
        if marker.kind() != SketchInputKind::LineOrCircle || marker.coordinates_m.is_none() {
            continue;
        }
        let mut typed_candidate = None;
        let mut ambiguous = false;
        for entity in &*entities {
            if entity.sketch != *sketch_id
                || entity.native_ref.as_deref() != Some(marker.id())
                || entity.geometry_ref.as_deref() != Some(relation.id.as_str())
                || !matches!(
                    *entity.geometry.definition(),
                    SketchGeometryDefinition::Circle { .. }
                )
            {
                continue;
            }
            if typed_candidate.is_some() {
                ambiguous = true;
                break;
            }
            typed_candidate = Some(entity);
        }
        let Some(typed_entity) = typed_candidate.filter(|_| !ambiguous) else {
            continue;
        };
        replacements.insert(marker.id(), typed_entity.id());
    }
    if replacements.is_empty() {
        return;
    }
    let mut removed = HashMap::<SketchEntityId, SketchEntityId>::new();
    for entity in &*entities {
        if entity.sketch != *sketch_id
            || !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            )
        {
            continue;
        }
        let Some(replacement) = entity
            .native_ref
            .as_deref()
            .and_then(|native| replacements.get(native))
        else {
            continue;
        };
        removed.insert(
            SketchEntityId::clone(entity.id()),
            SketchEntityId::clone(replacement),
        );
    }
    if removed.is_empty() {
        return;
    }
    if let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id) {
        let mut profiles = Vec::new();
        for profile in &sketch.profiles {
            let mut present = HashSet::<&SketchEntityId>::new();
            for usage in profile {
                if removed.contains_key(&usage.entity) || present.contains(&usage.entity) {
                    continue;
                }
                present.insert(&usage.entity);
            }
            let mut updated = Vec::new();
            for usage in profile {
                let id = if let Some(replacement) = removed.get(&usage.entity) {
                    if present.contains(replacement) {
                        continue;
                    }
                    present.insert(replacement);
                    replacement
                } else {
                    &usage.entity
                };
                updated.push(SketchEntityUse {
                    entity: SketchEntityId::clone(id),
                    reversed: usage.reversed,
                });
            }
            if !updated.is_empty() {
                profiles.push(updated);
            }
        }
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles) else {
            return;
        };
        sketch.profiles = profiles;
    }
    entities.retain(|entity| !removed.contains_key(entity.id()));
}

fn marker_circle_carrier_reference(lane_key: &str, offset: usize) -> String {
    format!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}")
}

fn unique_marker_circle_center(
    transforms: &[super::transforms::MarkerTransform],
    native: super::grid::GridPoint,
    quantum: f64,
) -> Option<Point2> {
    let mut unique = None;
    for transform in transforms {
        let Some(center) = transform.apply(native) else {
            continue;
        };
        if unique.is_some_and(|previous| previous != center) {
            return None;
        }
        unique = Some(center);
    }
    unique.and_then(|center| super::grid::GridPoint::from(center).point(quantum))
}

fn marker_circle_one_to_one(
    markers: &[(&SketchInputEntity, [f64; 2])],
    dimensions: &[(&cadmpeg_ir::features::DesignParameter, f64)],
    center: [f64; 2],
) -> bool {
    if markers.len() != dimensions.len() {
        return false;
    }
    let [cu, cv] = center;
    let mut used = HashSet::new();
    for (_, radius) in dimensions {
        let mut unique = None;
        for (index, (_, [ru, rv])) in markers.iter().enumerate() {
            if !same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, *radius) {
                continue;
            }
            if unique.is_some() {
                return false;
            }
            unique = Some(index);
        }
        let Some(index) = unique else {
            return false;
        };
        if !used.insert(index) {
            return false;
        }
    }
    used.len() == markers.len()
}

fn append_marker_circle(
    entities: &mut Vec<SketchEntity>,
    sketch: &mut Sketch,
    entity: SketchEntity,
    profile: bool,
) {
    if profile {
        let id = SketchEntityId::clone(entity.id());
        sketch.profiles.push_single(SketchEntityUse {
            entity: id,
            reversed: false,
        });
    }
    entities.push(entity);
}

fn copy_marker_circle_sketch_id(
    id: &cadmpeg_ir::sketches::SketchId,
) -> Result<cadmpeg_ir::sketches::SketchId, cadmpeg_core::CodecError> {
    let text = id.as_str().to_owned();
    cadmpeg_ir::sketches::SketchId::mint(text).map_err(|_| {
        cadmpeg_core::CodecError::malformed("invalid SLDPRT marker circle sketch identity")
    })
}

/// Materialize marker-only circles whose radial witnesses have exact radial
/// dimensions, including repeated circles constrained to the same radius.
pub(crate) fn project_marker_dimensioned_circles(
    entities: &mut Vec<SketchEntity>,
    sketches: &mut [Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let transforms = marker_transform_candidates_by_feature(features, sketches, entities, lanes);
    let mut radial_records_by_lane = HashMap::<&str, Vec<_>>::new();
    for lane in lanes {
        let records = radial_circle_records(&lane.native_payload);
        radial_records_by_lane.insert(lane.id.as_str(), records);
    }
    'feature: for feature in features {
        let (
            Some(native_ref),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id)),
            }),
        ) = (
            feature.native_ref.as_deref(),
            feature.evaluation.definition(),
        )
        else {
            continue;
        };
        reconcile_direct_circle_dimension_carriers(
            entities, sketches, sketch_id, native_ref, lanes,
        );
        let mut radial_dimensions = Vec::new();
        for parameter in parameters {
            if parameter.owner.as_ref() != Some(&feature.id) {
                continue;
            }
            let Some(radius) = radial_dimension_radius(parameter) else {
                continue;
            };
            radial_dimensions.push((parameter, radius));
        }
        if radial_dimensions.is_empty() {
            continue;
        }
        let feature_key = feature
            .id
            .as_str()
            .rsplit_once('#')
            .map_or(feature.id.as_str(), |(_, key)| key);
        let mut owned_lanes = Vec::new();
        for lane in lanes {
            if lane
                .sketch_entities
                .iter()
                .any(|marker| marker.feature_ref.as_deref() == Some(native_ref))
            {
                owned_lanes.push(lane);
            }
        }
        let mut markers = Vec::new();
        for marker in owned_lanes.iter().flat_map(|lane| &lane.sketch_entities) {
            if marker.feature_ref.as_deref() != Some(native_ref) {
                continue;
            }
            let Some(coordinates) = marker.coordinates_m else {
                continue;
            };
            markers.push((marker, coordinates.get()));
        }
        let feature_transforms: &[super::transforms::MarkerTransform] =
            transforms.get(native_ref).map_or(&[], Vec::as_slice);
        let native_carriers = (entities.iter().filter(|entity| {
            entity.sketch == *sketch_id
                && matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Native { .. }
                )
        }))
        .collect::<Vec<_>>();
        let has_resolved_curves = entities.iter().any(|entity| {
            entity.sketch == *sketch_id
                && matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                        | SketchGeometryDefinition::Arc { .. }
                        | SketchGeometryDefinition::Circle { .. }
                        | SketchGeometryDefinition::Ellipse { .. }
                        | SketchGeometryDefinition::Nurbs { .. }
                )
        });
        let circle_only_carrier = match native_carriers.as_slice() {
            [carrier] if !has_resolved_curves => {
                if let Some(reference) = carrier.native_ref.as_deref() {
                    native_radial_record_for_marker(lanes, native_ref, reference).map(
                        |(radial_index, construction)| {
                            (carrier.id(), reference, radial_index, construction)
                        },
                    )
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((carrier_id, carrier_ref, radial_index, carrier_construction)) =
            circle_only_carrier
        {
            let mut roster = (markers.iter().copied().filter(|(marker, _)| {
                matches!(
                    marker.kind(),
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
            }))
            .collect::<Vec<_>>();
            roster.sort_unstable_by_key(|(left, _)| left.offset());
            // Only this suffix can have exactly one witness per dimension.
            if let Some(center_index) = roster
                .len()
                .checked_sub(radial_dimensions.len())
                .and_then(|index| index.checked_sub(1))
            {
                let (_, [cu, cv]) = roster[center_index];
                if marker_circle_one_to_one(
                    &roster[center_index + 1..],
                    &radial_dimensions,
                    [cu, cv],
                ) {
                    let carrier_radius = roster
                        .get(radial_index)
                        .map(|(_, [ru, rv])| (ru - cu).hypot(rv - cv) * NATIVE_TO_IR);
                    let native_center =
                        quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                    if let Some(center) =
                        unique_marker_circle_center(feature_transforms, native_center, QUANTUM)
                    {
                        let removed = SketchEntityId::clone(carrier_id);
                        let carrier_ref = carrier_ref.to_owned();
                        entities.retain(|entity| entity.id() != &removed);
                        let Some(sketch) =
                            sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                        else {
                            continue;
                        };
                        sketch.profiles.retain_uses(|usage| usage.entity != removed);
                        for (index, (parameter, radius)) in
                            radial_dimensions.iter().copied().enumerate()
                        {
                            let id_text = format!(
                                "sldprt:model:sketch-entity#radial-roster:{feature_key}:{index}"
                            );
                            let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                                continue;
                            };
                            let Ok(geometry) =
                                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                                    center,
                                    radius: Length::new(radius).ok_or_else(|| {
                                        cadmpeg_core::CodecError::Malformed(
                                            "SolidWorks projected length must be finite".into(),
                                        )
                                    })?,
                                })
                            else {
                                continue;
                            };
                            let same_carrier = carrier_radius
                                .is_some_and(|carrier| same_dimension_length(carrier, radius));
                            let native_reference = if same_carrier {
                                Some(carrier_ref.clone())
                            } else {
                                None
                            };
                            let geometry_reference = parameter.native_ref.clone();
                            let entity = SketchEntity::new(
                                entity_id,
                                copy_marker_circle_sketch_id(sketch_id)?,
                                geometry,
                            )
                            .with_construction(carrier_construction && same_carrier)
                            .with_native_ref(native_reference)
                            .with_geometry_ref(geometry_reference);
                            append_marker_circle(entities, sketch, entity, true);
                        }
                        continue;
                    }
                }
            }
            continue 'feature;
        }
        let mut radial_records = Vec::new();
        for lane in &owned_lanes {
            let mut range: Option<(usize, usize)> = None;
            for marker in &lane.sketch_entities {
                if marker.feature_ref.as_deref() != Some(native_ref) {
                    continue;
                }
                let offset = usize::try_from(marker.offset()).map_err(|_| {
                    cadmpeg_core::CodecError::malformed(
                        "SLDPRT sketch marker offset exceeds the address space",
                    )
                })?;
                range = Some(range.map_or((offset, offset), |(start, end)| {
                    (start.min(offset), end.max(offset))
                }));
            }
            let Some((start, end)) = range else {
                continue;
            };
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            for record in radial_records_by_lane
                .get(lane.id.as_str())
                .into_iter()
                .flatten()
            {
                if record.0 < start || record.0 > end {
                    continue;
                }
                let carrier_ref = marker_circle_carrier_reference(lane_key, record.0);
                if entities.iter().any(|entity| {
                    entity.sketch == *sketch_id
                        && entity.native_ref.as_deref() == Some(carrier_ref.as_str())
                        && matches!(
                            entity.geometry.definition(),
                            SketchGeometryDefinition::Native { .. }
                        )
                }) {
                    radial_records.push((*lane, *record));
                }
            }
        }
        let mut repeated_radial_sets = Vec::new();
        for (lane, (offset, radial_index, construction)) in &radial_records {
            if *construction {
                continue;
            }
            let mut roster = (lane
                .sketch_entities
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                .filter(|marker| marker.coordinates_m.is_some()))
            .collect::<Vec<_>>();
            roster.sort_unstable_by_key(|left| left.offset());
            for (parameter, radius) in &radial_dimensions {
                let Some(pairs) = terminal_repeated_radial_circle_pairs(
                    *radial_index,
                    &roster,
                    *radius / NATIVE_TO_IR,
                ) else {
                    continue;
                };
                repeated_radial_sets.push((*lane, *offset, *parameter, *radius, pairs));
            }
        }
        if let [(lane, offset, parameter, radius, pairs)] = repeated_radial_sets.as_slice() {
            let mut transformed = Vec::new();
            for (center, _) in pairs {
                let Some([cu, cv]) = center
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let native = quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                let Some(center) = unique_marker_circle_center(feature_transforms, native, QUANTUM)
                else {
                    continue;
                };
                let Some(radius) = Length::new(*radius) else {
                    continue;
                };
                let Ok(geometry) =
                    SketchGeometry::try_from(SketchGeometryDefinition::Circle { center, radius })
                else {
                    continue;
                };
                transformed.push(geometry);
            }
            if transformed.len() == pairs.len() {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let carrier_ref = marker_circle_carrier_reference(lane_key, *offset);
                let mut pair_radial_object_indices = HashSet::new();
                for (_, radial) in pairs {
                    if let Some(index) = radial.object_index() {
                        pair_radial_object_indices.insert(index);
                    }
                }
                let mut consumed_carrier_refs = HashSet::new();
                for (candidate_offset, candidate_radial_index, construction) in
                    radial_records_by_lane
                        .get(lane.id.as_str())
                        .into_iter()
                        .flatten()
                {
                    if *construction {
                        continue;
                    }
                    let candidate_offset_u64 =
                        cadmpeg_core::decode::u64_from_index(*candidate_offset);
                    if lane.sketch_entities.iter().any(|marker| {
                        marker.feature_ref.as_deref() == Some(native_ref)
                            && marker.offset() == candidate_offset_u64
                    }) && (*candidate_offset == *offset
                        || id_from_index(*candidate_radial_index)
                            .is_some_and(|index| pair_radial_object_indices.contains(&index)))
                    {
                        let reference =
                            marker_circle_carrier_reference(lane_key, *candidate_offset);
                        consumed_carrier_refs.insert(reference);
                    }
                }
                let mut removed = HashSet::new();
                for entity in &*entities {
                    if entity.sketch == *sketch_id
                        && entity
                            .native_ref
                            .as_deref()
                            .is_some_and(|reference| consumed_carrier_refs.contains(reference))
                    {
                        let id = SketchEntityId::clone(entity.id());
                        removed.insert(id);
                    }
                }
                entities.retain(|entity| !removed.contains(entity.id()));
                let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                else {
                    continue;
                };
                sketch
                    .profiles
                    .retain_uses(|usage| !removed.contains(&usage.entity));
                for (index, geometry) in transformed.into_iter().enumerate() {
                    let id_text = format!("sldprt:model:sketch-entity#repeated-radial-circle:{lane_key}:{offset}:{index}");
                    let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                        continue;
                    };
                    let native_reference = if index == pairs.len() - 1 {
                        Some(carrier_ref.clone())
                    } else {
                        None
                    };
                    let geometry_reference = parameter.native_ref.clone();
                    let entity = SketchEntity::new(
                        entity_id,
                        copy_marker_circle_sketch_id(sketch_id)?,
                        geometry,
                    )
                    .with_native_ref(native_reference)
                    .with_geometry_ref(geometry_reference);
                    append_marker_circle(entities, sketch, entity, true);
                }
                continue 'feature;
            }
        }
        if !radial_records.is_empty() {
            let radial_record_count = radial_records.len();
            let mut resolved = Vec::new();
            for (lane, (offset, radial_index, construction)) in radial_records {
                let mut roster = (lane
                    .sketch_entities
                    .iter()
                    .filter(|marker| marker.feature_ref.as_deref() == Some(native_ref))
                    .filter_map(|marker| {
                        marker
                            .coordinates_m
                            .map(|coordinates| (marker, coordinates.get()))
                    }))
                .collect::<Vec<_>>();
                roster.sort_unstable_by_key(|(left, _)| left.offset());
                let Some((radial, [ru, rv])) = roster.get(radial_index).copied() else {
                    continue;
                };
                let mut candidates = Vec::new();
                for (marker, [cu, cv]) in &markers {
                    if marker.id() == radial.id() {
                        continue;
                    }
                    let measured_radius = (ru - cu).hypot(rv - cv) * NATIVE_TO_IR;
                    let mut unique = None;
                    let mut ambiguous = false;
                    for (parameter, radius) in &radial_dimensions {
                        if !same_dimension_length(*radius, measured_radius) {
                            continue;
                        }
                        if unique.is_some() {
                            ambiguous = true;
                            break;
                        }
                        unique = Some((*parameter, *radius));
                    }
                    if ambiguous {
                        continue;
                    }
                    let Some((parameter, radius)) = unique else {
                        continue;
                    };
                    candidates.push((
                        quantize(Point2::new(*cu, *cv), QUANTUM),
                        *marker,
                        parameter,
                        radius,
                    ));
                }
                candidates.sort_unstable_by(
                    |(left_center, left_marker, _, _), (right_center, right_marker, _, _)| {
                        (*left_center, left_marker.offset())
                            .cmp(&(*right_center, right_marker.offset()))
                    },
                );
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
                let mut transformed = Vec::new();
                for record in &resolved {
                    let Some(native_point) = record.3.point(QUANTUM) else {
                        continue;
                    };
                    let native = quantize(
                        Point2::new(native_point.u * NATIVE_TO_IR, native_point.v * NATIVE_TO_IR),
                        QUANTUM,
                    );
                    let Some(center) =
                        unique_marker_circle_center(feature_transforms, native, QUANTUM)
                    else {
                        continue;
                    };
                    let Some(radius) = Length::new(record.6) else {
                        continue;
                    };
                    let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                        center,
                        radius,
                    }) else {
                        continue;
                    };
                    transformed.push((record, geometry));
                }
                if transformed.len() == resolved.len() {
                    let mut carrier_refs = HashSet::new();
                    let mut center_refs = HashSet::new();
                    for (lane, offset, _, _, marker, ..) in &resolved {
                        let lane_key = lane
                            .id
                            .rsplit_once('#')
                            .map_or(lane.id.as_str(), |(_, key)| key);
                        let reference = marker_circle_carrier_reference(lane_key, *offset);
                        carrier_refs.insert(reference);
                        center_refs.insert(marker.id());
                    }
                    let mut removed = HashSet::new();
                    for entity in &*entities {
                        if entity.sketch == *sketch_id
                            && entity.native_ref.as_deref().is_some_and(|reference| {
                                carrier_refs.contains(reference)
                                    || (center_refs.contains(reference)
                                        && !matches!(
                                            entity.geometry.definition(),
                                            SketchGeometryDefinition::Point { .. }
                                        ))
                            })
                        {
                            let id = SketchEntityId::clone(entity.id());
                            removed.insert(id);
                        }
                    }
                    entities.retain(|entity| !removed.contains(entity.id()));
                    let Some(sketch) = sketches.iter_mut().find(|sketch| sketch.id == *sketch_id)
                    else {
                        continue;
                    };
                    sketch
                        .profiles
                        .retain_uses(|usage| !removed.contains(&usage.entity));
                    for (record, geometry) in transformed {
                        let lane_key = record
                            .0
                            .id
                            .rsplit_once('#')
                            .map_or(record.0.id.as_str(), |(_, key)| key);
                        let id_text = format!(
                            "sldprt:model:sketch-entity#radial-circle:{lane_key}:{}",
                            record.1
                        );
                        let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                            continue;
                        };
                        let native_reference = marker_circle_carrier_reference(lane_key, record.1);
                        let geometry_reference = record.5.native_ref.clone();
                        let entity = SketchEntity::new(
                            entity_id,
                            copy_marker_circle_sketch_id(sketch_id)?,
                            geometry,
                        )
                        .with_construction(record.2)
                        .with_native_ref(Some(native_reference))
                        .with_geometry_ref(geometry_reference);
                        append_marker_circle(entities, sketch, entity, !record.2);
                    }
                    continue;
                }
            }
        }
        let centers = (markers
            .iter()
            .copied()
            .filter(|(marker, _)| marker.kind() == SketchInputKind::LineOrCircle))
        .collect::<Vec<_>>();
        let radial = (markers.iter().copied().filter(|(marker, _)| {
            matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        }))
        .collect::<Vec<_>>();
        let [(center_marker, coordinates)] = centers.as_slice() else {
            continue;
        };
        let [cu, cv] = *coordinates;
        if !marker_circle_one_to_one(&radial, &radial_dimensions, [cu, cv]) {
            continue;
        }
        let native_center = quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
        let Some(center) = unique_marker_circle_center(feature_transforms, native_center, QUANTUM)
        else {
            continue;
        };
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
            if entities.iter().any(|entity| entity.sketch == *sketch_id && matches!(entity.geometry.definition(),
                SketchGeometryDefinition::Circle { center: existing, radius: existing_radius }
                    if quantize(existing.get(), QUANTUM) == quantize(center, QUANTUM) && same_dimension_length(existing_radius.get(), radius))) { continue; }
            let id_text = format!(
                "sldprt:model:sketch-entity#marker-circle:{feature_key}:{}",
                parameter.ordinal
            );
            let Ok(entity_id) = SketchEntityId::mint(id_text) else {
                continue;
            };
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center,
                radius: Length::new(radius).ok_or_else(|| {
                    cadmpeg_core::CodecError::Malformed(
                        "SolidWorks projected length must be finite".into(),
                    )
                })?,
            }) else {
                continue;
            };
            let geometry_reference = parameter.native_ref.clone();
            let entity = SketchEntity::new(
                entity_id,
                copy_marker_circle_sketch_id(sketch_id)?,
                geometry,
            )
            .with_construction(construction)
            .with_geometry_ref(geometry_reference);
            append_marker_circle(entities, sketch, entity, true);
        }
    }

    Ok(())
}

#[cfg(test)]
mod dimensions_tests;
