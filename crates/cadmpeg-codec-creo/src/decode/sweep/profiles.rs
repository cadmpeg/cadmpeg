// SPDX-License-Identifier: Apache-2.0
//! Sketch profile connectivity, intersection, and containment.

use super::super::holes::placement::ExtrusionSpan;
use super::nurbs::{oriented_sketch_nurbs_curve, sketch_nurbs_curve, sketch_nurbs_pcurve};
use crate::decode::analytic::edges::nurbs_intrinsic_parameter_range;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, pcurve::PcurveGeometry, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};

macro_rules! require_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

const EPS_ENDPOINT_AGREEMENT: f64 = 1.0e-9;
const EPS_PARAMETER_SCALE: f64 = 1.0e-12;
const EPS_FULL_TURN: f64 = 1.0e-12;
const EPS_AREA: f64 = 1.0e-12;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;

fn sketch_geometry_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SketchGeometry,
) -> Result<Option<[[f64; 2]; 2]>, cadmpeg_core::CodecError> {
    let endpoints = match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => Some([[start.u, start.v], [end.u, end.v]]),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            [
                center.u + radius.get() * start_angle.get().cos(),
                center.v + radius.get() * start_angle.get().sin(),
            ],
            [
                center.u + radius.get() * end_angle.get().cos(),
                center.v + radius.get() * end_angle.get().sin(),
            ],
        ]),
        SketchGeometryDefinition::Circle { center, radius } => {
            let seam = [center.u + radius.get(), center.v];
            Some([seam, seam])
        }
        SketchGeometryDefinition::Nurbs { .. } => {
            let Some(nurbs) = sketch_nurbs_curve(geometry) else {
                return Ok(None);
            };
            let Some(range) = nurbs_intrinsic_parameter_range(&nurbs) else {
                return Ok(None);
            };
            let [lower, upper] = cadmpeg_ir::scalar::FiniteReal::raw_array(range);
            let carrier = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
            let (Some(first), Some(last)) = (
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, lower),
                )?)?,
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, upper),
                )?)?,
            ) else {
                return Ok(None);
            };
            Some([[first.x, first.y], [last.x, last.y]])
        }
        _ => None,
    };
    Ok(endpoints.filter(|points| points.iter().flatten().all(|value| value.is_finite())))
}

fn unique_profile_sketch<'a>(
    ir: &'a CadIr,
    sketch_id: &SketchId,
) -> Option<&'a cadmpeg_ir::sketches::Sketch> {
    let mut found = None;
    for sketch in &ir.model.sketches {
        if sketch.id == *sketch_id {
            if found.is_some() {
                return None;
            }
            found = Some(sketch);
        }
    }
    found
}

fn unique_profile_entity<'a>(
    ir: &'a CadIr,
    sketch_id: &SketchId,
    entity_id: &cadmpeg_ir::sketches::SketchEntityId,
) -> Option<&'a cadmpeg_ir::sketches::SketchEntity> {
    let mut found = None;
    for entity in &ir.model.sketch_entities {
        if entity.sketch == *sketch_id && entity.id() == entity_id {
            if found.is_some() {
                return None;
            }
            found = Some(entity);
        }
    }
    found
}

pub(in super::super) fn connected_sketch_profile_vertices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    sketch_id: &SketchId,
) -> Result<
    impl ExactSizeIterator<Item = (usize, Vec<[f64; 2]>)> + std::fmt::Debug,
    cadmpeg_core::CodecError,
> {
    let Some(sketch) = unique_profile_sketch(ir, sketch_id) else {
        return Ok(Vec::new().into_iter());
    };
    let mut profiles = Vec::new();
    for (profile_index, profile) in sketch.profiles.iter().enumerate() {
        if profile.is_empty() {
            continue;
        }
        let mut uses = Vec::new();
        let mut valid = true;
        for entity_use in profile {
            let Some(geometry) = unique_profile_entity(ir, sketch_id, &entity_use.entity)
                .map(|entity| source_carriers.sketch_geometry(entity))
            else {
                valid = false;
                break;
            };
            let Some([mut start, mut end]) = sketch_geometry_endpoints(ctx, geometry)? else {
                valid = false;
                break;
            };
            if entity_use.reversed {
                std::mem::swap(&mut start, &mut end);
            }
            uses.push((start, end));
        }
        if !valid {
            continue;
        }
        let scale = uses
            .iter()
            .flat_map(|(start, end)| start.iter().chain(end))
            .map(|coordinate| coordinate.abs())
            .fold(1.0, f64::max);
        if !uses.windows(2).all(|adjacent| {
            let end = adjacent[0].1;
            let next = adjacent[1].0;
            (end[0] - next[0]).hypot(end[1] - next[1]) <= EPS_ENDPOINT_AGREEMENT * scale
        }) {
            continue;
        }
        let Some(first) = uses.first().map(|use_row| use_row.0) else {
            continue;
        };
        let Some(terminal) = uses.last().map(|use_row| use_row.1) else {
            continue;
        };
        let mut vertices = Vec::new();
        vertices.extend(uses.iter().map(|(start, _)| *start));
        if (terminal[0] - first[0]).hypot(terminal[1] - first[1]) > EPS_ENDPOINT_AGREEMENT * scale {
            vertices.push(terminal);
        }
        profiles.push((profile_index, vertices));
    }
    Ok(profiles.into_iter())
}

pub(in super::super) fn oriented_arc_parameterization(
    reversed: bool,
    start: f64,
    end: f64,
) -> (f64, [f64; 2]) {
    let (axis_sign, raw_start, raw_end) = if reversed {
        (-1.0, -end, -start)
    } else {
        (1.0, start, end)
    };
    let raw_span = raw_end - raw_start;
    let full_turn = raw_span.is_finite()
        && (raw_span.abs() - std::f64::consts::TAU).abs()
            <= EPS_PARAMETER_SCALE * raw_span.abs().max(std::f64::consts::TAU);
    let start = raw_start.rem_euclid(std::f64::consts::TAU);
    let mut end = raw_end.rem_euclid(std::f64::consts::TAU);
    if end < start || (full_turn && (end - start).abs() <= EPS_FULL_TURN) {
        end += std::f64::consts::TAU;
    }
    (axis_sign, [start, end])
}

fn forward_arc_sweep(start: f64, end: f64) -> f64 {
    let raw_span = end - start;
    if raw_span.is_finite()
        && (raw_span - std::f64::consts::TAU).abs()
            <= EPS_PARAMETER_SCALE * raw_span.abs().max(std::f64::consts::TAU)
    {
        std::f64::consts::TAU
    } else if raw_span.is_finite() {
        raw_span.rem_euclid(std::f64::consts::TAU)
    } else {
        (end.rem_euclid(std::f64::consts::TAU) - start.rem_euclid(std::f64::consts::TAU))
            .rem_euclid(std::f64::consts::TAU)
    }
}

pub(in super::super) fn line_pcurve(start: [f64; 2], end: [f64; 2]) -> Option<PcurveGeometry> {
    Some(PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(start[0], start[1]),
            Point2::new(end[0] - start[0], end[1] - start[1]),
        )
        .ok()?,
    ))
}

