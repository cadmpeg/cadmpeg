// SPDX-License-Identifier: Apache-2.0
//! Section geometry conversion from live and saved section entities.

use super::axis::SectionAxis;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length, PositiveLength};
use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId};
use cadmpeg_ir::units::FinitePoint2;

use super::super::sketch_ids::sketch_entity_id_admitted;
use super::radii::trim_segment_id;
use super::skamp::section_line_entity_fixed_coordinate;
use crate::decode::sketch_transfer::identity::{
    saved_section_internal_id_is_unique, saved_section_ordinary_geometry_allowed,
    semantic_saved_section_entities,
};
use crate::decode::sketch_transfer::loci::section_saved_entity;

const EPS_POINT_NONZERO: f64 = 1.0e-12;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;
const EPS_DENOMINATOR_NONZERO: f64 = 1.0e-12;
/// Bounds every slot of a saved section conic's twelve-slot local system: the two in-plane axis
/// lengths, their dot product, their `2x2` determinant, and the out-of-plane slots the section
/// record holds at zero or one. This is a plane record, not the three-dimensional direction pair
/// of [`cadmpeg_ir::units::OrthonormalFrame3::new`].
const EPS_SECTION_FRAME_ORTHONORMAL: f64 = 1.0e-9;
const EPS_PARAMETER_AGREEMENT: f64 = 1.0e-9;
const EPS_PARAMETER_FULL_TURN: f64 = 1.0e-9;
const EPS_ANGLE_FULL_TURN: f64 = 1.0e-12;

pub(in crate::decode) fn section_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    let crate::feature::definitions::FeatureSegmentKind::Line([start, end]) = segment.kind else {
        return None;
    };
    let start = points.get(&start)?;
    let end = points.get(&end)?;
    let scale = start
        .iter()
        .chain(end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (((end[0] - start[0]) / scale).hypot((end[1] - start[1]) / scale) > EPS_POINT_NONZERO)
        .then_some(())?;
    SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
    .ok()
}

pub(in crate::decode) fn section_point_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    let crate::feature::definitions::FeatureSegmentKind::Point(point) = segment.kind else {
        return None;
    };
    let position = points.get(&point)?;
    SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: cadmpeg_ir::math::Point2::new(position[0], position[1]),
    })
    .ok()
}

pub(in crate::decode) fn section_arc_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    (matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Arc(_)
    ) && segment.arc_orientation == Some(0))
    .then_some(())?;
    let center = points.get(&segment.center_id?)?;
    let first = points.get(&segment.point_ids()[0])?;
    let second = points.get(&segment.point_ids()[1])?;
    let offset = |point: &[f64; 2]| [point[0] - center[0], point[1] - center[1]];
    let first_offset = offset(first);
    let second_offset = offset(second);
    let first_radius = first_offset[0].hypot(first_offset[1]);
    let second_radius = second_offset[0].hypot(second_offset[1]);
    let scale = first_radius.max(second_radius);
    if !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius <= EPS_POINT_NONZERO
        || (first_radius - second_radius).abs() > EPS_RADIUS_AGREEMENT * scale
    {
        return None;
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: cadmpeg_ir::math::Point2::new(center[0], center[1]),
        radius: Length::new(first_radius)?,
        start_angle: Angle::new(start)?,
        end_angle: Angle::new(end)?,
    })
    .ok()
}

pub(in crate::decode) fn section_circle_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    radii: &BTreeMap<u32, f64>,
    segment: &crate::feature::definitions::FeatureCircleSegment,
) -> Option<SketchGeometry> {
    let center = points.get(&segment.center_id)?;
    let radius = *radii.get(&segment.radius_ref)?;
    SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(center[0], center[1]),
        radius: Length::new(radius)?,
    })
    .ok()
}

pub(in crate::decode) fn section_point_row_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeaturePointSegment,
) -> Option<SketchGeometry> {
    let point = points.get(&segment.point_id)?;
    SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(point[0], point[1]),
    })
    .ok()
}

pub(in crate::decode) fn section_centered_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureCenteredLineSegment,
) -> Option<SketchGeometry> {
    let start = points.get(&0)?;
    let end = points.get(&1)?;
    let center = points.get(&segment.center_id)?;
    let scale = start
        .iter()
        .chain(end)
        .chain(center)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((end[0] - start[0]).hypot(end[1] - start[1]) > EPS_POINT_NONZERO * scale).then_some(())?;
    ((start[0] + end[0] - 2.0 * center[0]).hypot(start[1] + end[1] - 2.0 * center[1])
        <= EPS_RADIUS_AGREEMENT * scale)
        .then_some(())?;
    SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(start[0], start[1]),
        end: Point2::new(end[0], end[1]),
    })
    .ok()
}

