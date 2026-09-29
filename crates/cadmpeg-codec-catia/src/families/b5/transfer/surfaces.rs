// SPDX-License-Identifier: Apache-2.0
//! Surface-layer transfer: neutral surface lowering and the surface/procedural
//! emit pass.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_usize};
use cadmpeg_core::decode::u64_from_index;

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SupportPcurve, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::{B5Graph, B5Profile, B5Surface};
use super::super::vecmath::{add, components, coordinates, cross, scale};
use super::{
    annotate, dot, point3, subtract, RevolutionPlan, SurfacePlan, SurfaceProcedure, TransferPlan,
};
use crate::assemble::cgm_source;

/// Direct geometry or a procedural surface construction.
pub(super) enum B5SurfaceCarrier<'a> {
    Analytic(SurfaceGeometry),
    Procedural(B5ProceduralSurface<'a>),
}

/// Surface constructions that require procedural lowering.
pub(super) enum B5ProceduralSurface<'a> {
    Unresolved,
    RollingBall {
        carrier_object_id: u32,
        definition: &'a ProceduralSurfaceDefinition,
    },
    Revolution {
        profile_curve: u32,
        axis_origin: FinitePoint3,
        axis_direction: UnitVector3,
        angular_scale: PositiveReal,
        bounds: [[f64; 2]; 2],
    },
}

/// Classify a surface into its direct geometry or procedural construction.
pub(super) fn surface_carrier<'a>(
    ctx: &DecodeContext<'_>,
    surface: &'a B5Surface,
) -> Result<B5SurfaceCarrier<'a>, CodecError> {
    Ok(match surface {
        B5Surface::Plane { origin, frame, .. } => {
            B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::new(*origin, *frame),
            )))
        }
        B5Surface::Cylinder {
            origin,
            frame,
            radius,
            ..
        } => B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::new(*origin, *frame, *radius),
        ))),
        B5Surface::Cone { surface, .. } => surface.map_or(
            B5SurfaceCarrier::Procedural(B5ProceduralSurface::Unresolved),
            |surface| {
                B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                    surface,
                )))
            },
        ),
        B5Surface::Sphere {
            center,
            frame,
            radius,
            ..
        } => B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            cadmpeg_ir::geometry::analytic::SphereSurface::new(*center, *frame, (*radius).into()),
        ))),
        B5Surface::Torus {
            center,
            frame,
            major_radius,
            minor_radius,
            ..
        } => B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            cadmpeg_ir::geometry::analytic::TorusSurface::new(
                *center,
                *frame,
                *major_radius,
                (*minor_radius).into(),
            ),
        ))),
        B5Surface::Nurbs(surface) => {
            B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                surface.try_clone_for_decode(ctx, "catia_b5_surface_carrier_nurbs")?,
            )))
        }
        B5Surface::UnresolvedNurbs { .. } | B5Surface::Unknown { .. } => {
            B5SurfaceCarrier::Procedural(B5ProceduralSurface::Unresolved)
        }
        B5Surface::RollingBall {
            carrier_object_id,
            definition,
        } => B5SurfaceCarrier::Procedural(B5ProceduralSurface::RollingBall {
            carrier_object_id: *carrier_object_id,
            definition,
        }),
        B5Surface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            angular_scale,
            profile_range,
            angular_range,
            ..
        } => B5SurfaceCarrier::Procedural(B5ProceduralSurface::Revolution {
            profile_curve: *profile_curve,
            axis_origin: *axis_origin,
            axis_direction: *axis_direction,
            angular_scale: *angular_scale,
            bounds: [profile_range.endpoints(), angular_range.endpoints()],
        }),
    })
}

pub(super) fn neutral_surface(
    ctx: &DecodeContext<'_>,
    surface: &B5Surface,
    graph: &B5Graph,
    surface_id: u32,
    payload: &UnknownId,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<SurfacePlan, CodecError> {
    let carrier = match surface_carrier(ctx, surface)? {
        B5SurfaceCarrier::Analytic(geometry) => {
            return Ok(SurfacePlan {
                geometry,
                procedure: None,
            })
        }
        B5SurfaceCarrier::Procedural(carrier) => carrier,
    };
    if let Some(extrusion) = super::resolved_extrusion_surface(ctx, graph, surface_id, refusal)? {
        return Ok(SurfacePlan {
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(payload.try_clone_for_decode(ctx, "catia_b5_extrusion_unknown_id")?),
            }),
            // The resolved extrusion is retained by the surface plan.
            procedure: {
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<super::ResolvedExtrusionSurface>()),
                    "catia_b5_extrusion_procedure",
                )?;
                Some(SurfaceProcedure::Extrusion(Box::new(extrusion)))
            },
        });
    }
    let mut procedure = None;
    let geometry = match carrier {
        B5ProceduralSurface::Unresolved => {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(payload.try_clone_for_decode(ctx, "catia_b5_unresolved_unknown_id")?),
            })
        }
        B5ProceduralSurface::RollingBall {
            carrier_object_id,
            definition,
        } => {
            procedure = Some(SurfaceProcedure::RollingBall {
                carrier_object_id,
                definition: Box::new(copy_rolling_ball_definition(ctx, definition)?),
            });
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(
                    payload.try_clone_for_decode(ctx, "catia_b5_rolling_ball_unknown_id")?,
                ),
            })
        }
        B5ProceduralSurface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            angular_scale,
            bounds,
        } => match revolution_surface(
            ctx,
            graph.profiles.get(&profile_curve),
            (axis_origin, axis_direction),
            angular_scale,
            bounds,
            &format_args!("b5 revolution surface record #{surface_id}"),
            refusal,
        )? {
            Some((surface, plan)) => {
                procedure = Some(SurfaceProcedure::Revolution(plan));
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface))
            }
            None => SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(payload.try_clone_for_decode(ctx, "catia_b5_revolution_unknown_id")?),
            }),
        },
    };

    Ok(SurfacePlan {
        geometry,
        procedure,
    })
}

