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
use cadmpeg_core::decode::View;
use cadmpeg_core::decode::{id_from_index, DecodeContext};
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

const DIMENSIONED_CARRIER_OPERATION: &str = "resolve SLDPRT dimensioned carrier";

/// Resolve the construction state carried by a native radial-circle record
/// for one dimension center.  The radial record's role is authoritative; a
/// center/radius match alone is not enough because an ordinary circle and a
/// construction circle can share the same solved center and radius.
fn native_dimensioned_circle_construction_state(
    ctx: &DecodeContext<'_>,
    lanes: &[FeatureInputLane],
    feature: &str,
    center: &SketchInputEntity,
    radius: f64,
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    if !ctx.equal(
        &center.feature_ref.as_deref(),
        &Some(feature),
        DIMENSIONED_CARRIER_OPERATION,
    )? || !radius.is_finite()
        || radius <= 0.0
    {
        return Ok(None);
    }
    let Some([cu, cv]) = center
        .coordinates_m
        .map(cadmpeg_ir::units::FiniteVector::get)
    else {
        return Ok(None);
    };
    let mut state = None;
    for lane in ctx.admit_iter(lanes, DIMENSIONED_CARRIER_OPERATION)? {
        let mut owns_center = false;
        for marker in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
            if ctx.equal(marker.id(), center.id(), DIMENSIONED_CARRIER_OPERATION)?
                && ctx.equal(
                    &marker.feature_ref.as_deref(),
                    &Some(feature),
                    DIMENSIONED_CARRIER_OPERATION,
                )?
            {
                owns_center = true;
                break;
            }
        }
        if !owns_center {
            continue;
        }
        let mut roster = Vec::new();
        for marker in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
            if !ctx.equal(
                &marker.feature_ref.as_deref(),
                &Some(feature),
                DIMENSIONED_CARRIER_OPERATION,
            )? || marker.coordinates_m.is_none()
            {
                continue;
            }
            ctx.reserve_vec(&mut roster, 1, DIMENSIONED_CARRIER_OPERATION)?;
            roster.push(marker);
        }
        ctx.sort_unstable_by_key(
            &mut roster,
            |value| value.offset(),
            Ord::cmp,
            DIMENSIONED_CARRIER_OPERATION,
        )?;
        for record in radial_circle_records(ctx, &lane.native_payload)? {
            let Some((_, radial_index, construction)) = record? else {
                continue;
            };
            ctx.charge_work(64, DIMENSIONED_CARRIER_OPERATION)?;
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
                    return Ok(None);
                }
                state = Some(construction);
            }
        }
    }
    Ok(state)
}

fn native_radial_record_for_marker(
    ctx: &DecodeContext<'_>,
    lanes: &[FeatureInputLane],
    feature: &str,
    marker_id: &str,
) -> Result<Option<(usize, bool)>, cadmpeg_core::CodecError> {
    for lane in ctx.admit_iter(lanes, DIMENSIONED_CARRIER_OPERATION)? {
        let mut marker = None;
        for candidate in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
            if ctx.equal(candidate.id(), marker_id, DIMENSIONED_CARRIER_OPERATION)?
                && ctx.equal(
                    &candidate.feature_ref.as_deref(),
                    &Some(feature),
                    DIMENSIONED_CARRIER_OPERATION,
                )?
            {
                marker = Some(candidate);
                break;
            }
        }
        let Some(marker) = marker else {
            continue;
        };
        for record in radial_circle_records(ctx, &lane.native_payload)? {
            let Some((offset, radial_index, construction)) = record? else {
                continue;
            };
            if usize::try_from(marker.offset()).ok() == Some(offset) {
                return Ok(Some((radial_index, construction)));
            }
        }
        if let Ok(offset) = usize::try_from(marker.offset()) {
            if let Some(radial_index) =
                extended_radial_circle_index(ctx, &lane.native_payload, offset)?
            {
                return Ok(Some((radial_index, false)));
            }
        }
    }
    Ok(None)
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
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    center: &SketchInputEntity,
    expected_radius: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some([cu, cv]) = center
        .coordinates_m
        .map(cadmpeg_ir::units::FiniteVector::get)
    else {
        return Ok(false);
    };
    let mut count = 0;
    for candidate in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
        if !ctx.equal(
            &candidate.feature_ref,
            &center.feature_ref,
            DIMENSIONED_CARRIER_OPERATION,
        )? || candidate.offset() <= center.offset()
            || !matches!(
                candidate.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        {
            continue;
        }
        let Some([ru, rv]) = candidate
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let radius = (ru - cu).hypot(rv - cv) * 1000.0;
        if radius.is_finite() && same_dimension_length(radius, expected_radius) {
            count += 1;
            if count == 2 {
                break;
            }
        }
    }
    Ok(count == 1)
}

fn dimensioned_arc_native_geometry(
    ctx: &DecodeContext<'_>,
    lanes: &[FeatureInputLane],
    marker: &SketchInputEntity,
    expected_radius: f64,
) -> Result<Option<DimensionedCurveNative>, cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    if marker.kind() != SketchInputKind::Arc {
        return Ok(None);
    }
    let mut found = None;
    for lane in ctx.admit_iter(lanes, DIMENSIONED_CARRIER_OPERATION)? {
        let mut owns_marker = false;
        for candidate in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
            if ctx.equal(candidate.id(), marker.id(), DIMENSIONED_CARRIER_OPERATION)? {
                owns_marker = true;
                break;
            }
        }
        if owns_marker {
            found = Some(lane);
            break;
        }
    }
    let Some(lane) = found else {
        return Ok(None);
    };
    let mut object_markers = Vec::new();
    let mut markers_by_id = HashMap::<&str, &SketchInputEntity>::new();
    for candidate in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
        ctx.charge_work(64, DIMENSIONED_CARRIER_OPERATION)?;
        ctx.reserve_vec(&mut object_markers, 1, DIMENSIONED_CARRIER_OPERATION)?;
        object_markers.push(candidate);
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut markers_by_id,
                candidate.id(),
                candidate,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    let endpoints = marker_curve_endpoint_markers(
        ctx,
        &lane.native_payload,
        marker,
        &markers_by_id,
        &object_markers,
    )?;
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
            _ => return Ok(None),
        }
    } else {
        // The endpoint search does not establish a bounded arc. Keep the
        // existing exact radial-witness fallback.

        if unique_native_radial_witness(ctx, lane, marker, expected_radius)? {
            let Some(center) = marker
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return Ok(None);
            };
            return Ok(Some(DimensionedCurveNative::Circle { center }));
        }
        return Ok(None);
    };
    let endpoint_pair = if let [first, second] = endpoints.as_slice() {
        let first = ctx.format_retained(
            format_args!("{}", first.id()),
            DIMENSIONED_CARRIER_OPERATION,
        )?;
        let second = ctx.format_retained(
            format_args!("{}", second.id()),
            DIMENSIONED_CARRIER_OPERATION,
        )?;
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
    Ok(
        valid.then_some(DimensionedCurveNative::Arc(DimensionedArcNative {
            center,
            start,
            end,
            endpoints: endpoint_pair,
        })),
    )
}

/// Resolve the duplicate-link arc carrier used by a declared entity handle.
///
/// The coordinate-less handle is a reference identity, not a second curve.
/// Exactly two identical links to one earlier arc marker select that marker;
/// the link local identifier and feature scope must agree.  The arc still
/// needs the normal endpoint or radial-witness validation, so a duplicate
/// link cannot turn an arbitrary relation handle into a circular carrier.
fn unique_linked_declared_entity_handle_arc_carrier<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Result<Option<(&'a SketchInputEntity, DimensionedCurveNative)>, cadmpeg_core::CodecError> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return Ok(None);
    }
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(None);
    };
    let mut unique = None;
    for handle in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
        if !ctx.equal(
            &handle.feature_ref.as_deref(),
            &Some(feature),
            DIMENSIONED_CARRIER_OPERATION,
        )? || handle.offset() >= operand.offset
            || handle.coordinates_m.is_some()
            || handle.kind() != SketchInputKind::LineOrCircle
        {
            continue;
        }
        let [first, second] = handle.links() else {
            continue;
        };
        if !ctx.equal(
            &first.entity_ref,
            &second.entity_ref,
            DIMENSIONED_CARRIER_OPERATION,
        )? || first.local_id != second.local_id
        {
            continue;
        }

        let mut arc = None;
        for candidate in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
            if ctx.equal(
                candidate.id(),
                first.entity_ref.as_str(),
                DIMENSIONED_CARRIER_OPERATION,
            )? && ctx.equal(
                &candidate.feature_ref.as_deref(),
                &Some(feature),
                DIMENSIONED_CARRIER_OPERATION,
            )? && candidate.offset() < handle.offset()
                && candidate.local_id() == Some(u32::from(first.local_id))
                && candidate.coordinates_m.is_some()
                && candidate.kind() == SketchInputKind::Arc
            {
                arc = Some(candidate);
                break;
            }
        }
        let Some(arc) = arc else {
            continue;
        };
        let Some(curve) = dimensioned_arc_native_geometry(ctx, lanes, arc, expected_radius)? else {
            continue;
        };
        if unique.is_some() {
            return Ok(None);
        }
        unique = Some((arc, curve));
    }
    Ok(unique)
}