pub(in crate::decode) fn section_reference_line_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureReferenceLineSegment,
) -> Option<SketchGeometry> {
    let [Some(start_id), Some(end_id)] = segment.point_ids else {
        return None;
    };
    let start = points.get(&start_id)?;
    let end = points.get(&end_id)?;
    let scale = start
        .iter()
        .chain(end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let direction = [end[0] - start[0], end[1] - start[1]];
    (direction[0].hypot(direction[1]) > EPS_POINT_NONZERO * scale).then_some(())?;
    SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(direction[0], direction[1]),
    })
    .ok()
}

pub(in crate::decode) fn resolved_section_reference_line_geometry(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureReferenceLineSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(geometry) = section_reference_line_geometry(points, segment) {
        return Ok(Some(geometry));
    }
    let [Some(start_id), Some(end_id)] = segment.point_ids else {
        return Ok(None);
    };
    let Some(fixed_coordinate) =
        section_line_entity_fixed_coordinate(ctx, definition, segment.external_id)?
    else {
        return Ok(None);
    };
    Ok((|| {
        let [Some(first), Some(second)] =
            [start_id, end_id].map(|point| variable_points.get(&point)?[fixed_coordinate.index()])
        else {
            return None;
        };
        let scale = first.abs().max(second.abs()).max(1.0);
        ((first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale)
            .then(|| {
                if fixed_coordinate == SectionAxis::U {
                    SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                        origin: Point2::new(first, 0.0),
                        direction: Point2::new(0.0, 1.0),
                    })
                    .ok()
                } else {
                    SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
                        origin: Point2::new(0.0, first),
                        direction: Point2::new(1.0, 0.0),
                    })
                    .ok()
                }
            })
            .flatten()
    })())
}

pub(in crate::decode) fn section_segment_geometry(
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Option<SketchGeometry> {
    section_line_geometry(points, segment)
        .or_else(|| section_arc_geometry(points, segment))
        .or_else(|| section_point_geometry(points, segment))
}

pub(in crate::decode) fn saved_section_line_geometry(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    let Some(order_table) = (|| {
        matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Line(_)).then_some(())?;
        saved_section_ordinary_geometry_allowed(definition, segment).then_some(())?;
        definition.order_table.as_ref()
    })() else { return Ok(None); };
    let mut internal_id = order_table.internal_id(ctx, segment.external_id)?;
    if internal_id.is_none() {
        let interpolation = (|| {
            let segment_table = definition.segments.as_ref()?;
            segment_table.is_complete().then_some(())?;
            let position = segment_table.rows.ordinary().position(|candidate| candidate.external_id == segment.external_id)?;
            Some((segment_table, position))
        })();
        if let Some((segment_table, position)) = interpolation {
            let mut previous = None;
            for candidate in segment_table.rows.ordinary().take(position) {
                if let Some(id) = order_table.internal_id(ctx, candidate.external_id)? { previous = Some(id); }
            }
            let mut next = None;
            for candidate in segment_table.rows.ordinary().skip(position + 1) {
                if let Some(id) = order_table.internal_id(ctx, candidate.external_id)? { next = Some(id); break; }
            }
            internal_id = (|| {
                let internal_id = previous?.checked_add(1)?;
                (next? == internal_id.checked_add(1)? && semantic_saved_section_entities(definition).any(|entity| {
                    matches!(entity, crate::feature::definitions::FeatureSavedEntity::Line(line) if line.entity_id == internal_id)
                })).then_some(internal_id)
            })();
        }
    }
    let internal_id = internal_id
        .or_else(|| {
            order_table.is_complete().then_some(())?;
            let trimmed = definition.trim_entities.as_ref()?;
            (trimmed.has_complete_bucket_frame() && trimmed.has_unique_external_ids())
                .then_some(())?;
            let segment_table = definition.segments.as_ref()?;
            segment_table.is_complete().then_some(())?;
            let external_id = crate::decode::uniqueness::exactly_one(segment_table.rows.ordinary()
                .filter(|candidate| {
                    matches!(candidate.kind, crate::feature::definitions::FeatureSegmentKind::Line(_))
                        && trimmed.rows.iter().filter_map(|row| trim_segment_id(definition, row))
                            .any(|id| id == candidate.external_id)
                        && !order_table.rows.iter().any(|row| row.external_id == candidate.external_id)
                })
                .map(|candidate| candidate.external_id))?;
            let internal_id = crate::decode::uniqueness::exactly_one(semantic_saved_section_entities(definition)
                .filter_map(|entity| match entity {
                    crate::feature::definitions::FeatureSavedEntity::Line(line)
                        if !order_table.rows.iter().any(|row| row.internal_id == line.entity_id) =>
                    {
                        Some(line.entity_id)
                    }
                    _ => None,
                }))?;
            (external_id == segment.external_id).then_some(internal_id)
        });
    Ok((|| {
    let internal_id = internal_id?;
    saved_section_internal_id_is_unique(definition, internal_id).then_some(())?;
    let line = semantic_saved_section_entities(definition).find_map(|entity| match entity {
        crate::feature::definitions::FeatureSavedEntity::Line(line)
            if line.entity_id == internal_id =>
        {
            Some(line)
        }
        _ => None,
    })?;
    let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints else {
        return None;
    };
    SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: cadmpeg_ir::math::Point2::new(start_u, start_v),
        end: cadmpeg_ir::math::Point2::new(end_u, end_v),
    })
    .ok()    })())
}

