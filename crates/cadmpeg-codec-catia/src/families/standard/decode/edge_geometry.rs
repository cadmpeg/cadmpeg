// SPDX-License-Identifier: Apache-2.0
//! Standard carrier curves, pcurves, and analytic geometry.

use super::NURBS_SURFACE_MEMBERSHIP_TOLERANCE;
use crate::nurbs::reverse_nurbs_curve;
use crate::solve::missing_edge;
use std::cell::RefCell;
use std::collections::BTreeMap;

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::geometry::nurbs::NurbsSurface;

use super::{
    annotate, cgm_source, circle_parameter_range_from_surface_branch, face_surface,
    nurbs_surface_parameter_domain, ordered_range, point_on_nurbs_surface, rational_pcurve_arc,
    standard_id, standard_native_support_endpoint_pair, unit_vector, unwrap_angle,
    AnnotationBuilder, CadIr, CodecError, Curve, CurveGeometry, CurveId, DecodeContext,
    DirectedParameterRange, Exactness, FamilyEntityAdmission, FinitePoint3, FiniteVector3, HashMap,
    IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, OrthonormalFrame3, PcurveGeometry,
    Point, Point2, Point3, PositiveLength, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralCurveId, ProceduralSurface, ProceduralSurfaceId, SolvedCurveGeometry,
    SolvedSurfaceGeometry, StandardEdgeSupport, SupportPcurve, Surface, SurfaceGeometry, SurfaceId,
    UnitVector3, UnknownId, Vector3, ANALYTIC_CURVE_ENDPOINT_TOLERANCE,
    CYLINDER_PLANE_CONIC_TOLERANCE, EPS_STANDARD_DECODE_COARSE_GEOMETRY,
    EPS_STANDARD_DECODE_GEOMETRY, PERPENDICULAR_CYLINDER_CONIC_TOLERANCE,
    SPHERE_CENTER_COINCIDENCE_TOLERANCE, SPHERE_SECTION_ENDPOINT_TOLERANCE,
    STANDARD_FACE_BOUNDS_TOLERANCE, SUPPORT_AGREEMENT_TOLERANCE,
};

pub(super) fn standard_pcurve_geometry(
    ctx: &DecodeContext<'_>,
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    (start, end): (Point3, Point3),
    witness: Option<FinitePoint3>,
    edge_curve: Option<&CurveGeometry>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<(PcurveGeometry, [f64; 2])>, cadmpeg_core::CodecError> {
    if matches!(
        edge_curve,
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }))
    ) {
        return Ok(None);
    }
    let on_start = point_on_surface(ctx, start, surface)?;
    let on_end = point_on_surface(ctx, end, surface)?;
    if !on_start || !on_end {
        return Ok(None);
    }
    let Some(start_uv) = analytic_surface_uv(surface, start) else {
        return Ok(None);
    };
    let Some(end_uv) = analytic_surface_uv(surface, end) else {
        return Ok(None);
    };
    let mut uv = [start_uv, end_uv];
    if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = surface {
        let origin = cone_surface.origin().get();
        let axis = cone_surface.frame().axis().as_raw();
        let radius = cone_surface.radius().get();
        let half_angle = cone_surface.half_angle().get();
        let tangent = half_angle.tan();
        if tangent.is_finite() && tangent != 0.0 {
            let apex_offset = -radius / tangent;
            if apex_offset.is_finite() {
                let apex = Point3::new(
                    origin.x + apex_offset * axis.x,
                    origin.y + apex_offset * axis.y,
                    origin.z + apex_offset * axis.z,
                );
                if start.distance_squared(apex) <= EPS_STANDARD_DECODE_COARSE_GEOMETRY {
                    uv[0].u = uv[1].u;
                }
                if end.distance_squared(apex) <= EPS_STANDARD_DECODE_COARSE_GEOMETRY {
                    uv[1].u = uv[0].u;
                }
            }
        }
    }
    let reference_uv = uv[0];
    unwrap_standard_uv(surface, &mut uv[1], reference_uv);

    if let (
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
        Some(witness),
    ) = (&support.geometry, witness)
    {
        if let Some(end) = witnessed_surface_circle_end(
            ctx,
            surface,
            center.get(),
            radius.get(),
            uv,
            witness.get(),
        )? {
            uv[1] = end;
        }
    }

    if let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
    ) = (surface, &support.geometry)
    {
        const CIRCLE_TOLERANCE: f64 = 2e-3;

        let center = center.get();
        let radius = radius.get();
        let normal = plane_surface.frame().axis().as_raw();
        let contained_carrier = point_on_surface(ctx, center, surface)?
            && (start.distance(center) - radius).abs() <= CIRCLE_TOLERANCE
            && (end.distance(center) - radius).abs() <= CIRCLE_TOLERANCE
            && edge_curve.is_none_or(|curve| {
                matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                                if {
                                    let axis = circle_curve.frame().axis().as_raw();
                let curve_radius = circle_curve.radius().get();
                                    axis.cross(*normal).norm() <= CIRCLE_TOLERANCE
                                    && (curve_radius - radius).abs() <= CIRCLE_TOLERANCE
                                })
            });
        if !contained_carrier {
            return Ok(None);
        }
        let Some(center_uv) = analytic_surface_uv(surface, center) else {
            return Ok(None);
        };
        let range = if start == end {
            let angle = (uv[0].v - center_uv.v).atan2(uv[0].u - center_uv.u);
            [angle, angle + std::f64::consts::TAU]
        } else {
            let range = uv.map(|point| (point.v - center_uv.v).atan2(point.u - center_uv.u));
            ordered_range([range[0], unwrap_angle(range[1], range[0])])
        };
        let Some(geometry) = rational_pcurve_arc(
            ctx,
            [center_uv.u, center_uv.v],
            radius,
            range,
            refusal,
            "standard arc pcurve derived from its support",
        )?
        else {
            return Ok(None);
        };
        return Ok(Some((geometry, range)));
    }

    let direction = Point2::new(uv[1].u - uv[0].u, uv[1].v - uv[0].v);
    let midpoint_uv = Point2::new(uv[0].u + 0.5 * direction.u, uv[0].v + 0.5 * direction.v);
    let Some(midpoint) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::surface_point(ctx, surface, midpoint_uv.u, midpoint_uv.v),
        )?)?
    else {
        return Ok(None);
    };
    let on_curve = match &support.geometry {
        crate::families::standard::records::StandardCurveGeometry::Line => {
            let chord = end.vector_from(start);
            let offset = midpoint.vector_from(start);
            let Some(chord) = FiniteVector3::new(chord) else {
                return Ok(None);
            };
            let Some(direction) = chord.unit_nonzero() else {
                return Ok(None);
            };
            direction.cross(offset).norm() <= STANDARD_FACE_BOUNDS_TOLERANCE
        }
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
            (midpoint.distance_squared(center.get()).sqrt() - radius.get()).abs() <= 2e-3
        }
        crate::families::standard::records::StandardCurveGeometry::Bspline => match edge_curve {
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve))) => {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                let offset = midpoint.vector_from(origin);
                direction.cross(offset).norm() <= STANDARD_FACE_BOUNDS_TOLERANCE
            }
            _ => false,
        },
    };
    let Ok(line) = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(uv[0], direction) else {
        return Ok(None);
    };
    Ok(on_curve.then_some((PcurveGeometry::Line(line), [0.0, 1.0])))
}

pub(super) fn witness_arc_end(start: f64, short_end: f64, witness: f64) -> Option<f64> {
    let delta = short_end - start;
    if delta == 0.0 {
        return None;
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    let contains = |end: f64| {
        (-2..=2).any(|turn| {
            let witness = witness + f64::from(turn) * std::f64::consts::TAU;
            witness > start.min(end) && witness < start.max(end)
        })
    };
    match (contains(short_end), contains(long_end)) {
        (true, false) => Some(short_end),
        (false, true) => Some(long_end),
        _ => None,
    }
}

pub(super) fn witnessed_surface_circle_end(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    center: Point3,
    radius: f64,
    uv: [Point2; 2],
    witness: Point3,
) -> Result<Option<Point2>, cadmpeg_core::decode::ResourceLimit> {
    let Some(witness_uv) = analytic_surface_uv(surface, witness) else {
        return Ok(None);
    };
    let lanes: &[usize] = match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => &[0],
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => &[0],
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => &[0],
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => &[0, 1],
        _ => return Ok(None),
    };
    let mut selected_candidate = None;
    for lane in lanes {
        let mut candidate = uv[1];
        let (start, short_end, witness) = if *lane == 0 {
            (uv[0].u, uv[1].u, witness_uv.u)
        } else {
            (uv[0].v, uv[1].v, witness_uv.v)
        };
        let Some(selected) = witness_arc_end(start, short_end, witness) else {
            continue;
        };
        if *lane == 0 {
            candidate.u = selected;
        } else {
            candidate.v = selected;
        }
        let Some(midpoint) = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
                ctx,
                surface,
                0.5 * (uv[0].u + candidate.u),
                0.5 * (uv[0].v + candidate.v),
            ))?,
        )?
        else {
            continue;
        };
        if (midpoint.distance_squared(center).sqrt() - radius).abs() <= 2e-3 {
            if selected_candidate.is_some() {
                return Ok(None);
            }
            selected_candidate = Some(candidate);
        }
    }
    Ok(selected_candidate)
}

pub(super) fn analytic_surface_uv(surface: &SurfaceGeometry, point: Point3) -> Option<Point2> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let offset = point.vector_from(origin);
            let v_axis = (*normal).cross(*u_axis);
            Some(Point2::new(offset.dot(*u_axis), offset.dot(v_axis)))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let offset = point.vector_from(origin);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                offset.dot(tangent).atan2(offset.dot(*ref_direction)),
                offset.dot(*axis),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let ratio = cone_surface.ratio().get();
            let offset = point.vector_from(origin);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                (offset.dot(tangent) / ratio).atan2(offset.dot(*ref_direction)),
                offset.dot(*axis),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            let offset = point.vector_from(center);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                offset.dot(tangent).atan2(offset.dot(*ref_direction)),
                (offset.dot(*axis) / radius).clamp(-1.0, 1.0).asin(),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let offset = point.vector_from(center);
            let tangent = (*axis).cross(*ref_direction);
            let u = offset.dot(tangent).atan2(offset.dot(*ref_direction));
            let radial = Vector3::new(
                u.cos() * ref_direction.x + u.sin() * tangent.x,
                u.cos() * ref_direction.y + u.sin() * tangent.y,
                u.cos() * ref_direction.z + u.sin() * tangent.z,
            );
            Some(Point2::new(
                u,
                offset.dot(*axis).atan2(offset.dot(radial) - major_radius),
            ))
        }
        _ => None,
    }
}

pub(super) fn unwrap_standard_uv(surface: &SurfaceGeometry, value: &mut Point2, reference: Point2) {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
            value.u = unwrap_angle(value.u, reference.u);
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
            value.u = unwrap_angle(value.u, reference.u);
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
            value.u = unwrap_angle(value.u, reference.u);
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
            value.u = unwrap_angle(value.u, reference.u);
            value.v = unwrap_angle(value.v, reference.v);
        }
        _ => {}
    }
}

pub(super) fn point_on_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    point: Point3,
    surface: &SurfaceGeometry,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    Ok(point_on_surface_if_supported(ctx, point, surface)?.unwrap_or(false))
}

pub(super) fn point_on_surface_if_supported(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    point: Point3,
    surface: &SurfaceGeometry,
) -> Result<Option<bool>, cadmpeg_core::decode::ResourceLimit> {
    const TOLERANCE: f64 = 1e-3;
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let residual = match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            point.vector_from(origin).dot(*normal).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            let axial = point.vector_from(origin).dot(*axis);
            let radial = (point.vector_from(origin) - axis.scale(axial)).norm();
            (radial - radius).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let radius = cone_surface.radius().get();
            let half_angle = cone_surface.half_angle().get();
            let axial = point.vector_from(origin).dot(*axis);
            let radial = (point.vector_from(origin) - axis.scale(axial)).norm();
            (radial - (radius + axial * half_angle.tan()).abs()).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let radius = sphere_surface.radius().get();
            (point.distance(center) - radius.abs()).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let axial = point.vector_from(center).dot(*axis);
            let radial = (point.vector_from(center) - axis.scale(axial)).norm();
            ((radial - major_radius).hypot(axial) - minor_radius.abs()).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
            return point_on_nurbs_surface(ctx, point, surface);
        }
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed(_)
            | SolvedSurfaceGeometry::Unknown { .. },
        )
        | SurfaceGeometry::Procedural { .. } => return Ok(None),
    };
    Ok(Some(residual <= TOLERANCE))
}

pub(super) fn standard_spline_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Result<Option<(CurveGeometry, [f64; 2])>, CodecError> {
    const TOLERANCE: f64 = 2e-3;
    ctx.charge_work_limit(0, "catia surface membership boundary")?;

    let surfaces = [
        face_surface(ctx, ir, bindings, surface_indices, support.faces[0])?,
        face_surface(ctx, ir, bindings, surface_indices, support.faces[1])?,
    ];
    let [Some(left), Some(right)] = surfaces else {
        return Ok(None);
    };
    let Some(start_point) = ir.model.points.get(points[0]) else {
        return Ok(None);
    };
    let start = start_point.position().get();
    let Some(end) = ir
        .model
        .points
        .get(points[1])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    if !point_on_surface(ctx, start, &left.geometry)?
        || !point_on_surface(ctx, start, &right.geometry)?
        || !point_on_surface(ctx, end, &left.geometry)?
        || !point_on_surface(ctx, end, &right.geometry)?
    {
        return Ok(None);
    }
    let Some(direction) = FiniteVector3::new(end.vector_from(start)) else {
        return Ok(None);
    };
    let length = direction.x.hypot(direction.y).hypot(direction.z);
    if !length.is_finite() || length == 0.0 {
        return Ok(None);
    }
    let Some(direction) = direction.unit_nonzero() else {
        return Ok(None);
    };
    let follows_carrier_line = match (&left.geometry, &right.geometry) {
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
        ) => {
            let left_normal = *plane_surface.frame().axis();
            let right_normal = *plane_surface_2.frame().axis();
            left_normal
                .finite_cross(right_normal)
                .unit_nonzero()
                .is_some_and(|intersection| direction.cross(intersection).norm() <= TOLERANCE)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)),
        ) if { support.faces[0] == support.faces[1] } => {
            let axis = cylinder_surface.frame().axis().as_raw();
            direction.cross(*axis).norm() <= TOLERANCE
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)),
        ) if { support.faces[0] == support.faces[1] } => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let radius = cone_surface.radius().get();
            let half_angle = cone_surface.half_angle().get();
            let tangent = half_angle.tan();
            if !tangent.is_finite() || tangent == 0.0 {
                false
            } else {
                let apex = origin.translated(*axis, -radius / tangent);
                apex.vector_from(start).cross(direction).norm() <= TOLERANCE
            }
        }
        _ => false,
    };
    if !follows_carrier_line {
        return Ok(None);
    }
    Ok(Some((
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::new(
                start_point.position(),
                match cadmpeg_ir::units::UnitVector3::new(direction) {
                    Some(value) => value,
                    None => return Ok(None),
                },
            ),
        )),
        [0.0, length],
    )))
}

pub(super) fn standard_spline_circle(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Result<Option<CurveGeometry>, CodecError> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let surfaces = [
        face_surface(ctx, ir, bindings, surface_indices, support.faces[0])?,
        face_surface(ctx, ir, bindings, surface_indices, support.faces[1])?,
    ];
    let [Some(left), Some(right)] = surfaces else {
        return Ok(None);
    };
    let (sphere_center, sphere_radius, plane_origin, plane_normal) =
        match (&left.geometry, &right.geometry) {
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            ) => {
                let center = sphere_surface.center().get();
                let radius = sphere_surface.radius().abs();
                let origin = plane_surface.origin().get();
                let normal = plane_surface.frame().axis().as_raw();
                (center, radius, origin, *normal)
            }
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface_2)),
            ) => {
                let origin = plane_surface_2.origin();
                let normal = plane_surface_2.frame().axis().as_raw();
                let center = sphere_surface_2.center();
                let radius = sphere_surface_2.radius().abs();
                (*center, radius, *origin, *normal)
            }
            _ => return Ok(None),
        };
    let axis = plane_normal;
    let sphere_radius = sphere_radius.get();
    let signed_distance = sphere_center.vector_from(plane_origin).dot(axis);
    if !signed_distance.is_finite() {
        return Ok(None);
    }
    let section_radius_squared = sphere_radius * sphere_radius - signed_distance * signed_distance;
    if !section_radius_squared.is_finite()
        || section_radius_squared <= SPHERE_SECTION_ENDPOINT_TOLERANCE.powi(2)
    {
        return Ok(None);
    }
    let section_center = sphere_center.translated(axis, -signed_distance);
    let section_radius = section_radius_squared.sqrt();
    let Some(start) = ir
        .model
        .points
        .get(points[0])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    let Some(end) = ir
        .model
        .points
        .get(points[1])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    if !point_on_surface(ctx, start, &left.geometry)?
        || !point_on_surface(ctx, start, &right.geometry)?
        || !point_on_surface(ctx, end, &left.geometry)?
        || !point_on_surface(ctx, end, &right.geometry)?
        || (start.distance(section_center) - section_radius).abs()
            > SPHERE_SECTION_ENDPOINT_TOLERANCE
        || (end.distance(section_center) - section_radius).abs() > SPHERE_SECTION_ENDPOINT_TOLERANCE
    {
        return Ok(None);
    }
    let Ok(circle) = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
        section_center,
        axis,
        cadmpeg_ir::geometry::derive_reference_direction(axis),
        section_radius,
    ) else {
        return Ok(None);
    };
    Ok(Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        circle,
    ))))
}