pub(in crate::families) fn copy_rolling_ball_definition(
    ctx: &DecodeContext<'_>,
    definition: &ProceduralSurfaceDefinition,
) -> Result<ProceduralSurfaceDefinition, CodecError> {
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return Err(CodecError::malformed(
            "B5 rolling-ball carrier requires a jet definition",
        ));
    };
    let stations = ctx.copy_retained_slice(jet.stations(), "catia_b5_rolling_ball_jet_stations")?;
    Ok(ProceduralSurfaceDefinition::RollingBallJet(
        cadmpeg_ir::geometry::RollingBallJetStations::from_admitted(jet.degree(), stations)
            .map_err(CodecError::malformed)?,
    ))
}

#[cfg(test)]
mod carrier_resource_tests {
    use super::{surface_carrier, B5SurfaceCarrier};
    use crate::families::b5::graph::B5Surface;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;

    #[test]
    fn nurbs_surface_carrier_refuses_collection_limit_below_copy_need() {
        let surface = B5Surface::Nurbs(
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2], None),
                false,
            )
            .expect("valid bilinear surface"),
        );
        let refused =
            crate::test_support::with_collection_limit(13, |ctx| surface_carrier(ctx, &surface));
        assert!(matches!(
            refused,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        let admitted =
            crate::test_support::with_service_context(|ctx| surface_carrier(ctx, &surface))
                .expect("service profile");
        assert!(matches!(admitted, B5SurfaceCarrier::Analytic(_)));
    }
}

