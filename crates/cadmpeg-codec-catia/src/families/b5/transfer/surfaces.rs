// SPDX-License-Identifier: Apache-2.0
//! Surface-layer transfer: neutral surface lowering and the surface/procedural
//! emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::alloc_filled;
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
pub(super) fn surface_carrier(surface: &B5Surface) -> B5SurfaceCarrier<'_> {
    match surface {
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
        B5Surface::Nurbs(surface) => B5SurfaceCarrier::Analytic(SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Nurbs(surface.clone()),
        )),
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
            bounds: [*profile_range, *angular_range],
        }),
    }
}

pub(super) fn neutral_surface(
    surface: &B5Surface,
    graph: &B5Graph,
    surface_id: u32,
    payload: &UnknownId,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> SurfacePlan {
    let carrier = match surface_carrier(surface) {
        B5SurfaceCarrier::Analytic(geometry) => {
            return SurfacePlan {
                geometry,
                procedure: None,
            }
        }
        B5SurfaceCarrier::Procedural(carrier) => carrier,
    };
    if let Some(extrusion) = super::resolved_extrusion_surface(graph, surface_id, refusal) {
        return SurfacePlan {
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            }),
            procedure: Some(SurfaceProcedure::Extrusion(Box::new(extrusion))),
        };
    }
    let mut procedure = None;
    let geometry = match carrier {
        B5ProceduralSurface::Unresolved => {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            })
        }
        B5ProceduralSurface::RollingBall {
            carrier_object_id,
            definition,
        } => {
            procedure = Some(SurfaceProcedure::RollingBall {
                carrier_object_id,
                definition: definition.clone(),
            });
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            })
        }
        B5ProceduralSurface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            angular_scale,
            bounds,
        } => revolution_surface(
            graph.profiles.get(&profile_curve),
            axis_origin,
            axis_direction,
            angular_scale,
            bounds,
            &format_args!("b5 revolution surface record #{surface_id}"),
            refusal,
        )
        .map_or_else(
            || {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: Some(payload.clone()),
                })
            },
            |(surface, plan)| {
                procedure = Some(SurfaceProcedure::Revolution(plan));
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface))
            },
        ),
    };

    SurfacePlan {
        geometry,
        procedure,
    }
}