pub(super) fn standard_spline_cylinder_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Result<Option<CurveGeometry>, CodecError> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let surfaces = [
        face_surface(ctx, ir, bindings, surface_indices, support.faces[0])?,
        face_surface(ctx, ir, bindings, surface_indices, support.faces[1])?,
    ];
    let [Some(left), Some(right)] = surfaces else {
        return Ok(None);
    };
    let (cylinder_axis, cylinder_origin, cylinder_radius, plane_origin, plane_axis) =
        match (&left.geometry, &right.geometry) {
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            )
            | (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
            ) => (
                *cylinder_surface.frame().axis().as_raw(),
                *cylinder_surface.origin(),
                cylinder_surface.radius(),
                *plane_surface.origin(),
                *plane_surface.frame().axis(),
            ),
            _ => return Ok(None),
        };
    let plane_normal = *plane_axis.as_raw();
    let axis_dot_normal = cylinder_axis.dot(plane_normal);
    if !axis_dot_normal.is_finite() || axis_dot_normal.abs() <= CYLINDER_PLANE_CONIC_TOLERANCE {
        return Ok(None);
    }
    let axis_parameter =
        -cylinder_origin.vector_from(plane_origin).dot(plane_normal) / axis_dot_normal;
    if !axis_parameter.is_finite() {
        return Ok(None);
    }
    let center = cylinder_origin.translated(cylinder_axis, axis_parameter);
    let Some(start) = ir
        .model
        .points
        .get(points[0])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    let Some(end) = ir
        .model
        .points
        .get(points[1])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    if !point_on_surface(ctx, start, &left.geometry)?
        || !point_on_surface(ctx, start, &right.geometry)?
        || !point_on_surface(ctx, end, &left.geometry)?
        || !point_on_surface(ctx, end, &right.geometry)?
    {
        return Ok(None);
    }
    let minor_vector = cylinder_axis.cross(plane_normal);
    let minor_norm = minor_vector.norm();
    if !minor_norm.is_finite() {
        return Ok(None);
    }
    if minor_norm <= CYLINDER_PLANE_CONIC_TOLERANCE {
        let Some(frame) = OrthonormalFrame3::from_units(
            plane_axis,
            match UnitVector3::new(cadmpeg_ir::geometry::derive_reference_direction(
                plane_normal,
            )) {
                Some(value) => value,
                None => return Ok(None),
            },
        ) else {
            return Ok(None);
        };
        return Ok(Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::new(
                match FinitePoint3::new(center) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                frame,
                cylinder_radius,
            ),
        ))));
    }
    let minor_direction = minor_vector.scale(1.0 / minor_norm);
    let radial_normal =
        (plane_normal - cylinder_axis.scale(axis_dot_normal)).scale(1.0 / minor_norm);
    let major_unscaled = radial_normal - cylinder_axis.scale(minor_norm / axis_dot_normal);
    let major_norm = major_unscaled.norm();
    if !major_norm.is_finite() || major_norm <= 0.0 {
        return Ok(None);
    }
    let major_direction = major_unscaled.scale(1.0 / major_norm);
    let Some(major_radius) = PositiveLength::new(cylinder_radius.get() * major_norm) else {
        return Ok(None);
    };
    let endpoint_is_on_ellipse = |point: Point3| {
        let offset = point.vector_from(center);
        let major = offset.dot(major_direction) / major_radius.get();
        let minor = offset.dot(minor_direction) / cylinder_radius.get();
        let equation = major * major + minor * minor;
        equation.is_finite() && (equation - 1.0).abs() <= CYLINDER_PLANE_CONIC_TOLERANCE
    };
    if !endpoint_is_on_ellipse(start) || !endpoint_is_on_ellipse(end) {
        return Ok(None);
    }
    let Some(major_direction) = UnitVector3::new(major_direction) else {
        return Ok(None);
    };
    let Some(frame) = OrthonormalFrame3::from_units(plane_axis, major_direction) else {
        return Ok(None);
    };
    let Some(center) = FinitePoint3::new(center) else {
        return Ok(None);
    };
    let Ok(ellipse) = cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
        center,
        frame,
        major_radius,
        cylinder_radius,
    ) else {
        return Ok(None);
    };
    Ok(Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        ellipse,
    ))))
}

pub(super) fn standard_spline_perpendicular_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Result<Option<CurveGeometry>, CodecError> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let surfaces = [
        face_surface(ctx, ir, bindings, surface_indices, support.faces[0])?,
        face_surface(ctx, ir, bindings, surface_indices, support.faces[1])?,
    ];
    let [Some(left), Some(right)] = surfaces else {
        return Ok(None);
    };
    let (first_axis, first_origin, first_radius, second_axis, second_origin, second_radius) =
        match (&left.geometry, &right.geometry) {
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
            ) => {
                let origin = cylinder_surface.origin().get();
                let axis = cylinder_surface.frame().axis().as_raw();
                let radius = cylinder_surface.radius().get();
                let second_origin = cylinder_surface_2.origin();
                let second_axis = cylinder_surface_2.frame().axis().as_raw();
                let second_radius = cylinder_surface_2.radius().get();
                (
                    *axis,
                    origin,
                    radius,
                    *second_axis,
                    *second_origin,
                    second_radius,
                )
            }
            _ => return Ok(None),
        };
    if (first_radius - second_radius).abs() > PERPENDICULAR_CYLINDER_CONIC_TOLERANCE {
        return Ok(None);
    }
    let axis_dot = first_axis.dot(second_axis);
    if !axis_dot.is_finite() || axis_dot.abs() > PERPENDICULAR_CYLINDER_CONIC_TOLERANCE {
        return Ok(None);
    }
    let denominator = 1.0 - axis_dot * axis_dot;
    if !denominator.is_finite() || denominator <= 0.0 {
        return Ok(None);
    }
    let axis_offset = second_origin.vector_from(first_origin);
    let first_parameter =
        (axis_offset.dot(first_axis) - axis_dot * axis_offset.dot(second_axis)) / denominator;
    let second_parameter = axis_dot * first_parameter - axis_offset.dot(second_axis);
    if !first_parameter.is_finite() || !second_parameter.is_finite() {
        return Ok(None);
    }
    let first_center = first_origin.translated(first_axis, first_parameter);
    let second_center = second_origin.translated(second_axis, second_parameter);
    if first_center.distance(second_center) > PERPENDICULAR_CYLINDER_CONIC_TOLERANCE {
        return Ok(None);
    }
    let center = Point3::new(
        (first_center.x + second_center.x) * 0.5,
        (first_center.y + second_center.y) * 0.5,
        (first_center.z + second_center.z) * 0.5,
    );
    let Some(start) = ir
        .model
        .points
        .get(points[0])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    let Some(end) = ir
        .model
        .points
        .get(points[1])
        .map(|point| point.position().get())
    else {
        return Ok(None);
    };
    if !point_on_surface(ctx, start, &left.geometry)?
        || !point_on_surface(ctx, start, &right.geometry)?
        || !point_on_surface(ctx, end, &left.geometry)?
        || !point_on_surface(ctx, end, &right.geometry)?
    {
        return Ok(None);
    }
    let Some(minor_direction) = unit_vector(first_axis.cross(second_axis)) else {
        return Ok(None);
    };
    let radius = (first_radius + second_radius) * 0.5;
    let major_radius = radius * 2.0_f64.sqrt();
    let Some(radius) = cadmpeg_ir::scalar::PositiveLength::new(radius) else {
        return Ok(None);
    };
    let Some(major_radius) = cadmpeg_ir::scalar::PositiveLength::new(major_radius) else {
        return Ok(None);
    };
    let Some(center) = FinitePoint3::new(center) else {
        return Ok(None);
    };
    let mut branches = [
        (first_axis - second_axis, first_axis + second_axis),
        (first_axis + second_axis, first_axis - second_axis),
    ]
    .into_iter()
    .filter_map(|(axis, major_direction)| {
        let axis = unit_vector(axis)?;
        let major_direction = unit_vector(major_direction)?;
        let endpoint_is_on_branch = |point: Point3| {
            let offset = point.vector_from(center.get());
            let major = offset.dot(*major_direction.as_raw()) / major_radius.get();
            let minor = offset.dot(*minor_direction.as_raw()) / radius.get();
            let equation = major * major + minor * minor;
            offset.dot(*axis.as_raw()).abs() <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
                && equation.is_finite()
                && (equation - 1.0).abs() <= PERPENDICULAR_CYLINDER_CONIC_TOLERANCE
        };
        let frame = OrthonormalFrame3::from_units(axis, major_direction)?;
        (endpoint_is_on_branch(start) && endpoint_is_on_branch(end)).then_some(
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_from_parts(
                    center,
                    frame,
                    major_radius,
                    radius,
                )
                .ok()?,
            )),
        )
    });
    let Some(geometry) = branches.next() else {
        return Ok(None);
    };
    Ok(branches.next().is_none().then_some(geometry))
}

pub(super) fn standard_native_support_witness(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native: &StandardEdgeSupport,
) -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
    let parameter = 0.5 * (native.parameter_range[0] + native.parameter_range[1]);
    let lift = |carrier: &crate::families::b5::transfer::ResolvedPcurveSurface,
                pcurve: &PcurveGeometry|
     -> Result<Option<Point3>, cadmpeg_core::decode::ResourceLimit> {
        let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
        else {
            return Ok(None);
        };
        let Some(uv) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, parameter),
            )?)?
        else {
            return Ok(None);
        };
        Ok(
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv.u, uv.v),
            )?)?
            .map(cadmpeg_ir::features::FinitePoint3::get),
        )
    };
    let Some(first) = lift(&native.carriers[0], &native.pcurves[0])? else {
        return Ok(None);
    };
    let Some(second) = lift(&native.carriers[1], &native.pcurves[1])? else {
        return Ok(None);
    };
    Ok((first.distance_squared(second).sqrt() <= SUPPORT_AGREEMENT_TOLERANCE).then_some(first))
}

pub(super) fn standard_analytic_curve_angle(
    geometry: &CurveGeometry,
    point: Point3,
) -> Option<f64> {
    let (center, first, second, first_radius, second_radius) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            (
                center,
                *ref_direction,
                axis.cross(*ref_direction),
                radius,
                radius,
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            (
                center,
                *major_direction,
                axis.cross(*major_direction),
                major_radius,
                minor_radius,
            )
        }
        _ => return None,
    };
    let offset = point.vector_from(center);
    let first_component = offset.dot(first) / first_radius;
    let second_component = offset.dot(second) / second_radius;
    let residual = first_component * first_component + second_component * second_component - 1.0;
    (residual.is_finite() && residual.abs() <= ANALYTIC_CURVE_ENDPOINT_TOLERANCE)
        .then(|| second_component.atan2(first_component))
}

pub(super) fn standard_analytic_curve_parameter_range(
    geometry: &CurveGeometry,
    start: Point3,
    end: Point3,
    witness: Option<Point3>,
) -> Option<[f64; 2]> {
    let start_angle = standard_analytic_curve_angle(geometry, start)?;
    let end_angle = standard_analytic_curve_angle(geometry, end)?;
    // A short chord does not establish a closed edge. The same source vertex does.
    if start == end {
        if let Some(witness) = witness {
            standard_analytic_curve_angle(geometry, witness)?;
        }
        return Some([0.0, std::f64::consts::TAU]);
    }
    let start = start_angle;
    let short_end = unwrap_angle(end_angle, start);
    let end = witness.map_or(Some(short_end), |witness| {
        witness_arc_end(
            start,
            short_end,
            standard_analytic_curve_angle(geometry, witness)?,
        )
    })?;
    crate::nurbs::canonical_periodic_range([start, end])
}

pub(super) fn standard_oriented_analytic_curve_parameter_range(
    geometry: &mut CurveGeometry,
    start: Point3,
    end: Point3,
    witness: Point3,
) -> Option<[f64; 2]> {
    if let Some(range) =
        standard_analytic_curve_parameter_range(geometry, start, end, Some(witness))
    {
        return Some(range);
    }
    let original = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => {
            let original = *circle;
            circle.reverse_parameterization();
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(original))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) => {
            let original = *ellipse;
            ellipse.reverse_parameterization();
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(original))
        }
        _ => return None,
    };
    let range = standard_analytic_curve_parameter_range(geometry, start, end, Some(witness));
    if range.is_none() {
        *geometry = original;
    }
    range
}

pub(super) fn standard_oriented_native_support_pcurves(
    ctx: &DecodeContext<'_>,
    native: &StandardEdgeSupport,
    points: &[Point],
    endpoint_pair: [usize; 2],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<[PcurveGeometry; 2]>, cadmpeg_core::CodecError> {
    let copy_native = || -> Result<[PcurveGeometry; 2], cadmpeg_core::CodecError> {
        Ok([
            native.pcurves[0]
                .try_clone_for_decode(ctx, "catia_standard_native_support_pcurve_copy")?,
            native.pcurves[1]
                .try_clone_for_decode(ctx, "catia_standard_native_support_pcurve_copy")?,
        ])
    };
    let Some(native_pair) = standard_native_support_endpoint_pair(
        ctx,
        native,
        points,
        &endpoint_pair,
        Some(endpoint_pair),
    )?
    else {
        return Ok(Some(copy_native()?));
    };
    if native_pair == endpoint_pair {
        return Ok(Some(copy_native()?));
    }
    let (first_label, _first_label_reservation) = ctx.format_scoped(format_args!(
            "standard native edge-support pcurve 0 of the edge between points {} and {}, reversed onto its edge",
            endpoint_pair[0], endpoint_pair[1]
        ), "catia_standard_native_pcurve_reverse_label")?;
    let Some(first) = crate::nurbs::reverse_pcurve_geometry(
        ctx,
        &native.pcurves[0],
        native.parameter_range,
        refusal,
        &first_label,
    )?
    else {
        return Ok(None);
    };
    let (second_label, _second_label_reservation) = ctx.format_scoped(format_args!(
            "standard native edge-support pcurve 1 of the edge between points {} and {}, reversed onto its edge",
            endpoint_pair[0], endpoint_pair[1]
        ), "catia_standard_native_pcurve_reverse_label")?;
    let Some(second) = crate::nurbs::reverse_pcurve_geometry(
        ctx,
        &native.pcurves[1],
        native.parameter_range,
        refusal,
        &second_label,
    )?
    else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn real_bits(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

fn point_bits(point: Point3) -> [u64; 3] {
    [point.x, point.y, point.z].map(real_bits)
}

fn frame_bits(frame: &OrthonormalFrame3) -> [[u64; 3]; 2] {
    [frame.axis().as_raw(), frame.reference().as_raw()]
        .map(|vector| [vector.x, vector.y, vector.z].map(real_bits))
}

pub(super) fn nurbs_curve_fingerprint(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
) -> Result<u64, CodecError> {
    use std::hash::{Hash, Hasher};
    const OP: &str = "catia_standard_curve_fingerprint";
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        curve.degree(),
        curve.periodic(),
        curve.knots().len(),
        curve.pole_count(),
    )
        .hash(&mut hash);
    for &knot in ctx.admit_iter(curve.knots().as_slice(), OP)? {
        real_bits(knot).hash(&mut hash);
    }
    match curve.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            0u8.hash(&mut hash);
            for point in ctx.admit_iter(points, OP)? {
                point_bits(point.get()).hash(&mut hash);
            }
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            1u8.hash(&mut hash);
            for pole in ctx.admit_iter(points, OP)? {
                (point_bits(pole.point.get()), real_bits(pole.weight.get())).hash(&mut hash);
            }
        }
    }
    Ok(hash.finish())
}

fn solved_surface_fingerprint(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedSurfaceGeometry,
) -> Result<u64, CodecError> {
    use std::hash::{Hash, Hasher};
    const OP: &str = "catia_native_surface_fingerprint";
    let _depth = ctx.enter_nested(OP)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    match geometry {
        SolvedSurfaceGeometry::Plane(value) => (
            0u8,
            point_bits(value.origin().get()),
            frame_bits(value.frame()),
        )
            .hash(&mut hash),
        SolvedSurfaceGeometry::Cylinder(value) => (
            1u8,
            point_bits(value.origin().get()),
            frame_bits(value.frame()),
            real_bits(value.radius().get()),
        )
            .hash(&mut hash),
        SolvedSurfaceGeometry::Cone(value) => (
            2u8,
            point_bits(value.origin().get()),
            frame_bits(value.frame()),
            real_bits(value.radius().get()),
            real_bits(value.ratio().get()),
        )
            .hash(&mut hash),
        SolvedSurfaceGeometry::Sphere(value) => (
            3u8,
            point_bits(value.center().get()),
            frame_bits(value.frame()),
            real_bits(value.radius().get()),
        )
            .hash(&mut hash),
        SolvedSurfaceGeometry::Torus(value) => (
            4u8,
            point_bits(value.center().get()),
            frame_bits(value.frame()),
            real_bits(value.major_radius().get()),
            real_bits(value.minor_radius().get()),
        )
            .hash(&mut hash),
        SolvedSurfaceGeometry::Nurbs(value) => {
            (
                5u8,
                value.u_degree(),
                value.v_degree(),
                value.normal_reversed(),
                value.u_periodic(),
                value.v_periodic(),
                value.u_knots().len(),
                value.v_knots().len(),
                value.u_count(),
                value.v_count(),
            )
                .hash(&mut hash);
            for knots in [value.u_knots(), value.v_knots()] {
                for &knot in ctx.admit_iter(knots.as_slice(), OP)? {
                    real_bits(knot).hash(&mut hash);
                }
            }
            match value.pole_grid() {
                cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Polynomial { rows } => {
                    0u8.hash(&mut hash);
                    for row in ctx.admit_iter(rows, OP)? {
                        row.len().hash(&mut hash);
                        for point in ctx.admit_iter(row, OP)? {
                            point_bits(point.get()).hash(&mut hash);
                        }
                    }
                }
                cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { rows } => {
                    1u8.hash(&mut hash);
                    for row in ctx.admit_iter(rows, OP)? {
                        row.len().hash(&mut hash);
                        for pole in ctx.admit_iter(row, OP)? {
                            (point_bits(pole.point.get()), real_bits(pole.weight.get()))
                                .hash(&mut hash);
                        }
                    }
                }
            }
        }
        SolvedSurfaceGeometry::Polygonal(value) => {
            (
                6u8,
                real_bits(value.chordal_deflection().get()),
                value.vertices().len(),
                value.triangles().len(),
            )
                .hash(&mut hash);
            for point in ctx.admit_iter(value.vertices(), OP)? {
                point_bits(point.get()).hash(&mut hash);
            }
            for triangle in ctx.admit_iter(value.triangles(), OP)? {
                triangle.hash(&mut hash);
            }
        }
        SolvedSurfaceGeometry::Transformed(value) => {
            (
                7u8,
                value.transform().rows().map(|row| row.map(real_bits)),
                solved_surface_fingerprint(ctx, value.basis())?,
            )
                .hash(&mut hash);
        }
        SolvedSurfaceGeometry::Unknown { record } => {
            (
                8u8,
                record
                    .as_ref()
                    .map(|id| ctx.hash_value(id.as_str(), OP))
                    .transpose()?,
            )
                .hash(&mut hash);
        }
    }
    Ok(hash.finish())
}