fn unique_declared_entity_handle_circular_carrier<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    feature: &str,
    operand: &FeatureInputOperand,
    expected_radius: f64,
) -> Result<Option<(&'a SketchInputEntity, DimensionedCurveNative)>, cadmpeg_core::CodecError> {
    if !expected_radius.is_finite() || expected_radius <= 0.0 {
        return Ok(None);
    }
    if let Some(carrier) = unique_linked_declared_entity_handle_arc_carrier(
        ctx,
        lanes,
        feature,
        operand,
        expected_radius,
    )? {
        return Ok(Some(carrier));
    }
    if declared_entity_handle_has_resolved_pair(ctx, lanes, feature, operand)? {
        return Ok(None);
    }
    let DeclaredEntityHandleOwner::Unique(lane) =
        declared_entity_handle_owner(ctx, lanes, operand)?
    else {
        return Ok(None);
    };
    let mut unique = None;
    for marker in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
        if !ctx.equal(
            &marker.feature_ref.as_deref(),
            &Some(feature),
            DIMENSIONED_CARRIER_OPERATION,
        )? {
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
                let Some(curve) =
                    dimensioned_arc_native_geometry(ctx, lanes, marker, expected_radius)?
                else {
                    continue;
                };
                curve
            }
            SketchInputKind::LineOrCircle => {
                if !unique_native_radial_witness(ctx, lane, marker, expected_radius)? {
                    continue;
                }
                DimensionedCurveNative::Circle { center }
            }
            _ => continue,
        };
        if unique.is_some() {
            return Ok(None);
        }
        unique = Some((marker, curve));
    }
    Ok(unique)
}

fn dimensioned_relation_carrier<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [FeatureInputLane],
    markers_by_id: &HashMap<&str, &'a SketchInputEntity>,
    feature: &str,
    operand: &FeatureInputOperand,
    radius: f64,
) -> Result<Option<DimensionedRelationCarrier<'a>>, cadmpeg_core::CodecError> {
    let explicit = match operand.entity_ref.as_deref() {
        Some(id) => ctx
            .get_hash_map(markers_by_id, id, DIMENSIONED_CARRIER_OPERATION)?
            .copied(),
        None => None,
    };
    let explicit_point_marker = explicit.is_some_and(|marker| {
        matches!(
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        )
    });
    if let Some((marker, center)) =
        declared_slot_handle_dimension_center(ctx, lanes, feature, operand)?
    {
        let Some(center) = center
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            return Ok(None);
        };
        return Ok(Some(DimensionedRelationCarrier {
            marker,
            geometry: DimensionedCarrierGeometry::Center(center),
            construction: Some(true),
        }));
    }
    if operand.kind == FeatureInputOperandKind::Native(NativeOperandTag::TAG_836E) {
        let Some(marker) = declared_entity_handle_indexed_circle_dimension_center(
            ctx, lanes, feature, operand, radius,
        )?
        else {
            return Ok(None);
        };
        let Some(center) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            return Ok(None);
        };
        return Ok(Some(DimensionedRelationCarrier {
            marker,
            geometry: DimensionedCarrierGeometry::Center(center),
            construction: Some(false),
        }));
    }
    if matches!(
        operand.kind,
        FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D4 | NativeOperandTag::TAG_80D5)
    ) {
        let Some(marker) =
            declared_entity_handle_point_dimension_center(ctx, lanes, feature, operand)?
        else {
            return Ok(None);
        };
        let Some(center) = marker
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
        else {
            return Ok(None);
        };
        return Ok(Some(DimensionedRelationCarrier {
            marker,
            geometry: DimensionedCarrierGeometry::Center(center),
            construction: Some(false),
        }));
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
        for lane in ctx.admit_iter(lanes, DIMENSIONED_CARRIER_OPERATION)? {
            let mut owns_marker = false;
            for candidate in ctx.admit_iter(&lane.sketch_entities, DIMENSIONED_CARRIER_OPERATION)? {
                if ctx.equal(candidate.id(), marker.id(), DIMENSIONED_CARRIER_OPERATION)?
                    && ctx.equal(
                        &candidate.feature_ref.as_deref(),
                        &Some(feature),
                        DIMENSIONED_CARRIER_OPERATION,
                    )?
                {
                    owns_marker = true;
                    break;
                }
            }
            if owns_marker && current_geometry_locus_arc_handle_point(&lane.native_payload, offset)
            {
                explicit_current_arc_handle_point = true;
                break;
            }
        }
    }
    let declared_owner = declared_entity_handle_owner(ctx, lanes, operand)?;
    let declared = declared_entity_handle_circular_marker(ctx, lanes, feature, operand, radius)?;
    let declared_entity_handle = !matches!(declared_owner, DeclaredEntityHandleOwner::Absent);
    let (marker, encoded_radius, fallback_curve) = if let Some((marker, radius)) = declared {
        (marker, Some(radius), None)
    } else if declared_entity_handle && explicit_current_arc_handle_point {
        let Some(marker) = explicit else {
            return Ok(None);
        };
        (marker, None, None)
    } else if declared_entity_handle && !explicit_circular_marker {
        if explicit.is_none() || explicit_point_marker {
            if let Some((marker, curve)) = unique_declared_entity_handle_circular_carrier(
                ctx, lanes, feature, operand, radius,
            )? {
                (marker, None, Some(curve))
            } else if matches!(operand.kind, FeatureInputOperandKind::Native(_))
                && explicit_point_marker
                && !declared_entity_handle_point_is_declared_radial(ctx, lanes, feature, operand)?
            {
                let Some(marker) =
                    declared_entity_handle_point_dimension_center(ctx, lanes, feature, operand)?
                else {
                    return Ok(None);
                };
                (marker, None, None)
            } else {
                return Ok(None);
            }
        } else {
            // A declared handle blocks point-based guessing. An explicit native
            // line-or-circle or arc marker remains a direct geometry carrier.
            return Ok(None);
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
                let Some((marker, radius)) = implicit_circle_marker(
                    ctx,
                    lanes,
                    feature,
                    operand.kind,
                    operand.entity_index,
                    radius,
                )?
                else {
                    return Ok(None);
                };
                (marker, Some(radius), None)
            }
        }
    };
    let curve = if fallback_curve.is_some() {
        fallback_curve
    } else if marker.kind() == SketchInputKind::Arc {
        dimensioned_arc_native_geometry(ctx, lanes, marker, radius)?
    } else {
        None
    };
    if marker.kind() == SketchInputKind::Arc && curve.is_none() {
        return Ok(None);
    }
    if !matches!(
        marker.kind(),
        SketchInputKind::Point
            | SketchInputKind::ConstrainedPoint
            | SketchInputKind::LineOrCircle
            | SketchInputKind::Arc
    ) {
        return Ok(None);
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
        return Ok(None);
    }
    if encoded_radius.is_some_and(|encoded| !same_dimension_length(encoded, radius)) {
        return Ok(None);
    }
    let mut construction =
        native_dimensioned_circle_construction_state(ctx, lanes, feature, marker, radius)?;
    if construction.is_none() {
        construction =
            direct_point_dimension_center(ctx, lanes, feature, operand, radius)?.map(|_| false);
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
            let Some(center) = marker
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
            else {
                return Ok(None);
            };
            DimensionedCarrierGeometry::Center(center)
        }
    };
    Ok(Some(DimensionedRelationCarrier {
        marker,
        geometry,
        construction,
    }))
}

/// A dimensioned arc's sketch geometry beside the radius it was built from.
struct DimensionedArcGeometry {
    geometry: SketchGeometry,
    radius: f64,
    endpoint_refs: Vec<String>,
}

fn transformed_dimensioned_arc(
    ctx: &DecodeContext<'_>,
    transform: super::transforms::MarkerTransform,
    arc: &DimensionedArcNative,
    native_to_ir: f64,
    quantum: f64,
) -> Result<Option<DimensionedArcGeometry>, cadmpeg_core::CodecError> {
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
    let Some((geometry, radius, reversed)) = geometry else {
        return Ok(None);
    };
    let mut endpoint_refs = Vec::new();
    if let Some(endpoints) = &arc.endpoints {
        let order = if reversed { [1, 0] } else { [0, 1] };
        ctx.reserve_vec(
            &mut endpoint_refs,
            2,
            "collect SLDPRT dimensioned arc endpoints",
        )?;
        for index in order {
            let text = ctx.format_retained(
                format_args!("{}", endpoints[index]),
                "retain SLDPRT dimensioned arc endpoint",
            )?;
            endpoint_refs.push(text);
        }
    }
    Ok(Some(DimensionedArcGeometry {
        geometry,
        radius,
        endpoint_refs,
    }))
}

