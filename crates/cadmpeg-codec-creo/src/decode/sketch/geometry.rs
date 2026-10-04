// SPDX-License-Identifier: Apache-2.0
//! Section geometry conversion from live and saved section entities.

use super::axis::SectionAxis;
use crate::feature::segment_rows::SegmentRow;

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
    visit_semantic_saved_section_entities,
};
use crate::decode::sketch_transfer::loci::section_saved_entity;
use std::ops::ControlFlow;

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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    ) || !saved_section_ordinary_geometry_allowed(definition, segment) {
        return Ok(None);
    }
    let Some(order_table) = definition.order_table.as_ref() else {
        return Ok(None);
    };
    let mut internal_id = order_table.internal_id(segment.external_id);
    if internal_id.is_none() {
        if let Some(segment_table) = definition
            .segments
            .as_ref()
            .filter(|table| table.is_complete())
        {
            if let Some(position) = ctx
                .admit_iter(
                    segment_table.rows.as_slice(),
                    "creo saved line segment position rows",
                )?
                .filter_map(|row| match row {
                    SegmentRow::Ordinary(segment) => Some(segment),
                    _ => None,
                })
                .position(|candidate| candidate.external_id == segment.external_id)
            {
                let previous = ctx
                    .admit_iter(
                        segment_table.rows.as_slice(),
                        "creo saved line previous segment rows",
                    )?
                    .filter_map(|row| match row {
                        SegmentRow::Ordinary(segment) => Some(segment),
                        _ => None,
                    })
                    .take(position)
                    .filter_map(|candidate| order_table.internal_id(candidate.external_id))
                    .last();
                if let Some(previous) = previous {
                    let next = ctx
                        .admit_iter(
                            segment_table.rows.as_slice(),
                            "creo saved line next segment rows",
                        )?
                        .filter_map(|row| match row {
                            SegmentRow::Ordinary(segment) => Some(segment),
                            _ => None,
                        })
                        .skip(position + 1)
                        .find_map(|candidate| order_table.internal_id(candidate.external_id));
                    if let Some(next) = next {
                        if let Some(candidate_id) = previous.checked_add(1) {
                            if candidate_id.checked_add(1) == Some(next) {
                                let mut matching_line = false;
                                let outcome = visit_semantic_saved_section_entities::<()>(
                                    ctx,
                                    definition,
                                    |entity| {
                                        if matches!(entity, crate::feature::definitions::FeatureSavedEntity::Line(line) if line.entity_id == candidate_id) {
                                            matching_line = true;
                                            return Ok(ControlFlow::Break(()));
                                        }
                                        Ok(ControlFlow::Continue(()))
                                    },
                                )?;
                                if matching_line && matches!(outcome, ControlFlow::Break(())) {
                                    internal_id = Some(candidate_id);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if internal_id.is_none() && order_table.is_complete() {
        if let (Some(trimmed), Some(segment_table)) = (
            definition
                .trim_entities
                .as_ref()
                .filter(|table| table.has_complete_bucket_frame() && table.has_unique_external_ids()),
            definition.segments.as_ref().filter(|table| table.is_complete()),
        ) {
            let mut matching_external_id = None;
            let mut multiple_matching_external_ids = false;
            for candidate in ctx
                .admit_iter(
                    segment_table.rows.as_slice(),
                    "creo saved line candidate segment rows",
                )?
                .filter_map(|row| match row {
                    SegmentRow::Ordinary(segment) => Some(segment),
                    _ => None,
                })
            {
                if !matches!(
                    candidate.kind,
                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                ) {
                    continue;
                }
                let mut is_trimmed = false;
                for row in ctx.admit_iter(&trimmed.rows, "creo saved line trim rows")? {
                    if trim_segment_id(ctx, definition, row)? == Some(candidate.external_id) {
                        is_trimmed = true;
                        break;
                    }
                }
                if !is_trimmed {
                    continue;
                }
                let already_ordered = ctx
                    .admit_iter(&order_table.rows, "creo saved line order rows")?
                    .any(|row| row.external_id == candidate.external_id);
                if !already_ordered {
                    if matching_external_id.is_some() {
                        multiple_matching_external_ids = true;
                        break;
                    }
                    matching_external_id = Some(candidate.external_id);
                }
            }
            let matching_external_id = if multiple_matching_external_ids {
                None
            } else {
                matching_external_id
            };
            if let Some(external_id) = matching_external_id {
                let mut candidate_internal_id = None;
                let outcome = visit_semantic_saved_section_entities::<()>(
                    ctx,
                    definition,
                    |entity| {
                        let crate::feature::definitions::FeatureSavedEntity::Line(line) = entity
                        else {
                            return Ok(ControlFlow::Continue(()));
                        };
                        let already_ordered = ctx
                            .admit_iter(&order_table.rows, "creo saved line order rows")?
                            .any(|row| row.internal_id == line.entity_id);
                        if !already_ordered {
                            if candidate_internal_id.is_some() {
                                return Ok(ControlFlow::Break(()));
                            }
                            candidate_internal_id = Some(line.entity_id);
                        }
                        Ok(ControlFlow::Continue(()))
                    },
                )?;
                if external_id == segment.external_id
                    && matches!(outcome, ControlFlow::Continue(()))
                {
                    internal_id = candidate_internal_id;
                }
            }
        }
    }
    let Some(internal_id) = internal_id else {
        return Ok(None);
    };
    if !saved_section_internal_id_is_unique(ctx, definition, internal_id)? {
        return Ok(None);
    }
    let mut matching_line = None;
    let outcome = visit_semantic_saved_section_entities::<()>(ctx, definition, |entity| {
        if let crate::feature::definitions::FeatureSavedEntity::Line(line) = entity {
            if line.entity_id == internal_id {
                matching_line = Some(line);
                return Ok(ControlFlow::Break(()));
            }
        }
        Ok(ControlFlow::Continue(()))
    })?;
    let ControlFlow::Break(()) = outcome else {
        return Ok(None);
    };
    let Some(line) = matching_line else {
        return Ok(None);
    };
    let [[Some(start_u), Some(start_v), _], [Some(end_u), Some(end_v), _]] = line.endpoints else {
        return Ok(None);
    };
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: cadmpeg_ir::math::Point2::new(start_u, start_v),
        end: cadmpeg_ir::math::Point2::new(end_u, end_v),
    })
    .ok())
}

pub(super) fn saved_section_arc_record<'a>(
    ctx: &DecodeContext<'_>,
    definition: &'a crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<&'a crate::feature::definitions::FeatureSavedArc>, CodecError> {
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Arc(_)
    ) || segment.arc_orientation != Some(0)
    {
        return Ok(None);
    }
    if !saved_section_ordinary_geometry_allowed(definition, segment) {
        return Ok(None);
    }
    let Some(internal_id) = definition
        .order_table
        .as_ref()
        .and_then(|order| order.internal_id(segment.external_id))
    else {
        return Ok(None);
    };
    if !saved_section_internal_id_is_unique(ctx, definition, internal_id)? {
        return Ok(None);
    }
    let outcome = visit_semantic_saved_section_entities(ctx, definition, |entity| {
        match entity {
            crate::feature::definitions::FeatureSavedEntity::Arc(arc)
                if arc.entity_id == internal_id =>
            {
                Ok(ControlFlow::Break(arc))
            }
            _ => Ok(ControlFlow::Continue(())),
        }
    })?;
    Ok(match outcome {
        ControlFlow::Break(arc) => Some(arc),
        ControlFlow::Continue(()) => None,
    })
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
    let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else {
        return Ok(None);
    };
    let [center_u, center_v, _] = arc.center;
    if let ([Some(center_u), Some(center_v)], Some(radius)) = (
        [center_u, center_v],
        arc.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
    ) {
        return Ok(SectionArcCarrier::new([center_u, center_v], radius));
    }
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return Ok(None);
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
                return Ok(None);
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
                return Ok(None);
            }
            let u = ((second_v - v).mul_add(
                second_v - v,
                second_u * second_u - (first_v - v) * (first_v - v) - first_u * first_u,
            )) / denominator;
            [u, v]
        }
        [None, None] => return Ok(None),
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
        return Ok(None);
    }
    let radius = arc.radius.unwrap_or(first_radius);
    Ok(SectionArcCarrier::new([center_u, center_v], radius))
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
    let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else {
        return Ok(None);
    };
    let Some(carrier) = saved_section_arc_carrier(ctx, definition, segment)? else {
        return Ok(None);
    };
    let ([center_u, center_v], radius) = carrier.raw();
    let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] = arc.endpoints
    else {
        return Ok(None);
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
        return Ok(None);
    }
    let start = second[1].atan2(second[0]);
    let mut end = first[1].atan2(first[0]);
    while end <= start {
        end += std::f64::consts::TAU;
    }
    let Some(start_angle) = Angle::new(start) else {
        return Ok(None);
    };
    let Some(end_angle) = Angle::new(end) else {
        return Ok(None);
    };
    Ok(Some(SavedSectionArc {
        center: carrier.center,
        radius: carrier.radius,
        start_angle,
        end_angle,
    }))
}