fn surface_fingerprint(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
) -> Result<u64, CodecError> {
    use std::hash::{Hash, Hasher};
    const OP: &str = "catia_native_surface_fingerprint";
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    match geometry {
        SurfaceGeometry::Solved(geometry) => {
            (0u8, solved_surface_fingerprint(ctx, geometry)?).hash(&mut hash);
        }
        SurfaceGeometry::Procedural {
            construction,
            cache,
        } => (
            1u8,
            ctx.hash_value(construction.as_str(), OP)?,
            cache
                .as_ref()
                .map(|geometry| solved_surface_fingerprint(ctx, geometry))
                .transpose()?,
        )
            .hash(&mut hash),
    }
    Ok(hash.finish())
}

struct GeometryBinding {
    representative: usize,
    unique: Option<usize>,
}

/// An append-only surface arena index. Repeated identities keep an explicit
/// tombstone when distinct surface ids match the same source or geometry.
pub(super) struct NativeSurfaceIndex<'ctx> {
    source: HashMap<String, Option<usize>>,
    geometry: HashMap<u64, Vec<GeometryBinding>>,
    indexed_len: usize,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> NativeSurfaceIndex<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            source: HashMap::new(),
            geometry: HashMap::new(),
            indexed_len: 0,
            storage: ctx.reserve_scoped(0, "catia_native_surface_index")?,
        })
    }

    fn synchronize(
        &mut self,
        ctx: &DecodeContext<'_>,
        surfaces: &[Surface],
    ) -> Result<(), CodecError> {
        const OP: &str = "catia_native_surface_index";
        let mut tail = surfaces[self.indexed_len..].iter().enumerate();
        while let Some((relative, surface)) = ctx.next_charged(&mut tail, OP)? {
            let index = self.indexed_len + relative;
            if let Some(source) = surface.source_object.as_ref().filter(|source| {
                source.format == cadmpeg_ir::codec_format!(crate::dialect::FORMAT)
                    && source.name.is_none()
                    && source.color.is_none()
                    && source.visible.is_none()
                    && source.layer.is_none()
                    && source.instance_path.is_empty()
            }) {
                if let Some(binding) =
                    ctx.get_mut_hash_map(&mut self.source, source.object_id.as_str(), OP)?
                {
                    if let Some(previous) = *binding {
                        if !ctx.equal(&surfaces[previous].id, &surface.id, OP)? {
                            *binding = None;
                        }
                    }
                } else {
                    self.storage.with_storage(|| {
                        let key = ctx.copy_retained_text(source.object_id.as_str(), OP)?;
                        ctx.insert_hash_map(&mut self.source, key, Some(index), OP)
                    })?;
                }
            }
            let key = surface_fingerprint(ctx, &surface.geometry)?;
            let mut matched = false;
            if let Some(bucket) = ctx.get_mut_hash_map(&mut self.geometry, &key, OP)? {
                // SurfaceGeometry has no DecodeCost; collision equality is unpriced.
                if let Some(binding) = ctx.find_by(
                    bucket,
                    |binding| Ok(surfaces[binding.representative].geometry == surface.geometry),
                    OP,
                )? {
                    if let Some(previous) = binding.unique {
                        if !ctx.equal(&surfaces[previous].id, &surface.id, OP)? {
                            binding.unique = None;
                        }
                    }
                    matched = true;
                }
            }
            if !matched {
                self.storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut self.geometry,
                        key,
                        GeometryBinding {
                            representative: index,
                            unique: Some(index),
                        },
                        OP,
                        OP,
                    )
                })?;
            }
        }
        self.indexed_len = surfaces.len();
        Ok(())
    }
}

pub(super) struct BuildStandardEdgeCurveInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
    'input7,
    'input8,
    'input9,
    'input10,
    'input11,
    AnnotationAccount,
> {
    pub(super) ir: &'input0 mut CadIr,
    pub(super) annotations: &'input1 mut AnnotationBuilder<AnnotationAccount>,
    pub(super) bindings: &'input2 [(SurfaceId, bool, usize)],
    pub(super) surface_indices: &'input3 HashMap<SurfaceId, usize>,
    pub(super) brep: &'input4 [u8],
    pub(super) support: &'input5 crate::families::standard::records::StandardCurveSupport,
    pub(super) points: [usize; 2],
    pub(super) native_support: Option<&'input6 StandardEdgeSupport>,
    pub(super) limit_curve: Option<(&'input7 NurbsCurve, [f64; 2])>,
    pub(super) refusal: &'input8 mut crate::nurbs::LaneRefusals,
    pub(super) native_surfaces: &'input11 mut NativeSurfaceIndex<'input9>,
    pub(super) admission: &'input11 mut FamilyEntityAdmission<'input9, 'input10>,
}

pub(super) fn build_standard_edge_curve(
    ctx: &DecodeContext<'_>,
    inputs: BuildStandardEdgeCurveInputs<
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        impl cadmpeg_ir::annotations::AnnotationStorage,
    >,
) -> Result<(Option<CurveId>, Option<[f64; 2]>), cadmpeg_core::CodecError> {
    let BuildStandardEdgeCurveInputs {
        ir,
        annotations,
        bindings,
        surface_indices,
        brep,
        support,
        points,
        native_support,
        limit_curve,
        refusal,
        native_surfaces,
        admission,
    } = inputs;

    let (mut geometry, mut param_range) = match &support.geometry {
        crate::families::standard::records::StandardCurveGeometry::Line => {
            let start_point = &ir.model.points[points[0]];
            let start = start_point.position().get();
            let end = ir.model.points[points[1]].position().get();
            let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
            let length = delta.x.hypot(delta.y).hypot(delta.z);
            if !length.is_finite() || length == 0.0 {
                return Ok((None, None));
            }
            let Some(direction) = cadmpeg_ir::units::UnitVector3::new(Vector3::new(
                delta.x / length,
                delta.y / length,
                delta.z / length,
            )) else {
                return Ok((None, None));
            };
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::new(
                        start_point.position(),
                        direction,
                    ),
                )),
                Some([0.0, length]),
            )
        }
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
            let admitted_center = *center;
            let admitted_radius = *radius;
            let center = admitted_center.get();
            let radius = admitted_radius.get();
            let start = ir.model.points[points[0]].position().get();
            let end = ir.model.points[points[1]].position().get();
            // Two face carriers and at most two native carriers can state the
            // axis; the endpoints state it only when no carrier does.
            let mut axes = [None; 4];
            for (slot, &face) in axes.iter_mut().zip(&support.faces) {
                *slot =
                    face_surface(ctx, ir, bindings, surface_indices, face)?.and_then(|surface| {
                        standard_circle_axis_from_carrier(center, radius, &surface.geometry)
                    });
            }
            if let Some(native) = native_support {
                for (slot, carrier) in axes[2..].iter_mut().zip(&native.carriers) {
                    if let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) =
                        carrier
                    {
                        *slot = standard_circle_axis_from_carrier(center, radius, surface);
                    }
                }
            }
            if axes.iter().all(Option::is_none) {
                axes[0] = circle_axis_from_endpoints(center, radius, start, end);
            }
            let mut stated = axes.into_iter().flatten();
            let axis = stated.next();
            let conflicting_axes = axis.is_some_and(|axis| {
                stated.any(|other| axis.as_raw().dot(*other.as_raw()).abs() < 0.9999)
            });
            match axis.filter(|_| !conflicting_axes) {
                Some(axis) if points[0] == points[1] => {
                    match full_circle_frame(center, radius, axis, start) {
                        Some(frame) => (
                            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                                cadmpeg_ir::geometry::analytic::CircleCurve::new(
                                    admitted_center,
                                    frame,
                                    admitted_radius,
                                ),
                            )),
                            Some([0.0, std::f64::consts::TAU]),
                        ),
                        None => (
                            CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                                record: Some(
                                    ctx.copy_retained_text(
                                        "catia:payload:unknown#brep-stream",
                                        "catia_standard_unknown_curve_record_id",
                                    )
                                    .and_then(|text| {
                                        UnknownId::mint(text)
                                            .map_err(cadmpeg_core::CodecError::malformed)
                                    })?,
                                ),
                            }),
                            None,
                        ),
                    }
                }
                Some(axis) => {
                    let mut selected = None;
                    let mut ambiguous = false;
                    for candidate_axis in [axis, axis.reversed()] {
                        let reference = cadmpeg_ir::geometry::derive_reference_direction(
                            *candidate_axis.as_raw(),
                        );
                        let mut range = standard_circle_param_range(ctx, crate::families::standard::decode::edge_geometry::StandardCircleParamRangeInputs { ir, bindings, surface_indices, brep, support, center, radius, axis: *candidate_axis.as_raw(), ref_direction: reference, start, end, refusal })?;
                        if range.is_none() {
                            if let Some(native) = native_support {
                                range = native_support_circle_param_range(
                                    ctx,
                                    native,
                                    center,
                                    radius,
                                    *candidate_axis.as_raw(),
                                    reference,
                                    [start, end],
                                )?;
                            }
                        }
                        if let Some(range) = range.and_then(crate::nurbs::canonical_periodic_range)
                        {
                            if selected.is_some() {
                                ambiguous = true;
                            } else {
                                selected = Some((candidate_axis, reference, range));
                            }
                        }
                    }
                    let (axis, ref_direction, param_range) = if ambiguous {
                        (
                            axis,
                            cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw()),
                            None,
                        )
                    } else {
                        selected.map_or_else(
                            || {
                                (
                                    axis,
                                    cadmpeg_ir::geometry::derive_reference_direction(
                                        *axis.as_raw(),
                                    ),
                                    None,
                                )
                            },
                            |(axis, reference, range)| (axis, reference, Some(range)),
                        )
                    };
                    let Some(ref_direction) = UnitVector3::new(ref_direction) else {
                        return Ok((None, None));
                    };
                    let Some(frame) = OrthonormalFrame3::from_units(axis, ref_direction) else {
                        return Ok((None, None));
                    };
                    (
                        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                            cadmpeg_ir::geometry::analytic::CircleCurve::new(
                                admitted_center,
                                frame,
                                admitted_radius,
                            ),
                        )),
                        param_range,
                    )
                }
                None => (
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: Some(
                            ctx.copy_retained_text(
                                "catia:payload:unknown#brep-stream",
                                "catia_standard_unknown_curve_record_id",
                            )
                            .and_then(|text| {
                                UnknownId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
                            })?,
                        ),
                    }),
                    None,
                ),
            }
        }
        crate::families::standard::records::StandardCurveGeometry::Bspline => {
            if let Some((limit_curve, parameter_range)) = limit_curve {
                (
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        limit_curve.try_clone_for_decode(ctx, "catia_standard_limit_curve_copy")?,
                    )),
                    Some(parameter_range),
                )
            } else {
                match standard_spline_line(ctx, ir, bindings, surface_indices, support, points)? {
                    Some((geometry, range)) => (geometry, Some(range)),
                    None => {
                        match standard_spline_circle(
                            ctx,
                            ir,
                            bindings,
                            surface_indices,
                            support,
                            points,
                        )? {
                            Some(geometry) => (geometry, None),
                            None => match standard_spline_cylinder_plane(
                                ctx,
                                ir,
                                bindings,
                                surface_indices,
                                support,
                                points,
                            )? {
                                Some(geometry) => (geometry, None),
                                None => match standard_spline_perpendicular_cylinders(
                                    ctx,
                                    ir,
                                    bindings,
                                    surface_indices,
                                    support,
                                    points,
                                )? {
                                    Some(geometry) => (geometry, None),
                                    None => (
                                        CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                                            record: Some(
                                                ctx.copy_retained_text(
                                                    "catia:payload:unknown#brep-stream",
                                                    "catia_standard_unknown_curve_record_id",
                                                )
                                                .and_then(|text| {
                                                    UnknownId::mint(text).map_err(
                                                        cadmpeg_core::CodecError::malformed,
                                                    )
                                                })?,
                                            ),
                                        }),
                                        None,
                                    ),
                                },
                            },
                        }
                    }
                }
            }
        }
    };
    if param_range.is_none()
        && matches!(
            geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_))
        )
    {
        let endpoints = [
            ir.model.points[points[0]].position().get(),
            ir.model.points[points[1]].position().get(),
        ];
        let witness = match native_support {
            Some(native) => standard_native_support_witness(ctx, native)?,
            None => None,
        };
        if let Some(witness) = witness {
            param_range = standard_oriented_analytic_curve_parameter_range(
                &mut geometry,
                endpoints[0],
                endpoints[1],
                witness,
            );
        }
    }
    let oriented_native_support_pcurves = if matches!(
        &support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Bspline
    ) {
        match native_support {
            Some(native) => {
                match standard_oriented_native_support_pcurves(
                    ctx,
                    native,
                    &ir.model.points,
                    points,
                    refusal,
                )? {
                    Some(pcurves) => Some(pcurves),
                    None => return Ok((None, None)),
                }
            }
            None => None,
        }
    } else {
        None
    };
    admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
    let id = standard_id(
        ctx,
        "curve",
        format_args!("{}", support.pos),
        CurveId::mint,
        "catia_standard_edge_curve_identity",
    )?;
    annotate(
        ctx,
        annotations,
        &id,
        "MainDataStream+SurfacicReps",
        u64_from_index(support.pos),
        "curve_support_60",
        match (&support.geometry, &geometry) {
            (_, CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })) => Exactness::Unknown,
            (crate::families::standard::records::StandardCurveGeometry::Bspline, _) => {
                Exactness::Derived
            }
            _ => Exactness::ByteExact,
        },
    )?;
    if matches!(
        &geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ) {
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.origin")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.direction")?;
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Bspline,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)),
        )
    ) {
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.center")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.axis")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.ref_direction")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.radius")?;
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Bspline,
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_)),
        )
    ) {
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.center")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.axis")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.major_direction")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.major_radius")?;
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.minor_radius")?;
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Circle { .. },
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)),
        )
    ) {
        crate::resource::derived_annotation(ctx, annotations, &id, "geometry.axis")?;
    }
    let geometry_is_unknown = matches!(
        &geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
    );
    ir.model.curves.push(Curve {
        id: id.try_clone_for_decode(ctx, "catia_standard_model_curve_id_copy")?,
        geometry,
        source_object: Some(cgm_source(ctx, "edge-support", support.tag)?),
    });
    if matches!(
        &support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Bspline
    ) {
        let sides = if let Some(native) = native_support {
            let Some([first_pcurve, second_pcurve]) = oriented_native_support_pcurves else {
                return Ok((None, None));
            };
            let first_surface = ensure_native_edge_support_surface(
                ir,
                annotations,
                native.surface_object_ids[0],
                &native.carriers[0],
                native_surfaces,
                admission,
            )?;
            let second_surface = ensure_native_edge_support_surface(
                ir,
                annotations,
                native.surface_object_ids[1],
                &native.carriers[1],
                native_surfaces,
                admission,
            )?;
            [
                IntcurveSupportSide {
                    surface: Some(first_surface),
                    pcurve: Some(SupportPcurve::new(
                        first_pcurve,
                        DirectedParameterRange::new(native.parameter_range).ok(),
                    )),
                },
                IntcurveSupportSide {
                    surface: Some(second_surface),
                    pcurve: Some(SupportPcurve::new(
                        second_pcurve,
                        DirectedParameterRange::new(native.parameter_range).ok(),
                    )),
                },
            ]
        } else {
            let side = |face| -> Result<IntcurveSupportSide, CodecError> {
                let surface = match bindings.get(face) {
                    Some((id, _, _))
                        if ctx.contains_key_hash_map(
                            surface_indices,
                            id,
                            "catia_standard_intersection_side_surface_lookup",
                        )? =>
                    {
                        Some(id.try_clone_for_decode(
                            ctx,
                            "catia_standard_intersection_side_surface_id",
                        )?)
                    }
                    _ => None,
                };
                Ok(IntcurveSupportSide {
                    surface,
                    pcurve: None,
                })
            };
            [side(support.faces[0])?, side(support.faces[1])?]
        };
        let distinct_sides = match (&sides[0].surface, &sides[1].surface) {
            (Some(first), Some(second)) => {
                Some(!ctx.equal(first, second, "catia_standard_intersection_side_surfaces")?)
            }
            _ => None,
        };
        if distinct_sides.is_some_and(|distinct| native_support.is_some() || distinct) {
            let curve_parameter_range = param_range.or_else(|| {
                geometry_is_unknown
                    .then(|| native_support.map_or([0.0, 1.0], |native| native.parameter_range))
            });
            if let Some(curve_parameter_range) = curve_parameter_range {
                let procedural_id = standard_id(
                    ctx,
                    "intersection",
                    format_args!("{}", support.pos),
                    ProceduralCurveId::mint,
                    "catia_standard_intersection_identity",
                )?;
                annotate(
                    ctx,
                    annotations,
                    &procedural_id,
                    "MainDataStream+SurfacicReps",
                    u64_from_index(support.pos),
                    "standard_surface_intersection",
                    Exactness::Derived,
                )?;
                crate::resource::derived_annotation(ctx, annotations, &procedural_id, "curve")?;
                crate::resource::derived_annotation(
                    ctx,
                    annotations,
                    &procedural_id,
                    "definition",
                )?;
                let Ok(context) = IntcurveSupportContext::try_new(
                    sides,
                    ordered_range(curve_parameter_range),
                    std::array::from_fn(|_| Vec::new()),
                ) else {
                    return Ok((None, None));
                };
                admission.reserve_entity(
                    &mut ir.model.procedural_curves,
                    "catia_family_emit_procedural_curves",
                )?;
                let procedural = ProceduralCurve::new(
                    procedural_id,
                    ProceduralCurveDefinition::Intersection {
                        context,
                        discontinuity_flag: false,
                        cache: None,
                    },
                );
                let _attached = ir.model.add_procedural_curve(
                    ctx,
                    &id.try_clone_for_decode(ctx, "catia_standard_edge_procedural_owner_id")?,
                    procedural,
                )?;
                param_range = Some(curve_parameter_range);
            }
        }
    }
    Ok((Some(id), param_range))
}