/// Materialize dimensioned circular sketch geometry omitted by a selected-profile stream.
pub(crate) fn project_dimensioned_sketch_geometry(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    surfaces: &[cadmpeg_ir::geometry::Surface],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    const OPERATION: &str = "project SLDPRT dimensioned sketch circles";
    let mut sketches_by_feature = HashMap::<&str, _>::new();
    for feature in ctx.admit_iter(features, "scan SLDPRT dimension geometry")? {
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
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut sketches_by_feature,
                native,
                sketch,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    let (ownership, _ownership_storage) = ctx
        .with_scoped_storage("SLDPRT relation ownership index", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    let mut parameters_by_id = HashMap::<&cadmpeg_ir::features::ParameterId, _>::new();
    for parameter in ctx.admit_iter(parameters, "scan SLDPRT dimension geometry")? {
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut parameters_by_id,
                &parameter.id,
                parameter,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    let relation_parameter = |relation: &FeatureInputRelationInstance| -> Result<
        Option<&cadmpeg_ir::features::DesignParameter>,
        cadmpeg_core::CodecError,
    > {
        let Some(parameter) = ctx
            .get_hash_map(&ownership, &relation.id, OPERATION)?
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        Ok(ctx
            .get_hash_map(&parameters_by_id, parameter, OPERATION)?
            .copied())
    };
    let mut markers_by_id = HashMap::<&str, _>::new();
    for iteration_lane in ctx.admit_iter(&lanes[..], "scan SLDPRT dimensions source records")? {
        for marker in ctx.admit_iter(
            &iteration_lane.sketch_entities,
            "scan SLDPRT dimensions records",
        )? {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut markers_by_id,
                    marker.id(),
                    marker,
                    "resolve SLDPRT dimensions keys",
                )
            })?;
        }
    }
    let marker_transforms =
        marker_transform_candidates_by_feature(ctx, features, sketches, entities, lanes)?;
    let mut transforms = HashMap::<&str, _>::new();
    for (feature, sketch_id) in
        ctx.admit_iter(&sketches_by_feature, "scan SLDPRT dimension geometry")?
    {
        let mut circles = Vec::new();
        for iteration_lane in ctx.admit_iter(&lanes[..], "scan SLDPRT dimensions source records")? {
            for relation in ctx.admit_iter(
                &iteration_lane.relation_instances,
                "scan SLDPRT dimensions records",
            )? {
                if !ctx.equal(
                    relation.feature_ref.as_str(),
                    *feature,
                    "compare SLDPRT dimensions records",
                )? || relation.family != FeatureInputRelationFamily::CircleDiameter
                {
                    continue;
                }
                let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                    continue;
                };
                let Some(parameter) = relation_parameter(relation)? else {
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
                    ctx,
                    lanes,
                    &markers_by_id,
                    relation.feature_ref.as_str(),
                    operand,
                    radius,
                )?
                else {
                    continue;
                };
                ctx.reserve_vec(&mut circles, 1, OPERATION)?;
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
        }
        let candidates = if let Some(existing) = ctx.get_hash_map(
            &marker_transforms,
            *feature,
            "resolve SLDPRT dimensions keys",
        )? {
            ctx.charge_work(
                u64::try_from(existing.len())
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            let mut candidates = Vec::new();
            ctx.reserve_vec(&mut candidates, existing.len(), OPERATION)?;
            candidates.extend_from_slice(existing);
            candidates
        } else if let Some(sketch) = {
            let mut search_result = None;
            for sketch in ctx.admit_iter(&(sketches)[..], "scan SLDPRT dimensions records")? {
                if ctx.equal(
                    &(sketch.id),
                    &(**sketch_id),
                    "compare SLDPRT dimensions records",
                )? {
                    search_result = Some(sketch);
                    break;
                }
            }
            search_result
        } {
            dimensioned_circle_surface_transforms(ctx, sketch, surfaces, &circles, QUANTUM)?
        } else {
            Vec::new()
        };
        let candidates = if let Some(sketch) = {
            let mut search_result = None;
            for sketch in ctx.admit_iter(&(sketches)[..], "scan SLDPRT dimensions records")? {
                if ctx.equal(
                    &(sketch.id),
                    &(**sketch_id),
                    "compare SLDPRT dimensions records",
                )? {
                    search_result = Some(sketch);
                    break;
                }
            }
            search_result
        } {
            marker_transforms_with_frame_fallback(candidates, sketch, QUANTUM)
        } else {
            candidates
        };
        let Some(transform) = dimensioned_circle_transform(ctx, &candidates, &circles)? else {
            continue;
        };
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut transforms,
                *feature,
                transform,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    for lane in ctx.admit_iter(lanes, "scan SLDPRT dimension geometry")? {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "resolve SLDPRT dimensions keys")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in
            ctx.admit_iter(&lane.relation_instances, "scan SLDPRT dimensions records")?
        {
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let (Some(sketch), Some(transform)) = (
                ctx.get_hash_map(
                    &sketches_by_feature,
                    relation.feature_ref.as_str(),
                    "resolve SLDPRT dimensions keys",
                )?,
                ctx.get_hash_map(
                    &transforms,
                    relation.feature_ref.as_str(),
                    "resolve SLDPRT dimensions keys",
                )?,
            ) else {
                continue;
            };
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let parameter = relation_parameter(relation)?;
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
                ctx,
                lanes,
                &markers_by_id,
                relation.feature_ref.as_str(),
                operand,
                radius,
            )?;
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
            if {
                let mut search_result = false;
                for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimensions records")? {
                    if {
                        ctx.equal(
                            &(entity.sketch),
                            &(**sketch),
                            "compare SLDPRT dimensions records",
                        )? && ctx.equal(
                            &(entity.geometry_ref.as_deref()),
                            &(Some(relation.id.as_str())),
                            "compare SLDPRT dimensions records",
                        )?
                    } {
                        search_result = true;
                        break;
                    }
                }
                search_result
            } {
                continue;
            }
            if carrier
                .curve()
                .and_then(DimensionedCurveNative::arc)
                .is_none()
                && {
                    let mut search_result = false;
                    for entity in
                        ctx.admit_iter(&(entities)[..], "scan SLDPRT dimensions records")?
                    {
                        if {
                            ctx.equal(
                                &(entity.sketch),
                                &(**sketch),
                                "compare SLDPRT dimensions records",
                            )? && match entity.geometry.definition() {
                                SketchGeometryDefinition::Circle {
                                    center: existing,
                                    radius: existing_radius,
                                } => {
                                    quantize(existing.get(), QUANTUM) == quantize(center, QUANTUM)
                                        && same_dimension_length(existing_radius.get(), radius)
                                }
                                _ => false,
                            }
                        } {
                            search_result = true;
                            break;
                        }
                    }
                    search_result
                }
            {
                continue;
            }
            let (geometry, endpoint_refs) =
                if let Some(arc) = carrier.curve().and_then(DimensionedCurveNative::arc) {
                    let Some(arc) =
                        transformed_dimensioned_arc(ctx, *transform, arc, NATIVE_TO_IR, QUANTUM)?
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
            let entity_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#dimension:{lane_key}:{}",
                    relation.offset
                ),
                OPERATION,
            )?;
            let Ok(entity_id) = ({
                let identity_text = entity_text;
                // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT dimensions identity",
                )?;
                SketchEntityId::mint(identity_text)
            }) else {
                continue;
            };
            let sketch_text =
                ctx.format_retained(format_args!("{}", sketch.as_str()), OPERATION)?;
            let Ok(sketch_id) = ({
                let identity_text = sketch_text;
                // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT dimensions identity",
                )?;
                cadmpeg_ir::sketches::SketchId::mint(identity_text)
            }) else {
                continue;
            };
            let native_ref =
                ctx.format_retained(format_args!("{}", carrier.marker.id()), OPERATION)?;
            let geometry_ref = ctx.format_retained(format_args!("{}", relation.id), OPERATION)?;
            ctx.reserve_vec(entities, 1, OPERATION)?;
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
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    let mut sketches_by_feature = HashMap::<&str, _>::new();
    for feature in ctx.admit_iter(features, "scan SLDPRT dimension geometry")? {
        ctx.charge_work(64, DIMENSIONED_CARRIER_OPERATION)?;
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };

        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut sketches_by_feature,
                native_ref,
                sketch,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    let (ownership, _ownership_storage) = ctx
        .with_scoped_storage("SLDPRT relation ownership index", || {
            owned_relation_parameters(ctx, features, parameters, lanes)
        })?;
    let mut parameters_by_id = HashMap::<&cadmpeg_ir::features::ParameterId, _>::new();
    for parameter in ctx.admit_iter(parameters, "scan SLDPRT dimension geometry")? {
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut parameters_by_id,
                &parameter.id,
                parameter,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    let mut markers_by_id = HashMap::<&str, _>::new();
    for iteration_lane in ctx.admit_iter(&lanes[..], "scan SLDPRT dimensions source records")? {
        for marker in ctx.admit_iter(
            &iteration_lane.sketch_entities,
            "scan SLDPRT dimensions records",
        )? {
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut markers_by_id,
                    marker.id(),
                    marker,
                    "resolve SLDPRT dimensions keys",
                )
            })?;
        }
    }

    for lane in ctx.admit_iter(lanes, "scan SLDPRT dimension geometry")? {
        let lane_key = ctx
            .rsplit_once(&lane.id, "#", "resolve SLDPRT dimensions keys")?
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in
            ctx.admit_iter(&lane.relation_instances, "scan SLDPRT dimensions records")?
        {
            ctx.charge_work(64, DIMENSIONED_CARRIER_OPERATION)?;
            if relation.family != FeatureInputRelationFamily::CircleDiameter {
                continue;
            }
            let ([operand] | [_, operand]) = relation.operands.as_slice() else {
                continue;
            };
            let Some(sketch) = ctx.get_hash_map(
                &sketches_by_feature,
                relation.feature_ref.as_str(),
                "resolve SLDPRT dimensions keys",
            )?
            else {
                continue;
            };
            let Some(parameter_id) = ctx
                .get_hash_map(&ownership, &relation.id, "resolve SLDPRT dimensions keys")?
                .and_then(Option::as_ref)
            else {
                continue;
            };

            let Some(parameter) = ctx.get_hash_map(
                &parameters_by_id,
                parameter_id,
                "resolve SLDPRT dimensions keys",
            )?
            else {
                continue;
            };

            let Some(radius) = radial_dimension_radius(parameter) else {
                continue;
            };
            let marker_id = if let Some(reference) = operand.entity_ref.as_deref() {
                Some(reference)
            } else {
                implicit_circle_marker(
                    ctx,
                    lanes,
                    relation.feature_ref.as_str(),
                    operand.kind,
                    operand.entity_index,
                    radius,
                )?
                .map(|(marker, _)| marker.id())
            };
            let Some(marker_id) = marker_id else {
                continue;
            };

            let Some(marker) = ctx
                .get_hash_map(&markers_by_id, marker_id, "resolve SLDPRT dimensions keys")?
                .copied()
            else {
                continue;
            };
            if !matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            ) {
                continue;
            }

            let mut center_entity = None;
            let mut ambiguous_center = false;
            for entity in ctx.admit_iter(&entities[..], DIMENSIONED_CARRIER_OPERATION)? {
                if ctx.equal(&entity.sketch, *sketch, DIMENSIONED_CARRIER_OPERATION)?
                    && ctx.equal(
                        &entity.native_ref.as_deref(),
                        &Some(marker_id),
                        DIMENSIONED_CARRIER_OPERATION,
                    )?
                    && matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    )
                {
                    if center_entity.is_some() {
                        ambiguous_center = true;
                        break;
                    }
                    center_entity = Some(entity);
                }
            }
            if ambiguous_center {
                continue;
            }
            let Some(center_entity) = center_entity else {
                continue;
            };
            let SketchGeometryDefinition::Point { position: center } =
                *center_entity.geometry.definition()
            else {
                continue;
            };
            let mut construction = native_dimensioned_circle_construction_state(
                ctx,
                lanes,
                relation.feature_ref.as_str(),
                marker,
                radius,
            )?;
            if construction.is_none() {
                let explicit_center = operand.entity_ref.is_some()
                    && (declared_entity_handle_point_dimension_center(
                        ctx,
                        lanes,
                        relation.feature_ref.as_str(),
                        operand,
                    )?
                    .is_some()
                        || direct_point_dimension_center(
                            ctx,
                            lanes,
                            relation.feature_ref.as_str(),
                            operand,
                            radius,
                        )?
                        .is_some())
                    && !declared_entity_handle_point_is_declared_radial(
                        ctx,
                        lanes,
                        relation.feature_ref.as_str(),
                        operand,
                    )?;
                construction = explicit_center.then_some(false);
            }
            let construction =
                construction.or_else(|| lane.native_payload.is_empty().then_some(false));
            let Some(construction) = construction else {
                continue;
            };

            if {
                let mut search_result = false;
                for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimensions records")? {
                    if {
                        ctx.equal(
                            &(entity.sketch),
                            &(**sketch),
                            "compare SLDPRT dimensions records",
                        )? && matches!(entity.geometry.definition(), SketchGeometryDefinition::Circle { center: existing, radius: existing_radius }
                        if quantize(existing.get(), EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8) == quantize(center.get(), EPS_DIMENSIONS_PROJECT_RELATION_POINT_DIMENSIONED_CIRCLES_E8)
                            && same_dimension_length(existing_radius.get(), radius))
                    } {
                        search_result = true;
                        break;
                    }
                }
                search_result
            } {
                continue;
            }

            let entity_id = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#dimension-point:{lane_key}:{}",
                    relation.offset
                ),
                "format SLDPRT dimensioned point identity",
            )?;
            let Ok(entity_id) = ({
                let identity_text = entity_id;
                // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT dimensions identity",
                )?;
                SketchEntityId::mint(identity_text)
            }) else {
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

            let sketch_id = ctx.format_retained(
                format_args!("{}", sketch.as_str()),
                "copy SLDPRT dimensioned point sketch",
            )?;
            let Ok(sketch_id) = ({
                let identity_text = sketch_id;
                // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT dimensions identity",
                )?;
                cadmpeg_ir::sketches::SketchId::mint(identity_text)
            }) else {
                continue;
            };

            let native_ref = ctx.format_retained(
                format_args!("{}", marker.id()),
                "copy SLDPRT dimensioned point marker reference",
            )?;

            let geometry_ref = ctx.format_retained(
                format_args!("{}", relation.id),
                "copy SLDPRT dimensioned point relation reference",
            )?;
            ctx.reserve_vec(entities, 1, "append SLDPRT dimensioned point circle")?;
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