pub(in super::super) fn circular_pcurve(
    center: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<PcurveGeometry>, cadmpeg_core::CodecError> {
    const MAX_CIRCULAR_PCURVE_SEGMENTS: u32 = 100_000;
    let span = end_angle - start_angle;
    let count = (span.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    if !count.is_finite() || count > f64::from(MAX_CIRCULAR_PCURVE_SEGMENTS) {
        refusal.note(
            format_args!("creo circular pcurve record for {record}"),
            &format_args!(
                "the arc span needs more than {MAX_CIRCULAR_PCURVE_SEGMENTS} quarter-turn segments"
            ),
        );
        return Ok(None);
    }
    let segment_count = cadmpeg_core::convert::truncate_f64_to_usize(count).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("Creo circular pcurve segments overflows")
    })?;
    let step = span
        / cadmpeg_core::convert::f64_from_index(segment_count).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(
                "Creo pcurve segment index cannot be represented exactly",
            )
        })?;
    let mut control_points = Vec::new();
    let mut weights = Vec::new();
    for segment in 0..segment_count {
        let first = start_angle
            + cadmpeg_core::convert::f64_from_index(segment).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Creo pcurve segment index cannot be represented exactly",
                )
            })? * step;
        let second = first + step;
        let middle = 0.5 * (first + second);
        let middle_weight = (0.5 * step).cos();
        if segment == 0 {
            control_points.push(Point2::new(
                center[0] + radius * first.cos(),
                center[1] + radius * first.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius * middle.cos() / middle_weight,
            center[1] + radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(Point2::new(
            center[0] + radius * second.cos(),
            center[1] + radius * second.sin(),
        ));
        weights.push(1.0);
    }
    let mut knots = Vec::new();
    knots.extend([0.0; 3]);
    for boundary in 1..segment_count {
        knots.extend(
            [cadmpeg_core::convert::f64_from_index(boundary).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Creo pcurve segment index cannot be represented exactly",
                )
            })? / cadmpeg_core::convert::f64_from_index(segment_count).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Creo pcurve segment index cannot be represented exactly",
                )
            })?; 2],
        );
    }
    knots.extend([1.0; 3]);
    let mut weighted = Vec::new();
    let nurbs = (|| -> Result<cadmpeg_ir::geometry::pcurve::PcurveNurbs, cadmpeg_ir::geometry::nurbs::NurbsError> {
        use cadmpeg_ir::geometry::nurbs::KnotVector;
        use cadmpeg_ir::geometry::pcurve::{PcurveNurbsPoles, WeightedPole2};
        use cadmpeg_ir::scalar::NonZeroReal;
        use cadmpeg_ir::units::FinitePoint2;

        for (index, &weight) in weights.iter().enumerate() {
            if NonZeroReal::new(weight).is_none() {
                return Err(cadmpeg_ir::geometry::nurbs::NurbsError::UnusableWeight {
                    field: "pcurve poles".to_owned(),
                    index,
                    weight,
                });
            }
        }
        for (index, (point, weight)) in control_points.into_iter().zip(weights).enumerate() {
            let Some(point) = FinitePoint2::new(point) else {
                return Err(cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".to_owned(),
                ));
            };
            let Some(admitted_weight) = NonZeroReal::new(weight) else {
                return Err(cadmpeg_ir::geometry::nurbs::NurbsError::UnusableWeight {
                    field: "pcurve poles".to_owned(),
                    index,
                    weight,
                });
            };
            weighted.push(WeightedPole2 { point, weight: admitted_weight });
        }
        let knots = KnotVector::new(knots)?;
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
            2,
            knots,
            PcurveNurbsPoles::Rational { points: weighted },
            false,
        )
    })();
    match nurbs {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            refusal.note(
                format_args!("creo circular pcurve record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

pub(in super::super) fn extrusion_cap_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<PcurveGeometry>, cadmpeg_core::CodecError> {
    match geometry.definition() {
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            // The cap traces the forward sweep the profile area and the edge
            // curves use: at most one turn from the start angle. An arc stored
            // within one turn keeps its stored end angle.
            let start = start_angle.get();
            let sweep = forward_arc_sweep(start, end_angle.get());
            let end = if end_angle.get() - start == sweep {
                end_angle.get()
            } else {
                start + sweep
            };
            let [start_angle, end_angle] = if reversed { [end, start] } else { [start, end] };
            circular_pcurve(
                [center.u, center.v],
                radius.get(),
                start_angle,
                end_angle,
                record,
                refusal,
            )
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            let [start_angle, end_angle] = oriented_full_turn_angles(reversed);
            circular_pcurve(
                [center.u, center.v],
                radius.get(),
                start_angle,
                end_angle,
                record,
                refusal,
            )
        }
        SketchGeometryDefinition::Nurbs { .. } => {
            Ok(sketch_nurbs_pcurve(geometry, reversed, record, refusal))
        }
        _ => Ok(line_pcurve(start, end)),
    }
}

pub(in super::super) fn extrusion_side_uvs(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> [[[f64; 2]; 2]; 4] {
    if matches!(
        geometry.definition(),
        SketchGeometryDefinition::Nurbs { .. }
    ) {
        if let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() {
            let degree = usize::try_from(curve.degree()).ok();
            let range = degree.and_then(|degree| {
                Some([
                    curve.knots().finite_knot(degree)?,
                    curve.knots().finite_knot(curve.pole_rows().count())?,
                ])
            });
            if let Some([lower, upper]) = range
                .filter(|range| range[0] < range[1])
                .map(cadmpeg_ir::scalar::FiniteReal::raw_array)
            {
                return [
                    [[lower, 0.0], [upper, 0.0]],
                    [[upper, 0.0], [upper, 1.0]],
                    [[lower, 1.0], [upper, 1.0]],
                    [[lower, 0.0], [lower, 1.0]],
                ];
            }
        }
    }
    let [first, second] = match geometry.definition() {
        SketchGeometryDefinition::Arc {
            start_angle,
            end_angle,
            ..
        } if reversed => [end_angle.get(), start_angle.get()],
        SketchGeometryDefinition::Arc {
            start_angle,
            end_angle,
            ..
        } => [start_angle.get(), end_angle.get()],
        SketchGeometryDefinition::Circle { .. } => oriented_full_turn_angles(reversed),
        _ => [0.0, (end[0] - start[0]).hypot(end[1] - start[1])],
    };
    [
        [[first, span.lower()], [second, span.lower()]],
        [[second, span.lower()], [second, span.upper()]],
        [[first, span.upper()], [second, span.upper()]],
        [[first, span.lower()], [first, span.upper()]],
    ]
}

pub(in super::super) fn extrusion_profile_signed_area(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profile: &[ProfileEntity],
) -> Result<Option<FiniteReal>, cadmpeg_core::CodecError> {
    let mut area_twice = 0.0;
    for entity in profile {
        let geometry = &entity.geometry;
        let reversed = &entity.reversed;
        let start = entity.start();
        let end = entity.end();
        let contribution = match geometry {
            ProfileGeometry::Nurbs { .. } => {
                let Some(sketch) = geometry.to_sketch() else {
                    return Ok(None);
                };
                let Some(area) = nurbs_profile_signed_area_twice(ctx, &sketch, *reversed)? else {
                    return Ok(None);
                };
                area
            }
            ProfileGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let forward_sweep = forward_arc_sweep(start_angle.get(), end_angle.get());
                let sweep = if *reversed {
                    -forward_sweep
                } else {
                    forward_sweep
                };
                center.u.mul_add(
                    end[1] - start[1],
                    -(center.v * (end[0] - start[0])) + radius.get() * radius.get() * sweep,
                )
            }
            ProfileGeometry::Circle { center, radius } => {
                let sweep = if *reversed {
                    -std::f64::consts::TAU
                } else {
                    std::f64::consts::TAU
                };
                center.u.mul_add(
                    end[1] - start[1],
                    -(center.v * (end[0] - start[0])) + radius.get() * radius.get() * sweep,
                )
            }
            ProfileGeometry::Line { .. } => start[0].mul_add(end[1], -(start[1] * end[0])),
        };
        area_twice += contribution;
    }
    let scale = profile
        .iter()
        .flat_map(|entity| entity.start().into_iter().chain(entity.end()))
        .map(f64::abs)
        .fold(1.0, f64::max);
    Ok((area_twice.abs() > EPS_AREA * scale * scale)
        .then_some(0.5 * area_twice)
        .and_then(FiniteReal::new))
}

#[derive(Debug, Clone, PartialEq)]
pub(in super::super) enum ProfileGeometry {
    Line {
        start: Point2,
        end: Point2,
    },
    Arc {
        center: Point2,
        radius: cadmpeg_ir::scalar::Length,
        start_angle: cadmpeg_ir::scalar::Angle,
        end_angle: cadmpeg_ir::scalar::Angle,
    },
    Circle {
        center: Point2,
        radius: cadmpeg_ir::scalar::Length,
    },
    Nurbs {
        curve: cadmpeg_ir::geometry::pcurve::PcurveNurbs,
    },
}

impl ProfileGeometry {
    fn from_sketch(geometry: SketchGeometry) -> Option<Self> {
        Some(match geometry.into_definition() {
            SketchGeometryDefinition::Line { start, end } => Self::Line {
                start: start.get(),
                end: end.get(),
            },
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => Self::Arc {
                center: center.get(),
                radius: cadmpeg_ir::scalar::Length::from(radius),
                start_angle,
                end_angle,
            },
            SketchGeometryDefinition::Circle { center, radius } => Self::Circle {
                center: center.get(),
                radius: cadmpeg_ir::scalar::Length::from(radius),
            },
            SketchGeometryDefinition::Nurbs { curve } => Self::Nurbs { curve },
            _ => return None,
        })
    }

    pub(super) fn to_sketch(&self) -> Option<SketchGeometry> {
        SketchGeometry::try_from(match self {
            Self::Line { start, end } => SketchGeometryDefinition::Line {
                start: *start,
                end: *end,
            },
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => SketchGeometryDefinition::Arc {
                center: *center,
                radius: *radius,
                start_angle: *start_angle,
                end_angle: *end_angle,
            },
            Self::Circle { center, radius } => SketchGeometryDefinition::Circle {
                center: *center,
                radius: *radius,
            },
            Self::Nurbs { curve } => SketchGeometryDefinition::Nurbs {
                curve: curve.clone(),
            },
        })
        .ok()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in super::super) struct ProfileEntity {
    geometry: ProfileGeometry,
    reversed: bool,
    start: cadmpeg_ir::units::FinitePoint2,
    end: cadmpeg_ir::units::FinitePoint2,
}

impl ProfileEntity {
    pub(in super::super) fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        geometry: SketchGeometry,
        reversed: bool,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some([mut start, mut end]) = sketch_geometry_endpoints(ctx, &geometry)? else {
            return Ok(None);
        };
        if reversed {
            std::mem::swap(&mut start, &mut end);
        }
        let (Some(start), Some(end)) = (
            cadmpeg_ir::units::FinitePoint2::new(Point2::new(start[0], start[1])),
            cadmpeg_ir::units::FinitePoint2::new(Point2::new(end[0], end[1])),
        ) else {
            return Ok(None);
        };
        Ok(ProfileGeometry::from_sketch(geometry).map(|geometry| Self {
            geometry,
            reversed,
            start,
            end,
        }))
    }

    pub(in super::super) fn geometry(&self) -> &ProfileGeometry {
        &self.geometry
    }
    pub(super) fn reversed(&self) -> bool {
        self.reversed
    }
    pub(in super::super) fn start(&self) -> [f64; 2] {
        let point = self.start.get();
        [point.u, point.v]
    }
    pub(in super::super) fn end(&self) -> [f64; 2] {
        let point = self.end.get();
        [point.u, point.v]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in super::super) struct ValidatedProfile {
    entities: ExtrusionProfile,
    area: FiniteReal,
}

impl ValidatedProfile {
    fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        entities: ExtrusionProfile,
        work: &mut ProfileValidationWork,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        for entity in &entities {
            if matches!(entity.geometry, ProfileGeometry::Nurbs { .. }) {
                let scale = entity
                    .start()
                    .into_iter()
                    .chain(entity.end())
                    .map(f64::abs)
                    .fold(1.0, f64::max);
                let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
                let polyline = match profile_nurbs_polyline(ctx, entity, tolerance)? {
                    Ok(polyline) => polyline,
                    Err(stop) => {
                        work.sampling_stopped(stop);
                        return Ok(None);
                    }
                };
                let segments = polyline.windows(2);
                let mut ordered = segments
                    .clone()
                    .enumerate()
                    .map(|(index, segment)| {
                        (
                            index,
                            [segment[0], segment[1]],
                            segment[0][0].min(segment[1][0]),
                            segment[0][0].max(segment[1][0]),
                        )
                    })
                    .collect::<Vec<_>>();
                ordered.sort_by(|first, second| first.2.total_cmp(&second.2));
                for (position, first) in ordered.iter().enumerate() {
                    for second in ordered.iter().skip(position + 1) {
                        if second.2 > first.3 + tolerance {
                            break;
                        }
                        if !work.charge(1) {
                            return Ok(None);
                        }
                        if first.1[0][1].min(first.1[1][1])
                            > second.1[0][1].max(second.1[1][1]) + tolerance
                            || second.1[0][1].min(second.1[1][1])
                                > first.1[0][1].max(first.1[1][1]) + tolerance
                        {
                            continue;
                        }
                        let (first, second) = if first.0 < second.0 {
                            (first, second)
                        } else {
                            (second, first)
                        };
                        let contacts = [
                            (second.0 == first.0 + 1).then_some(first.1[1]),
                            (first.0 == 0
                                && second.0 + 1 == segments.len()
                                && polyline.first() == polyline.last())
                            .then_some(first.1[0]),
                        ];
                        if segments_intersect(first.1, second.1, tolerance, contacts) {
                            return Ok(None);
                        }
                    }
                }
            }
        }
        Ok(extrusion_profile_signed_area(ctx, &entities)?.map(|area| Self { entities, area }))
    }

    pub(in super::super) fn entities(&self) -> &ExtrusionProfile {
        &self.entities
    }
    pub(in super::super) fn area(&self) -> f64 {
        self.area.get()
    }
}

pub(in super::super) type ExtrusionProfile = Vec<ProfileEntity>;

pub(in super::super) fn resolved_sketch_profiles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    sketch_id: &SketchId,
    minimum_entity_count: usize,
) -> Result<Option<Vec<ExtrusionProfile>>, cadmpeg_core::CodecError> {
    let Some(sketch) = unique_profile_sketch(ir, sketch_id) else {
        return Ok(None);
    };
    if sketch.profiles.is_empty() {
        return Ok(None);
    }
    let mut profiles = Vec::new();
    for profile in &sketch.profiles {
        let mut geometries = Vec::new();
        for entity_use in profile {
            let Some(entity) = unique_profile_entity(ir, sketch_id, &entity_use.entity) else {
                return Ok(None);
            };
            let source_geometry = source_carriers.sketch_geometry(entity);
            let source_geometry = match source_geometry.definition() {
                SketchGeometryDefinition::Nurbs { curve } => SketchGeometry::nurbs(curve.clone()),
                SketchGeometryDefinition::Line { .. }
                | SketchGeometryDefinition::Arc { .. }
                | SketchGeometryDefinition::Circle { .. } => source_geometry.clone(),
                _ => return Ok(None),
            };
            let Some(row) = ProfileEntity::new(ctx, source_geometry, entity_use.reversed)? else {
                return Ok(None);
            };
            geometries.push(row);
        }
        if geometries.len() < minimum_entity_count {
            return Ok(None);
        }
        let scale = geometries
            .iter()
            .flat_map(|entity| entity.start().into_iter().chain(entity.end()))
            .map(f64::abs)
            .fold(1.0, f64::max);
        if !geometries.iter().enumerate().all(|(index, entity)| {
            let end = entity.end();
            let next = geometries[(index + 1) % geometries.len()].start();
            (end[0] - next[0]).hypot(end[1] - next[1]) <= EPS_ENDPOINT_AGREEMENT * scale
        }) {
            return Ok(None);
        }
        profiles.push(geometries);
    }
    Ok(Some(profiles))
}

#[cfg(test)]
mod tests;

pub(in super::super) fn profile_arc(segment: &ProfileEntity) -> Option<([f64; 2], f64, f64, f64)> {
    let (center, radius, start, forward_delta) = match &segment.geometry {
        ProfileGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            [center.u, center.v],
            radius.get(),
            (segment.start()[1] - center.v).atan2(segment.start()[0] - center.u),
            forward_arc_sweep(start_angle.get(), end_angle.get()),
        ),
        ProfileGeometry::Circle { center, radius } => (
            [center.u, center.v],
            radius.get(),
            0.0,
            std::f64::consts::TAU,
        ),
        _ => return None,
    };
    let delta = if segment.reversed {
        -forward_delta
    } else {
        forward_delta
    };
    Some((center, radius, start, delta))
}