pub(super) fn ensure_native_edge_support_surface(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    surface_object_id: u32,
    carrier: &crate::families::b5::transfer::ResolvedPcurveSurface,
    index: &mut NativeSurfaceIndex<'_>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<SurfaceId, cadmpeg_core::CodecError> {
    let ctx = admission.context();
    index.synchronize(ctx, &ir.model.surfaces)?;
    let mut source_storage = ctx.reserve_scoped(0, "catia_native_edge_support_source")?;
    let source = source_storage.with_storage(|| cgm_source(ctx, "surface", surface_object_id))?;
    let source_match = ctx.get_hash_map(
        &index.source,
        source.object_id.as_str(),
        "catia_native_edge_support_source_scan",
    )?;
    if let Some(Some(surface)) = source_match {
        return ir.model.surfaces[*surface]
            .id
            .try_clone_for_decode(ctx, "catia_native_edge_support_matched_surface_id");
    }
    if source_match.is_none() {
        if let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry) = carrier {
            let key = surface_fingerprint(ctx, geometry)?;
            if let Some(bucket) = ctx.get_hash_map(
                &index.geometry,
                &key,
                "catia_native_edge_support_geometry_scan",
            )? {
                // SurfaceGeometry has no DecodeCost; collision equality is unpriced.
                if let Some(binding) = ctx.find_by(
                    bucket,
                    |binding| Ok(ir.model.surfaces[binding.representative].geometry == *geometry),
                    "catia_native_edge_support_geometry_scan",
                )? {
                    if let Some(surface) = binding.unique {
                        return ir.model.surfaces[surface].id.try_clone_for_decode(
                            ctx,
                            "catia_native_edge_support_matched_surface_id",
                        );
                    }
                }
            }
        }
    }
    let id = crate::resource::compose_u32_id(
        admission.context(),
        &cadmpeg_ir::identity_namespace!("catia", "standard", "edge-support-surface"),
        surface_object_id,
        SurfaceId::mint,
        "catia_native_edge_support_surface_id",
    )?;
    admission.reserve_entity(&mut ir.model.surfaces, "catia_family_emit_surfaces")?;
    let (geometry, procedural_id) = match carrier {
        crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry) => {
            let copy = geometry
                .try_clone_for_decode(admission.context(), "catia_native_edge_support_geometry")?;
            (copy, None)
        }
        crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall { .. } => {
            let procedural_id = crate::resource::compose_u32_id(
                admission.context(),
                &cadmpeg_ir::identity_namespace!("catia", "standard", "edge-support-definition"),
                surface_object_id,
                ProceduralSurfaceId::mint,
                "catia_native_edge_support_definition_id",
            )?;
            (
                SurfaceGeometry::Procedural {
                    construction: procedural_id.try_clone_for_decode(
                        admission.context(),
                        "catia_native_edge_support_construction_id",
                    )?,
                    cache: None,
                },
                Some(procedural_id),
            )
        }
    };
    annotate(
        admission.context(),
        annotations,
        &id,
        "CATPart",
        0,
        "native_edge_support_surface",
        Exactness::ByteExact,
    )?;
    source_storage.commit()?;
    ir.model.surfaces.push(Surface {
        id: id.try_clone_for_decode(admission.context(), "catia_native_edge_support_record_id")?,
        geometry,
        source_object: Some(source),
    });
    index.synchronize(ctx, &ir.model.surfaces)?;
    if let (
        Some(procedural_id),
        crate::families::b5::transfer::ResolvedPcurveSurface::RollingBall {
            carrier_object_id,
            definition,
        },
    ) = (procedural_id, carrier)
    {
        annotate(
            admission.context(),
            annotations,
            &procedural_id,
            "object_stream_a8_03_32",
            0,
            format_args!(
                "support_surface:{surface_object_id:08x}:result_carrier:{carrier_object_id:08x}"
            ),
            Exactness::ByteExact,
        )?;
        admission.reserve_entity(
            &mut ir.model.procedural_surfaces,
            "catia_family_emit_procedural_surfaces",
        )?;
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            procedural_id,
            crate::families::b5::transfer::surfaces::copy_rolling_ball_definition(
                admission.context(),
                definition,
            )?,
            None,
        ));
    }
    Ok(id)
}

fn circle_endpoint_range_choices(
    center: Point3,
    radius: f64,
    axis: UnitVector3,
    start: Point3,
    end: Point3,
) -> Option<CircleRangeChoices> {
    const ENDPOINT_TOLERANCE: f64 = 2e-3;

    if !radius.is_finite()
        || radius <= 0.0
        || (start.distance(center) - radius).abs() > ENDPOINT_TOLERANCE
        || (end.distance(center) - radius).abs() > ENDPOINT_TOLERANCE
    {
        return None;
    }
    if start.distance(end) <= ENDPOINT_TOLERANCE {
        return Some(CircleRangeChoices {
            ranges: [[0.0, std::f64::consts::TAU], [0.0; 2]],
            len: 1,
        });
    }
    let axis = axis.recharted_by_largest_component();
    let reference = cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw());
    let tangent = axis.as_raw().cross(reference);
    let angle = |point: Point3| {
        let offset = point.vector_from(center);
        offset
            .dot(tangent)
            .atan2(offset.dot(reference))
            .rem_euclid(std::f64::consts::TAU)
    };
    let mut endpoints = [angle(start), angle(end)];
    if endpoints.iter().any(|angle| !angle.is_finite()) {
        return None;
    }
    if endpoints[0].total_cmp(&endpoints[1]).is_gt() {
        endpoints.swap(0, 1);
    }
    let short = crate::nurbs::canonical_periodic_range(endpoints)?;
    let long = crate::nurbs::canonical_periodic_range([
        endpoints[1],
        endpoints[0] + std::f64::consts::TAU,
    ])?;
    Some(CircleRangeChoices {
        ranges: [short, long],
        len: 2,
    })
}

fn circular_segments(range: [f64; 2]) -> [Option<[f64; 2]>; 2] {
    let span = range[1] - range[0];
    let start = range[0].rem_euclid(std::f64::consts::TAU);
    let end = start + span;
    if end <= std::f64::consts::TAU {
        [Some([start, end]), None]
    } else {
        [
            Some([start, std::f64::consts::TAU]),
            Some([0.0, end - std::f64::consts::TAU]),
        ]
    }
}

/// Active summaries prune disjoint segments and whole coincident subtrees.
/// Group counts make selection and rollback independent of prefix length.
struct CircularIntervalIndex<'ctx> {
    tree: BoundsIndex<'ctx>,
    ranges: Vec<[f64; 2]>,
    leaves: Vec<[Option<usize>; 2]>,
    parents: Vec<Option<usize>>,
    active: Vec<Option<[[f64; 2]; 3]>>,
    counts: Vec<usize>,
}

impl<'ctx> CircularIntervalIndex<'ctx> {
    fn new<T: AsRef<[[f64; 2]]>>(
        ctx: &'ctx DecodeContext<'_>,
        choices: &[T],
    ) -> Result<(Self, Vec<Vec<usize>>), CodecError> {
        const OP: &str = "catia_standard_circle_interval_index";
        let mut groups = HashMap::new();
        let mut ranges = Vec::new();
        let mut rows = Vec::new();
        let mut entries = Vec::new();
        for choice in ctx.admit_iter(choices, OP)? {
            let mut row = Vec::new();
            let choices = choice.as_ref();
            let mut choices_iter = choices.iter();
            loop {
                let next = if choices.len() <= 2 {
                    choices_iter.next()
                } else {
                    ctx.next_charged(&mut choices_iter, OP)?
                };
                let Some(&range) = next else { break };
                let key = range.map(real_bits);
                let group = if let Some(&group) = ctx.get_hash_map(&groups, &key, OP)? {
                    group
                } else {
                    let group = ranges.len();
                    ctx.push_vec(&mut ranges, range, OP)?;
                    ctx.insert_hash_map(&mut groups, key, group, OP)?;
                    for segment in circular_segments(range).into_iter().flatten() {
                        if segment[1] - segment[0] <= EPS_STANDARD_DECODE_COARSE_GEOMETRY {
                            continue;
                        }
                        let bounds = if range.into_iter().chain(segment).all(f64::is_finite) {
                            [segment, [range[0]; 2], [range[1]; 2]]
                        } else {
                            [[f64::NEG_INFINITY, f64::INFINITY]; 3]
                        };
                        ctx.push_vec(
                            &mut entries,
                            BoundsEntry {
                                bounds,
                                item: group,
                            },
                            OP,
                        )?;
                    }
                    group
                };
                ctx.push_vec(&mut row, group, OP)?;
            }
            ctx.push_vec(&mut rows, row, OP)?;
        }
        let tree = BoundsIndex::new(ctx, &mut entries, OP)?;
        let mut parents = ctx.alloc_filled(tree.nodes.len(), None, OP)?;
        let mut leaves = ctx.alloc_filled(ranges.len(), [None; 2], OP)?;
        for (at, node) in ctx.admit_iter(&tree.nodes, OP)?.enumerate() {
            if let Some(group) = node.item {
                let slots = &mut leaves[group];
                slots[usize::from(slots[0].is_some())] = Some(at);
            } else {
                let left = at + 1;
                let right = tree.nodes[left].after;
                parents[left] = Some(at);
                parents[right] = Some(at);
            }
        }
        let active = ctx.alloc_filled(tree.nodes.len(), None, OP)?;
        let counts = ctx.alloc_filled(ranges.len(), 0, OP)?;
        Ok((
            Self {
                tree,
                ranges,
                leaves,
                parents,
                active,
                counts,
            },
            rows,
        ))
    }

    fn compatible(&self, ctx: &DecodeContext<'_>, group: usize) -> Result<bool, CodecError> {
        let range = self.ranges[group];
        if self.counts[group] > 0 && range.into_iter().all(f64::is_finite) {
            return Ok(true);
        }
        for segment in circular_segments(range).into_iter().flatten() {
            if segment[1] - segment[0] <= EPS_STANDARD_DECODE_COARSE_GEOMETRY {
                continue;
            }
            let relevant = |at: usize| {
                self.active[at].is_some_and(|bounds| {
                    let coincident = (bounds[1][0] - range[0]).abs()
                        <= EPS_STANDARD_DECODE_GEOMETRY
                        && (bounds[1][1] - range[0]).abs() <= EPS_STANDARD_DECODE_GEOMETRY
                        && (bounds[2][0] - range[1]).abs() <= EPS_STANDARD_DECODE_GEOMETRY
                        && (bounds[2][1] - range[1]).abs() <= EPS_STANDARD_DECODE_GEOMETRY;
                    !coincident
                        && !matches!(
                            (bounds[0][1].min(segment[1]) - bounds[0][0].max(segment[0]))
                                .partial_cmp(&EPS_STANDARD_DECODE_COARSE_GEOMETRY),
                            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                        )
                })
            };
            let nodes = std::iter::successors((!self.tree.nodes.is_empty()).then_some(0), |&at| {
                let next = if relevant(at) {
                    at + 1
                } else {
                    self.tree.nodes[at].after
                };
                (next < self.tree.nodes.len()).then_some(next)
            });
            if ctx.any_by(
                nodes,
                |at| {
                    Ok(relevant(at)
                        && self.tree.nodes[at].item.is_some_and(|other| {
                            !circular_ranges_are_compatible(self.ranges[other], range)
                        }))
                },
                "catia_standard_circle_range_selection",
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn select(
        &mut self,
        ctx: &DecodeContext<'_>,
        group: usize,
        add: bool,
    ) -> Result<(), CodecError> {
        if add {
            self.counts[group] += 1;
        } else {
            self.counts[group] -= 1;
        }
        if (add && self.counts[group] != 1) || (!add && self.counts[group] != 0) {
            return Ok(());
        }
        for leaf in self.leaves[group].into_iter().flatten() {
            self.active[leaf] = (self.counts[group] > 0).then_some(self.tree.nodes[leaf].bounds);
            let mut ancestors = std::iter::successors(self.parents[leaf], |&at| self.parents[at]);
            while let Some(at) =
                ctx.next_charged(&mut ancestors, "catia_standard_circle_interval_update")?
            {
                let left = at + 1;
                let right = self.tree.nodes[left].after;
                self.active[at] = match (self.active[left], self.active[right]) {
                    (Some(a), Some(b)) => Some(std::array::from_fn(|axis| {
                        [a[axis][0].min(b[axis][0]), a[axis][1].max(b[axis][1])]
                    })),
                    (Some(a), None) | (None, Some(a)) => Some(a),
                    (None, None) => None,
                };
            }
        }
        Ok(())
    }
}

pub(super) fn circular_range_choices_have_simple_selection<T: AsRef<[[f64; 2]]>>(
    ctx: &DecodeContext<'_>,
    choices: &[T],
) -> Result<bool, CodecError> {
    const MAX_SELECTION_STATES: usize = 4_096;
    fn visit(
        ctx: &DecodeContext<'_>,
        choices: &[Vec<usize>],
        index: usize,
        intervals: &mut CircularIntervalIndex<'_>,
        states: &mut usize,
    ) -> Result<Option<bool>, CodecError> {
        let _depth = ctx.enter_nested("catia_standard_circle_range_selection")?;
        if *states >= MAX_SELECTION_STATES {
            return Ok(None);
        }
        *states += 1;
        if index == choices.len() {
            return Ok(Some(true));
        }
        let row = &choices[index];
        let mut candidates = row.iter().enumerate();
        loop {
            let next = if row.len() <= 2 {
                candidates.next()
            } else {
                ctx.next_charged(&mut candidates, "catia_standard_iteration")?
            };
            let Some((choice_index, &group)) = next else {
                break;
            };
            if u16::try_from(choice_index).is_err() {
                return Ok(None);
            }
            if intervals.compatible(ctx, group)? {
                intervals.select(ctx, group, true)?;
                let result = visit(ctx, choices, index + 1, intervals, states)?;
                intervals.select(ctx, group, false)?;
                match result {
                    Some(true) | None => return Ok(result),
                    Some(false) => {}
                }
            }
        }
        Ok(Some(false))
    }
    if ctx.any_by(
        choices,
        |choice| Ok(choice.as_ref().is_empty()),
        "catia_standard_iteration",
    )? {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, "catia_standard_circle_interval_storage")?;
    storage.with_storage(|| {
        let (mut intervals, rows) = CircularIntervalIndex::new(ctx, choices)?;
        Ok(visit(ctx, &rows, 0, &mut intervals, &mut 0)?.unwrap_or(false))
    })
}

#[cfg(test)]
pub(super) fn circular_ranges_are_nonoverlapping_or_coincident(
    ctx: &DecodeContext<'_>,
    ranges: &[[f64; 2]],
) -> Result<bool, CodecError> {
    ctx.all_by(
        ranges.iter().enumerate(),
        |(left_index, &left)| {
            ctx.all_by(
                &ranges[left_index + 1..],
                |&right| Ok(circular_ranges_are_compatible(left, right)),
                "catia_standard_circle_range_pair_right",
            )
        },
        "catia_standard_circle_range_pair_left",
    )
}

/// Two circular parameter ranges are compatible when they coincide or their
/// arcs overlap by no more than the coarse geometry tolerance.
fn circular_ranges_are_compatible(left: [f64; 2], right: [f64; 2]) -> bool {
    let coincident = (right[0] - left[0]).abs() <= EPS_STANDARD_DECODE_GEOMETRY
        && (right[1] - left[1]).abs() <= EPS_STANDARD_DECODE_GEOMETRY;
    coincident
        || circular_segments(left)
            .into_iter()
            .flatten()
            .all(|left_segment| {
                circular_segments(right)
                    .into_iter()
                    .flatten()
                    .all(|right_segment| {
                        left_segment[1].min(right_segment[1])
                            - left_segment[0].max(right_segment[0])
                            <= EPS_STANDARD_DECODE_COARSE_GEOMETRY
                    })
            })
}

pub(super) struct StandardCircleParamRangeInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
> {
    pub(super) ir: &'input0 CadIr,
    pub(super) bindings: &'input1 [(SurfaceId, bool, usize)],
    pub(super) surface_indices: &'input2 HashMap<SurfaceId, usize>,
    pub(super) brep: &'input3 [u8],
    pub(super) support: &'input4 crate::families::standard::records::StandardCurveSupport,
    pub(super) center: Point3,
    pub(super) radius: f64,
    pub(super) axis: Vector3,
    pub(super) ref_direction: Vector3,
    pub(super) start: Point3,
    pub(super) end: Point3,
    pub(super) refusal: &'input5 mut crate::nurbs::LaneRefusals,
}

pub(super) fn standard_circle_param_range(
    ctx: &DecodeContext<'_>,
    inputs: StandardCircleParamRangeInputs<'_, '_, '_, '_, '_, '_>,
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let StandardCircleParamRangeInputs {
        ir,
        bindings,
        surface_indices,
        brep,
        support,
        center,
        radius,
        axis,
        ref_direction,
        start,
        end,
        refusal,
    } = inputs;

    let mut selected: Option<[f64; 2]> = None;
    for face in &support.faces {
        let Some(surface) = face_surface(ctx, ir, bindings, surface_indices, *face)? else {
            continue;
        };
        let Some(binding) = bindings.get(*face) else {
            continue;
        };
        let Some(witness) =
            crate::families::standard::records::standard_face_witness(brep, binding.2)
        else {
            continue;
        };
        let Some((PcurveGeometry::Line(line_pcurve), _)) = standard_pcurve_geometry(
            ctx,
            &surface.geometry,
            support,
            (start, end),
            Some(witness),
            None,
            refusal,
        )?
        else {
            continue;
        };
        let Some(range) = circle_parameter_range_from_surface_branch(
            ctx,
            crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                surface: &surface.geometry,
                center,
                radius,
                axis,
                ref_direction,
                start,
                end,
                pcurve_origin: *line_pcurve.origin(),
                pcurve_direction: (*line_pcurve.direction()).into(),
            },
        )?
        else {
            continue;
        };
        if let Some(first) = selected {
            if (range[0] - first[0]).abs() > EPS_STANDARD_DECODE_GEOMETRY
                || (range[1] - first[1]).abs() > EPS_STANDARD_DECODE_GEOMETRY
            {
                return Ok(None);
            }
        } else {
            selected = Some(range);
        }
    }
    Ok(selected)
}

pub(super) fn native_support_circle_param_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    support: &StandardEdgeSupport,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    endpoints: [Point3; 2],
) -> Result<Option<[f64; 2]>, cadmpeg_core::decode::ResourceLimit> {
    let [start, end] = endpoints;
    ctx.charge_work_limit(0, "geometry helper boundary")?;
    (|| -> Option<Result<[f64; 2], cadmpeg_core::decode::ResourceLimit>> {
        const GEOMETRY_TOLERANCE: f64 = 2e-3;

        let parameters = [
            support.parameter_range[0],
            0.5 * (support.parameter_range[0] + support.parameter_range[1]),
            support.parameter_range[1],
        ];
        let lift = |carrier: &crate::families::b5::transfer::ResolvedPcurveSurface,
                    pcurve: &PcurveGeometry| {
            let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) = carrier
            else {
                return None;
            };
            let carrier_axis = standard_circle_axis_from_carrier(center, radius, surface)?;
            (carrier_axis.as_raw().dot(axis) >= 0.9999).then_some(())?;
            // A non-finite lift is measured as a finite one is.
            Some(super::lifted_standard_support_parameters(
                ctx, surface, pcurve, parameters,
            ))
        };
        let [first_start, first_middle, first_end] =
            match lift(&support.carriers[0], &support.pcurves[0])? {
                Ok(points) => points,
                Err(limit) => return Some(Err(limit)),
            };
        let [second_start, second_middle, second_end] =
            match lift(&support.carriers[1], &support.pcurves[1])? {
                Ok(points) => points,
                Err(limit) => return Some(Err(limit)),
            };
        let first = [first_start?, first_middle?, first_end?];
        let second = [second_start?, second_middle?, second_end?];
        if first
            .iter()
            .zip(&second)
            .any(|(left, right)| left.distance_squared(*right).sqrt() > SUPPORT_AGREEMENT_TOLERANCE)
        {
            return None;
        }
        let endpoint_error = |left: Point3, right: Point3| left.distance_squared(right).sqrt();
        let source_forward = endpoint_error(first[0], start) <= GEOMETRY_TOLERANCE
            && endpoint_error(first[2], end) <= GEOMETRY_TOLERANCE;
        let source_reversed = endpoint_error(first[0], end) <= GEOMETRY_TOLERANCE
            && endpoint_error(first[2], start) <= GEOMETRY_TOLERANCE;
        if source_forward == source_reversed {
            return None;
        }
        let witness = first[1];
        let transverse = axis.cross(ref_direction);
        let angle = |point: Point3| {
            let radial = point.vector_from(center);
            let axial = radial.dot(axis);
            let radial_length = radial.norm();
            (axial.abs() <= GEOMETRY_TOLERANCE
                && (radial_length - radius).abs() <= GEOMETRY_TOLERANCE)
                .then(|| radial.dot(transverse).atan2(radial.dot(ref_direction)))
        };
        let start_angle = angle(start)?;
        let end_angle = unwrap_angle(angle(end)?, start_angle);
        let witness_angle = angle(witness)?;
        let selected_end = witness_arc_end(start_angle, end_angle, witness_angle)?;
        Some(Ok([start_angle, selected_end]))
    })()
    .transpose()
}