pub(in crate::decode) fn saved_section_segment_point_coordinates(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<[Option<(u32, [f64; 2])>; 3]>, CodecError> {
    let coordinates = match segment.kind {
        crate::feature::definitions::FeatureSegmentKind::Line(_) => {
            let Some(geometry) = saved_section_line_geometry(ctx, definition, segment)? else {
                return Ok(None);
            };
            let Some([start, end]) = saved_geometry_endpoints(&geometry) else {
                return Ok(None);
            };
            Some([
                Some((segment.point_ids()[0], start)),
                Some((segment.point_ids()[1], end)),
                None,
            ])
        }
        crate::feature::definitions::FeatureSegmentKind::Arc(_) => {
            let Some(arc) = saved_section_arc(ctx, definition, segment)? else {
                return Ok(None);
            };
            let center = *arc.center.as_raw();
            let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else {
                return Ok(None);
            };
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return Ok(None);
            };
            Some([
                Some((segment.point_ids()[0], [first_u, first_v])),
                Some((segment.point_ids()[1], [second_u, second_v])),
                Some((match segment.center_id {
                    Some(center_id) => center_id,
                    None => return Ok(None),
                }, [center.u, center.v])),
            ])
        }
        crate::feature::definitions::FeatureSegmentKind::Point(_) => None,
    };
    Ok(coordinates)
}