pub(in super::super) fn oriented_full_turn_angles(reversed: bool) -> [f64; 2] {
    if reversed {
        [std::f64::consts::TAU, 0.0]
    } else {
        [0.0, std::f64::consts::TAU]
    }
}

fn permitted_contact(point: [f64; 2], contacts: [Option<[f64; 2]>; 2], tolerance: f64) -> bool {
    contacts
        .into_iter()
        .flatten()
        .any(|contact| (point[0] - contact[0]).hypot(point[1] - contact[1]) <= tolerance)
}

fn segments_intersect(
    first: [[f64; 2]; 2],
    second: [[f64; 2]; 2],
    tolerance: f64,
    contacts: [Option<[f64; 2]>; 2],
) -> bool {
    use std::cmp::Ordering::{Greater, Less};
    let orient = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        cadmpeg_ir::math::planar::orientation(
            Point2::new(a[0], a[1]),
            Point2::new(b[0], b[1]),
            Point2::new(p[0], p[1]),
        )
    };
    let opposite = |a, b| {
        matches!(
            (a, b),
            (Some(Greater), Some(Less)) | (Some(Less), Some(Greater))
        )
    };
    if opposite(
        orient(first[0], first[1], second[0]),
        orient(first[0], first[1], second[1]),
    ) && opposite(
        orient(second[0], second[1], first[0]),
        orient(second[0], second[1], first[1]),
    ) {
        return true;
    }
    let on_segment = |segment: [[f64; 2]; 2], point: [f64; 2]| {
        let dx = segment[1][0] - segment[0][0];
        let dy = segment[1][1] - segment[0][1];
        let length = dx.hypot(dy);
        let x = point[0] - segment[0][0];
        let y = point[1] - segment[0][1];
        let distance = if length == 0.0 {
            x.hypot(y)
        } else {
            x.mul_add(dy / length, -y * (dx / length)).abs()
        };
        distance.is_finite()
            && distance <= tolerance
            && point[0] >= segment[0][0].min(segment[1][0]) - tolerance
            && point[0] <= segment[0][0].max(segment[1][0]) + tolerance
            && point[1] >= segment[0][1].min(segment[1][1]) - tolerance
            && point[1] <= segment[0][1].max(segment[1][1]) + tolerance
    };
    [
        second[0],
        second[1],
        [
            second[0][0].midpoint(second[1][0]),
            second[0][1].midpoint(second[1][1]),
        ],
    ]
    .into_iter()
    .any(|point| on_segment(first, point) && !permitted_contact(point, contacts, tolerance))
        || [
            first[0],
            first[1],
            [
                first[0][0].midpoint(first[1][0]),
                first[0][1].midpoint(first[1][1]),
            ],
        ]
        .into_iter()
        .any(|point| on_segment(second, point) && !permitted_contact(point, contacts, tolerance))
}