pub(super) fn saved_section_arc_record<'a>(
    ctx: &DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<&'a crate::feature::definitions::FeatureSavedArc>, CodecError> {
    let Some(order) = (|| {
        (matches!(segment.kind, crate::feature::definitions::FeatureSegmentKind::Arc(_)) && segment.arc_orientation == Some(0)).then_some(())?;
        saved_section_ordinary_geometry_allowed(definition, segment).then_some(())?;
        definition.order_table.as_ref()
    })() else { return Ok(None); };
    let Some(internal_id) = order.internal_id(ctx, segment.external_id)? else { return Ok(None); };
    Ok((|| {
    saved_section_internal_id_is_unique(definition, internal_id).then_some(())?;
    semantic_saved_section_entities(definition).find_map(|entity| match entity {
        crate::feature::definitions::FeatureSavedEntity::Arc(arc)
            if arc.entity_id == internal_id =>
        {
            Some(arc)
        }
        _ => None,
    })
    })())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::decode) struct SectionArcCarrier {
    pub(in crate::decode) center: FinitePoint2,
    pub(in crate::decode) radius: PositiveLength,
}

impl SectionArcCarrier {
    pub(in crate::decode) fn new(center: [f64; 2], radius: f64) -> Option<Self> {
        Some(Self {
            center: FinitePoint2::new(Point2::new(center[0], center[1]))?,
            radius: PositiveLength::new(radius)?,
        })
    }

    pub(in crate::decode) fn raw(self) -> ([f64; 2], f64) {
        let center = self.center.get();
        ([center.u, center.v], self.radius.get())
    }
}

pub(in crate::decode) fn saved_section_arc_carrier(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SectionArcCarrier>, CodecError> {
    let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else { return Ok(None); };
    Ok((|| {
    let [center_u, center_v, _] = arc.center;
    if let ([Some(center_u), Some(center_v)], Some(radius)) = (
        [center_u, center_v],
        arc.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
    ) {
        return SectionArcCarrier::new([center_u, center_v], radius);
    }
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let scale = [first_u, first_v, second_u, second_v]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    let [center_u, center_v] = match [center_u, center_v] {
        [Some(u), Some(v)] => [u, v],
        [Some(u), None] => {
            let denominator = 2.0 * (second_v - first_v);
            if denominator.abs() <= EPS_DENOMINATOR_NONZERO * scale {
                return None;
            }
            let v = ((second_u - u).mul_add(
                second_u - u,
                second_v * second_v - (first_u - u) * (first_u - u) - first_v * first_v,
            )) / denominator;
            [u, v]
        }
        [None, Some(v)] => {
            let denominator = 2.0 * (second_u - first_u);
            if denominator.abs() <= EPS_DENOMINATOR_NONZERO * scale {
                return None;
            }
            let u = ((second_v - v).mul_add(
                second_v - v,
                second_u * second_u - (first_v - v) * (first_v - v) - first_u * first_u,
            )) / denominator;
            [u, v]
        }
        [None, None] => return None,
    };
    let first_radius = (first_u - center_u).hypot(first_v - center_v);
    let second_radius = (second_u - center_u).hypot(second_v - center_v);
    let radial_scale = first_radius.max(second_radius);
    if !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius <= EPS_POINT_NONZERO
        || (first_radius - second_radius).abs() > EPS_RADIUS_AGREEMENT * radial_scale
        || arc.radius.is_some_and(|stored| {
            (stored - first_radius).abs() > EPS_RADIUS_AGREEMENT * stored.max(first_radius)
        })
    {
        return None;
    }
    let radius = arc.radius.unwrap_or(first_radius);
    SectionArcCarrier::new([center_u, center_v], radius)
    })())
}

/// The arc facts recovered from a saved-section row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::decode) struct SavedSectionArc {
    pub(in crate::decode) center: cadmpeg_ir::units::FinitePoint2,
    pub(in crate::decode) radius: cadmpeg_ir::scalar::PositiveLength,
    pub(in crate::decode) start_angle: Angle,
    pub(in crate::decode) end_angle: Angle,
}

