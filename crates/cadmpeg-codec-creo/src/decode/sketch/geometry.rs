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
use super::radii::trim_segment_ids;
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

/// The resolved coordinates of section point `id`.
fn section_point(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    id: u32,
) -> Result<Option<[f64; 2]>, CodecError> {
    Ok(ctx
        .get_btree_map(points, &id, "creo section point lookup")?
        .copied())
}

pub(in crate::decode) fn section_line_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let crate::feature::definitions::FeatureSegmentKind::Line([start, end]) = segment.kind else {
        return Ok(None);
    };
    let Some(start) = section_point(ctx, points, start)? else {
        return Ok(None);
    };
    let Some(end) = section_point(ctx, points, end)? else {
        return Ok(None);
    };
    let scale = start
        .iter()
        .chain(&end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if ((end[0] - start[0]) / scale).hypot((end[1] - start[1]) / scale) <= EPS_POINT_NONZERO {
        return Ok(None);
    }
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
    .ok())
}

pub(in crate::decode) fn section_point_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let crate::feature::definitions::FeatureSegmentKind::Point(point) = segment.kind else {
        return Ok(None);
    };
    let Some(position) = section_point(ctx, points, point)? else {
        return Ok(None);
    };
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: cadmpeg_ir::math::Point2::new(position[0], position[1]),
    })
    .ok())
}

pub(in crate::decode) fn section_arc_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Arc(_)
    ) || segment.arc_orientation != Some(0)
    {
        return Ok(None);
    }
    let Some(center_id) = segment.center_id else {
        return Ok(None);
    };
    let Some(center) = section_point(ctx, points, center_id)? else {
        return Ok(None);
    };
    let Some(first) = section_point(ctx, points, segment.point_ids()[0])? else {
        return Ok(None);
    };
    let Some(second) = section_point(ctx, points, segment.point_ids()[1])? else {
        return Ok(None);
    };
    let offset = |point: [f64; 2]| [point[0] - center[0], point[1] - center[1]];
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
        return Ok(None);
    }
    let start = second_offset[1].atan2(second_offset[0]);
    let mut end = first_offset[1].atan2(first_offset[0]);
    // `atan2` lies in [-pi, pi], so at most two turns bring the end past the start.
    while end <= start {
        end += std::f64::consts::TAU;
    }
    let Some(radius) = Length::new(first_radius) else {
        return Ok(None);
    };
    let Some(start_angle) = Angle::new(start) else {
        return Ok(None);
    };
    let Some(end_angle) = Angle::new(end) else {
        return Ok(None);
    };
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: cadmpeg_ir::math::Point2::new(center[0], center[1]),
        radius,
        start_angle,
        end_angle,
    })
    .ok())
}

pub(in crate::decode) fn section_circle_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    radii: &BTreeMap<u32, f64>,
    segment: &crate::feature::definitions::FeatureCircleSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    let Some(center) = section_point(ctx, points, segment.center_id)? else {
        return Ok(None);
    };
    let Some(&radius) =
        ctx.get_btree_map(radii, &segment.radius_ref, "creo section radius lookup")?
    else {
        return Ok(None);
    };
    let Some(radius) = Length::new(radius) else {
        return Ok(None);
    };
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(center[0], center[1]),
        radius,
    })
    .ok())
}

pub(in crate::decode) fn section_point_row_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeaturePointSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    let Some(point) = section_point(ctx, points, segment.point_id)? else {
        return Ok(None);
    };
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(point[0], point[1]),
    })
    .ok())
}

pub(in crate::decode) fn section_centered_line_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureCenteredLineSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    let (Some(start), Some(end), Some(center)) = (
        section_point(ctx, points, 0)?,
        section_point(ctx, points, 1)?,
        section_point(ctx, points, segment.center_id)?,
    ) else {
        return Ok(None);
    };
    let scale = start
        .iter()
        .chain(&end)
        .chain(&center)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if (end[0] - start[0]).hypot(end[1] - start[1]) <= EPS_POINT_NONZERO * scale
        || (start[0] + end[0] - 2.0 * center[0]).hypot(start[1] + end[1] - 2.0 * center[1])
            > EPS_RADIUS_AGREEMENT * scale
    {
        return Ok(None);
    }
    Ok(SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(start[0], start[1]),
        end: Point2::new(end[0], end[1]),
    })
    .ok())
}