/// Each face binding's surface geometry, through one index of the surface
/// arena; the first surface with an identity owns it.
fn bound_face_geometries<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    surfaces: &'a [Surface],
    bindings: &[(SurfaceId, bool, usize)],
) -> Result<
    (
        Vec<Option<&'a SurfaceGeometry>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    const OPERATION: &str = "catia_standard_bound_face_geometries";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut first_surfaces = HashMap::<&SurfaceId, usize>::new();
    for (index, surface) in ctx.admit_iter(surfaces, OPERATION)?.enumerate() {
        if ctx
            .get_hash_map(&first_surfaces, &&surface.id, OPERATION)?
            .is_none()
        {
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut first_surfaces, &surface.id, index, OPERATION)
            })?;
        }
    }
    let mut geometries = Vec::new();
    ctx.reserve_scoped_vec(&mut storage, &mut geometries, bindings.len(), OPERATION)?;
    for (surface_id, _, _) in ctx.admit_iter(bindings, OPERATION)? {
        geometries.push(
            ctx.get_hash_map(&first_surfaces, &surface_id, OPERATION)?
                .map(|&index| &surfaces[index].geometry),
        );
    }
    Ok((geometries, storage))
}

pub(super) fn attach_standard_circles(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    bindings: &[(SurfaceId, bool, usize)],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut plans = Vec::new();
    let (face_geometries, mut storage) = bound_face_geometries(ctx, &ir.model.surfaces, bindings)?;
    for support in ctx.admit_iter(supports, "catia_standard_attached_circles")? {
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            support.geometry
        else {
            continue;
        };
        let mut stated = support.faces.into_iter().filter_map(|face| {
            face_geometries
                .get(face)
                .copied()
                .flatten()
                .and_then(|geometry| {
                    standard_circle_axis_from_carrier(center.get(), radius.get(), geometry)
                })
        });
        let Some(axis) = stated.next() else {
            continue;
        };
        if stated.any(|other| axis.as_raw().dot(*other.as_raw()).abs() < 0.9999) {
            continue;
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut plans,
            (support, axis),
            "catia_standard_attached_circle_plans",
        )?;
    }
    drop(face_geometries);
    for &(support, axis) in ctx.admit_iter(&plans, "catia_standard_attached_circle_plans")? {
        let crate::families::standard::records::StandardCurveGeometry::Circle {
            center: admitted_center,
            radius: admitted_radius,
        } = support.geometry
        else {
            continue;
        };
        let index = ir.model.curves.len();
        let id = standard_id(
            admission.context(),
            "circle",
            format_args!("{index}"),
            CurveId::mint,
            "catia_standard_attached_circle_identity",
        )?;
        let Some(ref_direction) = UnitVector3::new(
            cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw()),
        ) else {
            continue;
        };
        let Some(frame) = OrthonormalFrame3::from_units(axis, ref_direction) else {
            continue;
        };
        let payload = cadmpeg_ir::geometry::analytic::CircleCurve::new(
            admitted_center,
            frame,
            admitted_radius,
        );
        annotate(
            admission.context(),
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            u64_from_index(support.pos),
            "curve_support_60_circle",
            Exactness::ByteExact,
        )?;
        crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &id,
            "geometry.axis",
        )?;
        admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(payload)),
            source_object: Some(cgm_source(
                admission.context(),
                "edge-support",
                support.tag,
            )?),
        });
    }
    Ok(())
}

pub(super) fn circle_axis_from_endpoints(
    center: Point3,
    radius: f64,
    start: Point3,
    end: Point3,
) -> Option<UnitVector3> {
    let start_radius = start.vector_from(center);
    let end_radius = end.vector_from(center);
    let start_length = start_radius.norm();
    let end_length = end_radius.norm();
    if (start_length - radius).abs() > 1e-3 || (end_length - radius).abs() > 1e-3 {
        return None;
    }
    let normal = start_radius.cross(end_radius);
    (normal.norm() > EPS_STANDARD_DECODE_COARSE_GEOMETRY * start_length * end_length)
        .then(|| unit_vector(normal))
        .flatten()
}

pub(super) fn full_circle_frame(
    center: Point3,
    radius: f64,
    axis: UnitVector3,
    start: Point3,
) -> Option<OrthonormalFrame3> {
    const TOLERANCE: f64 = 2e-3;

    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let axis = axis.recharted_by_largest_component();
    let radial = start.vector_from(center);
    let radial_length = radial.norm();
    if !radial_length.is_finite() || (radial_length - radius).abs() > TOLERANCE {
        return None;
    }
    let ref_direction = unit_vector(radial)?;
    (axis.as_raw().dot(*ref_direction.as_raw()).abs() <= TOLERANCE)
        .then(|| OrthonormalFrame3::from_units(axis, ref_direction))
        .flatten()
}

pub(super) fn canonical_unoriented_axis(axis: Vector3) -> Option<UnitVector3> {
    let axis = unit_vector(axis)?;
    let value = [axis.as_raw().x, axis.as_raw().y, axis.as_raw().z]
        .into_iter()
        .max_by(|left, right| left.abs().total_cmp(&right.abs()))?;
    Some(if value.is_sign_negative() {
        axis.reversed()
    } else {
        axis
    })
}

pub(super) fn standard_circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<UnitVector3> {
    if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface {
        let sphere_center = sphere_surface.center();
        let sphere_radius = sphere_surface.radius().get();
        let center_distance = center.distance(*sphere_center);
        if center_distance <= SPHERE_CENTER_COINCIDENCE_TOLERANCE
            && close_length(circle_radius, sphere_radius)
        {
            return None;
        }
    }
    circle_axis_from_carrier(center, circle_radius, surface)
}

pub(super) fn circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<UnitVector3> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            close_length(center.vector_from(origin).dot(*normal), 0.0)
                .then_some(*plane_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            let offset = center.vector_from(origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, radius))
                .then_some(*cylinder_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let radius = cone_surface.radius().get();
            let half_angle = cone_surface.half_angle().get();
            let offset = center.vector_from(origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let section_radius = (radius + axial * half_angle.tan()).abs();
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, section_radius))
                .then_some(*cone_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let sphere_center = sphere_surface.center();
            let sphere_radius = sphere_surface.radius().get();
            FiniteVector3::new(center.vector_from(*sphere_center)).and_then(|offset| {
                let distance = offset.x.hypot(offset.y).hypot(offset.z);
                (distance.is_finite()
                    && distance != 0.0
                    && close_squared_lengths(distance.hypot(circle_radius), sphere_radius))
                .then(|| UnitVector3::normalized_nonzero(offset))
                .flatten()
            })
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let torus_center = torus_surface.center();
            let axis = torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let offset = center.vector_from(*torus_center);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let radial_distance = radial.norm();
            if close_length(axial, 0.0)
                && close_length(radial_distance, major_radius)
                && close_length(circle_radius, minor_radius)
            {
                unit_vector((*axis).cross(radial))
            } else if close_length(radial_distance, 0.0)
                && close_squared_lengths((circle_radius - major_radius).hypot(axial), minor_radius)
            {
                Some(*torus_surface.frame().axis())
            } else {
                None
            }
        }
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Nurbs(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed(_)
            | SolvedSurfaceGeometry::Unknown { .. },
        )
        | SurfaceGeometry::Procedural { .. } => None,
    }
}

pub(super) fn close_length(left: f64, right: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs() <= 1e-5 * (1.0 + left.abs().max(right.abs()))
}

// Preserve the squared-residual tolerance without forming unbounded squares.
pub(super) fn close_squared_lengths(left: f64, right: f64) -> bool {
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let scale = left.abs().max(right.abs()).max(1.0);
    let left = left / scale;
    let right = right / scale;
    (left - right).abs() * (left + right).abs()
        <= 2e-5 * ((1.0 / scale).powi(2) + left.abs().max(right.abs()).powi(2))
}

pub(super) fn attach_standard_lines(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    bindings: &[(SurfaceId, bool, usize)],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut plans = Vec::new();
    let (face_geometries, mut storage) = bound_face_geometries(ctx, &ir.model.surfaces, bindings)?;
    for support in ctx.admit_iter(supports, "catia_standard_attached_lines")? {
        if !matches!(
            support.geometry,
            crate::families::standard::records::StandardCurveGeometry::Line
        ) {
            continue;
        }
        let plane = |face: usize| plane_of(face_geometries.get(face).copied().flatten()?);
        let Some((origin_a, normal_a)) = plane(support.faces[0]) else {
            continue;
        };
        let Some((origin_b, normal_b)) = plane(support.faces[1]) else {
            continue;
        };
        let Some((origin, direction)) =
            plane_intersection_line(origin_a, normal_a, origin_b, normal_b)
        else {
            continue;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut plans,
            (support, origin, direction),
            "catia_standard_attached_line_plans",
        )?;
    }
    drop(face_geometries);
    for &(support, origin, direction) in
        ctx.admit_iter(&plans, "catia_standard_attached_line_plans")?
    {
        let index = ir.model.curves.len();
        let id = standard_id(
            admission.context(),
            "line",
            format_args!("{index}"),
            CurveId::mint,
            "catia_standard_attached_line_identity",
        )?;
        let Ok(payload) = cadmpeg_ir::geometry::analytic::LineCurve::try_new(origin, direction)
        else {
            continue;
        };
        annotate(
            admission.context(),
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            u64_from_index(support.pos),
            "curve_support_60_line",
            Exactness::ByteExact,
        )?;
        crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &id,
            "geometry.origin",
        )?;
        crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &id,
            "geometry.direction",
        )?;
        admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(payload)),
            source_object: Some(cgm_source(
                admission.context(),
                "edge-support",
                support.tag,
            )?),
        });
    }
    Ok(())
}

pub(super) fn plane_intersection_line(
    origin_a: Point3,
    normal_a: Vector3,
    origin_b: Point3,
    normal_b: Vector3,
) -> Option<(Point3, Vector3)> {
    let direction = normal_a.cross(normal_b);
    let direction_length = direction.x.hypot(direction.y).hypot(direction.z);
    if !direction_length.is_finite() || direction_length == 0.0 {
        return None;
    }
    let direction = direction.scale(1.0 / direction_length);
    let d_a = normal_a.dot(Vector3::new(origin_a.x, origin_a.y, origin_a.z));
    let d_b = normal_b.dot(Vector3::new(origin_b.x, origin_b.y, origin_b.z));
    let numerator = Vector3::new(
        d_a * normal_b.x - d_b * normal_a.x,
        d_a * normal_b.y - d_b * normal_a.y,
        d_a * normal_b.z - d_b * normal_a.z,
    );
    let scaled_origin = numerator.cross(direction);
    let origin = Point3::new(
        scaled_origin.x / direction_length,
        scaled_origin.y / direction_length,
        scaled_origin.z / direction_length,
    );
    origin.is_finite().then_some((origin, direction))
}

fn plane_of(geometry: &SurfaceGeometry) -> Option<(Point3, Vector3)> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => Some((
            plane_surface.origin().get(),
            *plane_surface.frame().axis().as_raw(),
        )),
        _ => None,
    }
}

const EPS_SAME_CONE_GENERATOR: f64 = 2e-3;
const EPS_PARAM_RESOLUTION_SPAN: f64 = 1.0e-7;
const EPS_PARAM_TOLERANCE_SPAN: f64 = EPS_STANDARD_DECODE_GEOMETRY;
const LINE_SEGMENT_GEOMETRY_TOLERANCE: f64 = 2e-3;
const NURBS_SHARED_BOUNDARY_TOLERANCE: f64 = EPS_STANDARD_DECODE_GEOMETRY;
const NURBS_LINE_FACE_SAMPLES: [f64; 3] = [0.25, 0.5, 0.75];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StandardLimitCurveBinding {
    pub(super) curve: usize,
    pub(super) points: [usize; 2],
    pub(super) parameter_range: [f64; 2],
}

type BezierSpan = [Point3; 6];

/// De Casteljau levels of one quintic span: `levels[d][i]` is control `i`
/// after `d` halving steps.
fn bezier_levels(control: BezierSpan) -> [[Point3; 6]; 6] {
    let mut levels = [control; 6];
    for degree in 0..5 {
        let source = levels[degree];
        for index in 0..5 - degree {
            let left = source[index];
            let right = source[index + 1];
            levels[degree + 1][index] = Point3::new(
                left.x.midpoint(right.x),
                left.y.midpoint(right.y),
                left.z.midpoint(right.z),
            );
        }
    }
    levels
}

pub(super) fn split_bezier_half(control: BezierSpan) -> (BezierSpan, BezierSpan) {
    let levels = bezier_levels(control);
    let left = std::array::from_fn(|index| levels[index][0]);
    let right = std::array::from_fn(|index| levels[5 - index][index]);
    (left, right)
}

pub(super) fn collect_bezier_point_parameters(
    ctx: &DecodeContext<'_>,
    control: BezierSpan,
    range: [f64; 2],
    point: Point3,
    tolerance: f64,
    parameter_resolution: f64,
    parameters: &mut Vec<(f64, f64)>,
) -> Result<(), CodecError> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;

    struct Node {
        control: BezierSpan,
        range: [f64; 2],
        depth: usize,
    }

    let lower_bound = |control: &BezierSpan| {
        let bounds = |coordinate: fn(Point3) -> f64| {
            control
                .map(coordinate)
                .into_iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                    (low.min(value), high.max(value))
                })
        };
        let axis_distance = |value: f64, low: f64, high: f64| {
            if value < low {
                low - value
            } else if value > high {
                value - high
            } else {
                0.0
            }
        };
        let [(x0, x1), (y0, y1), (z0, z1)] = [bounds(|p| p.x), bounds(|p| p.y), bounds(|p| p.z)];
        axis_distance(point.x, x0, x1)
            .hypot(axis_distance(point.y, y0, y1))
            .hypot(axis_distance(point.z, z0, z1))
    };
    let midpoint = |control: &BezierSpan| bezier_levels(*control)[5][0];

    let root_lower_bound = lower_bound(&control);
    if root_lower_bound > tolerance {
        return Ok(());
    }
    let root_midpoint = midpoint(&control);
    let mut best = (range[0].midpoint(range[1]), root_midpoint.distance(point));
    let first = control[0];
    let last = control[5];
    for (parameter, position) in [(range[0], first), (range[1], last)] {
        let distance = position.distance(point);
        if distance < best.1 {
            best = (parameter, distance);
        }
    }

    let mut search_storage = ctx.reserve_scoped(0, "catia_bezier_search_storage")?;
    let mut nodes = Vec::new();
    ctx.push_scoped_vec(
        &mut search_storage,
        &mut nodes,
        Node {
            control,
            range,
            depth: 0,
        },
        "catia_bezier_search_nodes",
    )?;
    let mut queue = BinaryHeap::new();
    search_storage.with_storage(|| {
        ctx.push_heap(
            &mut queue,
            (Reverse(root_lower_bound.to_bits()), 0usize),
            "catia_bezier_search_queue",
        )
    })?;
    while let Some((Reverse(lower_bits), node_index)) =
        ctx.pop_heap(&mut queue, "catia_bezier_search_work")?
    {
        let lower = f64::from_bits(lower_bits);
        if lower > tolerance || lower > best.1 {
            continue;
        }
        let node = &nodes[node_index];
        if node.depth >= 48 || node.range[1] - node.range[0] <= parameter_resolution {
            let position = midpoint(&node.control);
            let candidate = (
                node.range[0].midpoint(node.range[1]),
                position.distance(point),
            );
            if candidate.1 < best.1 {
                best = candidate;
            }
            if candidate.1 <= tolerance {
                ctx.push_vec(parameters, candidate, "catia_bezier_parameters")?;
            }
            continue;
        }
        let (left, right) = split_bezier_half(node.control);
        let middle = node.range[0].midpoint(node.range[1]);
        let depth = node.depth + 1;
        for (control, range) in [
            (left, [node.range[0], middle]),
            (right, [middle, node.range[1]]),
        ] {
            let lower = lower_bound(&control);
            if lower > tolerance || lower > best.1 {
                continue;
            }
            let position = midpoint(&control);
            let candidate = (range[0].midpoint(range[1]), position.distance(point));
            if candidate.1 < best.1 {
                best = candidate;
            }
            let index = nodes.len();
            ctx.push_scoped_vec(
                &mut search_storage,
                &mut nodes,
                Node {
                    control,
                    range,
                    depth,
                },
                "catia_bezier_search_nodes",
            )?;
            search_storage.with_storage(|| {
                ctx.push_heap(
                    &mut queue,
                    (Reverse(lower.to_bits()), index),
                    "catia_bezier_search_queue",
                )
            })?;
        }
    }
    if best.1 <= tolerance {
        ctx.push_vec(parameters, best, "catia_bezier_parameters")?;
    }
    Ok(())
}