impl SavedSectionArc {
    fn into_geometry(self) -> Option<SketchGeometry> {
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: self.center.into(),
            radius: self.radius.into(),
            start_angle: self.start_angle,
            end_angle: self.end_angle,
        })
        .ok()
    }
}

pub(in crate::decode) fn saved_section_arc(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SavedSectionArc>, CodecError> {
    let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else { return Ok(None); };
    let Some(carrier) = saved_section_arc_carrier(ctx, definition, segment)? else { return Ok(None); };
    Ok((|| {
    let ([center_u, center_v], radius) = carrier.raw();
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return None;
    };
    let first = [first_u - center_u, first_v - center_v];
    let second = [second_u - center_u, second_v - center_v];
    let first_radius = first[0].hypot(first[1]);
    let second_radius = second[0].hypot(second[1]);
    let scale = radius.max(first_radius).max(second_radius);
    if !first_radius.is_finite()
        || !second_radius.is_finite()
        || (first_radius - radius).abs() > EPS_RADIUS_AGREEMENT * scale
        || (second_radius - radius).abs() > EPS_RADIUS_AGREEMENT * scale
    {
        return None;
    }
    let start = second[1].atan2(second[0]);
    let mut end = first[1].atan2(first[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    Some(SavedSectionArc {
        center: carrier.center,
        radius: carrier.radius,
        start_angle: Angle::new(start)?,
        end_angle: Angle::new(end)?,
    })
    })())
}

pub(in crate::decode) fn saved_section_segment_point_coordinates(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<impl Iterator<Item = (u32, [f64; 2])>>, CodecError> {
    let coordinates = match segment.kind {
        crate::feature::definitions::FeatureSegmentKind::Line(_) => {
            let Some(geometry) = saved_section_line_geometry(ctx, definition, segment)? else { return Ok(None); };
            let Some([start, end]) = saved_geometry_endpoints(&geometry) else { return Ok(None); };
            Some([
                Some((segment.point_ids()[0], start)),
                Some((segment.point_ids()[1], end)),
                None,
            ])
        }
        crate::feature::definitions::FeatureSegmentKind::Arc(_) => {
            let Some(saved_arc) = saved_section_arc(ctx, definition, segment)? else { return Ok(None); };
            let center = *saved_arc.center.as_raw();
            let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else { return Ok(None); };
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return Ok(None);
            };
            let Some(center_id) = segment.center_id else { return Ok(None); };
            Some([
                Some((segment.point_ids()[0], [first_u, first_v])),
                Some((segment.point_ids()[1], [second_u, second_v])),
                Some((center_id, [center.u, center.v])),
            ])
        }
        crate::feature::definitions::FeatureSegmentKind::Point(_) => None,
    };
    Ok(coordinates.map(|points| points.into_iter().flatten()))
}

pub(in crate::decode) fn saved_section_circle_values(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureCircleSegment,
) -> Result<Option<([f64; 2], f64)>, CodecError> {
    if definition.segments.as_ref().and_then(|segments| segments.rows.get(segment.external_id)).is_none() { return Ok(None); }
    let Some(entity) = section_saved_entity(ctx, definition, segment.external_id)? else { return Ok(None); };
    Ok((|| {
    let (_, geometry, _) = saved_section_entity_geometry(entity)?;
    let SketchGeometryDefinition::Circle { center, radius } = geometry.definition() else {
        return None;
    };
    Some(([center.u, center.v], radius.get()))
    })())
}

pub(in crate::decode) fn saved_section_entity_geometry(
    entity: &crate::feature::definitions::FeatureSavedEntity,
) -> Option<(u32, SketchGeometry, usize)> {
    match entity {
        crate::feature::definitions::FeatureSavedEntity::Line(line) => {
            let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints
            else {
                return None;
            };
            Some((
                line.entity_id,
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                })
                .ok()?,
                line.offset,
            ))
        }
        crate::feature::definitions::FeatureSavedEntity::Arc(arc) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [arc.center[0], arc.center[1]],
                arc.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
            ) else {
                return None;
            };
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return None;
            };
            let first = [first_u - center_u, first_v - center_v];
            let second = [second_u - center_u, second_v - center_v];
            let scale = radius
                .max(first[0].hypot(first[1]))
                .max(second[0].hypot(second[1]));
            if ![radius, first[0], first[1], second[0], second[1], scale]
                .into_iter()
                .all(f64::is_finite)
                || (first[0].hypot(first[1]) - radius).abs() > EPS_RADIUS_AGREEMENT * scale
                || (second[0].hypot(second[1]) - radius).abs() > EPS_RADIUS_AGREEMENT * scale
            {
                return None;
            }
            let start_angle = second[1].atan2(second[0]);
            let mut end_angle = first[1].atan2(first[0]);
            while end_angle <= start_angle {
                end_angle += std::f64::consts::TAU;
            }
            Some((
                arc.entity_id,
                SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                    center: Point2::new(center_u, center_v),
                    radius: Length::new(radius)?,
                    start_angle: Angle::new(start_angle)?,
                    end_angle: Angle::new(end_angle)?,
                })
                .ok()?,
                arc.offset,
            ))
        }
        crate::feature::definitions::FeatureSavedEntity::Circle(circle) => {
            let ([Some(center_u), Some(center_v)], Some(radius)) = (
                [circle.center[0], circle.center[1]],
                circle.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
            ) else {
                return None;
            };
            Some((
                circle.entity_id,
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(center_u, center_v),
                    radius: Length::new(radius)?,
                })
                .ok()?,
                circle.offset,
            ))
        }
        crate::feature::definitions::FeatureSavedEntity::Conic(conic) => {
            let (Some(frame), [Some(first_radius), Some(second_radius)]) =
                (conic.local_system, conic.coefficients)
            else {
                return None;
            };
            let first_axis = [frame[0], frame[1]];
            let second_axis = [frame[3], frame[4]];
            let first_length = first_axis[0].hypot(first_axis[1]);
            let second_length = second_axis[0].hypot(second_axis[1]);
            let scale = first_length.max(second_length).max(1.0);
            if first_radius <= EPS_POINT_NONZERO
                || second_radius <= EPS_POINT_NONZERO
                || (first_length - 1.0).abs() > EPS_SECTION_FRAME_ORTHONORMAL * scale
                || (second_length - 1.0).abs() > EPS_SECTION_FRAME_ORTHONORMAL * scale
                || (first_axis[0] * second_axis[0] + first_axis[1] * second_axis[1]).abs()
                    > EPS_SECTION_FRAME_ORTHONORMAL
                || (first_axis[0] * second_axis[1] - first_axis[1] * second_axis[0] - 1.0).abs()
                    > EPS_SECTION_FRAME_ORTHONORMAL
                || frame[2].abs() > EPS_SECTION_FRAME_ORTHONORMAL
                || frame[5].abs() > EPS_SECTION_FRAME_ORTHONORMAL
                || frame[6].abs() > EPS_SECTION_FRAME_ORTHONORMAL
                || frame[7].abs() > EPS_SECTION_FRAME_ORTHONORMAL
                || (frame[8] - 1.0).abs() > EPS_SECTION_FRAME_ORTHONORMAL
                || frame[11].abs() > EPS_SECTION_FRAME_ORTHONORMAL
            {
                return None;
            }
            let (major_axis, major_radius, minor_radius, parameter_shift) =
                if first_radius >= second_radius {
                    (first_axis, first_radius, second_radius, 0.0)
                } else {
                    (
                        second_axis,
                        second_radius,
                        first_radius,
                        -std::f64::consts::FRAC_PI_2,
                    )
                };
            let coincident_endpoints = conic.endpoints.iter().flatten().all(Option::is_some)
                && conic.endpoints[0]
                    .into_iter()
                    .zip(conic.endpoints[1])
                    .all(|(first, second)| {
                        let (Some(first), Some(second)) = (first, second) else {
                            return false;
                        };
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale
                    });
            let bounds = match conic.parameters {
                [Some(start), Some(end)]
                    if start.is_finite()
                        && end.is_finite()
                        && (end - start - std::f64::consts::TAU).abs()
                            <= EPS_PARAMETER_FULL_TURN =>
                {
                    None
                }
                [Some(start), Some(end)] if start.is_finite() && end > start => Some([
                    Angle::new(start + parameter_shift)?,
                    Angle::new(end + parameter_shift)?,
                ]),
                [Some(start), None]
                    if start.is_finite()
                        && start.abs() <= EPS_PARAMETER_FULL_TURN
                        && coincident_endpoints =>
                {
                    None
                }
                _ => return None,
            };
            Some((
                conic.entity_id,
                SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                    center: Point2::new(frame[9], frame[10]),
                    major_angle: Angle::new(major_axis[1].atan2(major_axis[0]))?,
                    radii: cadmpeg_ir::sketches::EllipseRadii {
                        major_radius: Length::new(major_radius)?,
                        minor_radius: Length::new(minor_radius)?,
                    },
                    bounds,
                })
                .ok()?,
                conic.offset,
            ))
        }
        crate::feature::definitions::FeatureSavedEntity::Spline(_)
        | crate::feature::definitions::FeatureSavedEntity::Dummy(_) => None,
    }
}

