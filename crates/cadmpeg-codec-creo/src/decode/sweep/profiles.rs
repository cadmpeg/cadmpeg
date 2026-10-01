// SPDX-License-Identifier: Apache-2.0
//! Sketch profile connectivity, intersection, and containment.

use super::super::holes::placement::ExtrusionSpan;
use super::super::uniqueness::exactly_one;
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
    Ok(match geometry.definition() {
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
            let Some(nurbs) = sketch_nurbs_curve(ctx, geometry)? else {
                return Ok(None);
            };
            let Some(range) = nurbs_intrinsic_parameter_range(&nurbs) else {
                return Ok(None);
            };
            let [lower, upper] = cadmpeg_ir::scalar::FiniteReal::raw_array(range);
            let carrier = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs));
            let (Some(first), Some(last)) = (
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::curve_point_for_decode(ctx, &carrier, lower)?,
                )?,
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::curve_point_for_decode(ctx, &carrier, upper)?,
                )?,
            ) else {
                return Ok(None);
            };
            Some([[first.x, first.y], [last.x, last.y]])
        }
        _ => None,
    })
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
    let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == *sketch_id),
    ) else {
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
            let Some(geometry) =
                exactly_one(ir.model.sketch_entities.iter().filter(|entity| {
                    entity.sketch == *sketch_id && entity.id() == &entity_use.entity
                }))
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
            ctx.reserve_vec(&mut uses, 1, "creo connected profile uses")?;
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
        ctx.reserve_vec(&mut vertices, uses.len(), "creo connected profile vertices")?;
        vertices.extend(uses.iter().map(|(start, _)| *start));
        if (terminal[0] - first[0]).hypot(terminal[1] - first[1]) > EPS_ENDPOINT_AGREEMENT * scale {
            ctx.reserve_vec(&mut vertices, 1, "creo connected profile vertices")?;
            vertices.push(terminal);
        }
        ctx.reserve_vec(&mut profiles, 1, "creo connected profile rows")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    center: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<PcurveGeometry>, cadmpeg_core::CodecError> {
    const MAX_CIRCULAR_PCURVE_SEGMENTS: f64 = 100_000.0;
    let span = end_angle - start_angle;
    let count = (span.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    if !count.is_finite() || count > MAX_CIRCULAR_PCURVE_SEGMENTS {
        return Ok(None);
    }
    let Some(segment_count) = cadmpeg_core::convert::truncate_f64_to_usize(count) else {
        return Ok(None);
    };
    let step = span
        / cadmpeg_core::convert::f64_from_index(segment_count).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(
                "Creo pcurve segment index cannot be represented exactly",
            )
        })?;
    let Some(pole_count) = segment_count.checked_mul(2).and_then(|n| n.checked_add(1)) else {
        return Ok(None);
    };
    let Some(knot_count) = segment_count.checked_mul(2).and_then(|n| n.checked_add(4)) else {
        return Ok(None);
    };
    let mut control_points = Vec::new();
    ctx.reserve_vec(
        &mut control_points,
        pole_count,
        "creo circular pcurve controls",
    )?;
    let mut weights = Vec::new();
    ctx.reserve_vec(&mut weights, pole_count, "creo circular pcurve weights")?;
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
    ctx.reserve_vec(&mut knots, knot_count, "creo circular pcurve knots")?;
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
    ctx.reserve_vec(
        &mut weighted,
        pole_count,
        "creo circular pcurve weighted poles",
    )?;
    let nurbs = (|| -> Result<Result<cadmpeg_ir::geometry::pcurve::PcurveNurbs, cadmpeg_ir::geometry::nurbs::NurbsError>, cadmpeg_core::CodecError> {
        use cadmpeg_ir::geometry::nurbs::KnotValue;
        use cadmpeg_ir::geometry::pcurve::{PcurveNurbsPoles, WeightedPole2};
        use cadmpeg_ir::scalar::NonZeroReal;
        use cadmpeg_ir::units::FinitePoint2;

        for (index, &weight) in weights.iter().enumerate() {
            if NonZeroReal::new(weight).is_none() {
                return Ok(Err(cadmpeg_ir::geometry::nurbs::NurbsError::UnusableWeight {
                    field: ctx.copy_retained_text("pcurve poles", "creo circular pcurve refusal field")?,
                    index,
                    weight,
                }));
            }
        }
        for (index, (point, weight)) in control_points.into_iter().zip(weights).enumerate() {
            let Some(point) = FinitePoint2::new(point) else {
                return Ok(Err(cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    ctx.copy_retained_text("control_points contains a non-finite point", "creo circular pcurve refusal text")?,
                )));
            };
            let Some(admitted_weight) = NonZeroReal::new(weight) else {
                return Ok(Err(cadmpeg_ir::geometry::nurbs::NurbsError::UnusableWeight {
                    field: ctx.copy_retained_text("pcurve poles", "creo circular pcurve refusal field")?,
                    index,
                    weight,
                }));
            };
            weighted.push(WeightedPole2 { point, weight: admitted_weight });
        }
        let knots = match KnotValue::admit(knots) {
            Ok(knots) => knots,
            Err(error) => return Ok(Err(error)),
        };
        Ok(cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_admitted_rows(
            2,
            knots,
            PcurveNurbsPoles::Rational { points: weighted },
            false,
        ))
    })()?;
    match nurbs {
        Ok(nurbs) => Ok(Some(PcurveGeometry::Nurbs { nurbs })),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo circular pcurve record for {record}"),
                &error,
            );
            Ok(None)
        }
    }
}