pub(in super::super) fn point_on_profile_arc(
    point: [f64; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let (center, radius, start, delta) = arc;
    let relative = [point[0] - center[0], point[1] - center[1]];
    let distance = relative[0].hypot(relative[1]);
    if !distance.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !start.is_finite()
        || !delta.is_finite()
        || (distance - radius).abs() > tolerance
    {
        return false;
    }
    let angle = relative[1].atan2(relative[0]);
    let travel = if delta >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    let angular_tolerance = tolerance / radius;
    travel <= delta.abs() + angular_tolerance || std::f64::consts::TAU - travel <= angular_tolerance
}

pub(in super::super) fn line_arc_intersect(
    line: [[f64; 2]; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
    contacts: [Option<[f64; 2]>; 2],
) -> bool {
    let start = Point2::new(line[0][0], line[0][1]);
    let end = Point2::new(line[1][0], line[1][1]);
    let length = (end.u - start.u).hypot(end.v - start.v);
    if !length.is_finite() || !tolerance.is_finite() || tolerance < 0.0 || length <= tolerance {
        return false;
    }
    let parameter_tolerance = tolerance / length;
    cadmpeg_ir::math::planar::line_circle_intersections(
        start,
        end,
        Point2::new(arc.0[0], arc.0[1]),
        arc.1,
    )
    .is_some_and(|parameters| {
        parameters.into_iter().any(|(t, point)| {
            (-parameter_tolerance..=1.0 + parameter_tolerance).contains(&t.get())
                && point_on_profile_arc([point.u, point.v], arc, tolerance)
                && !permitted_contact([point.u, point.v], contacts, tolerance)
        })
    })
}

pub(in super::super) fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
    contacts: [Option<[f64; 2]>; 2],
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3 * 0.5).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3 * 0.5).sin(),
                ],
                [
                    arc.0[0] + arc.1 * arc.2.cos(),
                    arc.0[1] + arc.1 * arc.2.sin(),
                ],
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3).sin(),
                ],
            ]
        };
        return endpoints(first).into_iter().any(|point| {
            point_on_profile_arc(point, second, tolerance)
                && !permitted_contact(point, contacts, tolerance)
        }) || endpoints(second).into_iter().any(|point| {
            point_on_profile_arc(point, first, tolerance)
                && !permitted_contact(point, contacts, tolerance)
        });
    }
    if distance <= tolerance {
        return false;
    }
    cadmpeg_ir::math::planar::circle_intersections(
        Point2::new(first.0[0], first.0[1]),
        first.1,
        Point2::new(second.0[0], second.0[1]),
        second.1,
    )
    .is_some_and(|points| {
        points.into_iter().flatten().any(|point| {
            point_on_profile_arc([point.u, point.v], first, tolerance)
                && point_on_profile_arc([point.u, point.v], second, tolerance)
                && !permitted_contact([point.u, point.v], contacts, tolerance)
        })
    })
}