fn radial_circle_records<'a, 'arena>(
    ctx: &'a DecodeContext<'arena>,
    payload: &'a [u8],
) -> Result<
    impl Iterator<Item = Result<Option<(usize, usize, bool)>, cadmpeg_core::CodecError>>
        + 'a
        + use<'a, 'arena>,
    cadmpeg_core::CodecError,
> {
    let Some(window) = std::num::NonZeroUsize::new(LEGACY_SKETCH_MARKER.len()) else {
        return Err(cadmpeg_core::CodecError::malformed(
            "empty SLDPRT sketch marker",
        ));
    };
    Ok(ctx
        .admit_iter(payload, "scan SLDPRT radial circle records")?
        .windows(window)
        .enumerate()
        .map(move |(offset, _)| {
            let radial = if let Some(radial) = compact_radial_circle_index(payload, offset) {
                Some(radial)
            } else {
                extended_terminal_repeated_radial_circle_index(ctx, payload, offset)?
            };
            Ok(radial.map(|radial| {
                (
                    offset,
                    radial,
                    marker_profile_curve_role(payload, offset) == Some(2),
                )
            }))
        }))
}

fn extended_terminal_repeated_radial_circle_index(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    if payload.get(offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len())
        != Some(LEGACY_EXTENDED_SKETCH_MARKER)
        || marker_native_code(payload, offset) != Some(2)
        || payload.get(offset + 23..offset + 31)
            != Some(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00])
        || payload.get(offset + 31..offset + 39)
            != Some(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00])
        || payload.get(offset + 48..offset + 56) != Some(&1.0f64.to_le_bytes())
        || !ctx.equal(
            &payload.get(offset + 56..offset + 58),
            &payload.get(offset + 58..offset + 60),
            "compare SLDPRT repeated radial indices",
        )?
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
        || offset
            .checked_add(104)
            .is_none_or(|offset| sketch_marker_prefix_at(payload, offset))
    {
        return Ok(None);
    }
    Ok(View::u16_le_at(payload, offset + 56).map(usize::from))
}

fn terminal_repeated_radial_circle_pairs<'a>(
    ctx: &DecodeContext<'_>,
    radial_index: usize,
    roster: &[&'a SketchInputEntity],
    radius: f64,
) -> Result<Option<Vec<(&'a SketchInputEntity, &'a SketchInputEntity)>>, cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    if radial_index != roster.len() || radius <= 0.0 || !radius.is_finite() {
        return Ok(None);
    }
    let Some(terminal) = roster.last().copied() else {
        return Ok(None);
    };
    let mut pairs = ctx.collect_vec(
        ctx.admit_iter(roster, MARKER_CIRCLE_OPERATION)?
            .zip(ctx.admit_iter(&roster[1..], MARKER_CIRCLE_OPERATION)?)
            .filter_map(|(center, radial)| {
                let center_index = center.object_index()?;
                let radial_index = radial.object_index()?;
                if center_index != radial_index.checked_add(1)? {
                    return None;
                }
                let [cu, cv] = center.coordinates_m?.get();
                let [ru, rv] = radial.coordinates_m?.get();
                same_dimension_length((ru - cu).hypot(rv - cv), radius)
                    .then_some((*center, *radial))
            }),
        MARKER_CIRCLE_OPERATION,
    )?;
    if pairs.len() < 2
        || !ctx.equal(
            &(pairs.last().map(|(_, radial)| radial.id())),
            &(Some(terminal.id())),
            "compare SLDPRT dimensions records",
        )?
    {
        return Ok(None);
    }
    let mut used = HashSet::new();
    for (center, radial) in ctx.admit_iter(&pairs, "scan SLDPRT dimension geometry")? {
        if !temporary_storage
            .with_storage(|| ctx.insert_hash_set(&mut used, center.id(), MARKER_CIRCLE_OPERATION))?
            || !temporary_storage.with_storage(|| {
                ctx.insert_hash_set(&mut used, radial.id(), MARKER_CIRCLE_OPERATION)
            })?
        {
            return Ok(None);
        }
    }
    ctx.sort_unstable_by_key(
        &mut pairs,
        |value| {
            let (left, _) = value;
            left.offset()
        },
        Ord::cmp,
        MARKER_CIRCLE_OPERATION,
    )?;
    Ok(Some(pairs))
}