pub(super) fn revolution_surface(
    ctx: &DecodeContext<'_>,
    profile: Option<&B5Profile>,
    axis: (FinitePoint3, UnitVector3),
    angular_scale: PositiveReal,
    bounds: [[f64; 2]; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<(NurbsSurface, RevolutionPlan)>, CodecError> {
    let (axis_origin, axis_direction) = axis;
    let Some(profile) = profile else {
        return Ok(None);
    };
    let [parameter_interval, native_angular_interval] = bounds;
    let Some(directrix) = profile_nurbs(ctx, profile, parameter_interval, record, refusal)? else {
        return Ok(None);
    };
    let angular_interval = [
        native_angular_interval[0] / angular_scale.get(),
        native_angular_interval[1] / angular_scale.get(),
    ];
    let surface = revolve_nurbs(
        ctx,
        &directrix,
        coordinates(axis_origin),
        components(&axis_direction),
        [angular_interval, native_angular_interval],
        record,
        refusal,
    )?;
    let Some(surface) = surface else {
        return Ok(None);
    };
    Ok(Some((
        surface,
        RevolutionPlan {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval: native_angular_interval,
            parameter_interval,
        },
    )))
}

fn profile_nurbs(
    ctx: &DecodeContext<'_>,
    profile: &B5Profile,
    interval: [f64; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<NurbsCurve>, CodecError> {
    if !profile
        .parameter_range()
        .endpoints()
        .into_iter()
        .zip(interval)
        .all(|(profile, surface)| profile.to_bits() == surface.to_bits())
    {
        return Ok(None);
    }
    Ok(match profile {
        B5Profile::Line {
            point, direction, ..
        } => crate::nurbs::note_refusal(
            ctx,
            NurbsCurve::from_lanes(
                1,
                ctx.collect_vec(
                    [interval[0], interval[0], interval[1], interval[1]],
                    "catia_b5_revolution_line_profile_knots",
                )?,
                ctx.collect_vec(
                    interval.into_iter().map(|parameter| {
                        point3(add(coordinates(*point), scale(direction.get(), parameter)))
                    }),
                    "catia_b5_revolution_line_profile_points",
                )?,
                None,
                false,
            ),
            refusal,
            format_args!("b5 line profile of a revolution surface: {record}"),
        )?,
        B5Profile::Arc {
            center,
            direction_x,
            direction_y,
            radius,
            ..
        } => rational_arc(
            ctx,
            coordinates(*center),
            (direction_x.get(), direction_y.get()),
            radius.get(),
            interval,
            record,
            refusal,
        )?,
    })
}

pub(super) fn rational_arc(
    ctx: &DecodeContext<'_>,
    center: [f64; 3],
    (direction_x, direction_y): ([f64; 3], [f64; 3]),
    radius: f64,
    interval: [f64; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<NurbsCurve>, CodecError> {
    let angles = [interval[0] / radius, interval[1] / radius];
    let span_count = ((angles[1] - angles[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS {
        return Ok(None);
    }
    // `ceil` answers zero only for an angular span of exactly zero: an arc that
    // sweeps no angle states no span, which this route refuses as it refuses
    // every other degeneracy.
    let Some(span_count) = truncate_f64_to_usize(span_count).and_then(std::num::NonZeroUsize::new)
    else {
        return Ok(None);
    };
    let span_count = span_count.get();
    let Some(control_count) = span_count
        .checked_mul(2)
        .and_then(|count| count.checked_add(1))
    else {
        return Ok(None);
    };
    let Some(knot_count) = control_count.checked_add(3) else {
        return Ok(None);
    };
    let mut control_points = Vec::new();
    ctx.reserve_vec(
        &mut control_points,
        control_count,
        "catia_b5_revolution_arc_points",
    )?;
    let mut weights = Vec::new();
    ctx.reserve_vec(
        &mut weights,
        control_count,
        "catia_b5_revolution_arc_weights",
    )?;
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, knot_count, "catia_b5_revolution_arc_knots")?;
    for span in 0..span_count {
        let fraction0 = match f64_from_index(span) {
            Some(value) => value,
            None => return Ok(None),
        } / match f64_from_index(span_count) {
            Some(value) => value,
            None => return Ok(None),
        };
        let fraction1 = match f64_from_index(span + 1) {
            Some(value) => value,
            None => return Ok(None),
        } / match f64_from_index(span_count) {
            Some(value) => value,
            None => return Ok(None),
        };
        let angle0 = angles[0] + (angles[1] - angles[0]) * fraction0;
        let angle1 = angles[0] + (angles[1] - angles[0]) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return Ok(None);
        }
        if span == 0 {
            control_points.push(point3(circle_point(
                center,
                direction_x,
                direction_y,
                radius,
                angle0,
            )));
            weights.push(1.0);
        }
        control_points.push(point3(circle_point(
            center,
            direction_x,
            direction_y,
            radius / middle_weight,
            middle,
        )));
        weights.push(middle_weight);
        control_points.push(point3(circle_point(
            center,
            direction_x,
            direction_y,
            radius,
            angle1,
        )));
        weights.push(1.0);
        if append_quadratic_span_knots(&mut knots, interval, span, span_count).is_none() {
            return Ok(None);
        }
    }
    crate::nurbs::note_refusal(
        ctx,
        NurbsCurve::from_lanes(2, knots, control_points, Some(weights), false),
        refusal,
        format_args!("b5 rational arc profile of a revolution surface: {record}"),
    )
}

pub(super) fn revolve_nurbs(
    ctx: &DecodeContext<'_>,
    profile: &NurbsCurve,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    intervals: [[f64; 2]; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<NurbsSurface>, CodecError> {
    let [angular_interval, native_interval] = intervals;
    (|| -> Option<Result<NurbsSurface, CodecError>> {
        let span_count = ((angular_interval[1] - angular_interval[0]).abs()
            / std::f64::consts::FRAC_PI_2)
            .ceil();
        if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS {
            return None;
        }
        // `ceil` answers zero only for an angular span of exactly zero: an arc that
        // sweeps no angle states no span, which this route refuses as it refuses
        // every other degeneracy.
        let span_count = std::num::NonZeroUsize::new(truncate_f64_to_usize(span_count)?)?.get();
        let angular_count = span_count.checked_mul(2)?.checked_add(1)?;
        let control_count =
            crate::nurbs_surface_control_count(profile.control_points().len(), angular_count)?;
        if let Err(error) =
            ctx.charge_collection_items(u64_from_index(angular_count), "catia b5 revolution angles")
        {
            return Some(Err(error));
        }
        let mut angles = Vec::new();
        if let Err(error) = cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut angles,
            angular_count,
            "catia b5 revolution angles",
        ) {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(angular_count),
            "catia b5 revolution angular weights",
        ) {
            return Some(Err(error));
        }
        let mut angular_weights = Vec::new();
        if let Err(error) = cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut angular_weights,
            angular_count,
            "catia b5 revolution angular weights",
        ) {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(angular_count + 3),
            "catia b5 revolution angular knots",
        ) {
            return Some(Err(error));
        }
        let mut v_knots = Vec::new();
        if let Err(error) = cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut v_knots,
            angular_count + 3,
            "catia b5 revolution angular knots",
        ) {
            return Some(Err(error));
        }
        for span in 0..span_count {
            let fraction0 = f64_from_index(span)? / f64_from_index(span_count)?;
            let fraction1 = f64_from_index(span + 1)? / f64_from_index(span_count)?;
            let angle0 =
                angular_interval[0] + (angular_interval[1] - angular_interval[0]) * fraction0;
            let angle1 =
                angular_interval[0] + (angular_interval[1] - angular_interval[0]) * fraction1;
            let middle = (angle0 + angle1) * 0.5;
            let middle_weight = ((angle1 - angle0) * 0.5).cos();
            if middle_weight <= f64::EPSILON {
                return None;
            }
            if span == 0 {
                angles.push((angle0, 1.0));
                angular_weights.push(1.0);
            }
            angles.push((middle, 1.0 / middle_weight));
            angular_weights.push(middle_weight);
            angles.push((angle1, 1.0));
            angular_weights.push(1.0);
            append_quadratic_span_knots(&mut v_knots, native_interval, span, span_count)?;
        }
        let profile_weights = match profile.pole_rows() {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                match ctx.collect_vec(
                    points.iter().map(|point| point.weight.get()),
                    "catia b5 revolution profile weights",
                ) {
                    Ok(weights) => weights,
                    Err(error) => return Some(Err(error)),
                }
            }
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { .. } => match ctx.alloc_filled(
                profile.control_points().len(),
                1.0,
                "catia b5 revolution profile weights",
            ) {
                Ok(weights) => weights,
                Err(error) => return Some(Err(error)),
            },
        };
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(control_count),
            "catia b5 revolution control net",
        ) {
            return Some(Err(error));
        }
        let mut control_points = Vec::new();
        if let Err(error) = cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut control_points,
            control_count,
            "catia b5 revolution control net",
        ) {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(control_count),
            "catia b5 revolution net weights",
        ) {
            return Some(Err(error));
        }
        let mut weights = Vec::new();
        if let Err(error) = cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut weights,
            control_count,
            "catia b5 revolution net weights",
        ) {
            return Some(Err(error));
        }
        for (profile_point, profile_weight) in profile.control_points().iter().zip(profile_weights)
        {
            let relative = [
                profile_point.x - axis_origin[0],
                profile_point.y - axis_origin[1],
                profile_point.z - axis_origin[2],
            ];
            let axial = scale(axis_direction, dot(relative, axis_direction));
            let radial = subtract(relative, axial);
            for ((angle, radial_scale), angular_weight) in
                angles.iter().copied().zip(angular_weights.iter().copied())
            {
                let rotated = rotate_vector(radial, axis_direction, angle);
                control_points.push(point3(add(
                    axis_origin,
                    add(axial, scale(rotated, radial_scale)),
                )));
                weights.push(profile_weight * angular_weight);
            }
        }
        let row_len = angular_count;
        let row_count = profile.control_points().len();
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(profile.knots().len()),
            "catia b5 revolution profile knots",
        ) {
            return Some(Err(error));
        }
        if let Err(error) =
            ctx.charge_collection_items(u64_from_index(row_count), "catia b5 revolution point rows")
        {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(control_count),
            "catia b5 revolution point row values",
        ) {
            return Some(Err(error));
        }
        if let Err(error) = ctx
            .charge_collection_items(u64_from_index(row_count), "catia b5 revolution weight rows")
        {
            return Some(Err(error));
        }
        if let Err(error) = ctx.charge_collection_items(
            u64_from_index(control_count),
            "catia b5 revolution weight row values",
        ) {
            return Some(Err(error));
        }
        let profile_knots = match cadmpeg_core::decode::DecodeContext::copy_admitted_slice(
            profile.knots().as_slice(),
            "catia b5 revolution profile knots",
        ) {
            Ok(knots) => knots,
            Err(error) => return Some(Err(error)),
        };
        let point_rows = match cadmpeg_core::decode::DecodeContext::copy_admitted_rows(
            &control_points,
            row_len,
            "catia b5 revolution point rows",
        ) {
            Ok(rows) => rows,
            Err(error) => return Some(Err(error)),
        };
        let weight_rows = match cadmpeg_core::decode::DecodeContext::copy_admitted_rows(
            &weights,
            row_len,
            "catia b5 revolution weight rows",
        ) {
            Ok(rows) => rows,
            Err(error) => return Some(Err(error)),
        };
        let surface = match crate::nurbs::note_refusal(
            ctx,
            NurbsSurface::from_lanes(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    profile.degree(),
                    profile_knots,
                    false,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(2, v_knots, false),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(point_rows, Some(weight_rows)),
                false,
            ),
            refusal,
            format_args!("b5 revolution surface built from its profile: {record}"),
        ) {
            Ok(Some(surface)) => surface,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        Some(Ok(surface))
    })()
    .transpose()
}

fn append_quadratic_span_knots(
    knots: &mut Vec<f64>,
    interval: [f64; 2],
    span: usize,
    span_count: usize,
) -> Option<()> {
    let at = |index: usize| {
        let fraction = f64_from_index(index)? / f64_from_index(span_count)?;
        let ordinary = interval[0] + (interval[1] - interval[0]) * fraction;
        if ordinary.is_finite() {
            Some(ordinary)
        } else {
            cadmpeg_ir::math::interpolate(interval[0], interval[1], fraction)
                .map(cadmpeg_ir::scalar::FiniteReal::get)
        }
    };
    let start = at(span)?;
    let end = at(span + 1)?;
    if span == 0 {
        knots.extend([start, start, start]);
    } else {
        knots.extend([start, start]);
    }
    if span + 1 == span_count {
        knots.extend([end, end, end]);
    }
    Some(())
}

fn circle_point(
    center: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
    radius: f64,
    angle: f64,
) -> [f64; 3] {
    add(
        center,
        scale(
            add(
                scale(direction_x, angle.cos()),
                scale(direction_y, angle.sin()),
            ),
            radius,
        ),
    )
}

fn rotate_vector(value: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    add(
        add(
            scale(value, angle.cos()),
            scale(cross(axis, value), angle.sin()),
        ),
        scale(axis, dot(axis, value) * (1.0 - angle.cos())),
    )
}

/// Emit the referenced surfaces, their procedural definitions, and the offset
/// procedural surfaces, returning the map from `object_id` to emitted
/// [`SurfaceId`]. Consumes the planned surfaces out of the transfer plan.
pub(super) fn emit_surfaces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &mut TransferPlan,
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<HashMap<u32, SurfaceId>, cadmpeg_core::CodecError> {
    let surface_plan: BTreeMap<u32, SurfacePlan> = std::mem::take(&mut plan.surface_plan);
    let namespace = cadmpeg_ir::identity_namespace!("catia", "b5", "surface");
    let mut surface_ids = HashMap::new();
    for object_id in surface_plan.keys().copied() {
        let index = usize::try_from(object_id).map_err(|_| {
            admission.context().refuse_codec_limit(
                "catia_b5_emitted_surface_id",
                u64::MAX,
                u64::MAX,
            )
        })?;
        let id = crate::resource::compose_index_id(
            admission.context(),
            &namespace,
            index,
            SurfaceId::mint,
            "catia_b5_emitted_surface_id",
        )?;
        admission.context().insert_hash_map(
            &mut surface_ids,
            object_id,
            id,
            "catia_b5_emitted_surface_ids",
        )?;
    }
    let mut face_surfaces = HashSet::new();
    for face in &graph.faces {
        admission.context().insert_hash_set(
            &mut face_surfaces,
            face.surface,
            "catia_b5_face_surface_ids",
        )?;
    }
    for (object_id, plan) in surface_plan {
        let id = surface_ids[&object_id]
            .try_clone_for_decode(admission.context(), "catia_b5_emitted_surface_ref")?;
        let revolution_cache = matches!(
            plan.procedure.as_ref(),
            Some(SurfaceProcedure::Revolution(_))
        );
        let rolling_ball_carrier = matches!(
            plan.procedure.as_ref(),
            Some(SurfaceProcedure::RollingBall { .. })
        );
        let exact_procedural_carrier = rolling_ball_carrier
            || matches!(
                plan.procedure.as_ref(),
                Some(SurfaceProcedure::Extrusion(_))
            );
        annotate(
            admission.context(),
            annotations,
            &id,
            "object_stream_b5_03",
            if face_surfaces.contains(&object_id) {
                "face_surface"
            } else {
                "construction_surface"
            },
            if exact_procedural_carrier {
                Exactness::ByteExact
            } else if matches!(
                plan.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            ) {
                Exactness::Unknown
            } else if revolution_cache {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        if revolution_cache {
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                id.as_str(),
                "geometry",
                "catia_b5_surface_annotation",
            )?;
        }
        let model_id = id.try_clone_for_decode(admission.context(), "catia_b5_model_surface_id")?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_b5_emit_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: model_id,
            geometry: plan.geometry,
            source_object: Some(cgm_source(admission.context(), "surface", object_id)?),
        });
        match plan.procedure {
            Some(SurfaceProcedure::Extrusion(extrusion)) => {
                emit_extrusion_procedure(
                    ir,
                    annotations,
                    &surface_ids,
                    &id,
                    object_id,
                    *extrusion,
                    admission,
                )?;
            }
            Some(SurfaceProcedure::Revolution(revolution)) => {
                let directrix_id = crate::resource::compose_u32_id(
                    admission.context(),
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "profile"),
                    object_id,
                    CurveId::mint,
                    "catia_b5_profile_id",
                )?;
                annotate(
                    admission.context(),
                    annotations,
                    &directrix_id,
                    "object_stream_b5_03",
                    "2d_profile_curve",
                    Exactness::Derived,
                )?;
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    directrix_id.as_str(),
                    "geometry",
                    "catia_b5_profile_annotation",
                )?;
                admission.reserve_entity(&mut ir.model.curves, "catia_b5_emit_curves")?;
                ir.model.curves.push(Curve {
                    id: directrix_id.try_clone_for_decode(
                        admission.context(),
                        "catia_b5_profile_curve_record_id",
                    )?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        revolution.directrix,
                    )),
                    source_object: None,
                });
                let procedural_id = crate::resource::compose_u32_id(
                    admission.context(),
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "procedural-surface"),
                    object_id,
                    ProceduralSurfaceId::mint,
                    "catia_b5_procedural_surface_id",
                )?;
                annotate(
                    admission.context(),
                    annotations,
                    &procedural_id,
                    "object_stream_b5_03",
                    "2d_surface_of_revolution",
                    Exactness::Derived,
                )?;
                admission.reserve_entity(
                    &mut ir.model.procedural_surfaces,
                    "catia_b5_emit_procedural_surfaces",
                )?;
                let _attached = ir.model.add_procedural_surface(
                    &id,
                    cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                        directrix_id,
                        (revolution.axis_origin, revolution.axis_direction),
                        revolution.angular_interval,
                        Some(revolution.angular_parameter_interval),
                        Some(revolution.parameter_interval),
                        false,
                        cadmpeg_ir::geometry::CacheContract::from_form(None),
                    )
                    .map(|admitted_payload| {
                        ProceduralSurface::new(
                            procedural_id,
                            ProceduralSurfaceDefinition::Revolution(admitted_payload),
                            None,
                        )
                    })
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                );
            }
            Some(SurfaceProcedure::RollingBall {
                carrier_object_id,
                definition,
            }) if graph
                .canonical_surface_id(object_id)
                .is_some_and(|id| !graph.offset_surfaces.contains_key(&id)) =>
            {
                let procedural_id = crate::resource::compose_u32_id(
                    admission.context(),
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "rolling-ball"),
                    object_id,
                    ProceduralSurfaceId::mint,
                    "catia_b5_rolling_ball_id",
                )?;
                let carrier_tag = admission.context().format_retained(
                    format_args!("result_carrier:{carrier_object_id:08x}"),
                    "catia_b5_rolling_ball_carrier_tag",
                )?;
                annotate(
                    admission.context(),
                    annotations,
                    &procedural_id,
                    "object_stream_a8_03_32",
                    &carrier_tag,
                    Exactness::ByteExact,
                )?;
                admission.reserve_entity(
                    &mut ir.model.procedural_surfaces,
                    "catia_b5_emit_procedural_surfaces",
                )?;
                let _attached = ir.model.add_procedural_surface(
                    &id,
                    ProceduralSurface::new(procedural_id, *definition, None),
                );
            }
            Some(SurfaceProcedure::RollingBall { .. }) | None => {}
        }
    }
    for &object_id in surface_ids.keys() {
        let Some(construction_id) = graph.canonical_surface_id(object_id) else {
            continue;
        };
        let Some(offset) = graph.offset_surfaces.get(&construction_id) else {
            continue;
        };
        let (Some(surface), Some(support)) = (
            surface_ids.get(&object_id),
            surface_ids.get(&offset.source_surface),
        ) else {
            continue;
        };
        let procedural_id = crate::resource::compose_u32_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "offset"),
            object_id,
            ProceduralSurfaceId::mint,
            "catia_b5_offset_id",
        )?;
        annotate(
            admission.context(),
            annotations,
            &procedural_id,
            "object_stream_b5_03",
            "30_offset_surface",
            Exactness::Derived,
        )?;
        let record_bounds = super::parameter_record_bounds(offset.parameter_bounds);
        admission.reserve_entity(
            &mut ir.model.procedural_surfaces,
            "catia_b5_emit_procedural_surfaces",
        )?;
        let _attached = ir.model.add_procedural_surface(
            &surface.try_clone_for_decode(admission.context(), "catia_b5_offset_surface_id")?,
            ProceduralSurface::new(
                procedural_id,
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                        support.try_clone_for_decode(
                            admission.context(),
                            "catia_b5_offset_support_id",
                        )?,
                        offset.distance,
                        None,
                        None,
                        false,
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                        None,
                    ),
                ),
                Some(record_bounds),
            ),
        );
    }
    Ok(surface_ids)
}