pub(super) fn standard_limit_curve_point_parameter(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    point: Point3,
    tolerance: f64,
) -> Result<Option<f64>, CodecError> {
    const SPAN_WIDTH: std::num::NonZeroUsize = match std::num::NonZeroUsize::new(6) {
        Some(width) => width,
        None => std::num::NonZeroUsize::MIN,
    };
    let mut storage = ctx.reserve_scoped(0, "catia_limit_curve_parameter_scratch")?;
    storage.with_storage(|| -> Result<_, CodecError> {
        let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } = curve.pole_rows()
        else {
            return Ok(None);
        };
        let span_count = points.len() / 6;
        if span_count == 0
            || span_count * 6 != points.len()
            || curve.knots().len() != (span_count + 1) * 6
            || curve.degree() != 5
        {
            return Ok(None);
        }
        let Some(domain) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve) else {
            return Ok(None);
        };
        let [parameter_start, parameter_end] = domain.endpoints();
        let parameter_span = parameter_end - parameter_start;
        let span_control = |control_points: &[FinitePoint3]| -> BezierSpan {
            std::array::from_fn(|index| control_points[index].get())
        };
        let mut control_polygon_length = 0.0;
        for control_points in ctx
            .admit_iter(points, "catia_limit_curve_control_spans")?
            .chunks(SPAN_WIDTH)
        {
            let control = span_control(control_points);
            control_polygon_length += (0..5)
                .map(|index| control[index].distance(control[index + 1]))
                .sum::<f64>();
        }
        let (parameter_tolerance, parameter_resolution) = if parameter_span.is_finite() {
            let parameter_tolerance = (4.0 * tolerance * parameter_span
                / control_polygon_length.max(tolerance))
            .max(EPS_PARAM_TOLERANCE_SPAN * parameter_span);
            (
                parameter_tolerance,
                0.05 * parameter_tolerance.min(EPS_PARAM_RESOLUTION_SPAN * parameter_span),
            )
        } else {
            let half_span = parameter_end * 0.5 - parameter_start * 0.5;
            let tolerance_fraction = (4.0 * tolerance / control_polygon_length.max(tolerance))
                .max(EPS_PARAM_TOLERANCE_SPAN);
            (
                2.0 * (half_span * tolerance_fraction),
                2.0 * (half_span * (0.05 * tolerance_fraction.min(EPS_PARAM_RESOLUTION_SPAN))),
            )
        };
        let mut parameters = Vec::new();
        for (span, control_points) in ctx
            .admit_iter(points, "catia_limit_curve_spans")?
            .chunks(SPAN_WIDTH)
            .enumerate()
        {
            let control = span_control(control_points);
            collect_bezier_point_parameters(
                ctx,
                control,
                [curve.knots()[span * 6], curve.knots()[(span + 1) * 6]],
                point,
                tolerance,
                parameter_resolution,
                &mut parameters,
            )?;
        }
        ctx.stable_sort_by(
            &mut parameters,
            |value| &value.1,
            f64::total_cmp,
            "catia_limit_curve_point_parameters_sort",
        )?;
        let Some(&(parameter, _)) = parameters.first() else {
            return Ok(None);
        };
        let ambiguous = ctx.any_by(
            parameters.get(1..).unwrap_or_default(),
            |&(other, _)| Ok((other - parameter).abs() > parameter_tolerance),
            "catia_limit_curve_parameter_candidates",
        )?;
        Ok((!ambiguous).then_some(parameter))
    })
}

pub(super) fn standard_limit_curve_bindings(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    curves: &[NurbsCurve],
) -> Result<Vec<Vec<StandardLimitCurveBinding>>, CodecError> {
    const VERTEX_MATCH_TOLERANCE: f64 = 2e-3;

    if curves.is_empty() || supports.is_empty() {
        return ctx.collect_indexed_vec(supports.len(), "catia_limit_curve_edge_rows", |_| {
            Ok(Vec::new())
        });
    }
    let mut storage = ctx.reserve_scoped(0, "catia_limit_curve_binding_scratch")?;
    let (curve_points, support_index) = storage.with_storage(|| -> Result<_, CodecError> {
        let point_index = {
            let mut entry_storage = ctx.reserve_scoped(0, "catia_limit_curve_point_index")?;
            let mut point_entries = entry_storage.with_storage(|| -> Result<_, CodecError> {
                let mut entries = Vec::new();
                for (item, point) in ctx
                    .admit_iter(&ir.model.points, "catia_limit_curve_point_index")?
                    .enumerate()
                {
                    ctx.push_vec(
                        &mut entries,
                        BoundsEntry {
                            bounds: point_bounds(point.position().get(), 0.0),
                            item,
                        },
                        "catia_limit_curve_point_index",
                    )?;
                }
                Ok(entries)
            })?;
            BoundsIndex::new(ctx, &mut point_entries, "catia_limit_curve_point_index")?
        };
        let mut entry_storage = ctx.reserve_scoped(0, "catia_limit_curve_support_index")?;
        let mut support_entries = entry_storage.with_storage(|| -> Result<_, CodecError> {
            let mut entries = Vec::new();
            let mut surface_bounds = HashMap::new();
            for (item, support) in ctx
                .admit_iter(supports, "catia_limit_curve_support_index")?
                .enumerate()
            {
                if !matches!(
                    support.geometry,
                    crate::families::standard::records::StandardCurveGeometry::Bspline
                ) {
                    continue;
                }
                let mut bounds = [[f64::NEG_INFINITY, f64::INFINITY]; 3];
                for face in support.faces {
                    let Some(surface) = face_surface(ctx, ir, bindings, surface_indices, face)?
                    else {
                        continue;
                    };
                    if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) =
                        &surface.geometry
                    {
                        let control = if let Some(bounds) = ctx.get_hash_map(
                            &surface_bounds,
                            surface.id.as_str(),
                            "catia_limit_curve_surface_bounds",
                        )? {
                            *bounds
                        } else {
                            let bounds = nurbs_surface_control_bounds(ctx, nurbs)?;
                            ctx.insert_hash_map(
                                &mut surface_bounds,
                                surface.id.as_str(),
                                bounds,
                                "catia_limit_curve_surface_bounds",
                            )?;
                            bounds
                        };
                        if let Some(control) = control {
                            let scale = control
                                .map(|[low, high]| low.abs().max(high.abs()))
                                .into_iter()
                                .fold(1.0_f64, f64::max);
                            let margin =
                                NURBS_SURFACE_MEMBERSHIP_TOLERANCE + 64.0 * f64::EPSILON * scale;
                            bounds = control.map(|[low, high]| [low - margin, high + margin]);
                            break;
                        }
                    }
                }
                ctx.push_vec(
                    &mut entries,
                    BoundsEntry { bounds, item },
                    "catia_limit_curve_support_index",
                )?;
            }
            Ok(entries)
        })?;
        let support_index =
            BoundsIndex::new(ctx, &mut support_entries, "catia_limit_curve_support_index")?;
        let mut curve_points = Vec::new();
        ctx.reserve_vec(
            &mut curve_points,
            curves.len(),
            "catia_limit_curve_point_rows",
        )?;
        for curve in ctx.admit_iter(curves, "catia_limit_curve_curves")? {
            let mut row = Vec::new();
            if let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } =
                curve.pole_rows()
            {
                let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
                for point in ctx.admit_iter(points, "catia_limit_curve_control_bounds")? {
                    for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                        bounds[axis][0] = bounds[axis][0].min(value);
                        bounds[axis][1] = bounds[axis][1].max(value);
                    }
                }
                let scale = bounds
                    .map(|[low, high]| low.abs().max(high.abs()))
                    .into_iter()
                    .fold(1.0_f64, f64::max);
                let margin = VERTEX_MATCH_TOLERANCE + 64.0 * f64::EPSILON * scale;
                let bounds = bounds.map(|[low, high]| [low - margin, high + margin]);
                let mut query_storage = ctx.reserve_scoped(0, "catia_limit_curve_point_queries")?;
                let candidates = query_storage.with_storage(|| {
                    point_index.matching_items(ctx, bounds, "catia_limit_curve_point_queries")
                })?;
                for point in ctx.admit_iter(candidates, "catia_limit_curve_point_candidates")? {
                    if let Some(parameter) = standard_limit_curve_point_parameter(
                        ctx,
                        curve,
                        ir.model.points[point].position().get(),
                        VERTEX_MATCH_TOLERANCE,
                    )? {
                        ctx.push_vec(
                            &mut row,
                            (point, parameter),
                            "catia_limit_curve_point_parameters",
                        )?;
                    }
                }
            }
            curve_points.push(row);
        }
        Ok((curve_points, support_index))
    })?;
    let mut edge_curves =
        ctx.collect_indexed_vec(supports.len(), "catia_limit_curve_edge_rows", |_| {
            Ok(Vec::<StandardLimitCurveBinding>::new())
        })?;
    for (curve, points) in ctx
        .admit_iter(&curve_points, "catia_standard_iteration")?
        .enumerate()
    {
        if points.is_empty() {
            continue;
        }
        let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for &(point, _) in ctx.admit_iter(points, "catia_limit_curve_bound_points")? {
            let point = ir.model.points[point].position().get();
            for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                bounds[axis][0] = bounds[axis][0].min(value);
                bounds[axis][1] = bounds[axis][1].max(value);
            }
        }
        let mut query_storage = ctx.reserve_scoped(0, "catia_limit_curve_support_queries")?;
        let edges = query_storage.with_storage(|| {
            support_index.matching_items(ctx, bounds, "catia_limit_curve_support_queries")
        })?;
        for edge in ctx.admit_iter(edges, "catia_standard_iteration")? {
            let support = &supports[edge];
            let mut candidate_storage =
                ctx.reserve_scoped(0, "catia_limit_curve_candidate_scratch")?;
            let binding = candidate_storage.with_storage(|| -> Result<_, CodecError> {
                let mut candidates = [None; 2];
                let mut count = 0;
                let mut point_rows = points.iter().copied();
                while let Some((point, parameter)) =
                    ctx.next_charged(&mut point_rows, "catia_limit_curve_candidates")?
                {
                    let res = {
                        let position = ir.model.points[point].position().get();
                        let mut all_faces = true;
                        for face in support.faces {
                            let Some(surface) =
                                face_surface(ctx, ir, bindings, surface_indices, face)?
                            else {
                                all_faces = false;
                                break;
                            };
                            if !matches!(
                                surface.geometry,
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                            ) && !point_on_surface(ctx, position, &surface.geometry)?
                            {
                                all_faces = false;
                                break;
                            }
                        }
                        all_faces
                    };
                    if res {
                        if count == candidates.len() {
                            return Ok(None);
                        }
                        candidates[count] = Some((point, parameter));
                        count += 1;
                    }
                }
                let [Some((start, start_parameter)), Some((end, end_parameter))] = candidates
                else {
                    return Ok(None);
                };
                let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                    curves[curve].try_clone_for_decode(ctx, "catia_limit_curve_geometry_copy")?,
                ));
                let midpoint = match cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::curve_point(
                        ctx,
                        &geometry,
                        0.5 * (start_parameter + end_parameter),
                    ),
                )? {
                    Ok(point) => point,
                    Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                        return Err(limit.into());
                    }
                    Err(
                        cadmpeg_ir::eval::EvaluationFailure::NoValue
                        | cadmpeg_ir::eval::EvaluationFailure::NonFinite(_),
                    ) => return Ok(None),
                };
                let mut checked_surface = false;
                let mut agrees = true;
                for face in support.faces {
                    let Some(surface) = face_surface(ctx, ir, bindings, surface_indices, face)?
                    else {
                        agrees = false;
                        break;
                    };
                    if matches!(
                        surface.geometry,
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                    ) {
                        continue;
                    }
                    checked_surface = true;
                    if !point_on_surface(ctx, midpoint.get(), &surface.geometry)? {
                        agrees = false;
                        break;
                    }
                }
                Ok(
                    (checked_surface && agrees).then_some(StandardLimitCurveBinding {
                        curve,
                        points: [start, end],
                        parameter_range: [start_parameter, end_parameter],
                    }),
                )
            })?;
            if let Some(binding) = binding {
                ctx.push_vec(
                    &mut edge_curves[edge],
                    binding,
                    "catia_limit_curve_edge_bindings",
                )?;
            }
        }
    }
    Ok(edge_curves)
}

pub(super) fn resolve_standard_limit_curve_binding(
    ctx: &DecodeContext<'_>,
    bindings: &[StandardLimitCurveBinding],
    points: [usize; 2],
) -> Result<Option<StandardLimitCurveBinding>, CodecError> {
    let mut found = None;
    let ambiguous = ctx.any_by(
        bindings,
        |binding| {
            if !missing_edge::same_unordered_pair(binding.points, points) {
                return Ok(false);
            }
            Ok(found.replace(*binding).is_some())
        },
        "catia_limit_curve_binding_candidates",
    )?;
    let Some(mut binding) = found.filter(|_| !ambiguous) else {
        return Ok(None);
    };
    if binding.points != points {
        binding.points.reverse();
        binding.parameter_range.reverse();
    }
    Ok(Some(binding))
}

#[derive(Clone, Copy)]
pub(super) struct BoundsEntry {
    pub(super) bounds: [[f64; 2]; 3],
    pub(super) item: usize,
}

pub(super) struct BoundsNode {
    pub(super) bounds: [[f64; 2]; 3],
    pub(super) item: Option<usize>,
    after: usize,
}

/// A balanced, preorder tree. A disjoint node skips its whole subtree.
/// The reservation owns node storage until the index is dropped.
pub(super) struct BoundsIndex<'ctx> {
    pub(super) nodes: Vec<BoundsNode>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn bounds_overlap(left: [[f64; 2]; 3], right: [[f64; 2]; 3]) -> bool {
    (0..3).all(|axis| !(left[axis][1] < right[axis][0] || right[axis][1] < left[axis][0]))
}

pub(super) fn point_bounds(point: Point3, tolerance: f64) -> [[f64; 2]; 3] {
    [point.x, point.y, point.z].map(|value| [value - tolerance, value + tolerance])
}

impl<'ctx> BoundsIndex<'ctx> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'_>,
        entries: &mut [BoundsEntry],
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        fn build(
            ctx: &DecodeContext<'_>,
            entries: &mut [BoundsEntry],
            nodes: &mut Vec<BoundsNode>,
            storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
            operation: &'static str,
        ) -> Result<(), CodecError> {
            let _depth = ctx.enter_nested(operation)?;
            if entries.is_empty() {
                return Ok(());
            }
            let bounds = ctx.fold(
                entries,
                [[f64::INFINITY, f64::NEG_INFINITY]; 3],
                |mut bounds, entry| {
                    for (axis, interval) in bounds.iter_mut().enumerate() {
                        interval[0] = interval[0].min(entry.bounds[axis][0]);
                        interval[1] = interval[1].max(entry.bounds[axis][1]);
                    }
                    Ok(bounds)
                },
                operation,
            )?;
            let at = nodes.len();
            ctx.push_scoped_vec(
                storage,
                nodes,
                BoundsNode {
                    bounds,
                    item: (entries.len() == 1).then_some(entries[0].item),
                    after: 0,
                },
                operation,
            )?;
            if entries.len() > 1 {
                // Half extents avoid overflow for finite opposite-sign bounds.
                let extent = bounds.map(|[low, high]| high * 0.5 - low * 0.5);
                let mut axis = 0;
                for candidate in 1..3 {
                    if extent[candidate].total_cmp(&extent[axis]).is_gt() {
                        axis = candidate;
                    }
                }
                if entries.len() == 2 {
                    if entries[0].bounds[axis][0]
                        .total_cmp(&entries[1].bounds[axis][0])
                        .is_gt()
                    {
                        entries.swap(0, 1);
                    }
                } else {
                    ctx.sort_unstable_by(
                        entries,
                        |entry| &entry.bounds[axis][0],
                        f64::total_cmp,
                        operation,
                    )?;
                }
                let middle = entries.len() / 2;
                let (left, right) = entries.split_at_mut(middle);
                build(ctx, left, nodes, storage, operation)?;
                build(ctx, right, nodes, storage, operation)?;
            }
            nodes[at].after = nodes.len();
            Ok(())
        }
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let mut nodes = Vec::new();
        build(ctx, entries, &mut nodes, &mut storage, operation)?;
        Ok(Self {
            nodes,
            _storage: storage,
        })
    }

    /// Return overlapping leaf ordinals in source order.
    fn matching_items(
        &self,
        ctx: &DecodeContext<'_>,
        bounds: [[f64; 2]; 3],
        operation: &'static str,
    ) -> Result<Vec<usize>, CodecError> {
        let mut items = Vec::new();
        let mut nodes = self.overlapping(bounds);
        while let Some(node) = ctx.next_charged(&mut nodes, operation)? {
            if let Some(item) = node.item.filter(|_| bounds_overlap(node.bounds, bounds)) {
                ctx.push_vec(&mut items, item, operation)?;
            }
        }
        if items.len() > 2 {
            ctx.sort_unstable_by(&mut items, |value| value, Ord::cmp, operation)?;
        } else if items.len() == 2 && items[0] > items[1] {
            items.swap(0, 1);
        }
        Ok(items)
    }

    /// Each advance does constant work; the caller admits each visited node.
    pub(super) fn overlapping(&self, bounds: [[f64; 2]; 3]) -> impl Iterator<Item = &BoundsNode> {
        self.pruned(bounds, |_, _| false).map(|(_, node)| node)
    }

    /// Skip a whole subtree when its bounds or caller-owned summary settle it.
    fn pruned(
        &self,
        bounds: [[f64; 2]; 3],
        settled: impl Fn(usize, &BoundsNode) -> bool,
    ) -> impl Iterator<Item = (usize, &BoundsNode)> {
        std::iter::successors((!self.nodes.is_empty()).then_some(0), move |&at| {
            let node = &self.nodes[at];
            let next = if bounds_overlap(node.bounds, bounds) && !settled(at, node) {
                at + 1
            } else {
                node.after
            };
            (next < self.nodes.len()).then_some(next)
        })
        .map(|at| (at, &self.nodes[at]))
    }
}