pub(in crate::decode) fn saved_section_circle_values(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureCircleSegment,
) -> Result<Option<([f64; 2], f64)>, cadmpeg_core::CodecError> {
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    let Some(_) = segments.rows.get(segment.external_id) else {
        return Ok(None);
    };
    let Some(entity) = section_saved_entity(ctx, definition, segment.external_id)? else {
        return Ok(None);
    };
    let Some((_, geometry, _)) = saved_section_entity_geometry(ctx, entity)? else {
        return Ok(None);
    };
    let SketchGeometryDefinition::Circle { center, radius } = geometry.definition() else {
        return Ok(None);
    };
    Ok(Some(([center.u, center.v], radius.get())))
}

pub(in crate::decode) fn saved_section_entity_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &crate::feature::definitions::FeatureSavedEntity,
) -> Result<Option<(u32, SketchGeometry, usize)>, cadmpeg_core::CodecError> {
    let conic_facts =
        if let crate::feature::definitions::FeatureSavedEntity::Conic(conic) = entity {
            let (Some(frame), [Some(first_radius), Some(second_radius)]) =
                (conic.local_system, conic.coefficients)
            else {
                return Ok(None);
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
                return Ok(None);
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
            let mut endpoints_complete = true;
            for endpoint in ctx.admit_iter(&conic.endpoints, "creo saved conic endpoint rows")? {
                if !ctx
                    .admit_iter(endpoint, "creo saved conic endpoint coordinates")?
                    .all(Option::is_some)
                {
                    endpoints_complete = false;
                    break;
                }
            }
            let coincident_endpoints = endpoints_complete
                && ctx
                    .admit_iter(
                        &conic.endpoints[0],
                        "creo first saved conic endpoint coordinates",
                    )?
                    .zip(ctx.admit_iter(
                        &conic.endpoints[1],
                        "creo second saved conic endpoint coordinates",
                    )?)
                    .all(|(first, second)| {
                        let (Some(first), Some(second)) = (*first, *second) else {
                            return false;
                        };
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale
                    });
            Some((
                frame,
                major_axis,
                major_radius,
                minor_radius,
                parameter_shift,
                coincident_endpoints,
            ))
        } else {
            None
        };
    let result = (|| match entity {
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
            let Some((
                frame,
                major_axis,
                major_radius,
                minor_radius,
                parameter_shift,
                coincident_endpoints,
            )) = conic_facts
            else {
                return None;
            };
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
    })();
    Ok(result)
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
    for row in ctx.admit_iter(&trim.rows, "creo missing-line trim rows")? {
        if let Some(id) = trim_segment_id(ctx, definition, row)? {
            ctx.insert_btree_set(
                &mut trimmed_external_ids,
                id,
                "creo missing-line trimmed ID nodes",
            )?;
        }
    }
    let Some(missing) = crate::decode::uniqueness::exactly_one(
        ctx.admit_iter(segments.rows.as_slice(), "creo missing-line segment rows")?
            .filter_map(|row| match row {
                SegmentRow::Ordinary(candidate) => Some(candidate),
                _ => None,
            })
            .filter(|candidate| {
                matches!(
                    candidate.kind,
                    crate::feature::definitions::FeatureSegmentKind::Line(_)
                ) && order.internal_id(candidate.external_id).is_none()
                    && trimmed_external_ids.contains(&candidate.external_id)
            }),
    )
    else {
        return Ok(None);
    };
    let Some(fixed_coordinate) = missing
        .vertical_horizontal
        .and_then(SectionAxis::from_selector)
    else {
        return Ok(None);
    };

    let mut geometries = Vec::new();
    // discarded-value: The visitor continues through every semantic saved entity.
    let _ = visit_semantic_saved_section_entities::<()>(ctx, definition, |entity| {
        let Some(geometry) = saved_section_entity_geometry(ctx, entity)? else {
            return Ok(ControlFlow::Continue(()));
        };
        if ctx
            .admit_iter(&order.rows, "creo missing-line order lookup")?
            .any(|row| row.internal_id == geometry.0)
        {
            ctx.reserve_vec(&mut geometries, 1, "creo missing-line saved geometries")?;
            geometries.push(geometry);
        }
        Ok(ControlFlow::Continue(()))
    })?;
    let mut ordered_ids = BTreeSet::new();
    for row in ctx.admit_iter(&order.rows, "creo missing-line ordered rows")? {
        ctx.insert_btree_set(
            &mut ordered_ids,
            row.internal_id,
            "creo missing-line ordered ID nodes",
        )?;
    }
    let mut geometry_ids = BTreeSet::new();
    for (internal_id, _, _) in ctx.admit_iter(&geometries, "creo missing-line geometry IDs")? {
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
    for (_, geometry, _) in ctx.admit_iter(&geometries, "creo missing-line geometries")? {
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
    for (index, endpoint) in ctx
        .admit_iter(&endpoints, "creo missing-line endpoints")?
        .enumerate()
    {
        let mut first_mate = None;
        let mut has_multiple_mates = false;
        for (candidate_index, candidate) in ctx
            .admit_iter(&endpoints, "creo missing-line endpoint candidates")?
            .enumerate()
        {
            if candidate_index == index {
                continue;
            }
            ctx.charge_work(1, "creo missing-line endpoint pairs")?;
            if saved_points_coincide(*endpoint, *candidate) {
                if first_mate.replace(candidate_index).is_some() {
                    has_multiple_mates = true;
                }
            }
        }
        if has_multiple_mates {
            return Ok(None);
        }
        if first_mate.is_none() {
            if open[0].is_none() {
                open[0] = Some(*endpoint);
            } else if open[1].is_none() {
                open[1] = Some(*endpoint);
            } else {
                return Ok(None);
            }
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
    for (external_id, geometry) in ctx.admit_iter(geometries, "creo saved profile geometries")? {
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
    let row_count = ctx
        .admit_iter(geometries, "creo saved profile endpoint count")?
        .filter(|(_, geometry)| saved_geometry_endpoints(geometry).is_some())
        .count();
    let mut mates = ctx.alloc_filled(row_count, [None; 2], "creo saved profile endpoint mates")?;
    let mut rows = Vec::new();
    for (external_id, geometry) in ctx.admit_iter(geometries, "creo saved profile endpoint rows")? {
        if let Some(endpoints) = saved_geometry_endpoints(geometry) {
            ctx.reserve_vec(&mut rows, 1, "creo saved profile endpoint rows")?;
            rows.push((*external_id, endpoints));
        }
    }
    for (row_index, (_, endpoints)) in ctx
        .admit_iter(&rows, "creo saved profile endpoint pairs")?
        .enumerate()
    {
        for endpoint_index in 0..2 {
            let mut mate = None;
            let mut has_second_mate = false;
            'candidate_rows: for (candidate_row, (_, candidate_endpoints)) in ctx
                .admit_iter(&rows, "creo saved profile endpoint candidates")?
                .enumerate()
            {
                for candidate_endpoint in 0..2 {
                    if (candidate_row != row_index || candidate_endpoint != endpoint_index)
                        && saved_points_coincide(
                            endpoints[endpoint_index],
                            candidate_endpoints[candidate_endpoint],
                        )
                    {
                        if mate
                            .replace((candidate_row, candidate_endpoint))
                            .is_some()
                        {
                            has_second_mate = true;
                            break 'candidate_rows;
                        }
                    }
                }
            }
            if let (Some(mate), false) = (mate, has_second_mate) {
                mates[row_index][endpoint_index] = Some(mate);
            }
        }
    }
    let mut remaining = BTreeSet::new();
    for (index, _) in ctx.admit_iter(&rows, "creo saved profile remaining nodes")?.enumerate() {
        ctx.insert_btree_set(&mut remaining, index, "creo saved profile remaining nodes")?;
    }
    while !remaining.is_empty() {
        ctx.charge_work(1, "creo saved profile components")?;
        let mut candidates = ctx.admit_iter(
            &remaining,
            "creo saved profile seed candidates",
        )?;
        let Some(first_candidate) = candidates.next() else {
            break;
        };
        let mut seed = *first_candidate;
        for candidate in candidates {
            if ctx.compare(
                &rows[*candidate].0,
                &rows[seed].0,
                "creo saved profile seed comparisons",
            )? == std::cmp::Ordering::Less
            {
                seed = *candidate;
            }
        }
        if mates[seed].iter().any(Option::is_none) {
            remaining.remove(&seed);
            continue;
        }
        let mut uses = Vec::new();
        let mut used = BTreeSet::new();
        let mut row = seed;
        let mut reversed = false;
        loop {
            ctx.charge_work(1, "creo saved profile chain steps")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    let stored = section_segment_geometry(points, segment);
    let saved_line = saved_section_line_geometry(ctx, definition, segment)?;
    let saved = if saved_line.is_some() {
        saved_line
    } else {
        saved_section_arc(ctx, definition, segment)?.and_then(SavedSectionArc::into_geometry)
    };
    let saved = match saved {
        Some(saved) => Some(saved),
        None => missing_line
            .filter(|(offset, _)| *offset == segment.offset)
            .map(|(_, geometry)| {
                geometry.try_clone_for_decode(ctx, "creo missing line fallback geometry")
            })
            .transpose()?,
    };
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