pub(in crate::decode) fn is_full_circle_geometry(geometry: &SketchGeometry) -> bool {
    matches!(
        geometry.definition(),
        SketchGeometryDefinition::Circle { .. }
    ) || matches!((
        geometry).definition(),
        SketchGeometryDefinition::Arc {
            start_angle,
            end_angle,
            ..
            } if (end_angle.get() - start_angle.get() - std::f64::consts::TAU).abs()
                <= EPS_ANGLE_FULL_TURN
    )
}

fn saved_geometry_endpoints(geometry: &SketchGeometry) -> Option<[[f64; 2]; 2]> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } if !is_full_circle_geometry(geometry) => Some([
            [
                center.u + radius.get() * start_angle.get().cos(),
                center.v + radius.get() * start_angle.get().sin(),
            ],
            [
                center.u + radius.get() * end_angle.get().cos(),
                center.v + radius.get() * end_angle.get().sin(),
            ],
        ]),
        SketchGeometryDefinition::Nurbs { curve } => {
            let (first, last) = match curve.pole_rows() {
                PcurveNurbsPoles::Polynomial { points } => {
                    (points.first()?.get(), points.last()?.get())
                }
                PcurveNurbsPoles::Rational { points } => {
                    (points.first()?.point.get(), points.last()?.point.get())
                }
            };
            Some([[first.u, first.v], [last.u, last.v]])
        }
        _ => None,
    }
}