pub(in super::super) fn extrusion_cap_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
            let [start_angle, end_angle] = if reversed {
                [end_angle.get(), start_angle.get()]
            } else {
                [start_angle.get(), end_angle.get()]
            };
            circular_pcurve(
                ctx,
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
                ctx,
                [center.u, center.v],
                radius.get(),
                start_angle,
                end_angle,
                record,
                refusal,
            )
        }
        SketchGeometryDefinition::Nurbs { .. } => {
            sketch_nurbs_pcurve(ctx, geometry, reversed, record, refusal)
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
    for ProfileEntity {
        geometry,
        reversed,
        start,
        end,
    } in profile
    {
        let contribution = match geometry {
            ProfileGeometry::Nurbs { .. } => {
                let Some(sketch) = geometry.to_sketch(ctx)? else {
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
        .flat_map(|ProfileEntity { start, end, .. }| start.iter().chain(end))
        .map(|value| value.abs())
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

    pub(super) fn to_sketch(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
        Ok(SketchGeometry::try_from(match self {
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
                curve: super::nurbs::copy_pcurve_nurbs(
                    ctx,
                    curve,
                    "creo profile sketch NURBS knots",
                    "creo profile sketch NURBS poles",
                )?,
            },
        })
        .ok())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in super::super) struct ProfileEntity {
    geometry: ProfileGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
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
        self.start
    }
    pub(in super::super) fn end(&self) -> [f64; 2] {
        self.end
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
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
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
    let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == *sketch_id),
    ) else {
        return Ok(None);
    };
    if sketch.profiles.is_empty() {
        return Ok(None);
    }
    let mut profiles = Vec::new();
    for profile in &sketch.profiles {
        let mut geometries = Vec::new();
        for entity_use in profile {
            let Some(entity) =
                exactly_one(ir.model.sketch_entities.iter().filter(|entity| {
                    entity.sketch == *sketch_id && entity.id() == &entity_use.entity
                }))
            else {
                return Ok(None);
            };
            let source_geometry = source_carriers.sketch_geometry(entity);
            let source_geometry = match source_geometry.definition() {
                SketchGeometryDefinition::Nurbs { curve } => {
                    let operation = "creo resolved profile NURBS copy";
                    SketchGeometry::nurbs(curve.try_clone_for_decode(ctx, operation)?)
                }
                SketchGeometryDefinition::Line { .. }
                | SketchGeometryDefinition::Arc { .. }
                | SketchGeometryDefinition::Circle { .. } => source_geometry
                    .try_clone_for_decode(ctx, "creo resolved analytic profile copy")?,
                _ => return Ok(None),
            };
            let Some(row) = ProfileEntity::new(ctx, source_geometry, entity_use.reversed)? else {
                return Ok(None);
            };
            ctx.reserve_vec(&mut geometries, 1, "creo resolved profile entities")?;
            geometries.push(row);
        }
        if geometries.len() < minimum_entity_count {
            return Ok(None);
        }
        let scale = geometries
            .iter()
            .flat_map(|ProfileEntity { start, end, .. }| start.iter().chain(end))
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        if !geometries
            .iter()
            .enumerate()
            .all(|(index, ProfileEntity { end, .. })| {
                let next = geometries[(index + 1) % geometries.len()].start;
                (end[0] - next[0]).hypot(end[1] - next[1]) <= EPS_ENDPOINT_AGREEMENT * scale
            })
        {
            return Ok(None);
        }
        ctx.reserve_vec(&mut profiles, 1, "creo resolved profile rows")?;
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
            (segment.start[1] - center.v).atan2(segment.start[0] - center.u),
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

fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2], tolerance: f64) -> bool {
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
    on_segment(first, second[0])
        || on_segment(first, second[1])
        || on_segment(second, first[0])
        || on_segment(second, first[1])
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
        })
    })
}