fn planar_point_segment_distance(point: [f64; 2], segment: [[f64; 2]; 2]) -> f64 {
    let direction = [segment[1][0] - segment[0][0], segment[1][1] - segment[0][1]];
    let relative = [point[0] - segment[0][0], point[1] - segment[0][1]];
    let length_squared = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    if length_squared == 0.0 {
        return relative[0].hypot(relative[1]);
    }
    let parameter = (relative[0].mul_add(direction[0], relative[1] * direction[1])
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest = [
        segment[0][0] + parameter * direction[0],
        segment[0][1] + parameter * direction[1],
    ];
    (point[0] - nearest[0]).hypot(point[1] - nearest[1])
}

const NURBS_AREA_GAUSS_NODES: [f64; 8] = [
    -0.960_289_856_497_536_3,
    -0.796_666_477_413_626_7,
    -0.525_532_409_916_329,
    -0.183_434_642_495_649_8,
    0.183_434_642_495_649_8,
    0.525_532_409_916_329,
    0.796_666_477_413_626_7,
    0.960_289_856_497_536_3,
];
const NURBS_AREA_GAUSS_WEIGHTS: [f64; 8] = [
    0.101_228_536_290_376_3,
    0.222_381_034_453_374_5,
    0.313_706_645_877_887_3,
    0.362_683_783_378_362,
    0.362_683_783_378_362,
    0.313_706_645_877_887_3,
    0.222_381_034_453_374_5,
    0.101_228_536_290_376_3,
];

struct NurbsProfileSpan {
    start: f64,
    end: f64,
    start_point: [f64; 2],
    end_point: [f64; 2],
    tolerance: f64,
    depth: usize,
}

fn nurbs_profile_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    evaluator: &mut cadmpeg_ir::eval::decode::NurbsPointEvaluator<'_>,
    nurbs: &NurbsCurve,
    parameter: f64,
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let parameter = require_some!(cadmpeg_ir::eval::map_nurbs_curve_parameter(
        nurbs,
        require_some!(cadmpeg_ir::scalar::FiniteReal::new(parameter)),
    ));
    let point = require_some!(cadmpeg_ir::eval::finite_or_refusal(
        evaluator.point(ctx, parameter.get())?
    )?);
    Ok(Some([point.x, point.y]))
}

const MAX_NURBS_PROFILE_POINTS: usize = 262_145;

/// Why a NURBS profile segment has no polyline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileSamplingStop {
    /// The curve does not evaluate to a finite polyline.
    Unresolved,
    /// Sampling reached a local cap.
    Capped { bound: &'static str, value: usize },
}

impl ProfileSamplingStop {
    fn cap_reason(self) -> Option<String> {
        match self {
            Self::Unresolved => None,
            Self::Capped { bound, value } => Some(format!(
                "NURBS profile sampling stopped at {bound} ({value})"
            )),
        }
    }
}

fn append_nurbs_profile_point(
    points: &mut Vec<[f64; 2]>,
    point: [f64; 2],
) -> Result<(), ProfileSamplingStop> {
    // A profile that needs more points than the ceiling is not sampled.
    if points.len() >= MAX_NURBS_PROFILE_POINTS {
        return Err(ProfileSamplingStop::Capped {
            bound: "MAX_NURBS_PROFILE_POINTS",
            value: MAX_NURBS_PROFILE_POINTS,
        });
    }
    points.push(point);
    Ok(())
}

fn append_nurbs_profile_span(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    evaluator: &mut cadmpeg_ir::eval::decode::NurbsPointEvaluator<'_>,
    nurbs: &NurbsCurve,
    span: &NurbsProfileSpan,
    points: &mut Vec<[f64; 2]>,
) -> Result<Result<(), ProfileSamplingStop>, cadmpeg_core::CodecError> {
    const MAX_DEPTH: usize = 24;
    let _depth = ctx.enter_nested("creo NURBS profile sampling depth")?;
    if !(span.start.is_finite() && span.end.is_finite() && span.start < span.end) {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    }
    let middle = span.start + (span.end - span.start) * 0.5;
    if middle == span.start || middle == span.end {
        return Ok(append_nurbs_profile_point(points, span.end_point));
    }
    let first_quarter = span.start + (span.end - span.start) * 0.25;
    let third_quarter = span.start + (span.end - span.start) * 0.75;
    let (Some(middle_point), Some(first_quarter_point), Some(third_quarter_point)) = (
        nurbs_profile_point(ctx, evaluator, nurbs, middle)?,
        nurbs_profile_point(ctx, evaluator, nurbs, first_quarter)?,
        nurbs_profile_point(ctx, evaluator, nurbs, third_quarter)?,
    ) else {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    };
    let chord = [span.start_point, span.end_point];
    let flatness = planar_point_segment_distance(first_quarter_point, chord)
        .max(planar_point_segment_distance(middle_point, chord))
        .max(planar_point_segment_distance(third_quarter_point, chord));
    if !(flatness.is_finite() && span.tolerance.is_finite() && span.tolerance > 0.0) {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    }
    if flatness <= span.tolerance {
        return Ok(append_nurbs_profile_point(points, span.end_point));
    }
    // A span that is not flat at the subdivision ceiling is not sampled.
    if span.depth >= MAX_DEPTH {
        return Ok(Err(ProfileSamplingStop::Capped {
            bound: "MAX_DEPTH",
            value: MAX_DEPTH,
        }));
    }
    if let Err(stop) = append_nurbs_profile_span(
        ctx,
        evaluator,
        nurbs,
        &NurbsProfileSpan {
            start: span.start,
            end: middle,
            start_point: span.start_point,
            end_point: middle_point,
            tolerance: span.tolerance,
            depth: span.depth + 1,
        },
        points,
    )? {
        return Ok(Err(stop));
    }
    append_nurbs_profile_span(
        ctx,
        evaluator,
        nurbs,
        &NurbsProfileSpan {
            start: middle,
            end: span.end,
            start_point: middle_point,
            end_point: span.end_point,
            tolerance: span.tolerance,
            depth: span.depth + 1,
        },
        points,
    )
}

fn nurbs_profile_polyline(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nurbs: &NurbsCurve,
    tolerance: f64,
) -> Result<Result<Vec<[f64; 2]>, ProfileSamplingStop>, cadmpeg_core::CodecError> {
    let Some(range) = nurbs_intrinsic_parameter_range(nurbs) else {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    };
    let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(nurbs);
    let [lower, upper] = cadmpeg_ir::scalar::FiniteReal::raw_array(range);
    let Some(first) = nurbs_profile_point(ctx, &mut evaluator, nurbs, lower)? else {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    };
    let mut points = Vec::new();
    if let Err(stop) = append_nurbs_profile_point(&mut points, first) {
        return Ok(Err(stop));
    }
    for pair in nurbs.knots().windows(2) {
        let start = pair[0].max(lower);
        let end = pair[1].min(upper);
        if start >= end {
            continue;
        }
        let (Some(start_point), Some(end_point)) = (
            nurbs_profile_point(ctx, &mut evaluator, nurbs, start)?,
            nurbs_profile_point(ctx, &mut evaluator, nurbs, end)?,
        ) else {
            return Ok(Err(ProfileSamplingStop::Unresolved));
        };
        if points.last().copied() != Some(start_point) {
            if let Err(stop) = append_nurbs_profile_point(&mut points, start_point) {
                return Ok(Err(stop));
            }
        }
        if let Err(stop) = append_nurbs_profile_span(
            ctx,
            &mut evaluator,
            nurbs,
            &NurbsProfileSpan {
                start,
                end,
                start_point,
                end_point,
                tolerance,
                depth: 0,
            },
            &mut points,
        )? {
            return Ok(Err(stop));
        }
    }
    Ok(if points.len() >= 2 {
        Ok(points)
    } else {
        Err(ProfileSamplingStop::Unresolved)
    })
}

fn profile_nurbs_polyline(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segment: &ProfileEntity,
    tolerance: f64,
) -> Result<Result<Vec<[f64; 2]>, ProfileSamplingStop>, cadmpeg_core::CodecError> {
    let Some(sketch) = segment.geometry.to_sketch() else {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    };
    let Some(nurbs) = oriented_sketch_nurbs_curve(&sketch, segment.reversed) else {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    };
    nurbs_profile_polyline(ctx, &nurbs, tolerance)
}