fn emit_extrusion_procedure(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    surface_ids: &HashMap<u32, SurfaceId>,
    surface_id: &SurfaceId,
    surface_object_id: u32,
    extrusion: super::ResolvedExtrusionSurface,
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let directrix_id = crate::resource::compose_u32_id(
        admission.context(),
        &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix"),
        extrusion.directrix_object_id,
        CurveId::mint,
        "catia_b5_extrusion_directrix_id",
    )?;
    match extrusion.directrix {
        super::ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            let make_side = |side: super::ResolvedExtrusionSupport| {
                Ok::<_, cadmpeg_core::CodecError>(IntcurveSupportSide {
                    surface: Some(surface_ids[&side.surface_object_id].try_clone_for_decode(
                        admission.context(),
                        "catia_b5_extrusion_support_surface_id",
                    )?),
                    pcurve: Some(SupportPcurve::new(
                        side.pcurve,
                        (side.pcurve_parameter_range
                            != extrusion.directrix_parameter_range.endpoints())
                        .then(|| DirectedParameterRange::new(side.pcurve_parameter_range).ok())
                        .flatten(),
                    )),
                })
            };
            let [first, second] = *supports;
            let sides = [make_side(first)?, make_side(second)?];
            annotate(
                admission.context(),
                annotations,
                &directrix_id,
                "object_stream_a8_03_25",
                "two_support_directrix",
                Exactness::Unknown,
            )?;
            admission.reserve_entity(&mut ir.model.curves, "catia_b5_emit_curves")?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(
                    admission.context(),
                    "catia_b5_extrusion_directrix_record_id",
                )?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source(
                    admission.context(),
                    "curve",
                    extrusion.directrix_object_id,
                )?),
            });
            let procedure_id = crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix-procedure"),
                extrusion.directrix_object_id,
                ProceduralCurveId::mint,
                "catia_b5_extrusion_directrix_procedure_id",
            )?;
            annotate(
                admission.context(),
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            )?;
            let procedure = ProceduralCurve::new(
                procedure_id,
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::over_interval(
                        sides,
                        extrusion.directrix_parameter_range,
                    ),
                    discontinuity_flag: false,
                    cache: Some(cadmpeg_ir::geometry::LegacyCache::new(
                        cadmpeg_ir::scalar::NonNegativeReal::from(cache_fit_tolerance).into(),
                    )),
                },
            );

            admission.reserve_entity(
                &mut ir.model.procedural_curves,
                "catia_b5_emit_procedural_curves",
            )?;
            let _attached = ir.model.add_procedural_curve_charged(
                admission.context(),
                &directrix_id.try_clone_for_decode(
                    admission.context(),
                    "catia_b5_extrusion_procedure_owner_id",
                )?,
                procedure,
            )?;
        }
        super::ResolvedExtrusionDirectrix::SurfaceCurve { curve, .. } => {
            annotate(
                admission.context(),
                annotations,
                &directrix_id,
                "object_stream_b5_03_24",
                "support_pcurve_lift",
                Exactness::Derived,
            )?;
            admission.reserve_entity(&mut ir.model.curves, "catia_b5_emit_curves")?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(
                    admission.context(),
                    "catia_b5_extrusion_directrix_record_id",
                )?,
                geometry: curve,
                source_object: Some(cgm_source(
                    admission.context(),
                    "curve",
                    extrusion.directrix_object_id,
                )?),
            });
        }
        super::ResolvedExtrusionDirectrix::Offset {
            source_object_id,
            support,
            source_curve,
            source_parameter_range,
            distance,
            direction,
        } => {
            let source_id = crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix-source"),
                source_object_id,
                CurveId::mint,
                "catia_b5_extrusion_directrix_source_id",
            )?;
            annotate(
                admission.context(),
                annotations,
                &source_id,
                "object_stream_b5_03_24",
                "support_pcurve_lift",
                Exactness::Derived,
            )?;
            admission.reserve_entity(&mut ir.model.curves, "catia_b5_emit_curves")?;
            ir.model.curves.push(Curve {
                id: source_id.try_clone_for_decode(
                    admission.context(),
                    "catia_b5_extrusion_source_record_id",
                )?,
                geometry: source_curve,
                source_object: Some(cgm_source(admission.context(), "curve", source_object_id)?),
            });
            annotate(
                admission.context(),
                annotations,
                &directrix_id,
                "object_stream_b5_03_14",
                "fixed_direction_offset_curve",
                Exactness::Unknown,
            )?;
            admission.reserve_entity(&mut ir.model.curves, "catia_b5_emit_curves")?;
            ir.model.curves.push(Curve {
                id: directrix_id.try_clone_for_decode(
                    admission.context(),
                    "catia_b5_extrusion_directrix_record_id",
                )?,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source(
                    admission.context(),
                    "curve",
                    extrusion.directrix_object_id,
                )?),
            });
            let procedure_id = crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix-procedure"),
                extrusion.directrix_object_id,
                ProceduralCurveId::mint,
                "catia_b5_extrusion_directrix_procedure_id",
            )?;
            annotate(
                admission.context(),
                annotations,
                &procedure_id,
                "object_stream_b5_03_14",
                "fixed_direction_offset_curve",
                Exactness::ByteExact,
            )?;
            admission.reserve_entity(
                &mut ir.model.procedural_curves,
                "catia_b5_emit_procedural_curves",
            )?;
            let _attached = ir.model.add_procedural_curve_charged(admission.context(),
                &directrix_id.try_clone_for_decode(admission.context(), "catia_b5_extrusion_procedure_owner_id")?,
                ProceduralCurve::new(
                    procedure_id,
                    ProceduralCurveDefinition::Offset(
                        cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::along_direction(
                            source_id,
                            distance,
                            direction,
                            Some(surface_ids[&support.surface_object_id].try_clone_for_decode(admission.context(), "catia_b5_extrusion_support_surface_id")?),
                            source_parameter_range,
                        ),
                    ),
                ),
            )?;
        }
    }
    let procedure_id = crate::resource::compose_u32_id(
        admission.context(),
        &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion"),
        surface_object_id,
        ProceduralSurfaceId::mint,
        "catia_b5_extrusion_id",
    )?;
    annotate(
        admission.context(),
        annotations,
        &procedure_id,
        "object_stream_b5_03",
        "2c_extrusion_surface",
        Exactness::ByteExact,
    )?;
    let record_bounds = super::parameter_record_bounds(extrusion.parameter_bounds);
    admission.reserve_entity(
        &mut ir.model.procedural_surfaces,
        "catia_b5_emit_procedural_surfaces",
    )?;
    let _attached = ir.model.add_procedural_surface(
        surface_id,
        ProceduralSurface::new(
            procedure_id,
            ProceduralSurfaceDefinition::Extrusion(
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
                    directrix_id,
                    Some(extrusion.directrix_parameter_range.into()),
                    extrusion.direction.into(),
                    None,
                    None,
                ),
            ),
            Some(record_bounds),
        ),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{append_quadratic_span_knots, emit_extrusion_procedure, revolve_nurbs};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::HashMap;

    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use cadmpeg_ir::math::{Point2, Vector3};

    use crate::families::b5::transfer::{
        ResolvedExtrusionDirectrix, ResolvedExtrusionSupport, ResolvedExtrusionSurface,
    };

    #[test]
    fn revolution_profile_weights_propagate_collection_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;
        use cadmpeg_ir::math::Point3;

        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 1.0)],
            None,
            false,
        )
        .expect("valid revolution profile");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three angular points, three weights, and six knots precede the profile weights.
        policy.limits.max_collection_items = 12;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let error = revolve_nurbs(
            &ctx,
            &profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [[0.0, std::f64::consts::FRAC_PI_2], [0.0, 1.0]],
            &"test record",
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect_err("profile weights exceed the collection limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia b5 revolution profile weights"));
    }

    #[test]
    fn revolution_control_rows_refuse_collection_limit_before_nested_copy() {
        use cadmpeg_ir::geometry::nurbs::NurbsCurve;
        use cadmpeg_ir::math::Point3;

        let profile = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 1.0)],
            None,
            false,
        )
        .expect("valid revolution profile");
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            revolve_nurbs(
                ctx,
                &profile,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                [[0.0, std::f64::consts::FRAC_PI_2], [0.0, 1.0]],
                &"test record",
                &mut crate::nurbs::LaneRefusals::new(),
            )
        };
        let refused = crate::test_support::with_collection_limit(37, run);
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia b5 revolution point row values")
        );
        let admitted =
            crate::test_support::with_service_context(run).expect("service resource budget");
        assert!(admitted.is_some());
    }

    #[test]
    fn quadratic_span_knots_remain_finite_across_a_wide_native_interval() {
        let mut knots = Vec::new();
        for span in 0..2 {
            append_quadratic_span_knots(&mut knots, [-f64::MAX, f64::MAX], span, 2)
                .expect("finite interval maps to finite knots");
        }
        assert_eq!(
            knots,
            vec![
                -f64::MAX,
                -f64::MAX,
                -f64::MAX,
                0.0,
                0.0,
                f64::MAX,
                f64::MAX,
                f64::MAX,
            ]
        );
    }

    #[test]
    fn extrusion_emits_exact_two_support_intersection() {
        let support_ids = HashMap::from([
            (
                10,
                SurfaceId::mint("catia:test:surface#support-10".to_string())
                    .expect("identity grammar"),
            ),
            (
                20,
                SurfaceId::mint("catia:test:surface#support-20".to_string())
                    .expect("identity grammar"),
            ),
        ]);
        let pcurve = |x| PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(x, 0.0), Point2::new(x, 1.0)],
                None,
                false,
            )
            .expect("valid support pcurve"),
        };
        let extrusion = ResolvedExtrusionSurface {
            surface_object_id: 30,
            directrix_object_id: 40,
            directrix_parameter_range: crate::test_support::test_b5::increasing([0.0, 1.0]),
            direction: crate::test_support::test_b5::unit([0.0, 0.0, 1.0]),
            parameter_bounds: crate::test_support::test_b5::increasing_bounds([
                [-2.0, 3.0],
                [0.0, 1.0],
            ]),
            directrix: ResolvedExtrusionDirectrix::Intersection {
                cache_fit_tolerance: crate::test_support::test_b5::positive(1e-5),
                supports: Box::new([
                    ResolvedExtrusionSupport {
                        surface_object_id: 10,
                        surface: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                                Vector3::new(1.0, 0.0, 0.0),
                                Vector3::new(0.0, 1.0, 0.0),
                            )
                            .expect("valid PlaneSurface fixture"),
                        )),
                        pcurve: pcurve(0.0),
                        pcurve_parameter_range: [0.0, 1.0],
                        curve: None,
                    },
                    ResolvedExtrusionSupport {
                        surface_object_id: 20,
                        surface: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                                Vector3::new(0.0, 1.0, 0.0),
                                Vector3::new(1.0, 0.0, 0.0),
                            )
                            .expect("valid PlaneSurface fixture"),
                        )),
                        pcurve: pcurve(1.0),
                        pcurve_parameter_range: [0.25, 0.75],
                        curve: None,
                    },
                ]),
            },
        };
        let mut ir = CadIr::empty();
        let surface_id = SurfaceId::mint("catia:test:surface#result-30").expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });

        crate::test_support::with_service_context(|ctx| {
            let mut admission = crate::families::FamilyEntityAdmission::new(ctx);
            emit_extrusion_procedure(
                &mut ir,
                &mut AnnotationBuilder::new(),
                &support_ids,
                &surface_id,
                30,
                extrusion,
                &mut admission,
            )
            .expect("service limits admit the extrusion procedure");
        });

        assert!(matches!(
            &ir.model.curves[0].geometry,
            CurveGeometry::Procedural { construction, cache: Some(cache) }
                if *construction == ir.model.procedural_curves[0].id
                    && matches!(cache, SolvedCurveGeometry::Unknown { record: None })
        ));
        let ProceduralCurveDefinition::Intersection { context, .. } =
            ir.model.procedural_curves[0].definition()
        else {
            panic!("expected intersection directrix");
        };
        assert_eq!(context.parameter_range().endpoints(), [0.0, 1.0]);
        assert_eq!(context.sides()[0].surface, Some(support_ids[&10].clone()));
        assert_eq!(
            context.sides()[0]
                .pcurve_parameter_range()
                .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints),
            None
        );
        assert_eq!(context.sides()[1].surface, Some(support_ids[&20].clone()));
        assert_eq!(
            context.sides()[1]
                .pcurve_parameter_range()
                .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints),
            Some([0.25, 0.75])
        );
        assert_eq!(
            ir.model.procedural_curves[0]
                .cache_fit_tolerance()
                .map(cadmpeg_ir::geometry::FitTolerance::get),
            Some(1e-5)
        );
        assert!(match ir.model.procedural_surfaces[0].definition() {
            ProceduralSurfaceDefinition::Extrusion(matched_payload) =>
                matches!((&matched_payload.parameter_interval().map(cadmpeg_ir::units::FiniteVector::get), matched_payload.direction(), &matched_payload.native_position(),), (Some([0.0, 1.0]), direction, None,) if *direction == Vector3::new(0.0, 0.0, 1.0)),
            _ => false,
        });
        assert_eq!(
            ir.model.procedural_surfaces[0]
                .record_bounds()
                .map(cadmpeg_ir::geometry::RecordBounds::get),
            Some([Some(-2.0), Some(3.0), Some(0.0), Some(1.0)])
        );
    }
}