pub(in crate::decode) fn saved_section_missing_line_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<Option<(usize, SketchGeometry)>, cadmpeg_core::CodecError> {
    let Some(order) = definition.order_table.as_ref() else {
        return Ok(None);
    };
    if !order.is_complete() {
        return Ok(None);
    }
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    if !segments.is_complete() {
        return Ok(None);
    }
    let Some(trim) = definition.trim_entities.as_ref() else {
        return Ok(None);
    };
    if !trim.has_complete_bucket_frame() || !trim.has_unique_external_ids() {
        return Ok(None);
    }
    let mut trimmed_external_ids = BTreeSet::new();
    for id in trim
        .rows
        .iter()
        .filter_map(|row| trim_segment_id(definition, row))
    {
        ctx.insert_btree_set(
            &mut trimmed_external_ids,
            id,
            "creo missing-line trimmed ID nodes",
        )?;
    }
    let mut missing = None;
    for candidate in segments.rows.ordinary() {
        if matches!(candidate.kind, crate::feature::definitions::FeatureSegmentKind::Line(_))
            && order.internal_id(ctx, candidate.external_id)?.is_none()
            && trimmed_external_ids.contains(&candidate.external_id) {
            if missing.is_some() { return Ok(None); }
            missing = Some(candidate);
        }
    }
    let Some(missing) = missing else { return Ok(None); };
    let Some(fixed_coordinate) = missing
        .vertical_horizontal
        .and_then(SectionAxis::from_selector)
    else {
        return Ok(None);
    };

    let mut geometries = Vec::new();
    for entity in semantic_saved_section_entities(definition) {
        let Some(geometry) = saved_section_entity_geometry(entity) else {
            continue;
        };
        if order.rows.iter().any(|row| row.internal_id == geometry.0) {
            ctx.reserve_vec(&mut geometries, 1, "creo missing-line saved geometries")?;
            geometries.push(geometry);
        }
    }
    let mut ordered_ids = BTreeSet::new();
    for row in &order.rows {
        ctx.insert_btree_set(
            &mut ordered_ids,
            row.internal_id,
            "creo missing-line ordered ID nodes",
        )?;
    }
    let mut geometry_ids = BTreeSet::new();
    for (internal_id, _, _) in &geometries {
        ctx.insert_btree_set(
            &mut geometry_ids,
            *internal_id,
            "creo missing-line geometry ID nodes",
        )?;
    }
    if ordered_ids.len() != order.rows.len()
        || geometry_ids.len() != geometries.len()
        || geometry_ids != ordered_ids
    {
        return Ok(None);
    }
    let mut endpoints = Vec::new();
    for (_, geometry, _) in &geometries {
        if let Some(pair) = saved_geometry_endpoints(geometry) {
            ctx.reserve_vec(&mut endpoints, 2, "creo missing-line endpoints")?;
            endpoints.extend(pair);
        }
    }
    let expected_endpoints = geometries.len().checked_mul(2).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Creo missing-line endpoint count overflow")
    })?;
    if endpoints.len() != expected_endpoints {
        return Ok(None);
    }
    let mut open = [None, None];
    let mut open_count = 0;
    for (index, endpoint) in endpoints.iter().enumerate() {
        let mut mate_count = 0;
        for (candidate_index, candidate) in endpoints.iter().enumerate() {
            if candidate_index == index {
                continue;
            }
            ctx.charge_work(1, "creo missing-line endpoint pairs")?;
            if saved_points_coincide(*endpoint, *candidate) {
                mate_count += 1;
            }
        }
        if mate_count > 1 {
            return Ok(None);
        }
        if mate_count == 0 {
            if open_count == 2 {
                return Ok(None);
            }
            open[open_count] = Some(*endpoint);
            open_count += 1;
        }
    }
    let [Some(start), Some(end)] = open else {
        return Ok(None);
    };
    let scale = start
        .iter()
        .chain(end.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if (start[fixed_coordinate.index()] - end[fixed_coordinate.index()]).abs()
        > EPS_PARAMETER_AGREEMENT * scale
    {
        return Ok(None);
    }
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(start[0], start[1]),
        end: Point2::new(end[0], end[1]),
    })
    .ok()
    .map(|geometry| (missing.offset, geometry)))
}