fn nurbs_profile_signed_area_twice(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SketchGeometry,
    reversed: bool,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(nurbs) = oriented_sketch_nurbs_curve(geometry, reversed) else {
        return Ok(None);
    };
    let Some(range) = nurbs_intrinsic_parameter_range(&nurbs) else {
        return Ok(None);
    };
    let [lower, upper] = cadmpeg_ir::scalar::FiniteReal::raw_array(range);
    let carrier = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &carrier else {
        return Ok(None);
    };
    let mut area_twice = 0.0;
    for pair in nurbs.knots().windows(2) {
        let start = pair[0].max(lower);
        let end = pair[1].min(upper);
        if start >= end {
            continue;
        }
        let sum = start + end;
        let span = end - start;
        let middle = if sum.is_finite() {
            0.5 * sum
        } else {
            start.midpoint(end)
        };
        let half_width = if span.is_finite() {
            0.5 * span
        } else {
            end * 0.5 - start * 0.5
        };
        for (node, weight) in NURBS_AREA_GAUSS_NODES
            .into_iter()
            .zip(NURBS_AREA_GAUSS_WEIGHTS)
        {
            let parameter = middle + half_width * node;
            let (Some(point), Some(tangent)) = (
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, parameter),
                )?)?,
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_tangent(ctx, &carrier, parameter),
                )?)?,
            ) else {
                return Ok(None);
            };
            area_twice += weight * (point.x * tangent.y - point.y * tangent.x) * half_width;
        }
    }
    Ok(area_twice.is_finite().then_some(area_twice))
}

#[cfg(test)]
fn polylines_intersect(
    first: &[[f64; 2]],
    second: &[[f64; 2]],
    tolerance: f64,
    contacts: [Option<[f64; 2]>; 2],
) -> bool {
    for first_segment in first.windows(2) {
        for second_segment in second.windows(2) {
            if segments_intersect(
                [first_segment[0], first_segment[1]],
                [second_segment[0], second_segment[1]],
                tolerance,
                contacts,
            ) {
                return true;
            }
        }
    }
    false
}