pub(super) fn intersection_line_direction(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
) -> Option<Vector3> {
    const ANGULAR_TOLERANCE: f64 = EPS_STANDARD_DECODE_GEOMETRY;

    match (left, right) {
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
        ) => {
            let left = plane_surface.frame().axis().as_raw();
            let right = plane_surface_2.frame().axis().as_raw();
            let direction = (*left).cross(*right);
            let norm = direction.x.hypot(direction.y).hypot(direction.z);
            (norm.is_finite() && norm != 0.0).then_some(direction)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
        ) => {
            let normal = plane_surface.frame().axis().as_raw();
            let axis = cylinder_surface.frame().axis().as_raw();
            ((*normal).dot(*axis).abs() <= ANGULAR_TOLERANCE).then_some(*axis)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface_2)),
        ) => {
            let axis = cylinder_surface_2.frame().axis().as_raw();
            let normal = plane_surface_2.frame().axis().as_raw();
            ((*normal).dot(*axis).abs() <= ANGULAR_TOLERANCE).then_some(*axis)
        }
        (
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface_2)),
        ) => {
            let left_axis = cylinder_surface.frame().axis().as_raw();
            let right_axis = cylinder_surface_2.frame().axis().as_raw();
            ((*left_axis).cross(*right_axis).norm() <= ANGULAR_TOLERANCE).then_some(*left_axis)
        }
        _ => None,
    }
}

/// A line on one right circular or elliptical cone is a generator through its
/// apex. Same-carrier line rows have no surface-intersection direction, so
/// their endpoint relation needs this independent straight-branch predicate.
pub(super) fn same_cone_generator_pair(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    start: Point3,
    end: Point3,
) -> bool {
    if left != right {
        return false;
    }
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) = left else {
        return false;
    };
    let origin = cone_surface.origin().get();
    let axis = cone_surface.frame().axis().as_raw();
    let radius = cone_surface.radius().get();
    let half_angle = cone_surface.half_angle().get();
    let tangent = half_angle.tan();
    if !tangent.is_finite() || tangent == 0.0 {
        return false;
    }
    let apex_offset = -radius / tangent;
    if !apex_offset.is_finite() {
        return false;
    }
    let apex = Point3::new(
        origin.x + apex_offset * axis.x,
        origin.y + apex_offset * axis.y,
        origin.z + apex_offset * axis.z,
    );
    if !apex.is_finite() {
        return false;
    }
    let segment = end.vector_from(start);
    let segment_length = segment.norm();
    if !segment_length.is_finite() || segment_length == 0.0 {
        return false;
    }
    if start.distance(apex) <= EPS_SAME_CONE_GENERATOR
        || end.distance(apex) <= EPS_SAME_CONE_GENERATOR
    {
        return true;
    }
    let line_distance = start.vector_from(apex).cross(segment).norm() / segment_length;
    line_distance.is_finite() && line_distance <= EPS_SAME_CONE_GENERATOR
}

/// Check a point against face bounds and supported surface geometry.
pub(super) fn point_on_standard_face(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    point: Point3,
    surface: &SurfaceGeometry,
    bounds: Option<crate::families::standard::records::StandardFaceBounds>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    if bounds.is_some_and(|bounds| !point_inside_standard_face_bounds(point, bounds)) {
        return Ok(false);
    }
    Ok(point_on_surface_if_supported(ctx, point, surface)? != Some(false))
}

fn point_inside_standard_face_bounds(
    point: Point3,
    bounds: crate::families::standard::records::StandardFaceBounds,
) -> bool {
    let coordinates = [point.x, point.y, point.z];
    let inside_aabb = coordinates.iter().enumerate().all(|(axis, coordinate)| {
        (*coordinate - bounds.aabb_center[axis].get()).abs()
            <= bounds.aabb_half_extents[axis].get() + STANDARD_FACE_BOUNDS_TOLERANCE
    });
    let distance_squared = coordinates
        .iter()
        .enumerate()
        .map(|(axis, coordinate)| (*coordinate - bounds.sphere_center[axis].get()).powi(2))
        .sum::<f64>();
    inside_aabb
        && distance_squared.sqrt() <= bounds.sphere_radius.get() + STANDARD_FACE_BOUNDS_TOLERANCE
}

pub(super) fn standard_nurbs_line_pair_on_face(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    pair: &[usize; 2],
    points: &[Point],
    bounds: Option<crate::families::standard::records::StandardFaceBounds>,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    if !matches!(
        surface,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ) || !matches!(
        support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Line
    ) {
        return Ok(true);
    }
    let Some(start) = points.get(pair[0]).map(|point| point.position().get()) else {
        return Ok(false);
    };
    let Some(end) = points.get(pair[1]).map(|point| point.position().get()) else {
        return Ok(false);
    };
    for fraction in NURBS_LINE_FACE_SAMPLES {
        let point = Point3::new(
            start.x + fraction * (end.x - start.x),
            start.y + fraction * (end.y - start.y),
            start.z + fraction * (end.z - start.z),
        );
        if !point_on_standard_face(ctx, point, surface, bounds)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn nurbs_surface_control_bounds(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
) -> Result<Option<[[f64; 2]; 3]>, cadmpeg_core::decode::ResourceLimit> {
    const OPERATION: &str = "catia surface control bounds";
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    let mut include = |point: FinitePoint3| {
        for (axis, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
            bounds[axis][0] = bounds[axis][0].min(coordinate);
            bounds[axis][1] = bounds[axis][1].max(coordinate);
        }
    };
    match surface.pole_grid() {
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Polynomial { rows } => {
            ctx.all_by_limit(
                rows,
                |row| {
                    ctx.all_by_limit(
                        row,
                        |point| {
                            include(*point);
                            Ok(true)
                        },
                        OPERATION,
                    )
                },
                OPERATION,
            )?;
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { rows } => {
            if !ctx.all_by_limit(
                rows,
                |row| {
                    ctx.all_by_limit(
                        row,
                        |pole| {
                            if pole.weight.get() <= 0.0 {
                                return Ok(false);
                            }
                            include(pole.point);
                            Ok(true)
                        },
                        OPERATION,
                    )
                },
                OPERATION,
            )? {
                return Ok(None);
            }
        }
    }
    Ok(Some(bounds))
}

fn nurbs_shared_boundary_scalar_matches(left: f64, right: f64) -> bool {
    (left - right).abs() <= NURBS_SHARED_BOUNDARY_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

fn nurbs_shared_boundary_curves_match(
    ctx: &DecodeContext<'_>,
    left: &NurbsCurve,
    right: &NurbsCurve,
) -> Result<bool, CodecError> {
    let same_payload = |left: &NurbsCurve, right: &NurbsCurve| -> Result<bool, CodecError> {
        use cadmpeg_ir::geometry::nurbs::NurbsPoles3;
        if left.degree() != right.degree()
            || left.periodic() != right.periodic()
            || left.knots().len() != right.knots().len()
        {
            return Ok(false);
        }
        if !ctx.all_by(
            left.knots().as_slice().iter().zip(right.knots().as_slice()),
            |(left, right)| Ok(nurbs_shared_boundary_scalar_matches(*left, *right)),
            "catia_shared_nurbs_knots",
        )? {
            return Ok(false);
        }
        if left.pole_count() != right.pole_count() {
            return Ok(false);
        }
        let point_matches = |left: FinitePoint3, right: FinitePoint3| {
            let left = left.get();
            let right = right.get();
            [left.x, left.y, left.z]
                .into_iter()
                .zip([right.x, right.y, right.z])
                .all(|(left, right)| nurbs_shared_boundary_scalar_matches(left, right))
        };
        match (left.pole_rows(), right.pole_rows()) {
            (
                NurbsPoles3::Polynomial { points: left },
                NurbsPoles3::Polynomial { points: right },
            ) => ctx.all_by(
                left.iter().zip(right),
                |(left, right)| Ok(point_matches(*left, *right)),
                "catia_shared_nurbs_poles",
            ),
            (NurbsPoles3::Rational { points: left }, NurbsPoles3::Rational { points: right }) => {
                ctx.all_by(
                    left.iter().zip(right),
                    |(left, right)| {
                        Ok(point_matches(left.point, right.point)
                            && nurbs_shared_boundary_scalar_matches(
                                left.weight.get(),
                                right.weight.get(),
                            ))
                    },
                    "catia_shared_nurbs_poles",
                )
            }
            // Pole counts are equal, so mixed forms match only when both are empty.
            (NurbsPoles3::Polynomial { points: left }, NurbsPoles3::Rational { .. }) => {
                Ok(left.is_empty())
            }
            (NurbsPoles3::Rational { points: left }, NurbsPoles3::Polynomial { .. }) => {
                Ok(left.is_empty())
            }
        }
    };
    if same_payload(left, right)? {
        return Ok(true);
    }
    let Some(range) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(right)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(false);
    };
    let mut storage = ctx.reserve_scoped(0, "catia_shared_nurbs_reversed_boundary")?;
    match storage
        .with_storage(|| reverse_nurbs_curve(ctx, right, range))?
        .ok()
    {
        Some(reversed) => same_payload(left, &reversed),
        None => Ok(false),
    }
}

fn nurbs_surface_boundary_curves(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
) -> Result<Option<[NurbsCurve; 4]>, cadmpeg_core::decode::ResourceLimit> {
    let Some([[u_lower, u_upper], [v_lower, v_upper]]) = nurbs_surface_parameter_domain(surface)
    else {
        return Ok(None);
    };
    let curve =
        |axis, parameter| cadmpeg_ir::eval::nurbs_surface_isocurve(ctx, surface, axis, parameter);
    let Some(u_lower) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        u_lower,
    )?
    else {
        return Ok(None);
    };
    let Some(u_upper) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U,
        u_upper,
    )?
    else {
        return Ok(None);
    };
    let Some(v_lower) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
        v_lower,
    )?
    else {
        return Ok(None);
    };
    let Some(v_upper) = curve(
        cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V,
        v_upper,
    )?
    else {
        return Ok(None);
    };
    Ok(Some([u_lower, u_upper, v_lower, v_upper]))
}

fn nurbs_boundary_contains_point(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    point: Point3,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some([lower, upper]) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(curve)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(false);
    };
    for seed in [lower, 0.5 * (lower + upper), upper] {
        if cadmpeg_ir::eval::nurbs_curve_parameter_near_point(
            ctx,
            curve,
            point,
            NURBS_SURFACE_MEMBERSHIP_TOLERANCE,
            seed,
        )?
        .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Return endpoint pairs that lie on an exact shared NURBS carrier boundary.
///
/// A shared boundary is a positive relation between two tensor-product
/// carriers. It is not inferred from carrier AABBs or from a sampled surface
/// intersection. `None` means that the relation is unavailable; `Some` may be
/// empty when the relation is present but no supplied pair lies on it.
pub(super) fn standard_shared_nurbs_boundary_pair_options(
    ctx: &DecodeContext<'_>,
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
    points: &[Point3],
    options: &[[usize; 2]],
) -> Result<Option<Vec<[usize; 2]>>, CodecError> {
    let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(left)),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(right)),
    ) = (left, right)
    else {
        return Ok(None);
    };
    let mut storage = ctx.reserve_scoped(0, "catia_shared_nurbs_boundary_scratch")?;
    let Some((left_boundaries, right_boundaries)) =
        storage.with_storage(|| -> Result<_, CodecError> {
            let Some(left_boundaries) = nurbs_surface_boundary_curves(ctx, left)? else {
                return Ok(None);
            };
            let Some(right_boundaries) = nurbs_surface_boundary_curves(ctx, right)? else {
                return Ok(None);
            };
            Ok(Some((left_boundaries, right_boundaries)))
        })?
    else {
        return Ok(None);
    };
    let mut shared = [false; 4];
    for (index, left) in left_boundaries.iter().enumerate() {
        for right in &right_boundaries {
            if nurbs_shared_boundary_curves_match(ctx, left, right)? {
                shared[index] = true;
                break;
            }
        }
    }
    if !shared.contains(&true) {
        return Ok(None);
    }
    let mut matched = Vec::new();
    for &pair in ctx.admit_iter(options, "catia_shared_nurbs_endpoint_options")? {
        let mut pair_matches = false;
        for (boundary, is_shared) in left_boundaries.iter().zip(shared) {
            if !is_shared {
                continue;
            }
            let mut both = true;
            for point in pair {
                let Some(point) = points.get(point) else {
                    both = false;
                    break;
                };
                if !nurbs_boundary_contains_point(ctx, boundary, *point)? {
                    both = false;
                    break;
                }
            }
            if both {
                pair_matches = true;
                break;
            }
        }
        if pair_matches {
            ctx.push_vec(&mut matched, pair, "catia_shared_nurbs_boundary_pairs")?;
        }
    }
    Ok(Some(matched))
}

type CircleFaceKey = (u64, u64, u64, u64, usize);

#[derive(Clone, Copy)]
pub(super) struct CircleRangeChoices {
    ranges: [[f64; 2]; 2],
    len: usize,
}

impl AsRef<[[f64; 2]]> for CircleRangeChoices {
    fn as_ref(&self) -> &[[f64; 2]] {
        &self.ranges[..self.len]
    }
}

pub(super) struct StandardCirclePairConstraint<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    ranges_by_face: RefCell<BTreeMap<CircleFaceKey, Vec<CircleRangeChoices>>>,
}

