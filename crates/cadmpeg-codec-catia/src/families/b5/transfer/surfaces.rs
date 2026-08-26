// SPDX-License-Identifier: Apache-2.0
//! Surface-layer transfer: neutral surface lowering and the surface/procedural
//! emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, NurbsSurface,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::{B5Graph, B5Profile, B5Surface};
use super::super::vecmath::{add, cross, scale};
use super::{
    annotate, dot, length, point, point3, subtract, unit, vector, RevolutionPlan, SurfacePlan,
    SurfaceProcedure, TransferPlan,
};
use crate::assemble::cgm_source;

const EPS_FRAME_ORTHONORMAL: f64 = 1.0e-9;

pub(super) fn neutral_analytic_surface(surface: &B5Surface) -> Option<SurfaceGeometry> {
    match surface {
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
            ..
        } => orthonormal_plane(*origin, *direction_u, *direction_v),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
            ..
        } => Some(SurfaceGeometry::Cylinder {
            origin: point(*origin),
            axis: vector(*axis),
            ref_direction: vector(*reference_x),
            radius: *radius,
        }),
        B5Surface::Cone {
            apex,
            direction_x,
            axis,
            half_angle,
            slant_range,
            ..
        } => {
            let slant = slant_range[0];
            Some(SurfaceGeometry::Cone {
                origin: point(add(*apex, scale(*axis, slant * half_angle.cos()))),
                axis: vector(*axis),
                ref_direction: vector(*direction_x),
                radius: slant * half_angle.sin(),
                ratio: 1.0,
                half_angle: *half_angle,
            })
        }
        B5Surface::Sphere {
            center,
            direction_x,
            axis,
            radius,
            ..
        } => Some(SurfaceGeometry::Sphere {
            center: point(*center),
            axis: vector(*axis),
            ref_direction: vector(*direction_x),
            radius: *radius,
        }),
        B5Surface::Torus {
            center,
            direction_x,
            axis,
            major_radius,
            minor_radius,
            ..
        } => Some(SurfaceGeometry::Torus {
            center: point(*center),
            axis: vector(*axis),
            ref_direction: vector(*direction_x),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        }),
        B5Surface::Nurbs(surface) => Some(SurfaceGeometry::Nurbs(surface.clone())),
        B5Surface::UnresolvedNurbs { .. }
        | B5Surface::Unknown { .. }
        | B5Surface::RollingBall { .. }
        | B5Surface::Revolution { .. } => None,
    }
}

pub(super) fn neutral_surface(
    surface: &B5Surface,
    graph: &B5Graph,
    surface_id: u32,
    payload: &UnknownId,
) -> SurfacePlan {
    if let Some(geometry) = neutral_analytic_surface(surface) {
        return SurfacePlan {
            geometry,
            procedure: None,
        };
    }
    if let Some(extrusion) = super::resolved_extrusion_surface(graph, surface_id) {
        return SurfacePlan {
            geometry: SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            },
            procedure: Some(SurfaceProcedure::Extrusion(Box::new(extrusion))),
        };
    }
    let mut procedure = None;
    let geometry = match surface {
        B5Surface::UnresolvedNurbs { .. } | B5Surface::Unknown { .. } => SurfaceGeometry::Unknown {
            record: Some(payload.clone()),
        },
        B5Surface::RollingBall {
            carrier_object_id,
            definition,
        } => {
            procedure = Some(SurfaceProcedure::RollingBall {
                carrier_object_id: *carrier_object_id,
                definition: definition.clone(),
            });
            SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            }
        }
        B5Surface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            profile_range,
            angular_range,
            angular_scale,
            ..
        } => revolution_surface(
            graph.profiles.get(profile_curve),
            *axis_origin,
            *axis_direction,
            *angular_scale,
            [*profile_range, *angular_range],
        )
        .map_or_else(
            || SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            },
            |(surface, plan)| {
                procedure = Some(SurfaceProcedure::Revolution(plan));
                SurfaceGeometry::Nurbs(surface)
            },
        ),
        B5Surface::Plane { .. }
        | B5Surface::Cylinder { .. }
        | B5Surface::Cone { .. }
        | B5Surface::Sphere { .. }
        | B5Surface::Torus { .. }
        | B5Surface::Nurbs(_) => unreachable!("analytic carriers returned above"),
    };
    SurfacePlan {
        geometry,
        procedure,
    }
}