fn saved_points_coincide(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(left, right)| (left - right).abs() <= EPS_PARAMETER_AGREEMENT * scale)
}

pub(in crate::decode) fn saved_profile_chains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    geometries: &[(u32, SketchGeometry)],
) -> Result<Vec<Vec<SketchEntityUse>>, cadmpeg_core::CodecError> {
    let mut profiles = Vec::new();
    for (external_id, geometry) in geometries {
        if !is_full_circle_geometry(geometry) {
            continue;
        }
        let Some(entity) = sketch_entity_id_admitted(ctx, sketch, external_id)? else {
            continue;
        };
        let uses = ctx.collect_vec(
            [SketchEntityUse {
                entity,
                reversed: false,
            }],
            "creo saved circular profile uses",
        )?;
        ctx.reserve_vec(&mut profiles, 1, "creo saved profile rows")?;
        profiles.push(uses);
    }
    let row_count = geometries
        .iter()
        .filter(|(_, geometry)| saved_geometry_endpoints(geometry).is_some())
        .count();
    let mut mates = ctx.alloc_filled(row_count, [None; 2], "creo saved profile endpoint mates")?;
    let mut rows = Vec::new();
    for (external_id, geometry) in geometries {
        if let Some(endpoints) = saved_geometry_endpoints(geometry) {
            ctx.reserve_vec(&mut rows, 1, "creo saved profile endpoint rows")?;
            rows.push((*external_id, endpoints));
        }
    }
    for (row_index, (_, endpoints)) in rows.iter().enumerate() {
        for endpoint_index in 0..2 {
            let mut matches = rows
                .iter()
                .enumerate()
                .flat_map(|(candidate_row, (_, candidate_endpoints))| {
                    (0..2).map(move |candidate_endpoint| {
                        (candidate_row, candidate_endpoint, candidate_endpoints)
                    })
                })
                .filter(|(candidate_row, candidate_endpoint, candidate_endpoints)| {
                    (*candidate_row != row_index || *candidate_endpoint != endpoint_index)
                        && saved_points_coincide(
                            endpoints[endpoint_index],
                            candidate_endpoints[*candidate_endpoint],
                        )
                })
                .map(|(candidate_row, candidate_endpoint, _)| (candidate_row, candidate_endpoint));
            if let (Some(mate), None) = (matches.next(), matches.next()) {
                mates[row_index][endpoint_index] = Some(mate);
            }
        }
    }
    let mut remaining = BTreeSet::new();
    for index in 0..rows.len() {
        ctx.insert_btree_set(&mut remaining, index, "creo saved profile remaining nodes")?;
    }
    while let Some(seed) = remaining
        .iter()
        .min_by_key(|index| rows[**index].0)
        .copied()
    {
        if mates[seed].iter().any(Option::is_none) {
            remaining.remove(&seed);
            continue;
        }
        let mut uses = Vec::new();
        let mut used = BTreeSet::new();
        let mut row = seed;
        let mut reversed = false;
        loop {
            if used.contains(&row) {
                break;
            }
            ctx.insert_btree_set(&mut used, row, "creo saved profile visited nodes")?;
            let Some(entity) = sketch_entity_id_admitted(ctx, sketch, rows[row].0)? else {
                continue;
            };
            ctx.reserve_vec(&mut uses, 1, "creo saved profile uses")?;
            uses.push(SketchEntityUse { entity, reversed });
            let outgoing = usize::from(!reversed);
            let Some((next_row, next_endpoint)) = mates[row][outgoing] else {
                break;
            };
            row = next_row;
            reversed = next_endpoint == 1;
            if row == seed {
                if !reversed {
                    ctx.reserve_vec(&mut profiles, 1, "creo saved profile rows")?;
                    profiles.push(uses);
                }
                break;
            }
        }
        remaining.retain(|index| !used.contains(index));
    }
    Ok(profiles)
}