/// Whether two profile segments meet away from `contacts`, one segment pair
/// at a time. [`ordered_extrusion_profiles`] answers the same question for
/// every pair of a profile set at once.
#[cfg(test)]
pub(in super::super) fn profile_segments_intersect(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    first: &ProfileEntity,
    second: &ProfileEntity,
    tolerance: f64,
    contacts: [Option<[f64; 2]>; 2],
) -> Result<bool, cadmpeg_core::CodecError> {
    let first_nurbs = matches!(first.geometry, ProfileGeometry::Nurbs { .. });
    let second_nurbs = matches!(second.geometry, ProfileGeometry::Nurbs { .. });
    if first_nurbs || second_nurbs {
        if first_nurbs {
            if let Some(arc) = profile_arc(second) {
                let Ok(polyline) = profile_nurbs_polyline(ctx, first, tolerance)? else {
                    return Ok(false);
                };
                for segment in polyline.windows(2) {
                    if line_arc_intersect([segment[0], segment[1]], arc, tolerance, contacts) {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
        }
        if second_nurbs {
            if let Some(arc) = profile_arc(first) {
                let Ok(polyline) = profile_nurbs_polyline(ctx, second, tolerance)? else {
                    return Ok(false);
                };
                for segment in polyline.windows(2) {
                    if line_arc_intersect([segment[0], segment[1]], arc, tolerance, contacts) {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
        }
        let first_line = [first.start(), first.end()];
        let second_line = [second.start(), second.end()];
        let first_polyline = if first_nurbs {
            profile_nurbs_polyline(ctx, first, tolerance)?.ok()
        } else {
            None
        };
        if first_nurbs && first_polyline.is_none() {
            return Ok(true);
        }
        let second_polyline = if second_nurbs {
            profile_nurbs_polyline(ctx, second, tolerance)?.ok()
        } else {
            None
        };
        if second_nurbs && second_polyline.is_none() {
            return Ok(true);
        }
        return Ok(polylines_intersect(
            first_polyline
                .as_ref()
                .map_or(first_line.as_slice(), Vec::as_slice),
            second_polyline
                .as_ref()
                .map_or(second_line.as_slice(), Vec::as_slice),
            tolerance,
            contacts,
        ));
    }
    Ok(match (profile_arc(first), profile_arc(second)) {
        (None, None) => segments_intersect(
            [first.start(), first.end()],
            [second.start(), second.end()],
            tolerance,
            contacts,
        ),
        (None, Some(arc)) => {
            line_arc_intersect([first.start(), first.end()], arc, tolerance, contacts)
        }
        (Some(arc), None) => {
            line_arc_intersect([second.start(), second.end()], arc, tolerance, contacts)
        }
        (Some(first), Some(second)) => arcs_intersect(first, second, tolerance, contacts),
    })
}

/// An axis-aligned box. An empty box has inverted, infinite corners.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PieceBounds {
    min: [f64; 2],
    max: [f64; 2],
}

impl PieceBounds {
    const EMPTY: Self = Self {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };

    fn include(&mut self, point: [f64; 2]) {
        for ((min, max), value) in self.min.iter_mut().zip(&mut self.max).zip(point) {
            *min = min.min(value);
            *max = max.max(value);
        }
    }

    fn union(mut self, other: Self) -> Self {
        self.include(other.min);
        self.include(other.max);
        self
    }

    fn widened(self, margin: f64) -> Self {
        Self {
            min: self.min.map(|value| value - margin),
            max: self.max.map(|value| value + margin),
        }
    }

    fn contains(self, point: [f64; 2]) -> bool {
        (0..2).all(|axis| self.min[axis] <= point[axis] && point[axis] <= self.max[axis])
    }

    fn covers(self, other: Self) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    fn overlaps(self, other: Self) -> bool {
        (0..2).all(|axis| self.min[axis] <= other.max[axis] && other.min[axis] <= self.max[axis])
    }
}

/// A straight piece of a profile segment, or a whole arc segment.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ProfilePiece {
    Line([[f64; 2]; 2]),
    Arc(([f64; 2], f64, f64, f64)),
}

impl ProfilePiece {
    /// Bounds of the piece; an arc is bounded by its whole circle.
    fn bounds(self) -> PieceBounds {
        let mut bounds = PieceBounds::EMPTY;
        match self {
            Self::Line([start, end]) => {
                bounds.include(start);
                bounds.include(end);
            }
            Self::Arc((center, radius, _, _)) => {
                let radius = radius.abs();
                bounds.include([center[0] - radius, center[1] - radius]);
                bounds.include([center[0] + radius, center[1] + radius]);
            }
        }
        bounds
    }

    /// Whether the pieces of two distinct segments meet away from
    /// `contacts`, with `self` from the segment earlier in profile order.
    fn intersects(self, other: Self, tolerance: f64, contacts: [Option<[f64; 2]>; 2]) -> bool {
        match (self, other) {
            (Self::Line(first), Self::Line(second)) => {
                segments_intersect(first, second, tolerance, contacts)
            }
            (Self::Line(line), Self::Arc(arc)) | (Self::Arc(arc), Self::Line(line)) => {
                line_arc_intersect(line, arc, tolerance, contacts)
            }
            (Self::Arc(first), Self::Arc(second)) => {
                arcs_intersect(first, second, tolerance, contacts)
            }
        }
    }
}

/// The pieces one profile's winding number walks, with their bounds and the
/// segment endpoints widened by the profile tolerance.
///
/// A point outside the bounds is separated from every piece by a line, so
/// the angle it sees the profile through stays below a half turn and the
/// profile does not strictly contain it.
struct ContainmentShape {
    pieces: Vec<ProfilePiece>,
    bounds: PieceBounds,
    tolerance: f64,
}

impl ContainmentShape {
    fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        profile: &ExtrusionProfile,
    ) -> Result<Result<Self, ProfileSamplingStop>, cadmpeg_core::CodecError> {
        let scale = profile
            .iter()
            .flat_map(|entity| entity.start().into_iter().chain(entity.end()))
            .map(f64::abs)
            .fold(1.0, f64::max);
        let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
        let mut pieces = Vec::new();
        let mut bounds = PieceBounds::EMPTY;
        for segment in profile {
            bounds.include(segment.start());
            bounds.include(segment.end());
            if matches!(segment.geometry, ProfileGeometry::Nurbs { .. }) {
                match profile_nurbs_polyline(ctx, segment, tolerance)? {
                    Ok(polyline) => pieces.extend(
                        polyline
                            .windows(2)
                            .map(|pair| ProfilePiece::Line([pair[0], pair[1]])),
                    ),
                    Err(stop) => return Ok(Err(stop)),
                }
            } else if let Some(arc) = profile_arc(segment) {
                pieces.push(ProfilePiece::Arc(arc));
            } else {
                pieces.push(ProfilePiece::Line([segment.start(), segment.end()]));
            }
        }
        for piece in &pieces {
            bounds = bounds.union(piece.bounds());
        }
        Ok(Ok(Self {
            pieces,
            bounds: bounds.widened(tolerance),
            tolerance,
        }))
    }

    fn strictly_contains(&self, point: [f64; 2]) -> bool {
        let mut winding = 0.0;
        for piece in &self.pieces {
            match *piece {
                ProfilePiece::Line([first, second]) => {
                    let first = [first[0] - point[0], first[1] - point[1]];
                    let second = [second[0] - point[0], second[1] - point[1]];
                    winding += first[0]
                        .mul_add(second[1], -(first[1] * second[0]))
                        .atan2(first[0].mul_add(second[0], first[1] * second[1]));
                }
                ProfilePiece::Arc((center, radius, start, delta)) => {
                    let relative = [point[0] - center[0], point[1] - center[1]];
                    let distance = relative[0].hypot(relative[1]);
                    if !distance.is_finite()
                        || point_on_profile_arc(
                            point,
                            (center, radius, start, delta),
                            self.tolerance,
                        )
                    {
                        return false;
                    }
                    let correction = if distance < radius {
                        let [x, y] = relative.map(|v| v / radius);
                        let angle = |t: f64| {
                            (x * t.sin() - y * t.cos()).atan2(1.0 - x * t.cos() - y * t.sin())
                        };
                        delta + angle(start + delta) - angle(start)
                    } else {
                        let [x, y] = relative.map(|v| -v / distance);
                        let ratio = radius / distance;
                        let angle = |t: f64| {
                            let a = x + ratio * t.cos();
                            let b = y + ratio * t.sin();
                            (x * b - y * a).atan2(x * a + y * b)
                        };
                        angle(start + delta) - angle(start)
                    };
                    if !correction.is_finite() {
                        return false;
                    }
                    winding += correction;
                }
            }
        }
        winding.abs() > std::f64::consts::PI
    }
}

#[cfg(test)]
pub(in super::super) fn profile_strictly_contains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profile: &ExtrusionProfile,
    point: [f64; 2],
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(ContainmentShape::new(ctx, profile)?.is_ok_and(|shape| shape.strictly_contains(point)))
}

/// Work units one profile set's validation may spend per piece it compares.
const PROFILE_VALIDATION_WORK_PER_PIECE: usize = 256;
/// Work units any profile set's validation may spend.
const PROFILE_VALIDATION_WORK_FLOOR: usize = 1 << 22;

/// Local work of one profile set's validation: the piece pairs and point
/// candidates its sweeps visit, the pieces its containment tests walk, and
/// the NURBS self-intersection pairs. The first cap that stops validation
/// is kept as the reason.
struct ProfileValidationWork {
    budget: cadmpeg_core::decode::WorkBudget,
    limit: usize,
    capped: Option<String>,
}

impl ProfileValidationWork {
    /// The work of a set with `pieces` pieces, or `None` when its limit is
    /// not a `usize`.
    fn new(pieces: usize) -> Option<Self> {
        let limit = pieces
            .checked_mul(PROFILE_VALIDATION_WORK_PER_PIECE)?
            .max(PROFILE_VALIDATION_WORK_FLOOR);
        Some(Self {
            budget: cadmpeg_core::decode::WorkBudget::new(limit),
            limit,
            capped: None,
        })
    }

    fn charge(&mut self, work: usize) -> bool {
        if self.budget.charge_by(work) {
            return true;
        }
        self.capped.get_or_insert_with(|| {
            format!(
                "profile validation reached its local work limit of {} units \
                 (PROFILE_VALIDATION_WORK_PER_PIECE ({PROFILE_VALIDATION_WORK_PER_PIECE}) per \
                 compared piece, at least PROFILE_VALIDATION_WORK_FLOOR \
                 ({PROFILE_VALIDATION_WORK_FLOOR}))",
                self.limit
            )
        });
        false
    }

    fn sampling_stopped(&mut self, stop: ProfileSamplingStop) {
        if self.capped.is_none() {
            self.capped = stop.cap_reason();
        }
    }
}

/// One piece of a profile set with its segment's index in profile order and
/// its bounds widened by the set tolerance.
#[derive(Debug, Clone, Copy)]
struct SourcedPiece {
    source: usize,
    piece: ProfilePiece,
    bounds: PieceBounds,
}

impl SourcedPiece {
    fn new(source: usize, piece: ProfilePiece, tolerance: f64) -> Self {
        Self {
            source,
            piece,
            bounds: piece.bounds().widened(tolerance),
        }
    }
}

/// The pieces the intersection test compares for every segment of a profile
/// set, sampling each NURBS segment once.
///
/// An unresolved NURBS segment meets every other segment that is not an arc,
/// so it stops the set unless the other segments are arcs; then it has no
/// pieces.
fn profile_set_pieces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profiles: &[ExtrusionProfile],
    tolerance: f64,
) -> Result<Result<Vec<SourcedPiece>, ProfileSamplingStop>, cadmpeg_core::CodecError> {
    let mut pieces = Vec::new();
    let mut unresolved = false;
    for (source, segment) in profiles.iter().flatten().enumerate() {
        if matches!(segment.geometry, ProfileGeometry::Nurbs { .. }) {
            match profile_nurbs_polyline(ctx, segment, tolerance)? {
                Ok(polyline) => pieces.extend(polyline.windows(2).map(|pair| {
                    SourcedPiece::new(source, ProfilePiece::Line([pair[0], pair[1]]), tolerance)
                })),
                Err(ProfileSamplingStop::Unresolved) => unresolved = true,
                Err(stop) => return Ok(Err(stop)),
            }
        } else if let Some(arc) = profile_arc(segment) {
            pieces.push(SourcedPiece::new(source, ProfilePiece::Arc(arc), tolerance));
        } else {
            pieces.push(SourcedPiece::new(
                source,
                ProfilePiece::Line([segment.start(), segment.end()]),
                tolerance,
            ));
        }
    }
    let unarced = profiles
        .iter()
        .flatten()
        .filter(|segment| profile_arc(segment).is_none())
        .count();
    if unresolved && unarced >= 2 {
        return Ok(Err(ProfileSamplingStop::Unresolved));
    }
    Ok(Ok(pieces))
}

/// The endpoints two segments of a profile set may share: the joint of
/// consecutive segments and the closing joint of a profile.
fn segment_contacts(
    profiles: &[ExtrusionProfile],
    locations: &[(usize, usize)],
    first: usize,
    second: usize,
) -> [Option<[f64; 2]>; 2] {
    let ((first_profile, first_index), (second_profile, second_index)) =
        (locations[first], locations[second]);
    if first_profile != second_profile {
        return [None, None];
    }
    let profile = &profiles[first_profile];
    [
        (second_index == first_index + 1).then_some(profile[first_index].end()),
        (first_index == 0 && second_index + 1 == profile.len())
            .then_some(profile[first_index].start()),
    ]
}

/// Whether two segments of the set meet away from their shared endpoints, or
/// the work ran out.
///
/// The sweep visits only piece pairs whose widened x ranges overlap and tests
/// only pairs whose widened bounds overlap; a pair that meets within the
/// tolerance always has overlapping widened bounds.
fn profile_set_intersects(
    profiles: &[ExtrusionProfile],
    pieces: &mut [SourcedPiece],
    tolerance: f64,
    work: &mut ProfileValidationWork,
) -> bool {
    let locations = profiles
        .iter()
        .enumerate()
        .flat_map(|(profile, segments)| (0..segments.len()).map(move |index| (profile, index)))
        .collect::<Vec<_>>();
    pieces.sort_by(|first, second| first.bounds.min[0].total_cmp(&second.bounds.min[0]));
    for (position, first) in pieces.iter().enumerate() {
        for second in &pieces[position + 1..] {
            if second.bounds.min[0] > first.bounds.max[0] {
                break;
            }
            if !work.charge(1) {
                return true;
            }
            if first.source == second.source || !first.bounds.overlaps(second.bounds) {
                continue;
            }
            let (first, second) = if first.source < second.source {
                (first, second)
            } else {
                (second, first)
            };
            let contacts = segment_contacts(profiles, &locations, first.source, second.source);
            if first.piece.intersects(second.piece, tolerance, contacts) {
                return true;
            }
        }
    }
    false
}

/// The one profile that strictly contains the first point of every other
/// profile, or `None` when there is not exactly one or the work ran out.
fn sole_outer_profile(
    first_points: &[[f64; 2]],
    shapes: &[Option<ContainmentShape>],
    work: &mut ProfileValidationWork,
) -> Option<usize> {
    let mut first_point_bounds = PieceBounds::EMPTY;
    for point in first_points {
        first_point_bounds.include(*point);
    }
    let mut outer = None;
    for (candidate, shape) in shapes.iter().enumerate() {
        // A shape's bounds hold its own first point, so a first point outside
        // them belongs to another profile that the shape cannot contain.
        let Some(shape) = shape
            .as_ref()
            .filter(|shape| shape.bounds.covers(first_point_bounds))
        else {
            continue;
        };
        let mut contains_all = true;
        for (index, point) in first_points.iter().enumerate() {
            if index == candidate {
                continue;
            }
            if !work.charge(cadmpeg_core::decode::work_units(shape.pieces.len())) {
                return None;
            }
            if !shape.strictly_contains(*point) {
                contains_all = false;
                break;
            }
        }
        if contains_all && outer.replace(candidate).is_some() {
            return None;
        }
    }
    outer
}

/// Whether a profile other than `outer` strictly contains the first point of
/// another such profile, or the work ran out.
fn inner_profiles_nest(
    first_points: &[[f64; 2]],
    shapes: &[Option<ContainmentShape>],
    outer: usize,
    work: &mut ProfileValidationWork,
) -> bool {
    let mut by_x = (0..first_points.len())
        .filter(|index| *index != outer)
        .collect::<Vec<_>>();
    by_x.sort_by(|first, second| first_points[*first][0].total_cmp(&first_points[*second][0]));
    for (container, shape) in shapes.iter().enumerate() {
        let Some(shape) = shape.as_ref().filter(|_| container != outer) else {
            continue;
        };
        let from = by_x.partition_point(|index| first_points[*index][0] < shape.bounds.min[0]);
        for &other in &by_x[from..] {
            let point = first_points[other];
            if point[0] > shape.bounds.max[0] {
                break;
            }
            if !work.charge(1) {
                return true;
            }
            if other == container || !shape.bounds.contains(point) {
                continue;
            }
            if !work.charge(cadmpeg_core::decode::work_units(shape.pieces.len())) {
                return true;
            }
            if shape.strictly_contains(point) {
                return true;
            }
        }
    }
    false
}

/// Order a profile set as one outer profile followed by its holes, when no
/// two segments meet away from their shared endpoints, exactly one profile
/// contains the others, no hole contains another, and every hole winds
/// against the outer profile.
///
/// Validation that stops at a local cap returns `None` and records the cap in
/// `refusal`.
pub(in super::super) fn ordered_extrusion_profiles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profiles: Vec<ExtrusionProfile>,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<Vec<ValidatedProfile>>, cadmpeg_core::CodecError> {
    if profiles.is_empty() || profiles.iter().any(Vec::is_empty) {
        return Ok(None);
    }
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|entity| entity.start().into_iter().chain(entity.end()))
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
    let mut pieces = match profile_set_pieces(ctx, &profiles, tolerance)? {
        Ok(pieces) => pieces,
        Err(stop) => {
            if let Some(reason) = stop.cap_reason() {
                refusal.note("extrusion profile set", &reason);
            }
            return Ok(None);
        }
    };
    let Some(mut work) = ProfileValidationWork::new(pieces.len()) else {
        refusal.note(
            "extrusion profile set",
            &format_args!(
                "{} pieces overflow the profile validation work limit",
                pieces.len()
            ),
        );
        return Ok(None);
    };
    let ordered = order_validated_profiles(ctx, profiles, &mut pieces, tolerance, &mut work)?;
    if let Some(reason) = work.capped {
        refusal.note("extrusion profile set", &reason);
        return Ok(None);
    }
    Ok(ordered)
}