pub(super) fn revolution_surface(
    profile: Option<&B5Profile>,
    axis_origin: FinitePoint3,
    axis_direction: UnitVector3,
    angular_scale: PositiveReal,
    bounds: [[f64; 2]; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<(NurbsSurface, RevolutionPlan)> {
    let profile = profile?;
    let [parameter_interval, native_angular_interval] = bounds;
    let directrix = profile_nurbs(profile, parameter_interval, record, refusal)?;
    let angular_interval = [
        native_angular_interval[0] / angular_scale.get(),
        native_angular_interval[1] / angular_scale.get(),
    ];
    let surface = revolve_nurbs(
        &directrix,
        coordinates(axis_origin),
        components(&axis_direction),
        angular_interval,
        native_angular_interval,
        record,
        refusal,
    )?;
    Some((
        surface,
        RevolutionPlan {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval: native_angular_interval,
            parameter_interval,
        },
    ))
}

fn profile_nurbs(
    profile: &B5Profile,
    interval: [f64; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsCurve> {
    (profile
        .parameter_range()
        .into_iter()
        .zip(interval)
        .all(|(profile, surface)| profile.to_bits() == surface.to_bits()))
    .then_some(())?;
    match profile {
        B5Profile::Line {
            point, direction, ..
        } => crate::nurbs::note_refusal(
            NurbsCurve::from_lanes(
                1,
                vec![interval[0], interval[0], interval[1], interval[1]],
                interval
                    .map(|parameter| point3(add(*point, scale(*direction, parameter))))
                    .to_vec(),
                None,
                false,
            ),
            refusal,
            format_args!("b5 line profile of a revolution surface: {record}"),
        ),
        B5Profile::Arc {
            center,
            direction_x,
            direction_y,
            radius,
            ..
        } => rational_arc(
            *center,
            *direction_x,
            *direction_y,
            *radius,
            interval,
            record,
            refusal,
        ),
    }
}

pub(super) fn rational_arc(
    center: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
    radius: f64,
    interval: [f64; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsCurve> {
    let angles = [interval[0] / radius, interval[1] / radius];
    let span_count = ((angles[1] - angles[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    // `ceil` answers zero only for an angular span of exactly zero: an arc that
    // sweeps no angle states no span, which this route refuses as it refuses
    // every other degeneracy.
    let span_count = std::num::NonZeroUsize::new(span_count as usize)?.get();
    let control_count = span_count.checked_mul(2)?.checked_add(1)?;
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_points.capacity());
    let mut knots = Vec::with_capacity(control_points.capacity() + 3);
    for span in 0..span_count {
        let fraction0 = span as f64 / span_count as f64;
        let fraction1 = (span + 1) as f64 / span_count as f64;
        let angle0 = angles[0] + (angles[1] - angles[0]) * fraction0;
        let angle1 = angles[0] + (angles[1] - angles[0]) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return None;
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
        append_quadratic_span_knots(&mut knots, interval, span, span_count);
    }
    crate::nurbs::note_refusal(
        NurbsCurve::from_lanes(2, knots, control_points, Some(weights), false),
        refusal,
        format_args!("b5 rational arc profile of a revolution surface: {record}"),
    )
}

pub(super) fn revolve_nurbs(
    profile: &NurbsCurve,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    angular_interval: [f64; 2],
    native_interval: [f64; 2],
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<NurbsSurface> {
    let span_count =
        ((angular_interval[1] - angular_interval[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    // `ceil` answers zero only for an angular span of exactly zero: an arc that
    // sweeps no angle states no span, which this route refuses as it refuses
    // every other degeneracy.
    let span_count = std::num::NonZeroUsize::new(span_count as usize)?.get();
    let angular_count = span_count.checked_mul(2)?.checked_add(1)?;
    let control_count =
        crate::nurbs_surface_control_count(profile.control_points().len(), angular_count)?;
    let mut angles = Vec::with_capacity(angular_count);
    let mut angular_weights = Vec::with_capacity(angular_count);
    let mut v_knots = Vec::with_capacity(angular_count + 3);
    for span in 0..span_count {
        let fraction0 = span as f64 / span_count as f64;
        let fraction1 = (span + 1) as f64 / span_count as f64;
        let angle0 = angular_interval[0] + (angular_interval[1] - angular_interval[0]) * fraction0;
        let angle1 = angular_interval[0] + (angular_interval[1] - angular_interval[0]) * fraction1;
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
        append_quadratic_span_knots(&mut v_knots, native_interval, span, span_count);
    }
    let profile_weights = match profile.weights() {
        Some(weights) => weights,
        None => alloc_filled(
            profile.control_points().len(),
            1.0,
            "catia b5 revolution profile weights",
        )
        .ok()?,
    };
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_points.capacity());
    for (profile_point, profile_weight) in profile.control_points().iter().zip(profile_weights) {
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
    crate::nurbs::note_refusal(
        NurbsSurface::from_lanes(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                profile.degree(),
                profile.knots().to_vec(),
                false,
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(2, v_knots, false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                control_points.chunks(row_len).map(<[_]>::to_vec).collect(),
                Some(weights).map(|values| values.chunks(row_len).map(<[_]>::to_vec).collect()),
            ),
            false,
        ),
        refusal,
        format_args!("b5 revolution surface built from its profile: {record}"),
    )
}

fn append_quadratic_span_knots(
    knots: &mut Vec<f64>,
    interval: [f64; 2],
    span: usize,
    span_count: usize,
) {
    let start = interval[0] + (interval[1] - interval[0]) * span as f64 / span_count as f64;
    let end = interval[0] + (interval[1] - interval[0]) * (span + 1) as f64 / span_count as f64;
    if span == 0 {
        knots.extend([start, start, start]);
    } else {
        knots.extend([start, start]);
    }
    if span + 1 == span_count {
        knots.extend([end, end, end]);
    }
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
) -> Result<HashMap<u32, SurfaceId>, cadmpeg_core::CodecError> {
    let surface_plan: BTreeMap<u32, SurfacePlan> = std::mem::take(&mut plan.surface_plan);
    let surface_ids = surface_plan
        .keys()
        .map(|object_id| {
            (
                *object_id,
                SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "surface"),
                    object_id,
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let face_surfaces = graph
        .faces
        .iter()
        .map(|face| face.surface)
        .collect::<HashSet<_>>();
    for (object_id, plan) in surface_plan {
        let id = surface_ids[&object_id].clone();
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
        );
        if revolution_cache {
            annotations
                .derived(&id, "geometry")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: plan.geometry,
            source_object: Some(cgm_source("surface", object_id)),
        });
        match plan.procedure {
            Some(SurfaceProcedure::Extrusion(extrusion)) => {
                emit_extrusion_procedure(ir, annotations, &surface_ids, id, object_id, *extrusion)?;
            }
            Some(SurfaceProcedure::Revolution(revolution)) => {
                let directrix_id = CurveId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "profile"),
                    object_id,
                );
                annotate(
                    annotations,
                    &directrix_id,
                    "object_stream_b5_03",
                    "2d_profile_curve",
                    Exactness::Derived,
                );
                annotations
                    .derived(&directrix_id, "geometry")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                ir.model.curves.push(Curve {
                    id: directrix_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        revolution.directrix,
                    )),
                    source_object: None,
                });
                let procedural_id = ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "procedural-surface"),
                    object_id,
                );
                annotate(
                    annotations,
                    &procedural_id,
                    "object_stream_b5_03",
                    "2d_surface_of_revolution",
                    Exactness::Derived,
                );
                let _attached = ir.model.add_procedural_surface(
                    id,
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
                let procedural_id = ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "b5", "rolling-ball"),
                    object_id,
                );
                let carrier_tag = format!("result_carrier:{carrier_object_id:08x}");
                annotate(
                    annotations,
                    &procedural_id,
                    "object_stream_a8_03_32",
                    &carrier_tag,
                    Exactness::ByteExact,
                );
                let _attached = ir.model.add_procedural_surface(
                    id,
                    ProceduralSurface::new(procedural_id, definition, None),
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
        let procedural_id = ProceduralSurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "offset"),
            object_id,
        );
        annotate(
            annotations,
            &procedural_id,
            "object_stream_b5_03",
            "30_offset_surface",
            Exactness::Derived,
        );
        let record_bounds = super::parameter_record_bounds(offset.parameter_bounds);
        let _attached = ir.model.add_procedural_surface(
            surface.clone(),
            ProceduralSurface::new(
                procedural_id,
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                        support.clone(),
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
    surface_id: SurfaceId,
    surface_object_id: u32,
    extrusion: super::ResolvedExtrusionSurface,
) -> Result<(), cadmpeg_core::CodecError> {
    let directrix_id = CurveId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix"),
        extrusion.directrix_object_id,
    );
    match extrusion.directrix {
        super::ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            let sides = (*supports).map(|side| IntcurveSupportSide {
                surface: Some(surface_ids[&side.surface_object_id].clone()),
                pcurve: Some(SupportPcurve::new(
                    side.pcurve,
                    (side.pcurve_parameter_range != extrusion.directrix_parameter_range)
                        .then(|| DirectedParameterRange::new(side.pcurve_parameter_range).ok())
                        .flatten(),
                )),
            });
            annotate(
                annotations,
                &directrix_id,
                "object_stream_a8_03_25",
                "two_support_directrix",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
            let procedure_id = ProceduralCurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix-procedure"),
                extrusion.directrix_object_id,
            );
            annotate(
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            );
            let procedure = ProceduralCurve::new(
                procedure_id,
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::try_new(
                        sides,
                        extrusion.directrix_parameter_range,
                        std::array::from_fn(|_| Vec::new()),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                    discontinuity_flag: false,
                    cache: Some(
                        cadmpeg_ir::geometry::LegacyCache::try_new(cache_fit_tolerance)
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                    ),
                },
            );

            let _attached = ir
                .model
                .add_procedural_curve(directrix_id.clone(), procedure);
        }
        super::ResolvedExtrusionDirectrix::SurfaceCurve { curve, .. } => {
            annotate(
                annotations,
                &directrix_id,
                "object_stream_b5_03_24",
                "support_pcurve_lift",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: curve,
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
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
            let source_id = CurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix-source"),
                source_object_id,
            );
            annotate(
                annotations,
                &source_id,
                "object_stream_b5_03_24",
                "support_pcurve_lift",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: source_id.clone(),
                geometry: source_curve,
                source_object: Some(cgm_source("curve", source_object_id)),
            });
            annotate(
                annotations,
                &directrix_id,
                "object_stream_b5_03_14",
                "fixed_direction_offset_curve",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
            let procedure_id = ProceduralCurveId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion-directrix-procedure"),
                extrusion.directrix_object_id,
            );
            annotate(
                annotations,
                &procedure_id,
                "object_stream_b5_03_14",
                "fixed_direction_offset_curve",
                Exactness::ByteExact,
            );
            let side = cadmpeg_ir::geometry::OffsetSide::Direction {
                direction,
                support: Some(surface_ids[&support.surface_object_id].clone()),
            };
            let _attached = ir.model.add_procedural_curve(
                directrix_id.clone(),
                cadmpeg_ir::geometry::CurveOffsetRange::uniform(source_parameter_range)
                    .and_then(|range| {
                        cadmpeg_ir::geometry::curve_payloads::OffsetCurveConstruction::try_new(
                            source_id,
                            distance,
                            side,
                            Some(range),
                        )
                    })
                    .map(|admitted_payload| {
                        ProceduralCurve::new(
                            procedure_id,
                            ProceduralCurveDefinition::Offset(admitted_payload),
                        )
                    })
                    .map_err(cadmpeg_core::CodecError::malformed)?,
            );
        }
    }
    let procedure_id = ProceduralSurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "b5", "extrusion"),
        surface_object_id,
    );
    annotate(
        annotations,
        &procedure_id,
        "object_stream_b5_03",
        "2c_extrusion_surface",
        Exactness::ByteExact,
    );
    let record_bounds = super::parameter_record_bounds(extrusion.parameter_bounds);
    let _attached = ir.model.add_procedural_surface(
        surface_id,
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
            directrix_id,
            Some(extrusion.directrix_parameter_range),
            extrusion.direction,
            None,
            cadmpeg_ir::geometry::CacheContract::from_form(None),
        )
        .map(|admitted_payload| {
            ProceduralSurface::new(
                procedure_id,
                ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                Some(record_bounds),
            )
        })
        .map_err(cadmpeg_core::CodecError::malformed)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::emit_extrusion_procedure;
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
            directrix_parameter_range: [0.0, 1.0],
            direction: Vector3::new(0.0, 0.0, 1.0),
            parameter_bounds: crate::test_support::test_b5::finite_bounds([
                [-2.0, 3.0],
                [0.0, 1.0],
            ]),
            directrix: ResolvedExtrusionDirectrix::Intersection {
                cache_fit_tolerance: 1e-5,
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

        emit_extrusion_procedure(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &support_ids,
            surface_id,
            30,
            extrusion,
        )
        .expect("valid source object identity");

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
        assert_eq!(context.parameter_range(), [0.0, 1.0]);
        assert_eq!(context.sides()[0].surface, Some(support_ids[&10].clone()));
        assert_eq!(context.sides()[0].pcurve_parameter_range(), None);
        assert_eq!(context.sides()[1].surface, Some(support_ids[&20].clone()));
        assert_eq!(
            context.sides()[1].pcurve_parameter_range(),
            Some([0.25, 0.75])
        );
        assert_eq!(
            ir.model.procedural_curves[0].cache_fit_tolerance(),
            Some(1e-5)
        );
        assert!(match ir.model.procedural_surfaces[0].definition() {
            ProceduralSurfaceDefinition::Extrusion(matched_payload) =>
                matches!((&matched_payload.parameter_interval(), matched_payload.direction(), &matched_payload.native_position(),), (Some([0.0, 1.0]), direction, None,) if *direction == Vector3::new(0.0, 0.0, 1.0)),
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