pub(super) fn revolution_surface(
    profile: Option<&B5Profile>,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    angular_scale: f64,
    bounds: [[f64; 2]; 2],
) -> Option<(NurbsSurface, RevolutionPlan)> {
    let profile = profile?;
    let [parameter_interval, native_angular_interval] = bounds;
    let directrix = profile_nurbs(profile, parameter_interval)?;
    if angular_scale <= 0.0 {
        return None;
    }
    let angular_interval = [
        native_angular_interval[0] / angular_scale,
        native_angular_interval[1] / angular_scale,
    ];
    let surface = revolve_nurbs(
        &directrix,
        axis_origin,
        axis_direction,
        angular_interval,
        native_angular_interval,
    )?;
    Some((
        surface,
        RevolutionPlan {
            directrix,
            axis_origin: point(axis_origin),
            axis_direction: vector(axis_direction),
            angular_interval,
            angular_parameter_interval: native_angular_interval,
            parameter_interval,
        },
    ))
}

pub(super) fn profile_nurbs(profile: &B5Profile, interval: [f64; 2]) -> Option<NurbsCurve> {
    (profile
        .parameter_range()
        .into_iter()
        .zip(interval)
        .all(|(profile, surface)| profile.to_bits() == surface.to_bits()))
    .then_some(())?;
    match profile {
        B5Profile::Line {
            point, direction, ..
        } => Some(NurbsCurve {
            degree: 1,
            knots: vec![interval[0], interval[0], interval[1], interval[1]],
            control_points: interval
                .map(|parameter| point3(add(*point, scale(*direction, parameter))))
                .to_vec(),
            weights: None,
            periodic: false,
        }),
        B5Profile::Arc {
            center,
            direction_x,
            direction_y,
            radius,
            ..
        } => rational_arc(*center, *direction_x, *direction_y, *radius, interval),
    }
}