pub(in super::super) fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
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
        return endpoints(first)
            .into_iter()
            .any(|point| point_on_profile_arc(point, second, tolerance))
            || endpoints(second)
                .into_iter()
                .any(|point| point_on_profile_arc(point, first, tolerance));
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
    evaluator: &mut cadmpeg_ir::eval::decode::NurbsPointEvaluator<'_, '_>,
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

fn append_nurbs_profile_span(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    evaluator: &mut cadmpeg_ir::eval::decode::NurbsPointEvaluator<'_, '_>,
    nurbs: &NurbsCurve,
    span: &NurbsProfileSpan,
    points: &mut Vec<[f64; 2]>,
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    const MAX_DEPTH: usize = 24;
    const MAX_POINTS: usize = 262_145;
    let _depth = ctx.enter_nested("creo NURBS profile sampling depth")?;
    ctx.charge_work(1, "creo NURBS profile sampling spans")?;
    if !(span.start.is_finite() && span.end.is_finite() && span.start < span.end) {
        return Ok(None);
    }
    let middle = span.start + (span.end - span.start) * 0.5;
    if middle == span.start || middle == span.end {
        if points.len() >= MAX_POINTS {
            return Ok(None);
        }
        ctx.reserve_vec(points, 1, "creo NURBS profile polyline points")?;
        points.push(span.end_point);
        return Ok(Some(()));
    }
    let first_quarter = span.start + (span.end - span.start) * 0.25;
    let third_quarter = span.start + (span.end - span.start) * 0.75;
    let (Some(middle_point), Some(first_quarter_point), Some(third_quarter_point)) = (
        nurbs_profile_point(ctx, evaluator, nurbs, middle)?,
        nurbs_profile_point(ctx, evaluator, nurbs, first_quarter)?,
        nurbs_profile_point(ctx, evaluator, nurbs, third_quarter)?,
    ) else {
        return Ok(None);
    };
    let chord = [span.start_point, span.end_point];
    let flatness = planar_point_segment_distance(first_quarter_point, chord)
        .max(planar_point_segment_distance(middle_point, chord))
        .max(planar_point_segment_distance(third_quarter_point, chord));
    if !(flatness.is_finite() && span.tolerance.is_finite() && span.tolerance > 0.0) {
        return Ok(None);
    }
    if flatness <= span.tolerance {
        if points.len() >= MAX_POINTS {
            return Ok(None);
        }
        ctx.reserve_vec(points, 1, "creo NURBS profile polyline points")?;
        points.push(span.end_point);
        return Ok(Some(()));
    }
    if span.depth >= MAX_DEPTH {
        return Ok(None);
    }
    if append_nurbs_profile_span(
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
    )?
    .is_none()
    {
        return Ok(None);
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
) -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
    let Some(range) = nurbs_intrinsic_parameter_range(nurbs) else {
        return Ok(None);
    };
    let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(ctx, nurbs)?;
    let [lower, upper] = cadmpeg_ir::scalar::FiniteReal::raw_array(range);
    let Some(first) = nurbs_profile_point(ctx, &mut evaluator, nurbs, lower)? else {
        return Ok(None);
    };
    let mut points = Vec::new();
    ctx.reserve_vec(&mut points, 1, "creo NURBS profile polyline points")?;
    points.push(first);
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
            return Ok(None);
        };
        if points.last().copied() != Some(start_point) {
            ctx.reserve_vec(&mut points, 1, "creo NURBS profile polyline points")?;
            points.push(start_point);
        }
        if append_nurbs_profile_span(
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
        )?
        .is_none()
        {
            return Ok(None);
        }
    }
    Ok((points.len() >= 2).then_some(points))
}

fn profile_nurbs_polyline(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segment: &ProfileEntity,
    tolerance: f64,
) -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
    let Some(sketch) = segment.geometry.to_sketch(ctx)? else {
        return Ok(None);
    };
    let Some(nurbs) = oriented_sketch_nurbs_curve(ctx, &sketch, segment.reversed)? else {
        return Ok(None);
    };
    nurbs_profile_polyline(ctx, &nurbs, tolerance)
}