pub(in crate::decode) fn section_reference_line_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureReferenceLineSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let [Some(start_id), Some(end_id)] = segment.point_ids else {
        return Ok(None);
    };
    let (Some(start), Some(end)) = (
        section_point(ctx, points, start_id)?,
        section_point(ctx, points, end_id)?,
    ) else {
        return Ok(None);
    };
    let scale = start
        .iter()
        .chain(&end)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let direction = [end[0] - start[0], end[1] - start[1]];
    if direction[0].hypot(direction[1]) <= EPS_POINT_NONZERO * scale {
        return Ok(None);
    }
    Ok(
        SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(start[0], start[1]),
            direction: Point2::new(direction[0], direction[1]),
        })
        .ok(),
    )
}

pub(in crate::decode) fn resolved_section_reference_line_geometry(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    variable_points: &BTreeMap<u32, [Option<f64>; 2]>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureReferenceLineSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(geometry) = section_reference_line_geometry(ctx, points, segment)? {
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
    let fixed_value = |point: u32| -> Result<Option<f64>, CodecError> {
        Ok(ctx
            .get_btree_map(
                variable_points,
                &point,
                "creo section variable point lookup",
            )?
            .and_then(|coordinates| coordinates[fixed_coordinate.index()]))
    };
    let (Some(first), Some(second)) = (fixed_value(start_id)?, fixed_value(end_id)?) else {
        return Ok(None);
    };
    let scale = first.abs().max(second.abs()).max(1.0);
    if (first - second).abs() > EPS_PARAMETER_AGREEMENT * scale {
        return Ok(None);
    }
    let (origin, direction) = if fixed_coordinate == SectionAxis::U {
        (Point2::new(first, 0.0), Point2::new(0.0, 1.0))
    } else {
        (Point2::new(0.0, first), Point2::new(1.0, 0.0))
    };
    Ok(
        SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine { origin, direction })
            .ok(),
    )
}