pub(super) fn extended_radial_circle_index(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let Some(index) = View::u16_le_at(payload, offset + 64) else {
        return Ok(None);
    };
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
        && ctx.equal(
            &payload.get(offset + 64..offset + 66),
            &payload.get(offset + 66..offset + 68),
            "compare SLDPRT extended radial indices",
        )?
        && payload.get(offset + 64..offset + 66) != Some(&[0; 2])
        && payload.get(offset + 68..offset + 72) == Some(&1u32.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
        && payload.get(offset + 80..offset + 84) == Some(&1u32.to_le_bytes());
    Ok(supported.then_some(usize::from(index)))
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
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketches: &mut [Sketch],
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    feature: &str,
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    const OPERATION: &str = "reconcile SLDPRT direct circle carriers";
    let mut replacements = HashMap::<&str, &SketchEntityId>::new();
    for iteration_lane in ctx.admit_iter(&lanes[..], "scan SLDPRT dimensions source records")? {
        for relation in ctx.admit_iter(
            &iteration_lane.relation_instances,
            "scan SLDPRT dimensions records",
        )? {
            if !ctx.equal(
                relation.feature_ref.as_str(),
                feature,
                "compare SLDPRT dimensions records",
            )? || relation.family != FeatureInputRelationFamily::CircleDiameter
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
            for iteration_lane in
                ctx.admit_iter(&lanes[..], "scan SLDPRT dimensions source records")?
            {
                for marker in ctx.admit_iter(
                    &iteration_lane.sketch_entities,
                    "scan SLDPRT dimensions records",
                )? {
                    if !ctx.equal(
                        &(marker.id()),
                        &(marker_id),
                        "compare SLDPRT dimensions records",
                    )? || !ctx.equal(
                        &(marker.feature_ref.as_deref()),
                        &(Some(feature)),
                        "compare SLDPRT dimensions records",
                    )? {
                        continue;
                    }
                    if candidate.is_some() {
                        ambiguous = true;
                        break;
                    }
                    candidate = Some(marker);
                }
            }
            let Some(marker) = candidate.filter(|_| !ambiguous) else {
                continue;
            };
            if marker.kind() != SketchInputKind::LineOrCircle || marker.coordinates_m.is_none() {
                continue;
            }
            let mut typed_candidate = None;
            let mut ambiguous = false;
            for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimension geometry")? {
                if !ctx.equal(&entity.sketch, sketch_id, OPERATION)?
                    || !ctx.equal(
                        &(entity.native_ref.as_deref()),
                        &(Some(marker.id())),
                        "compare SLDPRT dimensions records",
                    )?
                    || !ctx.equal(
                        &(entity.geometry_ref.as_deref()),
                        &(Some(relation.id.as_str())),
                        "compare SLDPRT dimensions records",
                    )?
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
            temporary_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut replacements,
                    marker.id(),
                    typed_entity.id(),
                    "resolve SLDPRT dimensions keys",
                )
            })?;
        }
    }
    if replacements.is_empty() {
        return Ok(());
    }
    let mut removed = HashMap::<SketchEntityId, SketchEntityId>::new();
    for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimension geometry")? {
        if !ctx.equal(&entity.sketch, sketch_id, OPERATION)?
            || !matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            )
        {
            continue;
        }
        let replacement = match entity.native_ref.as_deref() {
            Some(native) => ctx.get_hash_map(&replacements, native, OPERATION)?,
            None => None,
        };
        let Some(replacement) = replacement else {
            continue;
        };
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut removed,
                copy_circle_carrier_entity_id(ctx, entity.id())?,
                copy_circle_carrier_entity_id(ctx, replacement)?,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    if removed.is_empty() {
        return Ok(());
    }
    if let Some(sketch) = {
        let mut sketch_index = None;
        for (index, sketch) in ctx
            .admit_iter(&sketches[..], "find SLDPRT dimension sketch")?
            .enumerate()
        {
            if ctx.equal(&sketch.id, sketch_id, "compare SLDPRT dimension sketch")? {
                sketch_index = Some(index);
                break;
            }
        }
        match sketch_index {
            Some(index) => Some(&mut sketches[index]),
            None => None,
        }
    } {
        let mut profiles = Vec::new();
        for profile in
            ctx.admit_iter(sketch.profiles.as_slice(), "scan SLDPRT dimensions records")?
        {
            let mut present = HashSet::<&SketchEntityId>::new();
            for usage in ctx.admit_iter(profile, "scan SLDPRT dimensions records")? {
                if ctx.contains_key_hash_map(
                    &removed,
                    &usage.entity,
                    "resolve SLDPRT dimensions keys",
                )? || ctx.contains_hash_set(
                    &present,
                    &usage.entity,
                    "resolve SLDPRT dimensions keys",
                )? {
                    continue;
                }
                temporary_storage
                    .with_storage(|| ctx.insert_hash_set(&mut present, &usage.entity, OPERATION))?;
            }
            let mut updated = Vec::new();
            for usage in ctx.admit_iter(profile, "scan SLDPRT dimensions records")? {
                let id = if let Some(replacement) =
                    ctx.get_hash_map(&removed, &usage.entity, "resolve SLDPRT dimensions keys")?
                {
                    if ctx.contains_hash_set(
                        &present,
                        replacement,
                        "resolve SLDPRT dimensions keys",
                    )? {
                        continue;
                    }
                    temporary_storage.with_storage(|| {
                        ctx.insert_hash_set(&mut present, replacement, OPERATION)
                    })?;
                    replacement
                } else {
                    &usage.entity
                };
                ctx.reserve_vec(&mut updated, 1, OPERATION)?;
                updated.push(SketchEntityUse {
                    entity: copy_circle_carrier_entity_id(ctx, id)?,
                    reversed: usage.reversed,
                });
            }
            if !updated.is_empty() {
                ctx.reserve_vec(&mut profiles, 1, OPERATION)?;
                profiles.push(updated);
            }
        }
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles) else {
            return Ok(());
        };
        sketch.profiles = profiles;
    }
    ctx.retain_vec(
        entities,
        |entity| Ok(!ctx.contains_key_hash_map(&removed, entity.id(), OPERATION)?),
        OPERATION,
    )?;
    Ok(())
}

fn copy_circle_carrier_entity_id(
    ctx: &DecodeContext<'_>,
    id: &SketchEntityId,
) -> Result<SketchEntityId, cadmpeg_core::CodecError> {
    const OPERATION: &str = "copy SLDPRT circle carrier entity identity";
    let text = ctx.format_retained(format_args!("{}", id.as_str()), OPERATION)?;
    {
        let identity_text = text;
        // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(identity_text.len()),
            "validate SLDPRT dimensions identity",
        )?;
        SketchEntityId::mint(identity_text)
    }
    .map_err(|_| {
        cadmpeg_core::CodecError::malformed("invalid SLDPRT circle carrier entity identity")
    })
}

const MARKER_CIRCLE_OPERATION: &str = "project SLDPRT marker circles";

fn marker_circle_carrier_reference(
    ctx: &DecodeContext<'_>,
    lane_key: &str,
    offset: usize,
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.format_retained(
        format_args!("sldprt:feature-input:sketch-entity#{lane_key}:{offset}"),
        MARKER_CIRCLE_OPERATION,
    )
}

fn unique_marker_circle_center(
    ctx: &DecodeContext<'_>,
    transforms: &[super::transforms::MarkerTransform],
    native: super::grid::GridPoint,
    quantum: f64,
) -> Result<Option<Point2>, cadmpeg_core::CodecError> {
    let mut unique = None;
    for transform in ctx.admit_iter(transforms, MARKER_CIRCLE_OPERATION)? {
        ctx.charge_work(64, MARKER_CIRCLE_OPERATION)?;
        let Some(center) = transform.apply(native) else {
            continue;
        };
        if unique.is_some_and(|previous| previous != center) {
            return Ok(None);
        }
        unique = Some(center);
    }
    Ok(unique.and_then(|center| super::grid::GridPoint::from(center).point(quantum)))
}

fn marker_circle_one_to_one(
    ctx: &DecodeContext<'_>,
    markers: &[(&SketchInputEntity, [f64; 2])],
    dimensions: &[(&cadmpeg_ir::features::DesignParameter, f64)],
    center: [f64; 2],
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    if markers.len() != dimensions.len() {
        return Ok(false);
    }
    let [cu, cv] = center;
    let mut used = HashSet::new();
    for (_, radius) in ctx.admit_iter(dimensions, MARKER_CIRCLE_OPERATION)? {
        let mut unique = None;
        for (index, (_, [ru, rv])) in ctx
            .admit_iter(markers, MARKER_CIRCLE_OPERATION)?
            .enumerate()
        {
            ctx.charge_work(64, MARKER_CIRCLE_OPERATION)?;
            if !same_dimension_length((ru - cu).hypot(rv - cv) * 1000.0, *radius) {
                continue;
            }
            if unique.is_some() {
                return Ok(false);
            }
            unique = Some(index);
        }
        let Some(index) = unique else {
            return Ok(false);
        };
        if !temporary_storage
            .with_storage(|| ctx.insert_hash_set(&mut used, index, MARKER_CIRCLE_OPERATION))?
        {
            return Ok(false);
        }
    }
    Ok(used.len() == markers.len())
}

fn append_marker_circle(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketch: &mut Sketch,
    entity: SketchEntity,
    profile: bool,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.reserve_vec(entities, 1, MARKER_CIRCLE_OPERATION)?;
    if profile {
        let id = copy_circle_carrier_entity_id(ctx, entity.id())?;
        sketch.profiles.push_single(
            ctx,
            SketchEntityUse {
                entity: id,
                reversed: false,
            },
        )?;
    }
    entities.push(entity);
    Ok(())
}

fn copy_marker_circle_sketch_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::sketches::SketchId,
) -> Result<cadmpeg_ir::sketches::SketchId, cadmpeg_core::CodecError> {
    let text = ctx.format_retained(format_args!("{}", id.as_str()), MARKER_CIRCLE_OPERATION)?;
    {
        let identity_text = text;
        // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(identity_text.len()),
            "validate SLDPRT dimensions identity",
        )?;
        cadmpeg_ir::sketches::SketchId::mint(identity_text)
    }
    .map_err(|_| {
        cadmpeg_core::CodecError::malformed("invalid SLDPRT marker circle sketch identity")
    })
}