fn nurbs_profile_signed_area_twice(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &SketchGeometry,
    reversed: bool,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let Some(nurbs) = oriented_sketch_nurbs_curve(ctx, geometry, reversed)? else {
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
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::curve_point_for_decode(ctx, &carrier, parameter)?,
                )?,
                cadmpeg_ir::eval::finite_or_refusal(
                    cadmpeg_ir::eval::decode::curve_tangent_for_decode(ctx, &carrier, parameter)?,
                )?,
            ) else {
                return Ok(None);
            };
            area_twice += weight * (point.x * tangent.y - point.y * tangent.x) * half_width;
        }
    }
    Ok(area_twice.is_finite().then_some(area_twice))
}

fn polylines_intersect(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    first: &[[f64; 2]],
    second: &[[f64; 2]],
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    for first_segment in first.windows(2) {
        for second_segment in second.windows(2) {
            ctx.charge_work(1, "creo profile polyline intersection pairs")?;
            if segments_intersect(
                [first_segment[0], first_segment[1]],
                [second_segment[0], second_segment[1]],
                tolerance,
            ) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(in super::super) fn profile_segments_intersect(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    first: &ProfileEntity,
    second: &ProfileEntity,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.charge_work(1, "creo profile segment intersection")?;
    let first_nurbs = matches!(first.geometry, ProfileGeometry::Nurbs { .. });
    let second_nurbs = matches!(second.geometry, ProfileGeometry::Nurbs { .. });
    if first_nurbs || second_nurbs {
        if first_nurbs {
            if let Some(arc) = profile_arc(second) {
                let Some(polyline) = profile_nurbs_polyline(ctx, first, tolerance)? else {
                    return Ok(false);
                };
                for segment in polyline.windows(2) {
                    ctx.charge_work(1, "creo profile NURBS arc intersection segments")?;
                    if line_arc_intersect([segment[0], segment[1]], arc, tolerance) {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
        }
        if second_nurbs {
            if let Some(arc) = profile_arc(first) {
                let Some(polyline) = profile_nurbs_polyline(ctx, second, tolerance)? else {
                    return Ok(false);
                };
                for segment in polyline.windows(2) {
                    ctx.charge_work(1, "creo profile NURBS arc intersection segments")?;
                    if line_arc_intersect([segment[0], segment[1]], arc, tolerance) {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
        }
        let first_line = [first.start, first.end];
        let second_line = [second.start, second.end];
        let first_polyline = if first_nurbs {
            profile_nurbs_polyline(ctx, first, tolerance)?
        } else {
            None
        };
        if first_nurbs && first_polyline.is_none() {
            return Ok(true);
        }
        let second_polyline = if second_nurbs {
            profile_nurbs_polyline(ctx, second, tolerance)?
        } else {
            None
        };
        if second_nurbs && second_polyline.is_none() {
            return Ok(true);
        }
        return polylines_intersect(
            ctx,
            first_polyline.as_deref().unwrap_or(&first_line),
            second_polyline.as_deref().unwrap_or(&second_line),
            tolerance,
        );
    }
    Ok(match (profile_arc(first), profile_arc(second)) {
        (None, None) => segments_intersect(
            [first.start, first.end],
            [second.start, second.end],
            tolerance,
        ),
        (None, Some(arc)) => line_arc_intersect([first.start, first.end], arc, tolerance),
        (Some(arc), None) => line_arc_intersect([second.start, second.end], arc, tolerance),
        (Some(first), Some(second)) => arcs_intersect(first, second, tolerance),
    })
}

pub(in super::super) fn profile_strictly_contains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profile: &ExtrusionProfile,
    point: [f64; 2],
) -> Result<bool, cadmpeg_core::CodecError> {
    let scale = profile
        .iter()
        .flat_map(|ProfileEntity { start, end, .. }| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
    let mut winding = 0.0;
    for segment in profile {
        let mut accumulate = |first: [f64; 2], second: [f64; 2]| {
            let first = [first[0] - point[0], first[1] - point[1]];
            let second = [second[0] - point[0], second[1] - point[1]];
            winding += first[0]
                .mul_add(second[1], -(first[1] * second[0]))
                .atan2(first[0].mul_add(second[0], first[1] * second[1]));
        };
        if matches!(segment.geometry, ProfileGeometry::Nurbs { .. }) {
            let Some(polyline) = profile_nurbs_polyline(ctx, segment, tolerance)? else {
                return Ok(false);
            };
            for pair in polyline.windows(2) {
                ctx.charge_work(1, "creo profile NURBS winding segments")?;
                accumulate(pair[0], pair[1]);
            }
        } else if let Some((center, radius, start, delta)) = profile_arc(segment) {
            let Some(pieces) = cadmpeg_core::convert::truncate_f64_to_usize(
                (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0),
            ) else {
                return Ok(false);
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(pieces),
                "creo profile arc winding pieces",
            )?;
            for piece in 0..pieces {
                let first = start
                    + delta
                        * cadmpeg_core::convert::f64_from_index(piece).ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo winding segment index cannot be represented exactly",
                            )
                        })?
                        / cadmpeg_core::convert::f64_from_index(pieces).ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo winding segment index cannot be represented exactly",
                            )
                        })?;
                let second = start
                    + delta
                        * cadmpeg_core::convert::f64_from_index(piece + 1).ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo winding segment index cannot be represented exactly",
                            )
                        })?
                        / cadmpeg_core::convert::f64_from_index(pieces).ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo winding segment index cannot be represented exactly",
                            )
                        })?;
                accumulate(
                    [
                        center[0] + radius * first.cos(),
                        center[1] + radius * first.sin(),
                    ],
                    [
                        center[0] + radius * second.cos(),
                        center[1] + radius * second.sin(),
                    ],
                );
            }
        } else {
            ctx.charge_work(1, "creo profile line winding segments")?;
            accumulate(segment.start, segment.end);
        }
    }
    Ok(winding.abs() > std::f64::consts::PI)
}