pub(super) fn rational_arc(
    center: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
    radius: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    let angles = [interval[0] / radius, interval[1] / radius];
    let span_count = ((angles[1] - angles[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let span_count = (span_count as usize).max(1);
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
    Some(NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

pub(super) fn revolve_nurbs(
    profile: &NurbsCurve,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    angular_interval: [f64; 2],
    native_interval: [f64; 2],
) -> Option<NurbsSurface> {
    let span_count =
        ((angular_interval[1] - angular_interval[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let span_count = (span_count as usize).max(1);
    let angular_count = span_count.checked_mul(2)?.checked_add(1)?;
    let control_count =
        crate::nurbs_surface_control_count(profile.control_points.len(), angular_count)?;
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
    let profile_weights = match profile.weights.clone() {
        Some(weights) => weights,
        None => alloc_filled(
            profile.control_points.len(),
            1.0,
            "catia b5 revolution profile weights",
        )
        .ok()?,
    };
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_points.capacity());
    for (profile_point, profile_weight) in profile.control_points.iter().zip(profile_weights) {
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
    Some(NurbsSurface {
        u_degree: profile.degree,
        v_degree: 2,
        u_knots: profile.knots.clone(),
        v_knots,
        u_count: u32::try_from(profile.control_points.len()).ok()?,
        v_count: u32::try_from(angular_count).ok()?,
        control_points,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: false,
    })
}

pub(super) fn append_quadratic_span_knots(
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

pub(super) fn circle_point(
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

pub(super) fn rotate_vector(value: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    add(
        add(
            scale(value, angle.cos()),
            scale(cross(axis, value), angle.sin()),
        ),
        scale(axis, dot(axis, value) * (1.0 - angle.cos())),
    )
}

pub(super) fn orthonormal_plane(
    origin: [f64; 3],
    direction_u: [f64; 3],
    direction_v: [f64; 3],
) -> Option<SurfaceGeometry> {
    let u = unit(direction_u)?;
    let v = unit(direction_v)?;
    if (length(direction_u) - 1.0).abs() > EPS_FRAME_ORTHONORMAL
        || (length(direction_v) - 1.0).abs() > EPS_FRAME_ORTHONORMAL
        || dot(u, v).abs() > EPS_FRAME_ORTHONORMAL
    {
        return None;
    }
    Some(SurfaceGeometry::Plane {
        origin: point(origin),
        normal: vector(unit(cross(u, v))?),
        u_axis: vector(u),
    })
}

/// Emit the referenced surfaces, their procedural definitions, and the offset
/// procedural surfaces, returning the map from `object_id` to emitted
/// [`SurfaceId`]. Consumes the planned surfaces out of the transfer plan.
pub(super) fn emit_surfaces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &mut TransferPlan,
) -> HashMap<u32, SurfaceId> {
    let surface_plan: BTreeMap<u32, SurfacePlan> = std::mem::take(&mut plan.surface_plan);
    let surface_ids = surface_plan
        .keys()
        .map(|object_id| {
            (
                *object_id,
                SurfaceId(format!("catia:b5:surface#{object_id}")),
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
            } else if matches!(plan.geometry, SurfaceGeometry::Unknown { .. }) {
                Exactness::Unknown
            } else if revolution_cache {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        if revolution_cache {
            annotations.derived(&id, "geometry");
        }
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: plan.geometry,
            source_object: Some(cgm_source("surface", object_id)),
        });
        match plan.procedure {
            Some(SurfaceProcedure::Extrusion(extrusion)) => {
                emit_extrusion_procedure(ir, annotations, &surface_ids, id, object_id, *extrusion);
            }
            Some(SurfaceProcedure::Revolution(revolution)) => {
                let directrix_id = CurveId(format!("catia:b5:profile#{object_id}"));
                annotate(
                    annotations,
                    &directrix_id,
                    "object_stream_b5_03",
                    "2d_profile_curve",
                    Exactness::Derived,
                );
                annotations.derived(&directrix_id, "geometry");
                ir.model.curves.push(Curve {
                    id: directrix_id.clone(),
                    geometry: CurveGeometry::Nurbs(revolution.directrix),
                    source_object: None,
                });
                let procedural_id =
                    ProceduralSurfaceId(format!("catia:b5:procedural-surface#{object_id}"));
                annotate(
                    annotations,
                    &procedural_id,
                    "object_stream_b5_03",
                    "2d_surface_of_revolution",
                    Exactness::Derived,
                );
                ir.model.procedural_surfaces.push(ProceduralSurface {
                    id: procedural_id,
                    surface: id,
                    definition: ProceduralSurfaceDefinition::Revolution {
                        directrix: directrix_id,
                        axis_origin: revolution.axis_origin,
                        axis_direction: revolution.axis_direction,
                        angular_interval: revolution.angular_interval,
                        angular_parameter_interval: Some(revolution.angular_parameter_interval),
                        parameter_interval: Some(revolution.parameter_interval),
                        transposed: false,
                        revision_form: None,
                    },
                    cache_fit_tolerance: None,
                    record_bounds: None,
                });
            }
            Some(SurfaceProcedure::RollingBall {
                carrier_object_id,
                definition,
            }) if graph
                .canonical_surface_id(object_id)
                .is_some_and(|id| !graph.offset_surfaces.contains_key(&id)) =>
            {
                let procedural_id =
                    ProceduralSurfaceId(format!("catia:b5:rolling-ball#{object_id}"));
                let carrier_tag = format!("result_carrier:{carrier_object_id:08x}");
                annotate(
                    annotations,
                    &procedural_id,
                    "object_stream_a8_03_32",
                    &carrier_tag,
                    Exactness::ByteExact,
                );
                ir.model.procedural_surfaces.push(ProceduralSurface {
                    id: procedural_id,
                    surface: id,
                    definition,
                    cache_fit_tolerance: None,
                    record_bounds: None,
                });
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
        let procedural_id = ProceduralSurfaceId(format!("catia:b5:offset#{object_id}"));
        annotate(
            annotations,
            &procedural_id,
            "object_stream_b5_03",
            "30_offset_surface",
            Exactness::Derived,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: offset.distance,
                u_sense: None,
                v_sense: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: Some(parameter_record_bounds(offset.parameter_bounds)),
        });
    }
    surface_ids
}

fn parameter_record_bounds(bounds: [[f64; 2]; 2]) -> [Option<f64>; 4] {
    [
        Some(bounds[0][0]),
        Some(bounds[0][1]),
        Some(bounds[1][0]),
        Some(bounds[1][1]),
    ]
}

fn emit_extrusion_procedure(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    surface_ids: &HashMap<u32, SurfaceId>,
    surface_id: SurfaceId,
    surface_object_id: u32,
    extrusion: super::ResolvedExtrusionSurface,
) {
    let directrix_id = CurveId(format!(
        "catia:b5:extrusion-directrix#{}",
        extrusion.directrix_object_id
    ));
    match extrusion.directrix {
        super::ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            let sides = (*supports).map(|side| IntcurveSupportSide {
                surface: Some(surface_ids[&side.surface_object_id].clone()),
                pcurve: Some(side.pcurve),
                pcurve_parameter_range: (side.pcurve_parameter_range
                    != extrusion.directrix_parameter_range)
                    .then_some(side.pcurve_parameter_range),
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
                geometry: CurveGeometry::Unknown { record: None },
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
            let procedure_id = ProceduralCurveId(format!(
                "catia:b5:extrusion-directrix-procedure#{}",
                extrusion.directrix_object_id
            ));
            annotate(
                annotations,
                &procedure_id,
                "object_stream_a8_03_25",
                "two_surface_pcurve_intersection",
                Exactness::ByteExact,
            );
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedure_id,
                curve: directrix_id.clone(),
                definition: ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext {
                        sides,
                        parameter_range: extrusion.directrix_parameter_range,
                        discontinuities: std::array::from_fn(|_| Vec::new()),
                    },
                    discontinuity_flag: false,
                },
                cache_fit_tolerance: Some(cache_fit_tolerance),
            });
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
            let source_id = CurveId(format!(
                "catia:b5:extrusion-directrix-source#{source_object_id}"
            ));
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
                geometry: CurveGeometry::Unknown { record: None },
                source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
            });
            let procedure_id = ProceduralCurveId(format!(
                "catia:b5:extrusion-directrix-procedure#{}",
                extrusion.directrix_object_id
            ));
            annotate(
                annotations,
                &procedure_id,
                "object_stream_b5_03_14",
                "fixed_direction_offset_curve",
                Exactness::ByteExact,
            );
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedure_id,
                curve: directrix_id.clone(),
                definition: ProceduralCurveDefinition::Offset {
                    source: source_id,
                    distance,
                    direction: Some(direction),
                    support: Some(surface_ids[&support.surface_object_id].clone()),
                    normal: None,
                    parameter_range: Some(source_parameter_range),
                    distance_law: None,
                },
                cache_fit_tolerance: None,
            });
        }
    }
    let procedure_id = ProceduralSurfaceId(format!("catia:b5:extrusion#{surface_object_id}"));
    annotate(
        annotations,
        &procedure_id,
        "object_stream_b5_03",
        "2c_extrusion_surface",
        Exactness::ByteExact,
    );
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: procedure_id,
        surface: surface_id,
        definition: ProceduralSurfaceDefinition::Extrusion {
            directrix: directrix_id,
            parameter_interval: Some(extrusion.directrix_parameter_range),
            direction: extrusion.direction,
            native_position: None,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: Some(parameter_record_bounds(extrusion.parameter_bounds)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::units::Units;

    use crate::families::b5::transfer::{
        ResolvedExtrusionDirectrix, ResolvedExtrusionSupport, ResolvedExtrusionSurface,
    };

    #[test]
    fn extrusion_emits_exact_two_support_intersection() {
        let support_ids = HashMap::from([
            (10, SurfaceId("support-10".to_string())),
            (20, SurfaceId("support-20".to_string())),
        ]);
        let pcurve = |x| PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point2::new(x, 0.0), Point2::new(x, 1.0)],
            weights: None,
            periodic: false,
        };
        let extrusion = ResolvedExtrusionSurface {
            surface_object_id: 30,
            directrix_object_id: 40,
            directrix_parameter_range: [0.0, 1.0],
            direction: Vector3::new(0.0, 0.0, 1.0),
            parameter_bounds: [[-2.0, 3.0], [0.0, 1.0]],
            directrix: ResolvedExtrusionDirectrix::Intersection {
                cache_fit_tolerance: 1e-5,
                supports: Box::new([
                    ResolvedExtrusionSupport {
                        surface_object_id: 10,
                        surface: SurfaceGeometry::Plane {
                            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                            normal: Vector3::new(1.0, 0.0, 0.0),
                            u_axis: Vector3::new(0.0, 1.0, 0.0),
                        },
                        pcurve: pcurve(0.0),
                        pcurve_parameter_range: [0.0, 1.0],
                        curve: None,
                    },
                    ResolvedExtrusionSupport {
                        surface_object_id: 20,
                        surface: SurfaceGeometry::Plane {
                            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                            normal: Vector3::new(0.0, 1.0, 0.0),
                            u_axis: Vector3::new(1.0, 0.0, 0.0),
                        },
                        pcurve: pcurve(1.0),
                        pcurve_parameter_range: [0.25, 0.75],
                        curve: None,
                    },
                ]),
            },
        };
        let mut ir = CadIr::empty(Units::default());

        emit_extrusion_procedure(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &support_ids,
            SurfaceId("result-30".to_string()),
            30,
            extrusion,
        );

        assert!(matches!(
            ir.model.curves[0].geometry,
            CurveGeometry::Unknown { record: None }
        ));
        let ProceduralCurveDefinition::Intersection { context, .. } =
            &ir.model.procedural_curves[0].definition
        else {
            panic!("expected intersection directrix");
        };
        assert_eq!(context.parameter_range, [0.0, 1.0]);
        assert_eq!(context.sides[0].surface, Some(support_ids[&10].clone()));
        assert_eq!(context.sides[0].pcurve_parameter_range, None);
        assert_eq!(context.sides[1].surface, Some(support_ids[&20].clone()));
        assert_eq!(context.sides[1].pcurve_parameter_range, Some([0.25, 0.75]));
        assert_eq!(
            ir.model.procedural_curves[0].cache_fit_tolerance,
            Some(1e-5)
        );
        assert!(matches!(
            ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Extrusion {
                parameter_interval: Some([0.0, 1.0]),
                direction,
                native_position: None,
                ..
            } if direction == Vector3::new(0.0, 0.0, 1.0)
        ));
        assert_eq!(
            ir.model.procedural_surfaces[0].record_bounds,
            Some([Some(-2.0), Some(3.0), Some(0.0), Some(1.0)])
        );
    }
}