pub(in crate::decode) fn section_segment_geometry(
    ctx: &DecodeContext<'_>,
    points: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, CodecError> {
    if let Some(geometry) = section_line_geometry(ctx, points, segment)? {
        return Ok(Some(geometry));
    }
    if let Some(geometry) = section_arc_geometry(ctx, points, segment)? {
        return Ok(Some(geometry));
    }
    section_point_geometry(ctx, points, segment)
}

pub(in crate::decode) fn saved_section_line_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo saved section line geometry scratch")?;
    if !matches!(
        segment.kind,
        crate::feature::definitions::FeatureSegmentKind::Line(_)
    ) || !saved_section_ordinary_geometry_allowed(definition, segment)
    {
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
            let rows = segment_table.rows.as_slice();
            if let Some(position) = ctx.position_by(
                rows,
                |row| {
                    Ok(matches!(
                        row,
                        SegmentRow::Ordinary(candidate)
                            if candidate.external_id == segment.external_id
                    ))
                },
                "creo saved line segment position rows",
            )? {
                let previous = if position == 0 {
                    None
                } else {
                    ctx.find_map(
                        rows[..position].iter().rev(),
                        |row| {
                            Ok(match row {
                                SegmentRow::Ordinary(candidate) => {
                                    order_table.internal_id(candidate.external_id)
                                }
                                _ => None,
                            })
                        },
                        "creo saved line previous segment rows",
                    )?
                };
                if let Some(previous) = previous {
                    let next = if position + 1 == rows.len() {
                        None
                    } else {
                        ctx.find_map(
                            &rows[position + 1..],
                            |row| match row {
                                SegmentRow::Ordinary(candidate) => {
                                    Ok(order_table.internal_id(candidate.external_id))
                                }
                                _ => Ok(None),
                            },
                            "creo saved line next segment rows",
                        )?
                    };
                    if let Some(next) = next {
                        if let Some(candidate_id) = previous.checked_add(1) {
                            if candidate_id.checked_add(1) == Some(next) {
                                let mut matching_line = false;
                                let outcome = visit_semantic_saved_section_entities::<()>(
                                    ctx,
                                    definition,
                                    |entity| {
                                        if matches!(entity, crate::feature::definitions::FeatureSavedEntity::Line(line) if line.entity_id == candidate_id)
                                        {
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
        let trimmed = match definition.trim_entities.as_ref() {
            Some(table)
                if table.has_unique_external_ids(ctx)?
                    && table.has_complete_bucket_frame(ctx)? =>
            {
                Some(table)
            }
            _ => None,
        };
        if let (Some(_), Some(segment_table)) = (
            trimmed,
            definition
                .segments
                .as_ref()
                .filter(|table| table.is_complete()),
        ) {
            let mut ordered_external_ids = std::collections::HashSet::new();
            let mut ordered_internal_ids = std::collections::HashSet::new();
            for row in ctx.admit_iter(order_table.rows.as_slice(), "creo saved line order rows")? {
                scratch.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut ordered_external_ids,
                        row.external_id,
                        "creo saved line ordered external IDs",
                    )
                })?;
                scratch.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut ordered_internal_ids,
                        row.internal_id,
                        "creo saved line ordered internal IDs",
                    )
                })?;
            }
            let mut trimmed_ids = BTreeSet::new();
            for id in ctx
                .admit_iter(
                    scratch.with_storage(|| trim_segment_ids(ctx, definition))?,
                    "creo saved line trim rows",
                )?
                .flatten()
            {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(&mut trimmed_ids, id, "creo saved line trim nodes")
                })?;
            }
            // The one trimmed line the order table leaves out, if exactly one.
            let missing = crate::decode::uniqueness::exactly_one_by(
                ctx,
                segment_table.rows.as_slice(),
                |row| {
                    let SegmentRow::Ordinary(candidate) = row else {
                        return Ok(false);
                    };
                    if !matches!(
                        candidate.kind,
                        crate::feature::definitions::FeatureSegmentKind::Line(_)
                    ) {
                        return Ok(false);
                    }
                    let is_trimmed = ctx.contains_btree_set(
                        &trimmed_ids,
                        &candidate.external_id,
                        "creo saved line trim lookup",
                    )?;
                    Ok(is_trimmed && !ordered_external_ids.contains(&candidate.external_id))
                },
                "creo saved line candidate segment rows",
            )?;
            if let Some(SegmentRow::Ordinary(missing)) = missing {
                let external_id = missing.external_id;
                let mut candidate_internal_id = None;
                let outcome =
                    visit_semantic_saved_section_entities::<()>(ctx, definition, |entity| {
                        let crate::feature::definitions::FeatureSavedEntity::Line(line) = entity
                        else {
                            return Ok(ControlFlow::Continue(()));
                        };
                        let already_ordered = ordered_internal_ids.contains(&line.entity_id);
                        if !already_ordered {
                            if candidate_internal_id.is_some() {
                                return Ok(ControlFlow::Break(()));
                            }
                            candidate_internal_id = Some(line.entity_id);
                        }
                        Ok(ControlFlow::Continue(()))
                    })?;
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
    let Some(order) = definition.order_table.as_ref() else {
        return Ok(None);
    };
    let Some(internal_id) = order.internal_id(segment.external_id) else {
        return Ok(None);
    };
    if !saved_section_internal_id_is_unique(ctx, definition, internal_id)? {
        return Ok(None);
    }
    let outcome = visit_semantic_saved_section_entities(ctx, definition, |entity| match entity {
        crate::feature::definitions::FeatureSavedEntity::Arc(arc)
            if arc.entity_id == internal_id =>
        {
            Ok(ControlFlow::Break(arc))
        }
        _ => Ok(ControlFlow::Continue(())),
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

    pub(super) fn from_saved_record(
        arc: &crate::feature::definitions::FeatureSavedArc,
    ) -> Option<Self> {
        let [center_u, center_v, _] = arc.center;
        if let ([Some(center_u), Some(center_v)], Some(radius)) = (
            [center_u, center_v],
            arc.radius.filter(|radius| *radius > EPS_POINT_NONZERO),
        ) {
            return Self::new([center_u, center_v], radius);
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
        Self::new([center_u, center_v], radius)
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
    Ok(SectionArcCarrier::from_saved_record(arc))
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
    fn from_saved_record(arc: &crate::feature::definitions::FeatureSavedArc) -> Option<Self> {
        let carrier = SectionArcCarrier::from_saved_record(arc)?;
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
        // `atan2` lies in [-pi, pi], so at most two turns bring the end past the start.
        while end <= start {
            end += std::f64::consts::TAU;
        }
        let start_angle = Angle::new(start)?;
        let end_angle = Angle::new(end)?;
        Some(Self {
            center: carrier.center,
            radius: carrier.radius,
            start_angle,
            end_angle,
        })
    }

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
    Ok(SavedSectionArc::from_saved_record(arc))
}

pub(in crate::decode) type SavedSegmentPointCoordinates = [Option<(u32, [f64; 2])>; 3];

pub(in crate::decode) fn saved_section_segment_point_coordinates(
    ctx: &DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    segment: &crate::feature::definitions::FeatureSegment,
) -> Result<Option<SavedSegmentPointCoordinates>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
            let Some(arc) = saved_section_arc_record(ctx, definition, segment)? else {
                return Ok(None);
            };
            let Some(geometry) = SavedSectionArc::from_saved_record(arc) else {
                return Ok(None);
            };
            let center = *geometry.center.as_raw();
            let [[Some(first_u), Some(first_v), _], [Some(second_u), Some(second_v), _]] =
                arc.endpoints
            else {
                return Ok(None);
            };
            Some([
                Some((segment.point_ids()[0], [first_u, first_v])),
                Some((segment.point_ids()[1], [second_u, second_v])),
                Some((
                    match segment.center_id {
                        Some(center_id) => center_id,
                        None => return Ok(None),
                    },
                    [center.u, center.v],
                )),
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(segments) = definition.segments.as_ref() else {
        return Ok(None);
    };
    let Some(_) = segments.rows.get(segment.external_id) else {
        return Ok(None);
    };
    let Some(entity) = section_saved_entity(ctx, definition, segment.external_id)? else {
        return Ok(None);
    };
    let Some((_, geometry, _)) = saved_section_entity_geometry(entity) else {
        return Ok(None);
    };
    let SketchGeometryDefinition::Circle { center, radius } = geometry.definition() else {
        return Ok(None);
    };
    Ok(Some(([center.u, center.v], radius.get())))
}

pub(in crate::decode) fn saved_section_entity_geometry(
    entity: &crate::feature::definitions::FeatureSavedEntity,
) -> Option<(u32, SketchGeometry, usize)> {
    let conic_facts = if let crate::feature::definitions::FeatureSavedEntity::Conic(conic) = entity
    {
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
        // Two fixed three-slot endpoints.
        let endpoints_complete = conic
            .endpoints
            .iter()
            .all(|endpoint| endpoint.iter().all(Option::is_some));
        let coincident_endpoints = endpoints_complete
            && conic.endpoints[0]
                .iter()
                .zip(&conic.endpoints[1])
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
    let arc_angles = if let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = entity {
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
        // `atan2` lies in [-pi, pi], so at most two turns bring the end past the start.
        while end_angle <= start_angle {
            end_angle += std::f64::consts::TAU;
        }
        Some((center_u, center_v, radius, start_angle, end_angle))
    } else {
        None
    };

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
            let (center_u, center_v, radius, start_angle, end_angle) = arc_angles?;
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
            let (
                frame,
                major_axis,
                major_radius,
                minor_radius,
                parameter_shift,
                coincident_endpoints,
            ) = conic_facts?;
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
    let mut scratch = ctx.reserve_scoped(0, "creo saved section missing line geometry scratch")?;
    let Some(order) = definition.order_table.as_ref() else {
        return Ok(None);
    };
    if !order.is_complete() || order.rows.is_empty() {
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
    if !trim.has_complete_bucket_frame(ctx)? || !trim.has_unique_external_ids(ctx)? {
        return Ok(None);
    }
    let mut trimmed_external_ids = BTreeSet::new();
    for id in ctx
        .admit_iter(
            scratch.with_storage(|| trim_segment_ids(ctx, definition))?,
            "creo missing-line trim rows",
        )?
        .flatten()
    {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut trimmed_external_ids,
                id,
                "creo missing-line trimmed ID nodes",
            )
        })?;
    }
    let Some(SegmentRow::Ordinary(missing)) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        segments.rows.as_slice(),
        |row| {
            let SegmentRow::Ordinary(candidate) = row else {
                return Ok(false);
            };
            Ok(matches!(
                candidate.kind,
                crate::feature::definitions::FeatureSegmentKind::Line(_)
            ) && order.internal_id(candidate.external_id).is_none()
                && ctx.contains_btree_set(
                    &trimmed_external_ids,
                    &candidate.external_id,
                    "creo missing-line trimmed ID lookup",
                )?)
        },
        "creo missing-line segment rows",
    )?
    else {
        return Ok(None);
    };
    let Some(fixed_coordinate) = missing
        .vertical_horizontal
        .and_then(SectionAxis::from_selector)
    else {
        return Ok(None);
    };

    let mut ordered_ids = BTreeSet::new();
    for row in ctx.admit_iter(order.rows.as_slice(), "creo missing-line ordered rows")? {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut ordered_ids,
                row.internal_id,
                "creo missing-line ordered ID nodes",
            )
        })?;
    }
    let mut geometries = Vec::new();
    let ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
        std::convert::Infallible,
    >(ctx, definition, |entity| {
        let Some(geometry) = saved_section_entity_geometry(entity) else {
            return Ok(ControlFlow::Continue(()));
        };
        if ctx.contains_btree_set(&ordered_ids, &geometry.0, "creo missing-line order lookup")? {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut geometries, 1, "creo missing-line saved geometries")
            })?;
            geometries.push(geometry);
        }
        Ok(ControlFlow::Continue(()))
    })?;
    let mut geometry_ids = BTreeSet::new();
    for (internal_id, _, _) in ctx.admit_iter(&geometries, "creo missing-line geometry IDs")? {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut geometry_ids,
                *internal_id,
                "creo missing-line geometry ID nodes",
            )
        })?;
    }
    if ordered_ids.len() != order.rows.len()
        || geometry_ids.len() != geometries.len()
        || geometry_ids.len() != ordered_ids.len()
        || !ctx.all_by(
            geometry_ids.iter().zip(&ordered_ids),
            |(geometry, ordered)| Ok(geometry == ordered),
            "creo missing-line identity agreement",
        )?
    {
        return Ok(None);
    }
    let mut endpoints = Vec::new();
    for (_, geometry, _) in ctx.admit_iter(&geometries, "creo missing-line geometries")? {
        if let Some([start, end]) = saved_geometry_endpoints(geometry) {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut endpoints, 2, "creo missing-line endpoints")
            })?;
            endpoints.push(start);
            endpoints.push(end);
        }
    }
    let expected_endpoints = geometries.len().checked_mul(2).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Creo missing-line endpoint count overflow")
    })?;
    if endpoints.len() != expected_endpoints {
        return Ok(None);
    }
    let mut open = [None, None];
    let mut endpoint_rows = endpoints.iter().enumerate();
    while endpoint_rows.len() != 0 {
        let Some((index, endpoint)) =
            ctx.next_charged(&mut endpoint_rows, "creo missing-line endpoints")?
        else {
            break;
        };
        let mut first_mate = None;
        let has_multiple_mates = ctx.any_by(
            endpoints.iter().enumerate(),
            |(candidate_index, candidate)| {
                Ok(candidate_index != index
                    && saved_points_coincide(*endpoint, *candidate)
                    && first_mate.replace(candidate_index).is_some())
            },
            "creo missing-line endpoint candidates",
        )?;
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
    let mut scratch = ctx.reserve_scoped(0, "creo saved profile chains scratch")?;
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
    let mut rows = Vec::new();
    let mut mates = Vec::new();
    for (external_id, geometry) in ctx.admit_iter(geometries, "creo saved profile endpoint rows")? {
        if let Some(endpoints) = saved_geometry_endpoints(geometry) {
            scratch.with_storage(|| {
                ctx.push_vec(&mut mates, [None; 2], "creo saved profile endpoint mates")
            })?;
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut rows, 1, "creo saved profile endpoint rows")
            })?;
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
            // Each candidate row is charged as it is visited; a second mate ends the walk.
            let mut candidates = rows.iter().enumerate();
            'candidate_rows: while candidates.len() != 0 {
                let Some((candidate_row, (_, candidate_endpoints))) =
                    ctx.next_charged(&mut candidates, "creo saved profile endpoint candidates")?
                else {
                    break;
                };
                for (candidate_endpoint, candidate_point) in candidate_endpoints.iter().enumerate()
                {
                    if (candidate_row != row_index || candidate_endpoint != endpoint_index)
                        && saved_points_coincide(endpoints[endpoint_index], *candidate_point)
                        && mate.replace((candidate_row, candidate_endpoint)).is_some()
                    {
                        has_second_mate = true;
                        break 'candidate_rows;
                    }
                }
            }
            if let (Some(mate), false) = (mate, has_second_mate) {
                mates[row_index][endpoint_index] = Some(mate);
            }
        }
    }
    // Seeds in external identifier order, then row order: each component
    // starts from the smallest identifier not taken by an earlier component.
    let mut seeds = Vec::new();
    scratch.with_storage(|| ctx.reserve_vec(&mut seeds, rows.len(), "creo saved profile seeds"))?;
    for (index, _) in ctx
        .admit_iter(&rows, "creo saved profile seeds")?
        .enumerate()
    {
        seeds.push(index);
    }
    ctx.stable_sort_by_key(
        seeds.as_mut_slice(),
        |index| rows[*index].0,
        Ord::cmp,
        "creo saved profile seed ordering",
    )?;
    let mut taken = BTreeSet::new();
    for &seed in ctx.admit_iter(&seeds, "creo saved profile components")? {
        if ctx.contains_btree_set(&taken, &seed, "creo saved profile taken lookup")? {
            continue;
        }
        if mates[seed].iter().any(Option::is_none) {
            continue;
        }
        let mut chain_storage = ctx.reserve_scoped(0, "creo saved profile chain scratch")?;
        let mut uses = Vec::new();
        let mut visited_storage = ctx.reserve_scoped(0, "creo saved profile visited scratch")?;
        let mut used = BTreeSet::new();
        let mut row = seed;
        let mut reversed = false;
        loop {
            ctx.charge_work(1, "creo saved profile chain steps")?;
            if ctx.contains_btree_set(&used, &row, "creo saved profile visited lookup")? {
                break;
            }
            visited_storage.with_storage(|| {
                ctx.insert_btree_set(&mut used, row, "creo saved profile visited nodes")
            })?;
            let Some(entity) = chain_storage
                .with_storage(|| sketch_entity_id_admitted(ctx, sketch, rows[row].0))?
            else {
                continue;
            };
            chain_storage
                .with_storage(|| ctx.reserve_vec(&mut uses, 1, "creo saved profile uses"))?;
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
                    let uses = chain_storage.commit_value(uses)?;
                    profiles.push(uses);
                }
                break;
            }
        }
        for &row in ctx.admit_iter(&used, "creo saved profile visited rows")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut taken, row, "creo saved profile taken nodes")
            })?;
        }
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
    let stored = section_segment_geometry(ctx, points, segment)?;
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