/// Materialize marker-only circles whose radial witnesses have exact radial
/// dimensions, including repeated circles constrained to the same radius.
pub(crate) fn project_marker_dimensioned_circles(
    ctx: &DecodeContext<'_>,
    entities: &mut Vec<SketchEntity>,
    sketches: &mut [Sketch],
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut temporary_storage = ctx.reserve_scoped(0, "SLDPRT dimension temporary storage")?;

    const OPERATION: &str = "project SLDPRT marker circles";

    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

    let transforms =
        marker_transform_candidates_by_feature(ctx, features, sketches, entities, lanes)?;
    let mut radial_records_by_lane = HashMap::<&str, Vec<_>>::new();
    for lane in ctx.admit_iter(lanes, "scan SLDPRT dimension geometry")? {
        let mut records = Vec::new();
        for record in radial_circle_records(ctx, &lane.native_payload)? {
            let Some(record) = record? else {
                continue;
            };
            ctx.reserve_vec(&mut records, 1, OPERATION)?;
            records.push(record);
        }
        temporary_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut radial_records_by_lane,
                lane.id.as_str(),
                records,
                "resolve SLDPRT dimensions keys",
            )
        })?;
    }
    'feature: for feature in ctx.admit_iter(features, "scan SLDPRT dimension geometry")? {
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
            ctx, entities, sketches, sketch_id, native_ref, lanes,
        )?;
        let mut radial_dimensions = Vec::new();
        for parameter in ctx.admit_iter(parameters, "scan SLDPRT dimension geometry")? {
            if !ctx.equal(
                &(parameter.owner.as_ref()),
                &(Some(&feature.id)),
                "compare SLDPRT dimensions records",
            )? {
                continue;
            }
            let Some(radius) = radial_dimension_radius(parameter) else {
                continue;
            };
            ctx.reserve_vec(&mut radial_dimensions, 1, OPERATION)?;
            radial_dimensions.push((parameter, radius));
        }
        if radial_dimensions.is_empty() {
            continue;
        }
        let feature_key = ctx
            .rsplit_once(&feature.id.as_str(), "#", "resolve SLDPRT dimensions keys")?
            .map_or(feature.id.as_str(), |(_, key)| key);
        let mut owned_lanes = Vec::new();
        for lane in ctx.admit_iter(lanes, "scan SLDPRT dimension geometry")? {
            if {
                let mut search_result = false;
                for marker in
                    ctx.admit_iter(&lane.sketch_entities, "scan SLDPRT dimensions records")?
                {
                    if ctx.equal(
                        &(marker.feature_ref.as_deref()),
                        &(Some(native_ref)),
                        "compare SLDPRT dimensions records",
                    )? {
                        search_result = true;
                        break;
                    }
                }
                search_result
            } {
                ctx.reserve_vec(&mut owned_lanes, 1, OPERATION)?;
                owned_lanes.push(lane);
            }
        }
        let mut markers = Vec::new();
        for iteration_lane in
            ctx.admit_iter(&owned_lanes[..], "scan SLDPRT dimensions source records")?
        {
            for marker in ctx.admit_iter(
                &iteration_lane.sketch_entities,
                "scan SLDPRT dimensions records",
            )? {
                if !ctx.equal(&marker.feature_ref.as_deref(), &Some(native_ref), OPERATION)? {
                    continue;
                }
                let Some(coordinates) = marker.coordinates_m else {
                    continue;
                };
                ctx.reserve_vec(&mut markers, 1, OPERATION)?;
                markers.push((marker, coordinates.get()));
            }
        }
        let feature_transforms: &[super::transforms::MarkerTransform] = ctx
            .get_hash_map(&transforms, native_ref, "resolve SLDPRT dimensions keys")?
            .map_or(&[], Vec::as_slice);
        let mut native_carriers = Vec::new();
        for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimensions records")? {
            if ctx.equal(
                &entity.sketch,
                sketch_id,
                "compare SLDPRT dimensions records",
            )? && matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            ) {
                temporary_storage.with_storage(|| {
                    ctx.push_vec(&mut native_carriers, entity, MARKER_CIRCLE_OPERATION)
                })?;
            }
        }
        let has_resolved_curves = {
            let mut search_result = false;
            for entity in ctx.admit_iter(&(entities)[..], "scan SLDPRT dimensions records")? {
                if {
                    ctx.equal(
                        &(entity.sketch),
                        &(*sketch_id),
                        "compare SLDPRT dimensions records",
                    )? && matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Line { .. }
                            | SketchGeometryDefinition::Arc { .. }
                            | SketchGeometryDefinition::Circle { .. }
                            | SketchGeometryDefinition::Ellipse { .. }
                            | SketchGeometryDefinition::Nurbs { .. }
                    )
                } {
                    search_result = true;
                    break;
                }
            }
            search_result
        };
        let circle_only_carrier = match native_carriers.as_slice() {
            [carrier] if !has_resolved_curves => {
                if let Some(reference) = carrier.native_ref.as_deref() {
                    native_radial_record_for_marker(ctx, lanes, native_ref, reference)?.map(
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
            let mut roster = temporary_storage.with_storage(|| {
                ctx.collect_vec(
                    ctx.admit_iter(&markers, MARKER_CIRCLE_OPERATION)?
                        .copied()
                        .filter(|(marker, _)| {
                            matches!(
                                marker.kind(),
                                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                            )
                        }),
                    MARKER_CIRCLE_OPERATION,
                )
            })?;
            ctx.sort_unstable_by_key(
                &mut roster,
                |value| {
                    let (left, _) = value;
                    left.offset()
                },
                Ord::cmp,
                OPERATION,
            )?;
            // Only this suffix can have exactly one witness per dimension.
            if let Some(center_index) = roster
                .len()
                .checked_sub(radial_dimensions.len())
                .and_then(|index| index.checked_sub(1))
            {
                let (_, [cu, cv]) = roster[center_index];
                if marker_circle_one_to_one(
                    ctx,
                    &roster[center_index + 1..],
                    &radial_dimensions,
                    [cu, cv],
                )? {
                    let carrier_radius = roster
                        .get(radial_index)
                        .map(|(_, [ru, rv])| (ru - cu).hypot(rv - cv) * NATIVE_TO_IR);
                    let native_center =
                        quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                    if let Some(center) = unique_marker_circle_center(
                        ctx,
                        feature_transforms,
                        native_center,
                        QUANTUM,
                    )? {
                        let removed = copy_circle_carrier_entity_id(ctx, carrier_id)?;
                        let carrier_ref = ctx.format_retained(
                            format_args!("{carrier_ref}"),
                            MARKER_CIRCLE_OPERATION,
                        )?;
                        ctx.retain_vec(
                            entities,
                            |entity| Ok(!ctx.equal(entity.id(), &removed, OPERATION)?),
                            OPERATION,
                        )?;
                        let Some(sketch) = ({
                            let mut sketch_index = None;
                            for (index, sketch) in ctx
                                .admit_iter(&sketches[..], "find SLDPRT dimension sketch")?
                                .enumerate()
                            {
                                if ctx.equal(
                                    &sketch.id,
                                    sketch_id,
                                    "compare SLDPRT dimension sketch",
                                )? {
                                    sketch_index = Some(index);
                                    break;
                                }
                            }
                            match sketch_index {
                                Some(index) => Some(&mut sketches[index]),
                                None => None,
                            }
                        }) else {
                            continue;
                        };
                        sketch.profiles.retain_uses(ctx, |usage| {
                            Ok(!ctx.equal(&usage.entity, &removed, OPERATION)?)
                        })?;
                        for (index, (parameter, radius)) in ctx
                            .admit_iter(&(radial_dimensions)[..], "scan SLDPRT dimensions records")?
                            .copied()
                            .enumerate()
                        {
                            let id_text = ctx.format_retained(format_args!("sldprt:model:sketch-entity#radial-roster:{feature_key}:{index}"), OPERATION)?;
                            let Ok(entity_id) = ({
                                let identity_text = id_text;
                                // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                                ctx.charge_work(
                                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                                    "validate SLDPRT dimensions identity",
                                )?;
                                SketchEntityId::mint(identity_text)
                            }) else {
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
                                Some(ctx.format_retained(
                                    format_args!("{carrier_ref}"),
                                    MARKER_CIRCLE_OPERATION,
                                )?)
                            } else {
                                None
                            };
                            let geometry_reference = parameter
                                .native_ref
                                .as_deref()
                                .map(|reference| {
                                    ctx.format_retained(
                                        format_args!("{reference}"),
                                        MARKER_CIRCLE_OPERATION,
                                    )
                                })
                                .transpose()?;
                            let entity = SketchEntity::new(
                                entity_id,
                                copy_marker_circle_sketch_id(ctx, sketch_id)?,
                                geometry,
                            )
                            .with_construction(carrier_construction && same_carrier)
                            .with_native_ref(native_reference)
                            .with_geometry_ref(geometry_reference);
                            append_marker_circle(ctx, entities, sketch, entity, true)?;
                        }
                        continue;
                    }
                }
            }
            continue 'feature;
        }
        let mut radial_records = Vec::new();
        for lane in ctx.admit_iter(&owned_lanes, "scan SLDPRT dimension geometry")? {
            let mut range: Option<(usize, usize)> = None;
            for marker in ctx.admit_iter(&lane.sketch_entities, "scan SLDPRT dimension geometry")? {
                if !ctx.equal(&marker.feature_ref.as_deref(), &Some(native_ref), OPERATION)? {
                    continue;
                }
                let offset = usize::try_from(marker.offset())
                    .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                range = Some(range.map_or((offset, offset), |(start, end)| {
                    (start.min(offset), end.max(offset))
                }));
            }
            let Some((start, end)) = range else {
                continue;
            };
            let lane_key = ctx
                .rsplit_once(&lane.id, "#", "resolve SLDPRT dimensions keys")?
                .map_or(lane.id.as_str(), |(_, key)| key);
            let Some(records) = ctx.get_hash_map(
                &radial_records_by_lane,
                lane.id.as_str(),
                "resolve SLDPRT dimensions keys",
            )?
            else {
                continue;
            };
            for record in ctx.admit_iter(records, "scan SLDPRT dimensions records")? {
                ctx.charge_work(64, OPERATION)?;
                if record.0 < start || record.0 > end {
                    continue;
                }
                let carrier_ref = marker_circle_carrier_reference(ctx, lane_key, record.0)?;
                if {
                    let mut search_result = false;
                    for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimensions records")? {
                        if {
                            ctx.equal(
                                &(entity.sketch),
                                &(*sketch_id),
                                "compare SLDPRT dimensions records",
                            )? && ctx.equal(
                                &(entity.native_ref.as_deref()),
                                &(Some(carrier_ref.as_str())),
                                "compare SLDPRT dimensions records",
                            )? && matches!(
                                entity.geometry.definition(),
                                SketchGeometryDefinition::Native { .. }
                            )
                        } {
                            search_result = true;
                            break;
                        }
                    }
                    search_result
                } {
                    ctx.reserve_vec(&mut radial_records, 1, OPERATION)?;
                    radial_records.push((*lane, *record));
                }
            }
        }
        let mut repeated_radial_sets = Vec::new();
        for (lane, (offset, radial_index, construction)) in
            ctx.admit_iter(&radial_records, "scan SLDPRT dimension geometry")?
        {
            if *construction {
                continue;
            }
            let mut roster = Vec::new();
            for marker in ctx.admit_iter(&lane.sketch_entities, "scan SLDPRT dimension geometry")? {
                if !ctx.equal(&marker.feature_ref.as_deref(), &Some(native_ref), OPERATION)?
                    || marker.coordinates_m.is_none()
                {
                    continue;
                }
                temporary_storage
                    .with_storage(|| ctx.push_vec(&mut roster, marker, MARKER_CIRCLE_OPERATION))?;
            }
            ctx.sort_unstable_by_key(&mut roster, |value| value.offset(), Ord::cmp, OPERATION)?;
            for (parameter, radius) in
                ctx.admit_iter(&radial_dimensions, "scan SLDPRT dimension geometry")?
            {
                let Some(pairs) = temporary_storage.with_storage(|| {
                    terminal_repeated_radial_circle_pairs(
                        ctx,
                        *radial_index,
                        &roster,
                        *radius / NATIVE_TO_IR,
                    )
                })?
                else {
                    continue;
                };
                ctx.reserve_vec(&mut repeated_radial_sets, 1, OPERATION)?;
                repeated_radial_sets.push((*lane, *offset, *parameter, *radius, pairs));
            }
        }
        if let [(lane, offset, parameter, radius, pairs)] = repeated_radial_sets.as_slice() {
            let mut transformed = Vec::new();
            for (center, _) in ctx
                .admit_iter(pairs, "scan SLDPRT dimension geometry")?
                .copied()
            {
                let Some([cu, cv]) = center
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
                else {
                    continue;
                };
                let native = quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
                let Some(center) =
                    unique_marker_circle_center(ctx, feature_transforms, native, QUANTUM)?
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
                ctx.reserve_vec(&mut transformed, 1, OPERATION)?;
                transformed.push(geometry);
            }
            if transformed.len() == pairs.len() {
                let lane_key = ctx
                    .rsplit_once(&lane.id, "#", "resolve SLDPRT dimensions keys")?
                    .map_or(lane.id.as_str(), |(_, key)| key);
                let carrier_ref = marker_circle_carrier_reference(ctx, lane_key, *offset)?;
                let mut pair_radial_object_indices = HashSet::new();
                for (_, radial) in ctx
                    .admit_iter(pairs, "scan SLDPRT dimension geometry")?
                    .copied()
                {
                    ctx.charge_work(64, OPERATION)?;
                    if let Some(index) = radial.object_index() {
                        temporary_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut pair_radial_object_indices,
                                index,
                                MARKER_CIRCLE_OPERATION,
                            )
                        })?;
                    }
                }
                let mut consumed_carrier_refs = HashSet::new();
                let Some(records) = ctx.get_hash_map(
                    &radial_records_by_lane,
                    lane.id.as_str(),
                    "resolve SLDPRT dimensions keys",
                )?
                else {
                    continue;
                };
                for (candidate_offset, candidate_radial_index, construction) in
                    ctx.admit_iter(records, "scan SLDPRT dimensions records")?
                {
                    ctx.charge_work(64, OPERATION)?;
                    if *construction {
                        continue;
                    }
                    let candidate_offset_u64 = u64::try_from(*candidate_offset)
                        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    let has_matching_marker = {
                        let mut search_result = false;
                        for marker in
                            ctx.admit_iter(&lane.sketch_entities, "scan SLDPRT dimensions records")?
                        {
                            if {
                                ctx.equal(
                                    &(marker.feature_ref.as_deref()),
                                    &(Some(native_ref)),
                                    "compare SLDPRT dimensions records",
                                )? && marker.offset() == candidate_offset_u64
                            } {
                                search_result = true;
                                break;
                            }
                        }
                        search_result
                    };
                    if has_matching_marker
                        && (*candidate_offset == *offset
                            || match id_from_index(*candidate_radial_index) {
                                Some(index) => ctx.contains_hash_set(
                                    &pair_radial_object_indices,
                                    &index,
                                    "resolve SLDPRT dimensions radial indices",
                                )?,
                                None => false,
                            })
                    {
                        let reference =
                            marker_circle_carrier_reference(ctx, lane_key, *candidate_offset)?;
                        temporary_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut consumed_carrier_refs,
                                reference,
                                MARKER_CIRCLE_OPERATION,
                            )
                        })?;
                    }
                }
                let mut removed = HashSet::new();
                for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimension geometry")? {
                    if ctx.equal(&entity.sketch, sketch_id, OPERATION)?
                        && match entity.native_ref.as_deref() {
                            Some(reference) => ctx.contains_hash_set(
                                &(consumed_carrier_refs),
                                reference,
                                "resolve SLDPRT dimensions references",
                            )?,
                            None => false,
                        }
                    {
                        let id = copy_circle_carrier_entity_id(ctx, entity.id())?;
                        temporary_storage.with_storage(|| {
                            ctx.insert_hash_set(&mut removed, id, MARKER_CIRCLE_OPERATION)
                        })?;
                    }
                }
                ctx.retain_vec(
                    entities,
                    |entity| Ok(!ctx.contains_hash_set(&removed, entity.id(), OPERATION)?),
                    OPERATION,
                )?;
                let Some(sketch) = ({
                    let mut sketch_index = None;
                    for (index, sketch) in ctx
                        .admit_iter(&sketches[..], "find SLDPRT dimension sketch")?
                        .enumerate()
                    {
                        if ctx.equal(&sketch.id, sketch_id, "compare SLDPRT dimension sketch")? {
                            sketch_index = Some(index);
                            break;
                        }
                    }
                    match sketch_index {
                        Some(index) => Some(&mut sketches[index]),
                        None => None,
                    }
                }) else {
                    continue;
                };
                sketch.profiles.retain_uses(ctx, |usage| {
                    Ok(!ctx.contains_hash_set(&removed, &usage.entity, OPERATION)?)
                })?;
                for (index, geometry) in transformed.into_iter().enumerate() {
                    let id_text = ctx.format_retained(format_args!("sldprt:model:sketch-entity#repeated-radial-circle:{lane_key}:{offset}:{index}"), OPERATION)?;
                    let Ok(entity_id) = ({
                        let identity_text = id_text;
                        // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                        ctx.charge_work(
                            cadmpeg_core::decode::u64_from_index(identity_text.len()),
                            "validate SLDPRT dimensions identity",
                        )?;
                        SketchEntityId::mint(identity_text)
                    }) else {
                        continue;
                    };
                    let native_reference = if index == pairs.len() - 1 {
                        Some(ctx.format_retained(
                            format_args!("{carrier_ref}"),
                            MARKER_CIRCLE_OPERATION,
                        )?)
                    } else {
                        None
                    };
                    let geometry_reference = parameter
                        .native_ref
                        .as_deref()
                        .map(|reference| {
                            ctx.format_retained(
                                format_args!("{reference}"),
                                MARKER_CIRCLE_OPERATION,
                            )
                        })
                        .transpose()?;
                    let entity = SketchEntity::new(
                        entity_id,
                        copy_marker_circle_sketch_id(ctx, sketch_id)?,
                        geometry,
                    )
                    .with_native_ref(native_reference)
                    .with_geometry_ref(geometry_reference);
                    append_marker_circle(ctx, entities, sketch, entity, true)?;
                }
                continue 'feature;
            }
        }
        if !radial_records.is_empty() {
            let radial_record_count = radial_records.len();
            let mut resolved = Vec::new();
            for (lane, (offset, radial_index, construction)) in ctx
                .admit_iter(&radial_records, "scan SLDPRT dimension geometry")?
                .copied()
            {
                let mut roster = Vec::new();
                for marker in
                    ctx.admit_iter(&lane.sketch_entities, "scan SLDPRT dimension geometry")?
                {
                    if !ctx.equal(&marker.feature_ref.as_deref(), &Some(native_ref), OPERATION)? {
                        continue;
                    }
                    let Some(coordinates) = marker.coordinates_m else {
                        continue;
                    };
                    temporary_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut roster,
                            (marker, coordinates.get()),
                            MARKER_CIRCLE_OPERATION,
                        )
                    })?;
                }
                ctx.sort_unstable_by_key(
                    &mut roster,
                    |value| {
                        let (left, _) = value;
                        left.offset()
                    },
                    Ord::cmp,
                    OPERATION,
                )?;
                let Some((radial, [ru, rv])) = roster.get(radial_index).copied() else {
                    continue;
                };
                let mut candidates = Vec::new();
                for (marker, [cu, cv]) in
                    ctx.admit_iter(&markers, "scan SLDPRT dimension geometry")?
                {
                    if ctx.equal(
                        &(marker.id()),
                        &(radial.id()),
                        "compare SLDPRT dimensions records",
                    )? {
                        continue;
                    }
                    let measured_radius = (ru - cu).hypot(rv - cv) * NATIVE_TO_IR;
                    let mut unique = None;
                    let mut ambiguous = false;
                    for (parameter, radius) in
                        ctx.admit_iter(&radial_dimensions, "scan SLDPRT dimension geometry")?
                    {
                        ctx.charge_work(64, OPERATION)?;
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
                    ctx.reserve_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push((
                        quantize(Point2::new(*cu, *cv), QUANTUM),
                        *marker,
                        parameter,
                        radius,
                    ));
                }
                ctx.sort_unstable_by_key(
                    &mut candidates,
                    |value| {
                        let (left_center, left_marker, _, _) = value;
                        (*left_center, left_marker.offset())
                    },
                    Ord::cmp,
                    OPERATION,
                )?;
                ctx.dedup_by_key(&mut candidates, |(center, _, _, _)| Ok(*center), OPERATION)?;
                let [(center, marker, parameter, radius)] = candidates.as_slice() else {
                    continue;
                };
                temporary_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut resolved,
                        (
                            lane,
                            offset,
                            construction,
                            *center,
                            *marker,
                            *parameter,
                            *radius,
                        ),
                        OPERATION,
                    )
                })?;
            }
            if resolved.len() == radial_record_count {
                let mut transformed = Vec::new();
                for record in ctx.admit_iter(&resolved, "scan SLDPRT dimension geometry")? {
                    let Some(native_point) = record.3.point(QUANTUM) else {
                        continue;
                    };
                    let native = quantize(
                        Point2::new(native_point.u * NATIVE_TO_IR, native_point.v * NATIVE_TO_IR),
                        QUANTUM,
                    );
                    let Some(center) =
                        unique_marker_circle_center(ctx, feature_transforms, native, QUANTUM)?
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
                    ctx.reserve_vec(&mut transformed, 1, OPERATION)?;
                    transformed.push((record, geometry));
                }
                if transformed.len() == resolved.len() {
                    let mut carrier_refs = HashSet::new();
                    let mut center_refs = HashSet::new();
                    for (lane, offset, _, _, marker, ..) in
                        ctx.admit_iter(&resolved, "scan SLDPRT dimension geometry")?
                    {
                        let lane_key = ctx
                            .rsplit_once(&lane.id, "#", "resolve SLDPRT dimensions keys")?
                            .map_or(lane.id.as_str(), |(_, key)| key);
                        let reference = marker_circle_carrier_reference(ctx, lane_key, *offset)?;
                        temporary_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut carrier_refs,
                                reference,
                                MARKER_CIRCLE_OPERATION,
                            )
                        })?;
                        temporary_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut center_refs,
                                marker.id(),
                                MARKER_CIRCLE_OPERATION,
                            )
                        })?;
                    }
                    let mut removed = HashSet::new();
                    for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimension geometry")? {
                        if ctx.equal(&entity.sketch, sketch_id, OPERATION)?
                            && match entity.native_ref.as_deref() {
                                Some(reference) => {
                                    ctx.contains_hash_set(
                                        &(carrier_refs),
                                        reference,
                                        "resolve SLDPRT dimensions references",
                                    )? || (ctx.contains_hash_set(
                                        &(center_refs),
                                        reference,
                                        "resolve SLDPRT dimensions references",
                                    )? && !matches!(
                                        entity.geometry.definition(),
                                        SketchGeometryDefinition::Point { .. }
                                    ))
                                }
                                None => false,
                            }
                        {
                            let id = copy_circle_carrier_entity_id(ctx, entity.id())?;
                            temporary_storage.with_storage(|| {
                                ctx.insert_hash_set(&mut removed, id, MARKER_CIRCLE_OPERATION)
                            })?;
                        }
                    }
                    ctx.retain_vec(
                        entities,
                        |entity| Ok(!ctx.contains_hash_set(&removed, entity.id(), OPERATION)?),
                        OPERATION,
                    )?;
                    let Some(sketch) = ({
                        let mut sketch_index = None;
                        for (index, sketch) in ctx
                            .admit_iter(&sketches[..], "find SLDPRT dimension sketch")?
                            .enumerate()
                        {
                            if ctx.equal(
                                &sketch.id,
                                sketch_id,
                                "compare SLDPRT dimension sketch",
                            )? {
                                sketch_index = Some(index);
                                break;
                            }
                        }
                        match sketch_index {
                            Some(index) => Some(&mut sketches[index]),
                            None => None,
                        }
                    }) else {
                        continue;
                    };
                    sketch.profiles.retain_uses(ctx, |usage| {
                        Ok(!ctx.contains_hash_set(&removed, &usage.entity, OPERATION)?)
                    })?;
                    for (record, geometry) in transformed {
                        let lane_key = ctx
                            .rsplit_once(&record.0.id, "#", "resolve SLDPRT dimensions keys")?
                            .map_or(record.0.id.as_str(), |(_, key)| key);
                        let id_text = ctx.format_retained(
                            format_args!(
                                "sldprt:model:sketch-entity#radial-circle:{lane_key}:{}",
                                record.1
                            ),
                            OPERATION,
                        )?;
                        let Ok(entity_id) = ({
                            let identity_text = id_text;
                            // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                            ctx.charge_work(
                                cadmpeg_core::decode::u64_from_index(identity_text.len()),
                                "validate SLDPRT dimensions identity",
                            )?;
                            SketchEntityId::mint(identity_text)
                        }) else {
                            continue;
                        };
                        let native_reference =
                            marker_circle_carrier_reference(ctx, lane_key, record.1)?;
                        let geometry_reference = record
                            .5
                            .native_ref
                            .as_deref()
                            .map(|reference| {
                                ctx.format_retained(
                                    format_args!("{reference}"),
                                    MARKER_CIRCLE_OPERATION,
                                )
                            })
                            .transpose()?;
                        let entity = SketchEntity::new(
                            entity_id,
                            copy_marker_circle_sketch_id(ctx, sketch_id)?,
                            geometry,
                        )
                        .with_construction(record.2)
                        .with_native_ref(Some(native_reference))
                        .with_geometry_ref(geometry_reference);
                        append_marker_circle(ctx, entities, sketch, entity, !record.2)?;
                    }
                    continue;
                }
            }
        }
        let centers = temporary_storage.with_storage(|| {
            ctx.collect_vec(
                ctx.admit_iter(&markers, MARKER_CIRCLE_OPERATION)?
                    .copied()
                    .filter(|(marker, _)| marker.kind() == SketchInputKind::LineOrCircle),
                MARKER_CIRCLE_OPERATION,
            )
        })?;
        let radial = temporary_storage.with_storage(|| {
            ctx.collect_vec(
                ctx.admit_iter(&markers, MARKER_CIRCLE_OPERATION)?
                    .copied()
                    .filter(|(marker, _)| {
                        matches!(
                            marker.kind(),
                            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                        )
                    }),
                MARKER_CIRCLE_OPERATION,
            )
        })?;
        let [(center_marker, coordinates)] = centers.as_slice() else {
            continue;
        };
        let [cu, cv] = *coordinates;
        if !marker_circle_one_to_one(ctx, &radial, &radial_dimensions, [cu, cv])? {
            continue;
        }
        let native_center = quantize(Point2::new(cu * NATIVE_TO_IR, cv * NATIVE_TO_IR), QUANTUM);
        let Some(center) =
            unique_marker_circle_center(ctx, feature_transforms, native_center, QUANTUM)?
        else {
            continue;
        };
        let Some(sketch) = ({
            let mut sketch_index = None;
            for (index, sketch) in ctx
                .admit_iter(&sketches[..], "find SLDPRT dimension sketch")?
                .enumerate()
            {
                if ctx.equal(&sketch.id, sketch_id, "compare SLDPRT dimension sketch")? {
                    sketch_index = Some(index);
                    break;
                }
            }
            match sketch_index {
                Some(index) => Some(&mut sketches[index]),
                None => None,
            }
        }) else {
            continue;
        };
        for (parameter, radius) in ctx
            .admit_iter(&radial_dimensions, "scan SLDPRT dimension geometry")?
            .copied()
        {
            let Some(construction) = native_dimensioned_circle_construction_state(
                ctx,
                lanes,
                native_ref,
                center_marker,
                radius,
            )?
            else {
                continue;
            };
            if {
                let mut search_result = false;
                for entity in ctx.admit_iter(&entities[..], "scan SLDPRT dimensions records")? {
                    if ctx.equal(
                        &(entity.sketch),
                        &(*sketch_id),
                        "compare SLDPRT dimensions records",
                    )? && matches!(entity.geometry.definition(),
                SketchGeometryDefinition::Circle { center: existing, radius: existing_radius }
                    if quantize(existing.get(), QUANTUM) == quantize(center, QUANTUM) && same_dimension_length(existing_radius.get(), radius))
                    {
                        search_result = true;
                        break;
                    }
                }
                search_result
            } {
                continue;
            }
            let id_text = ctx.format_retained(
                format_args!(
                    "sldprt:model:sketch-entity#marker-circle:{feature_key}:{}",
                    parameter.ordinal
                ),
                OPERATION,
            )?;
            let Ok(entity_id) = ({
                let identity_text = id_text;
                // The moved String is scanned by mint's grammar validation; mint does not allocate temporary storage.
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(identity_text.len()),
                    "validate SLDPRT dimensions identity",
                )?;
                SketchEntityId::mint(identity_text)
            }) else {
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
            let geometry_reference = parameter
                .native_ref
                .as_deref()
                .map(|reference| {
                    ctx.format_retained(format_args!("{reference}"), MARKER_CIRCLE_OPERATION)
                })
                .transpose()?;
            let entity = SketchEntity::new(
                entity_id,
                copy_marker_circle_sketch_id(ctx, sketch_id)?,
                geometry,
            )
            .with_construction(construction)
            .with_geometry_ref(geometry_reference);
            append_marker_circle(ctx, entities, sketch, entity, true)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod dimensions_tests;