impl<'a, 'ctx> StandardCirclePairConstraint<'a, 'ctx> {
    pub(super) fn new(
        ctx: &'a DecodeContext<'ctx>,
        supports: &[crate::families::standard::records::StandardCurveSupport],
        endpoint_options: &[Vec<[usize; 2]>],
    ) -> Result<Self, CodecError> {
        // Each face key holds one prepaid slot per circle that can reach it,
        // so a solution check fills the rows without growing them.
        let mut ranges_by_face = BTreeMap::<CircleFaceKey, Vec<CircleRangeChoices>>::new();
        for (support, options) in ctx
            .admit_iter(supports, "catia_standard_circle_constraint_supports")?
            .zip(ctx.admit_iter(endpoint_options, "catia_standard_circle_constraint_options")?)
        {
            if options.len() <= 1 {
                continue;
            }
            let crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } = &support.geometry
            else {
                continue;
            };
            let center = center.get();
            let radius = radius.get();
            for &face in &support.faces {
                let key = (
                    center.x.to_bits(),
                    center.y.to_bits(),
                    center.z.to_bits(),
                    radius.to_bits(),
                    face,
                );
                let ranges = ctx
                    .entry_btree_map(
                        &mut ranges_by_face,
                        key,
                        "catia_standard_circle_constraint_faces",
                    )?
                    .or_default();
                ctx.push_vec(
                    ranges,
                    CircleRangeChoices {
                        ranges: [[0.0; 2]; 2],
                        len: 0,
                    },
                    "catia_standard_circle_constraint_ranges",
                )?;
            }
        }
        for (_, ranges) in
            ctx.admit_iter(&mut ranges_by_face, "catia_standard_circle_range_reset")?
        {
            ranges.clear();
        }
        Ok(Self {
            ctx,
            ranges_by_face: RefCell::new(ranges_by_face),
        })
    }

    pub(super) fn solution_is_simple(
        &self,
        ir: &CadIr,
        bindings: &[(SurfaceId, bool, usize)],
        surface_indices: &HashMap<SurfaceId, usize>,
        supports: &[crate::families::standard::records::StandardCurveSupport],
        endpoint_options: &[Vec<[usize; 2]>],
        pairs: &[Option<[usize; 2]>],
    ) -> Result<bool, CodecError> {
        let mut range_choices = self.ranges_by_face.borrow_mut();
        for (_, choices) in self
            .ctx
            .admit_iter(&mut *range_choices, "catia_standard_circle_range_reset")?
        {
            choices.clear();
        }
        {
            let mut visits = supports.iter().zip(endpoint_options).zip(pairs);
            while let Some(((support, options), pair)) = self
                .ctx
                .next_charged(&mut visits, "catia_standard_circle_supports")?
            {
                let Some(pair) = pair else {
                    continue;
                };
                if options.len() <= 1 {
                    continue;
                }
                let crate::families::standard::records::StandardCurveGeometry::Circle {
                    center,
                    radius,
                } = &support.geometry
                else {
                    continue;
                };
                let center = center.get();
                let radius = radius.get();
                let Some(start) = ir
                    .model
                    .points
                    .get(pair[0])
                    .map(|point| point.position().get())
                else {
                    return Ok(false);
                };
                let Some(end) = ir
                    .model
                    .points
                    .get(pair[1])
                    .map(|point| point.position().get())
                else {
                    return Ok(false);
                };
                let mut face_axes = [None, None];
                for (axis, &face) in face_axes.iter_mut().zip(&support.faces) {
                    *axis = face_surface(self.ctx, ir, bindings, surface_indices, face)?.and_then(
                        |surface| {
                            standard_circle_axis_from_carrier(center, radius, &surface.geometry)
                        },
                    );
                }
                let mut axes = face_axes.into_iter().flatten();
                let Some(axis) = axes
                    .next()
                    .and_then(|axis| canonical_unoriented_axis(*axis.as_raw()))
                else {
                    continue;
                };
                if axes.any(|other| {
                    canonical_unoriented_axis(*other.as_raw())
                        .is_none_or(|other| axis.as_raw().dot(*other.as_raw()).abs() < 0.9999)
                }) {
                    return Ok(false);
                }
                let Some(choices) = circle_endpoint_range_choices(center, radius, axis, start, end)
                else {
                    continue;
                };
                for &face in &support.faces {
                    let key = (
                        center.x.to_bits(),
                        center.y.to_bits(),
                        center.z.to_bits(),
                        radius.to_bits(),
                        face,
                    );
                    let Some(ranges) = self.ctx.get_mut_btree_map(
                        &mut range_choices,
                        &key,
                        "catia_standard_circle_range_rows",
                    )?
                    else {
                        return Ok(false);
                    };
                    self.ctx.push_vec(
                        ranges,
                        choices,
                        "catia_standard_circle_constraint_ranges",
                    )?;
                }
            }
        }
        {
            let mut visits = range_choices.values();
            while let Some(choices) = self
                .ctx
                .next_charged(&mut visits, "catia_standard_circle_range_choices")?
            {
                if !circular_range_choices_have_simple_selection(self.ctx, choices)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

/// Require line endpoint assignments to partition each shared straight
/// carrier into disjoint edge intervals. Exact coincident intervals remain
/// admissible because seam and duplicate-edge representations can share one
/// carrier; a partial collinear overlap is the non-simple alternative.
#[derive(Clone, Copy)]
struct StandardLineSegment {
    start: Point3,
    end: Point3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeLineRole {
    NotLine,
    Fixed,
    Flexible,
}

pub(super) struct StandardLinePairConstraint {
    points: Vec<Point3>,
    pub(super) edge_roles: Vec<EdgeLineRole>,
    edges_by_face: BTreeMap<usize, Vec<usize>>,
}

impl StandardLinePairConstraint {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        points: &[Point],
        supports: &[crate::families::standard::records::StandardCurveSupport],
        endpoint_options: &[Vec<[usize; 2]>],
    ) -> Result<Self, CodecError> {
        let points = ctx.collect_vec(
            points.iter().map(|point| point.position().get()),
            "catia_standard_line_constraint_points",
        )?;
        let edge_roles = ctx.collect_vec(
            supports.iter().enumerate().map(|(edge, support)| {
                if !matches!(
                    support.geometry,
                    crate::families::standard::records::StandardCurveGeometry::Line
                ) {
                    EdgeLineRole::NotLine
                } else if endpoint_options
                    .get(edge)
                    .is_some_and(|options| options.len() > 1)
                {
                    EdgeLineRole::Flexible
                } else {
                    EdgeLineRole::Fixed
                }
            }),
            "catia_standard_line_constraint_roles",
        )?;
        // Edges are visited in ascending order, so a repeated face of one edge
        // can only repeat the row's last entry.
        let mut edges_by_face = BTreeMap::<usize, Vec<usize>>::new();

        for (edge, support) in ctx
            .admit_iter(supports, "catia_standard_line_constraint_edges")?
            .enumerate()
        {
            if edge_roles[edge] != EdgeLineRole::Flexible {
                continue;
            }
            for &face in &support.faces {
                let edges = ctx
                    .entry_btree_map(
                        &mut edges_by_face,
                        face,
                        "catia_standard_line_constraint_faces",
                    )?
                    .or_default();
                if edges.last() != Some(&edge) {
                    ctx.push_vec(edges, edge, "catia_standard_line_constraint_face_edges")?;
                }
            }
        }

        Ok(Self {
            points,
            edge_roles,
            edges_by_face,
        })
    }

    pub(super) fn edge_pairs<'a>(
        &self,
        pairs: &'a [Option<[usize; 2]>],
    ) -> Option<StandardLineEdgePairs<'a>> {
        StandardLineEdgePairs::new(pairs, self.edge_roles.len())
    }

    pub(super) fn is_valid(
        &self,
        ctx: &DecodeContext<'_>,
        pairs: &StandardLineEdgePairs<'_>,
    ) -> Result<bool, CodecError> {
        let mut roles = self.edge_roles.iter().zip(pairs.pairs());
        while let Some((role, pair)) =
            ctx.next_charged(&mut roles, "catia_standard_line_valid_roles")?
        {
            if *role == EdgeLineRole::NotLine {
                continue;
            }
            let Some(pair) = pair else {
                continue;
            };
            let Some(segment) = standard_line_segment(&self.points, *pair) else {
                return Ok(false);
            };
            if !standard_line_segment_is_materializable(segment) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn is_simple(
        &self,
        ctx: &DecodeContext<'_>,
        pairs: &StandardLineEdgePairs<'_>,
    ) -> Result<bool, CodecError> {
        if !self.is_valid(ctx, pairs)? {
            return Ok(false);
        }
        let mut faces = self.edges_by_face.values();
        while let Some(edges) = ctx.next_charged(&mut faces, "catia_standard_line_face_lists")? {
            let mut storage = ctx.reserve_scoped(0, "catia_standard_line_segment_index")?;
            let simple = storage.with_storage(|| -> Result<bool, CodecError> {
                let mut by_segment = HashMap::<[[u64; 3]; 2], usize>::new();
                let mut segments = Vec::<(StandardLineSegment, usize, usize)>::new();
                let mut entries = Vec::new();
                let mut projection = None;
                {
                    let mut visits = edges.iter().enumerate();
                    while let Some((ordinal, &edge)) =
                        ctx.next_charged(&mut visits, "catia_standard_line_left_edges")?
                    {
                        let Some(pair) = pairs.pairs()[edge] else {
                            continue;
                        };
                        let Some(segment) = standard_line_segment(&self.points, pair) else {
                            continue;
                        };
                        let key = [segment.start, segment.end].map(|point| {
                            [point.x, point.y, point.z].map(|value| {
                                if value == 0.0 {
                                    0
                                } else {
                                    value.to_bits()
                                }
                            })
                        });
                        if let Some(&group) = ctx.get_hash_map(
                            &by_segment,
                            &key,
                            "catia_standard_line_segment_index",
                        )? {
                            if !standard_line_segments_are_simple(segments[group].0, segment) {
                                return Ok(false);
                            }
                            segments[group].2 = ordinal;
                        } else {
                            let group = segments.len();
                            ctx.insert_hash_map(
                                &mut by_segment,
                                key,
                                group,
                                "catia_standard_line_segment_index",
                            )?;
                            ctx.push_vec(
                                &mut segments,
                                (segment, ordinal, ordinal),
                                "catia_standard_line_segment_index",
                            )?;
                            let (origin, axes) = projection.get_or_insert_with(|| {
                                let axes = canonical_unoriented_axis(
                                    segment.end.vector_from(segment.start),
                                )
                                .map_or(
                                    [
                                        Vector3::new(1.0, 0.0, 0.0),
                                        Vector3::new(0.0, 1.0, 0.0),
                                        Vector3::new(0.0, 0.0, 1.0),
                                    ],
                                    |axis| {
                                        let axis = *axis.as_raw();
                                        let perpendicular =
                                            cadmpeg_ir::geometry::derive_reference_direction(axis);
                                        [axis, perpendicular, axis.cross(perpendicular)]
                                    },
                                );
                                (segment.start, axes)
                            });
                            let start =
                                axes.map(|axis| axis.dot(segment.start.vector_from(*origin)));
                            let end = axes.map(|axis| axis.dot(segment.end.vector_from(*origin)));
                            let length = segment.end.vector_from(segment.start).norm();
                            let scale = [segment.start, segment.end, *origin]
                                .into_iter()
                                .flat_map(|point| [point.x, point.y, point.z])
                                .chain(start)
                                .chain(end)
                                .fold(0.0_f64, |scale, value| scale.max(value.abs()));
                            // A common line frame separates parallel supports even
                            // when their world-coordinate boxes overlap. Include
                            // subtraction/projection error in the tolerance margin.
                            let margin =
                                LINE_SEGMENT_GEOMETRY_TOLERANCE + 128.0 * f64::EPSILON * scale;
                            let bounds = if length.is_finite()
                                && margin.is_finite()
                                && start.into_iter().chain(end).all(f64::is_finite)
                            {
                                std::array::from_fn(|axis| {
                                    [
                                        start[axis].min(end[axis]) - margin,
                                        start[axis].max(end[axis]) + margin,
                                    ]
                                })
                            } else {
                                [[f64::NEG_INFINITY, f64::INFINITY]; 3]
                            };
                            ctx.push_vec(
                                &mut entries,
                                BoundsEntry {
                                    bounds,
                                    item: group,
                                },
                                "catia_standard_line_segment_index",
                            )?;
                        }
                    }
                }
                let tree =
                    BoundsIndex::new(ctx, &mut entries, "catia_standard_line_segment_index")?;
                let mut endpoint_bounds = ctx.alloc_filled(
                    tree.nodes.len(),
                    None::<LineEndpointBounds>,
                    "catia_standard_line_endpoint_bounds",
                )?;
                for at in ctx
                    .admit_iter(0..tree.nodes.len(), "catia_standard_line_endpoint_bounds")?
                    .rev()
                {
                    let node = &tree.nodes[at];
                    endpoint_bounds[at] = Some(match node.item {
                        Some(item) => LineEndpointBounds::from_segment(segments[item].0),
                        None => {
                            let left = at + 1;
                            let right = tree.nodes[left].after;
                            let Some(left_bounds) = endpoint_bounds[left] else {
                                return Ok(false);
                            };
                            let Some(right_bounds) = endpoint_bounds[right] else {
                                return Ok(false);
                            };
                            left_bounds.union(right_bounds)
                        }
                    });
                }
                // Save each group's bounds before the tree's in-place ordering.
                let mut bounds = ctx.alloc_filled(
                    segments.len(),
                    [[0.0; 2]; 3],
                    "catia_standard_line_segment_index",
                )?;
                for entry in ctx.admit_iter(&entries, "catia_standard_line_segment_index")? {
                    bounds[entry.item] = entry.bounds;
                }
                {
                    let mut visits = segments.iter().enumerate();
                    while let Some((group, &(left, first, last))) =
                        ctx.next_charged(&mut visits, "catia_standard_line_left_edges")?
                    {
                        if ctx.any_by(
                            tree.pruned(bounds[group], |at, _| {
                                endpoint_bounds[at]
                                    .is_some_and(|summary| summary.coincides_with(left))
                            }),
                            |(_, node)| {
                                let Some(other) = node.item.filter(|&other| other > group) else {
                                    return Ok(false);
                                };
                                if !bounds_overlap(bounds[group], node.bounds) {
                                    return Ok(false);
                                }
                                let (right, right_first, right_last) = segments[other];
                                Ok((first < right_last
                                    && !standard_line_segments_are_simple(left, right))
                                    || (right_first < last
                                        && !standard_line_segments_are_simple(right, left)))
                            },
                            "catia_standard_line_right_edges",
                        )? {
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            })?;
            if !simple {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Owns the length agreement between a candidate solution and the edge roles
/// it is validated against; the field is unreachable outside this module.
pub(super) mod line_edge_pairs {
    /// Endpoint pairs whose length matches the constraint's edge roles.
    pub(in crate::families::standard::decode) struct StandardLineEdgePairs<'a> {
        pairs: &'a [Option<[usize; 2]>],
    }

    impl<'a> StandardLineEdgePairs<'a> {
        /// Admits a candidate solution that has one entry per edge role.
        pub(super) fn new(pairs: &'a [Option<[usize; 2]>], edge_count: usize) -> Option<Self> {
            (pairs.len() == edge_count).then_some(Self { pairs })
        }

        /// Returns the candidate entries, one per edge role.
        pub(super) fn pairs(&self) -> &'a [Option<[usize; 2]>] {
            self.pairs
        }
    }
}

use line_edge_pairs::StandardLineEdgePairs;

#[derive(Clone, Copy)]
struct LineEndpointBounds {
    ends: [[[f64; 2]; 3]; 2],
    max_length: f64,
}

impl LineEndpointBounds {
    fn from_segment(segment: StandardLineSegment) -> Self {
        let length = segment.end.vector_from(segment.start).norm();
        let mut points = [segment.start, segment.end].map(|point| [point.x, point.y, point.z]);
        if (0..3)
            .find_map(|axis| {
                let coordinates =
                    points.map(|point| if point[axis] == 0.0 { 0.0 } else { point[axis] });
                let order = coordinates[0].total_cmp(&coordinates[1]);
                (!order.is_eq()).then_some(order)
            })
            .is_some_and(std::cmp::Ordering::is_gt)
        {
            points.swap(0, 1);
        }
        Self {
            ends: points.map(|point| point.map(|value| [value, value])),
            max_length: length,
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            ends: std::array::from_fn(|endpoint| {
                std::array::from_fn(|axis| {
                    [
                        self.ends[endpoint][axis][0].min(other.ends[endpoint][axis][0]),
                        self.ends[endpoint][axis][1].max(other.ends[endpoint][axis][1]),
                    ]
                })
            }),
            max_length: self.max_length.max(other.max_length),
        }
    }

    fn coincides_with(self, segment: StandardLineSegment) -> bool {
        let length = segment.end.vector_from(segment.start).norm();
        if length <= LINE_SEGMENT_GEOMETRY_TOLERANCE
            || self.max_length <= LINE_SEGMENT_GEOMETRY_TOLERANCE
        {
            return true;
        }
        if !length.is_finite() || !self.max_length.is_finite() {
            return false;
        }
        let points = Self::from_segment(segment)
            .ends
            .map(|endpoint| endpoint.map(|[low, _]| low));
        let scale = self
            .ends
            .into_iter()
            .flatten()
            .flatten()
            .chain(points.into_iter().flatten())
            .fold(length.max(self.max_length), |scale, value| {
                scale.max(value.abs())
            });
        // Endpoint distance bounds imply both directed interval predicates.
        // Shrink the acceptance bound to cover the exact predicate's rounding.
        let tolerance = LINE_SEGMENT_GEOMETRY_TOLERANCE - 512.0 * f64::EPSILON * scale;
        tolerance > 0.0
            && (0..2).all(|endpoint| {
                let delta: [f64; 3] = std::array::from_fn(|axis| {
                    (self.ends[endpoint][axis][0] - points[endpoint][axis])
                        .abs()
                        .max((self.ends[endpoint][axis][1] - points[endpoint][axis]).abs())
                });
                delta[0].hypot(delta[1]).hypot(delta[2]) <= tolerance
            })
    }
}

fn standard_line_segment(points: &[Point3], pair: [usize; 2]) -> Option<StandardLineSegment> {
    Some(StandardLineSegment {
        start: *points.get(pair[0])?,
        end: *points.get(pair[1])?,
    })
}

fn standard_line_segment_is_materializable(segment: StandardLineSegment) -> bool {
    let delta = segment.end.vector_from(segment.start);
    let length = delta.x.hypot(delta.y).hypot(delta.z);
    length.is_finite() && length != 0.0
}

fn standard_line_segments_are_simple(
    left: StandardLineSegment,
    right: StandardLineSegment,
) -> bool {
    let left_axis = left.end.vector_from(left.start);
    let left_length = left_axis.norm();
    let right_axis = right.end.vector_from(right.start);
    let right_length = right_axis.norm();
    if left_length <= LINE_SEGMENT_GEOMETRY_TOLERANCE
        || right_length <= LINE_SEGMENT_GEOMETRY_TOLERANCE
    {
        return true;
    }
    let left_unit = left_axis.scale(1.0 / left_length);
    let parallel_error = left_unit.cross(right_axis.scale(1.0 / right_length)).norm();
    let line_error = left_unit
        .cross(right.start.vector_from(left.start))
        .norm()
        .max(left_unit.cross(right.end.vector_from(left.start)).norm());
    if parallel_error > LINE_SEGMENT_GEOMETRY_TOLERANCE
        || line_error > LINE_SEGMENT_GEOMETRY_TOLERANCE
    {
        return true;
    }
    let left_interval = [0.0, left_length];
    let right_interval = [
        left_unit.dot(right.start.vector_from(left.start)),
        left_unit.dot(right.end.vector_from(left.start)),
    ];
    let right_interval = [
        right_interval[0].min(right_interval[1]),
        right_interval[0].max(right_interval[1]),
    ];
    let overlap = left_interval[1].min(right_interval[1]) - left_interval[0].max(right_interval[0]);
    if overlap <= LINE_SEGMENT_GEOMETRY_TOLERANCE {
        return true;
    }
    (left_interval[0] - right_interval[0]).abs() <= LINE_SEGMENT_GEOMETRY_TOLERANCE
        && (left_interval[1] - right_interval[1]).abs() <= LINE_SEGMENT_GEOMETRY_TOLERANCE
}

#[cfg(test)]
pub(super) fn standard_line_pair_solution_is_simple(
    points: &[Point],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    let point_positions = points
        .iter()
        .map(|point| point.position().get())
        .collect::<Vec<_>>();
    let segments = supports
        .iter()
        .zip(endpoint_options)
        .zip(pairs)
        .filter_map(|((support, options), pair)| {
            if !matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Line
            ) {
                return None;
            }
            if options.len() <= 1 {
                return None;
            }
            let pair = pair.as_ref()?;
            Some((
                support.faces,
                standard_line_segment(&point_positions, *pair)?,
            ))
        })
        .collect::<Vec<_>>();
    if supports.iter().zip(pairs).any(|(support, pair)| {
        matches!(
            support.geometry,
            crate::families::standard::records::StandardCurveGeometry::Line
        ) && pair.is_some_and(|pair| {
            standard_line_segment(&point_positions, pair)
                .is_none_or(|segment| !standard_line_segment_is_materializable(segment))
        })
    }) {
        return false;
    }
    if segments
        .iter()
        .any(|(_, segment)| !standard_line_segment_is_materializable(*segment))
    {
        return false;
    }
    let mut segments_by_face = HashMap::<usize, Vec<StandardLineSegment>>::new();
    for (faces, segment) in segments {
        for face in faces {
            segments_by_face.entry(face).or_default().push(segment);
        }
    }
    segments_by_face.into_values().all(|segments| {
        segments.iter().enumerate().all(|(left_index, left)| {
            segments[left_index + 1..]
                .iter()
                .all(|right| standard_line_segments_are_simple(*left, *right))
        })
    })
}

#[cfg(test)]
pub(super) fn standard_line_pair_solution_is_simple_cached(
    points: &[Point],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    crate::test_support::with_service_context(|ctx| {
        let constraint = StandardLinePairConstraint::new(ctx, points, supports, endpoint_options)
            .expect("service budget admits line constraint");
        constraint.edge_pairs(pairs).is_some_and(|pairs| {
            constraint
                .is_simple(ctx, &pairs)
                .expect("service budget admits line constraint validation")
        })
    })
}
