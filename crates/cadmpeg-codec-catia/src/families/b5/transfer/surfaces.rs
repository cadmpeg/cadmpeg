// SPDX-License-Identifier: Apache-2.0
//! Surface-layer transfer: neutral surface lowering and the surface/procedural
//! emit pass.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, ProceduralSurface, ProceduralSurfaceDefinition,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::{B5Graph, B5Profile, B5Surface};
use super::super::vecmath::{add, cross, scale};
use super::{
    annotate, dot, length, point, point3, subtract, unit, vector, RevolutionPlan, SurfacePlan,
    SurfaceProcedure, TransferPlan,
};
use crate::assemble::cgm_source;

pub(super) fn neutral_surface(
    surface: &B5Surface,
    graph: &B5Graph,
    surface_id: u32,
    payload: &UnknownId,
) -> SurfacePlan {
    let mut procedure = None;
    let geometry = match surface {
        B5Surface::UnresolvedNurbs { .. } | B5Surface::Unknown { .. } => SurfaceGeometry::Unknown {
            record: Some(payload.clone()),
        },
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => orthonormal_plane(*origin, *direction_u, *direction_v).unwrap_or_else(|| {
            SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            }
        }),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => SurfaceGeometry::Cylinder {
            origin: point(*origin),
            axis: vector(*axis),
            ref_direction: vector(*reference_x),
            radius: *radius,
        },
        B5Surface::Cone {
            apex,
            direction_x,
            axis,
            half_angle,
            slant_range,
            ..
        } => {
            let slant = slant_range[0];
            SurfaceGeometry::Cone {
                origin: point(add(*apex, scale(*axis, slant * half_angle.cos()))),
                axis: vector(*axis),
                ref_direction: vector(*direction_x),
                radius: slant * half_angle.sin(),
                ratio: 1.0,
                half_angle: *half_angle,
            }
        }
        B5Surface::Torus {
            center,
            direction_x,
            axis,
            major_radius,
            minor_radius,
            ..
        } => SurfaceGeometry::Torus {
            center: point(*center),
            axis: vector(*axis),
            ref_direction: vector(*direction_x),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        B5Surface::Nurbs(surface) => SurfaceGeometry::Nurbs(surface.clone()),
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
            gauge_radius,
        } => revolution_surface(
            graph.profiles.get(profile_curve),
            *axis_origin,
            *axis_direction,
            *gauge_radius,
            surface_parameter_bounds(graph, surface_id),
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
    };
    SurfacePlan {
        geometry,
        procedure,
    }
}

pub(super) fn surface_parameter_bounds(graph: &B5Graph, surface_id: u32) -> Option<[[f64; 2]; 2]> {
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for point in graph
        .pcurves
        .values()
        .filter(|pcurve| pcurve.surface == surface_id)
        .flat_map(|pcurve| &pcurve.control_points)
    {
        for dimension in 0..2 {
            bounds[dimension][0] = bounds[dimension][0].min(point[dimension]);
            bounds[dimension][1] = bounds[dimension][1].max(point[dimension]);
        }
    }
    bounds
        .iter()
        .all(|range| range[0].is_finite() && range[0] < range[1])
        .then_some(bounds)
}

pub(super) fn revolution_surface(
    profile: Option<&B5Profile>,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    gauge_radius: f64,
    bounds: Option<[[f64; 2]; 2]>,
) -> Option<(NurbsSurface, RevolutionPlan)> {
    let profile = profile?;
    let [parameter_interval, native_angular_interval] = bounds?;
    let directrix = profile_nurbs(profile, parameter_interval)?;
    let sign = gauge_radius.signum();
    if sign == 0.0 {
        return None;
    }
    let effective_axis = scale(axis_direction, sign);
    let angular_interval = [
        native_angular_interval[0] / gauge_radius.abs(),
        native_angular_interval[1] / gauge_radius.abs(),
    ];
    let surface = revolve_nurbs(
        &directrix,
        axis_origin,
        effective_axis,
        angular_interval,
        native_angular_interval,
    )?;
    Some((
        surface,
        RevolutionPlan {
            directrix,
            axis_origin: point(axis_origin),
            axis_direction: vector(effective_axis),
            angular_interval,
            parameter_interval,
        },
    ))
}

pub(super) fn profile_nurbs(profile: &B5Profile, interval: [f64; 2]) -> Option<NurbsCurve> {
    match profile {
        B5Profile::Line { point, direction } => Some(NurbsCurve {
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
    let profile_weights = profile
        .weights
        .clone()
        .unwrap_or_else(|| vec![1.0; profile.control_points.len()]);
    let mut control_points = Vec::with_capacity(profile.control_points.len() * angular_count);
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
    if (length(direction_u) - 1.0).abs() > 1e-9
        || (length(direction_v) - 1.0).abs() > 1e-9
        || dot(u, v).abs() > 1e-9
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
    let mut surface_ids = HashMap::new();
    for (object_id, plan) in surface_plan {
        let id = SurfaceId(format!("catia:b5:surface#{object_id}"));
        let revolution_cache = matches!(
            plan.procedure.as_ref(),
            Some(SurfaceProcedure::Revolution(_))
        );
        let rolling_ball_carrier = matches!(
            plan.procedure.as_ref(),
            Some(SurfaceProcedure::RollingBall { .. })
        );
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "face_surface",
            if rolling_ball_carrier {
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
        surface_ids.insert(object_id, id.clone());
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: plan.geometry,
            source_object: Some(cgm_source("surface", object_id)),
        });
        match plan.procedure {
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
            }) if !graph.offset_surfaces.contains_key(&object_id) => {
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
                    definition: *definition,
                    cache_fit_tolerance: None,
                    record_bounds: None,
                });
            }
            Some(SurfaceProcedure::RollingBall { .. }) | None => {}
        }
    }
    for offset in graph.offset_surfaces.values() {
        let (Some(surface), Some(support)) = (
            surface_ids.get(&offset.object_id),
            surface_ids.get(&offset.source_surface),
        ) else {
            continue;
        };
        let procedural_id = ProceduralSurfaceId(format!("catia:b5:offset#{}", offset.object_id));
        annotate(
            annotations,
            &procedural_id,
            "object_stream_b5_03",
            "30_offset_surface",
            Exactness::Derived,
        );
        annotations.derived(&procedural_id, "definition.u_sense");
        annotations.derived(&procedural_id, "definition.v_sense");
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: offset.distance,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }
    surface_ids
}