pub(in super::super) fn ordered_extrusion_profiles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    profiles: Vec<ExtrusionProfile>,
) -> Result<Option<Vec<ValidatedProfile>>, cadmpeg_core::CodecError> {
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|ProfileEntity { start, end, .. }| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = EPS_GEOMETRY_AGREEMENT * scale;
    for profile in &profiles {
        for first in 0..profile.len() {
            for second in first + 1..profile.len() {
                if second == first + 1 || (first == 0 && second + 1 == profile.len()) {
                    continue;
                }
                ctx.charge_work(1, "creo profile self intersection pairs")?;
                if profile_segments_intersect(ctx, &profile[first], &profile[second], tolerance)? {
                    return Ok(None);
                }
            }
        }
    }
    for first in 0..profiles.len() {
        for second in first + 1..profiles.len() {
            for first_segment in &profiles[first] {
                for second_segment in &profiles[second] {
                    ctx.charge_work(1, "creo profile cross intersection pairs")?;
                    if profile_segments_intersect(ctx, first_segment, second_segment, tolerance)? {
                        return Ok(None);
                    }
                }
            }
        }
    }
    let mut outer = Vec::new();
    for (candidate, profile) in profiles.iter().enumerate() {
        let mut contains_all = true;
        for (index, inner) in profiles.iter().enumerate() {
            if index == candidate {
                continue;
            }
            ctx.charge_work(1, "creo outer profile containment pairs")?;
            if !profile_strictly_contains(ctx, profile, inner[0].start)? {
                contains_all = false;
                break;
            }
        }
        if contains_all {
            ctx.reserve_vec(&mut outer, 1, "creo outer extrusion profile candidates")?;
            outer.push(candidate);
        }
    }
    let [outer] = outer.as_slice() else {
        return Ok(None);
    };
    for first in 0..profiles.len() {
        if first == *outer {
            continue;
        }
        for second in first + 1..profiles.len() {
            if second == *outer {
                continue;
            }
            ctx.charge_work(1, "creo hole profile containment pairs")?;
            if profile_strictly_contains(ctx, &profiles[first], profiles[second][0].start)?
                || profile_strictly_contains(ctx, &profiles[second], profiles[first][0].start)?
            {
                return Ok(None);
            }
        }
    }
    let mut validated = Vec::new();
    for profile in profiles {
        let Some(profile) = ValidatedProfile::new(ctx, profile)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut validated, 1, "creo validated extrusion profiles")?;
        validated.push(profile);
    }
    let mut profiles = validated;
    let outer_area = profiles[*outer].area();
    if profiles.iter().enumerate().any(|(index, profile)| {
        index != *outer && profile.area().is_sign_positive() == outer_area.is_sign_positive()
    }) {
        return Ok(None);
    }
    profiles.swap(0, *outer);
    Ok(Some(profiles))
}

#[cfg(test)]
mod evaluation_tests;