fn order_validated_profiles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profiles: Vec<ExtrusionProfile>,
    pieces: &mut [SourcedPiece],
    tolerance: f64,
    work: &mut ProfileValidationWork,
) -> Result<Option<Vec<ValidatedProfile>>, cadmpeg_core::CodecError> {
    if profile_set_intersects(&profiles, pieces, tolerance, work) {
        return Ok(None);
    }
    let outer = if profiles.len() == 1 {
        0
    } else {
        let mut shapes = Vec::new();
        for profile in &profiles {
            shapes.push(match ContainmentShape::new(ctx, profile)? {
                Ok(shape) => Some(shape),
                Err(ProfileSamplingStop::Unresolved) => None,
                Err(stop) => {
                    work.sampling_stopped(stop);
                    return Ok(None);
                }
            });
        }
        let first_points = profiles
            .iter()
            .map(|profile| profile[0].start())
            .collect::<Vec<_>>();
        let Some(outer) = sole_outer_profile(&first_points, &shapes, work) else {
            return Ok(None);
        };
        if inner_profiles_nest(&first_points, &shapes, outer, work) {
            return Ok(None);
        }
        outer
    };
    let mut validated = Vec::new();
    for profile in profiles {
        let Some(profile) = ValidatedProfile::new(ctx, profile, work)? else {
            return Ok(None);
        };
        validated.push(profile);
    }
    let mut profiles = validated;
    let outer_area = profiles[outer].area();
    if profiles.iter().enumerate().any(|(index, profile)| {
        index != outer && profile.area().is_sign_positive() == outer_area.is_sign_positive()
    }) {
        return Ok(None);
    }
    profiles.swap(0, outer);
    Ok(Some(profiles))
}

#[cfg(test)]
mod evaluation_tests;

#[cfg(test)]
mod admission_tests;