pub(in crate::decode) fn resolved_section_segment_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    let missing_line = saved_section_missing_line_geometry(ctx, definition)?;
    resolved_section_segment_geometry_with_missing_line(
        ctx,
        definition,
        points,
        segment,
        missing_line.as_ref(),
    )
}

pub(in crate::decode) fn resolved_section_segment_geometry_with_missing_line(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Result<Option<SketchGeometry>, CodecError> {
    let stored = section_segment_geometry(points, segment);
    let saved = match saved_section_line_geometry(ctx, definition, segment)? {
        Some(geometry) => Some(geometry),
        None => saved_section_arc(ctx, definition, segment)?.and_then(SavedSectionArc::into_geometry),
    }
        .or_else(|| {
            missing_line
                .filter(|(offset, _)| *offset == segment.offset)
                .map(|(_, geometry)| geometry.clone())
        });
    Ok(match (stored, saved) {
        (Some(stored), Some(saved)) => {
            let agree = match (stored.definition(), saved.definition()) {
                (
                    SketchGeometryDefinition::Line {
                        start: stored_start,
                        end: stored_end,
                    },
                    SketchGeometryDefinition::Line {
                        start: saved_start,
                        end: saved_end,
                    },
                ) => {
                    saved_points_coincide(
                        [stored_start.u, stored_start.v],
                        [saved_start.u, saved_start.v],
                    ) && saved_points_coincide(
                        [stored_end.u, stored_end.v],
                        [saved_end.u, saved_end.v],
                    )
                }
                (
                    SketchGeometryDefinition::Arc {
                        center: stored_center,
                        radius: stored_radius,
                        ..
                    },
                    SketchGeometryDefinition::Arc {
                        center: saved_center,
                        radius: saved_radius,
                        ..
                    },
                ) => {
                    let radius_scale = stored_radius.get().max(saved_radius.get());
                    saved_points_coincide(
                        [stored_center.u, stored_center.v],
                        [saved_center.u, saved_center.v],
                    ) && (stored_radius.get() - saved_radius.get()).abs()
                        <= EPS_RADIUS_AGREEMENT * radius_scale
                        && saved_geometry_endpoints(&stored)
                            .zip(saved_geometry_endpoints(&saved))
                            .is_some_and(|(stored, saved)| {
                                stored
                                    .into_iter()
                                    .zip(saved)
                                    .all(|(stored, saved)| saved_points_coincide(stored, saved))
                            })
                }
                _ => false,
            };
            agree.then_some(stored)
        }
        (Some(geometry), None) | (None, Some(geometry)) => Some(geometry),
        (None, None) => None,
    })
}

#[cfg(test)]
mod tests;
