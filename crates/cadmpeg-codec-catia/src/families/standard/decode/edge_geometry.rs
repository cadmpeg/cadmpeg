// SPDX-License-Identifier: Apache-2.0
//! Standard carrier curves, pcurves, and analytic geometry.

use cadmpeg_core::decode::u64_from_index;

use super::{
    annotate, cgm_source, circle_parameter_range_from_surface_branch, face_surface, ordered_range,
    point_on_nurbs_surface, rational_pcurve_arc, standard_id,
    standard_native_support_endpoint_pair, unit_vector, unwrap_angle, AnnotationBuilder, CadIr,
    CircleRangeChoices, CodecError, Curve, CurveGeometry, CurveId, DecodeContext,
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
    for lane in ctx.admit_iter(lanes, "catia_standard_circle_witness_lanes")? {
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
) -> Result<Option<(CurveGeometry, [f64; 2])>, cadmpeg_core::decode::ResourceLimit> {
    const TOLERANCE: f64 = 2e-3;
    ctx.charge_work_limit(0, "catia surface membership boundary")?;

    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
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
) -> Result<Option<CurveGeometry>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
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
) -> Result<Option<CurveGeometry>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
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
) -> Result<Option<CurveGeometry>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface membership boundary")?;
    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
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
            let mut axes = ctx.collect_vec(
                ctx.admit_iter(&support.faces, "catia_standard_edge_circle_axes")?
                    .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
                    .filter_map(|surface| {
                        standard_circle_axis_from_carrier(center, radius, &surface.geometry)
                    }),
                "catia_standard_edge_circle_axes",
            )?;
            if let Some(native) = native_support {
                for carrier in ctx.admit_iter(
                    &native.carriers,
                    "catia_standard_native_circle_carriers",
                )? {
                    let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(surface) =
                        carrier
                    else {
                        continue;
                    };
                    if let Some(axis) = standard_circle_axis_from_carrier(center, radius, surface) {
                        ctx.push_vec(&mut axes, axis, "catia_standard_edge_circle_axes")?;
                    }
                }
            }
            if axes.is_empty() {
                if let Some(axis) = circle_axis_from_endpoints(center, radius, start, end) {
                    ctx.push_vec(&mut axes, axis, "catia_standard_edge_circle_axes")?;
                }
            }
            let axis = axes.first().copied();
            let conflicting_axes = if let Some(axis) = axis {
                ctx.admit_iter(&axes, "catia_standard_edge_circle_axis_conflicts")?
                    .skip(1)
                    .any(|other| axis.as_raw().dot(*other.as_raw()).abs() < 0.9999)
            } else {
                false
            };
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
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.origin",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.direction",
            "catia_annotation_field",
        )?;
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Bspline,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)),
        )
    ) {
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.center",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.axis",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.ref_direction",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.radius",
            "catia_annotation_field",
        )?;
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Bspline,
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_)),
        )
    ) {
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.center",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.axis",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.major_direction",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.major_radius",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.minor_radius",
            "catia_annotation_field",
        )?;
    } else if matches!(
        (&support.geometry, &geometry),
        (
            crate::families::standard::records::StandardCurveGeometry::Circle { .. },
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)),
        )
    ) {
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry.axis",
            "catia_annotation_field",
        )?;
    }
    let geometry_is_unknown = matches!(
        &geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
    );
    admission.reserve_entity(&mut ir.model.curves, "catia_family_emit_curves")?;
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
                admission,
            )?;
            let second_surface = ensure_native_edge_support_surface(
                ir,
                annotations,
                native.surface_object_ids[1],
                &native.carriers[1],
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
                    Some((id, _, _)) if surface_indices.contains_key(id) => {
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
        if sides.iter().all(|side| side.surface.is_some())
            && (native_support.is_some() || sides[0].surface != sides[1].surface)
        {
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
                crate::resource::derived_annotation(
                    ctx,
                    annotations,
                    &procedural_id,
                    "curve",
                    "catia_annotation_field",
                )?;
                crate::resource::derived_annotation(
                    ctx,
                    annotations,
                    &procedural_id,
                    "definition",
                    "catia_annotation_field",
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
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<SurfaceId, cadmpeg_core::CodecError> {
    let source = cgm_source(admission.context(), "surface", surface_object_id)?;
    let mut source_match = None::<&SurfaceId>;
    let mut source_ambiguous = false;
    for surface in &ir.model.surfaces {
        admission
            .context()
            .charge_work(1, "catia_native_edge_support_source_scan")?;
        if surface.source_object.as_ref() == Some(&source) {
            if source_match.is_some_and(|id| id != &surface.id) {
                source_ambiguous = true;
            } else {
                source_match = Some(&surface.id);
            }
        }
    }
    let source_matches_empty = source_match.is_none();
    if !source_ambiguous {
        if let Some(surface_id) = source_match {
            return surface_id.try_clone_for_decode(
                admission.context(),
                "catia_native_edge_support_matched_surface_id",
            );
        }
    }
    if let crate::families::b5::transfer::ResolvedPcurveSurface::Geometry(geometry) = carrier {
        let mut geometry_match = None::<&SurfaceId>;
        let mut geometry_ambiguous = false;
        for surface in &ir.model.surfaces {
            admission
                .context()
                .charge_work(1, "catia_native_edge_support_geometry_scan")?;
            if surface.geometry == *geometry {
                if geometry_match.is_some_and(|id| id != &surface.id) {
                    geometry_ambiguous = true;
                } else {
                    geometry_match = Some(&surface.id);
                }
            }
        }
        if source_matches_empty && !geometry_ambiguous {
            if let Some(surface_id) = geometry_match {
                return surface_id.try_clone_for_decode(
                    admission.context(),
                    "catia_native_edge_support_matched_surface_id",
                );
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
    ir.model.surfaces.push(Surface {
        id: id.try_clone_for_decode(admission.context(), "catia_native_edge_support_record_id")?,
        geometry,
        source_object: Some(source),
    });
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

pub(super) fn circle_endpoint_range_choices(
    ctx: &DecodeContext<'_>,
    center: Point3,
    radius: f64,
    axis: UnitVector3,
    start: Point3,
    end: Point3,
) -> Result<Option<CircleRangeChoices>, CodecError> {
    const ENDPOINT_TOLERANCE: f64 = 2e-3;

    if !radius.is_finite()
        || radius <= 0.0
        || (start.distance(center) - radius).abs() > ENDPOINT_TOLERANCE
        || (end.distance(center) - radius).abs() > ENDPOINT_TOLERANCE
    {
        return Ok(None);
    }
    if start.distance(end) <= ENDPOINT_TOLERANCE {
        return Ok(Some(CircleRangeChoices {
            ranges: [[0.0, std::f64::consts::TAU], [0.0; 2]],
            len: 1,
        }));
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
        return Ok(None);
    }
    ctx.stable_sort_by(
        &mut endpoints,
        |value| value,
        f64::total_cmp,
        "catia standard circle endpoint angles sort",
    )?;
    let Some(short) = crate::nurbs::canonical_periodic_range(endpoints) else {
        return Ok(None);
    };
    let Some(long) = crate::nurbs::canonical_periodic_range([
        endpoints[1],
        endpoints[0] + std::f64::consts::TAU,
    ]) else {
        return Ok(None);
    };
    Ok(Some(CircleRangeChoices {
        ranges: [short, long],
        len: 2,
    }))
}

pub(super) fn circular_range_choices_have_simple_selection<T: AsRef<[[f64; 2]]>>(
    ctx: &DecodeContext<'_>,
    choices: &[T],
) -> Result<bool, CodecError> {
    const MAX_SELECTION_STATES: usize = 4_096;

    fn visit<T: AsRef<[[f64; 2]]>>(
        ctx: &DecodeContext<'_>,
        choices: &[T],
        index: usize,
        selected: &mut [u16; MAX_SELECTION_STATES],
        states: &mut usize,
    ) -> Result<Option<bool>, CodecError> {
        if *states >= MAX_SELECTION_STATES {
            return Ok(None);
        }
        *states += 1;
        if index == choices.len() {
            return Ok(Some(true));
        }
        for (choice_index, _) in ctx
            .admit_iter(choices[index].as_ref(), "catia_standard_iteration")?
            .enumerate()
        {
            let Ok(choice_index) = u16::try_from(choice_index) else {
                return Ok(None);
            };
            selected[index] = choice_index;
            let Some(prefix) = choices.get(..index + 1) else {
                return Ok(None);
            };
            let compatible = circular_ranges_are_nonoverlapping_or_coincident_by(
                ctx,
                prefix,
                |at| choices[at].as_ref()[usize::from(selected[at])],
            )?;
            if compatible {
                match visit(ctx, choices, index + 1, selected, states)? {
                    Some(true) => {
                        return Ok(Some(true));
                    }
                    None => {
                        return Ok(None);
                    }
                    Some(false) => {}
                }
            }
        }
        Ok(Some(false))
    }

    if ctx
        .admit_iter(choices, "catia_standard_iteration")?
        .any(|choice| choice.as_ref().is_empty())
    {
        return Ok(false);
    }
    Ok(visit(ctx, choices, 0, &mut [0; MAX_SELECTION_STATES], &mut 0)?.unwrap_or(false))
}

#[cfg(test)]
pub(super) fn circular_ranges_are_nonoverlapping_or_coincident(
    ctx: &DecodeContext<'_>,
    ranges: &[[f64; 2]],
) -> Result<bool, CodecError> {
    circular_ranges_are_nonoverlapping_or_coincident_by(ctx, ranges, |index| ranges[index])
}

pub(super) fn circular_ranges_are_nonoverlapping_or_coincident_by<T>(
    ctx: &DecodeContext<'_>,
    source: &[T],
    range_at: impl Fn(usize) -> [f64; 2],
) -> Result<bool, CodecError> {
    fn segments(range: [f64; 2]) -> [Option<[f64; 2]>; 2] {
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

for (left_index, _) in ctx
    .admit_iter(source, "catia_standard_circle_range_pair_left")?
    .enumerate()
{
        let left = range_at(left_index);
        let Some(first_right) = left_index.checked_add(1) else {
            return Err(ctx.refuse_codec_limit(
                "catia_standard_circle_range_pair_right",
                u64::MAX,
                u64::MAX,
            ));
        };
        let Some(right_source) = source.get(first_right..) else {
            return Err(ctx.refuse_codec_limit(
                "catia_standard_circle_range_pair_right",
                u64::MAX,
                u64::MAX,
            ));
        };
        for (relative, _) in ctx
            .admit_iter(right_source, "catia_standard_circle_range_pair_right")?
            .enumerate()
        {
            let Some(right_index) = first_right.checked_add(relative) else {
                return Err(ctx.refuse_codec_limit(
                    "catia_standard_circle_range_pair_right",
                    u64::MAX,
                    u64::MAX,
                ));
            };
            let right = range_at(right_index);
            let coincident = (right[0] - left[0]).abs() <= EPS_STANDARD_DECODE_GEOMETRY
                && (right[1] - left[1]).abs() <= EPS_STANDARD_DECODE_GEOMETRY;
            if coincident {
                continue;
            }
            for left_segment in segments(left).into_iter().flatten() {
                for right_segment in segments(right).into_iter().flatten() {
                    if left_segment[1].min(right_segment[1])
                        - left_segment[0].max(right_segment[0])
                        > EPS_STANDARD_DECODE_COARSE_GEOMETRY
                    {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
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
        let Some(surface) = face_surface(ir, bindings, surface_indices, *face) else {
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

pub(super) fn attach_standard_circles(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    bindings: &[(SurfaceId, bool, usize)],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for support in ctx.admit_iter(supports, "catia_standard_attached_circles")? {
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            support.geometry
        else {
            continue;
        };
        let admitted_center = center;
        let admitted_radius = radius;
        let center = center.get();
        let radius = radius.get();
        let axes = ctx.try_collect_vec(
            ctx.admit_iter(&support.faces, "catia_standard_attached_circle_faces")?
                .map(|face| -> Result<_, cadmpeg_core::CodecError> {
                    let Some((surface_id, _, _)) = bindings.get(*face) else {
                        return Ok(None);
                    };
                    let Some(surface) = ctx
                        .admit_iter(
                            &ir.model.surfaces,
                            "catia_standard_attached_circle_surfaces",
                        )?
                        .find(|surface| surface.id == *surface_id)
                    else {
                        return Ok(None);
                    };
                    Ok(standard_circle_axis_from_carrier(
                        center,
                        radius,
                        &surface.geometry,
                    ))
                })
                .filter_map(Result::transpose),
            "catia_standard_attached_circle_axes",
        )?;
        let Some(axis) = axes.first().copied() else {
            continue;
        };
        if ctx
            .admit_iter(&axes, "catia_standard_attached_circle_axis_conflicts")?
            .skip(1)
            .any(|other| axis.as_raw().dot(*other.as_raw()).abs() < 0.9999)
        {
            continue;
        }
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
            "catia_annotation_field",
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
    for support in ctx.admit_iter(supports, "catia_standard_attached_lines")? {
        if !matches!(
            support.geometry,
            crate::families::standard::records::StandardCurveGeometry::Line
        ) {
            continue;
        }
        let Some((origin_a, normal_a)) = plane_for_face(ctx, ir, bindings, support.faces[0])? else {
            continue;
        };
        let Some((origin_b, normal_b)) = plane_for_face(ctx, ir, bindings, support.faces[1])? else {
            continue;
        };
        let Some((origin, direction)) =
            plane_intersection_line(origin_a, normal_a, origin_b, normal_b)
        else {
            continue;
        };
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
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            admission.context(),
            annotations,
            &id,
            "geometry.direction",
            "catia_annotation_field",
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

pub(super) fn plane_for_face(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    face: usize,
) -> Result<Option<(cadmpeg_ir::math::Point3, Vector3)>, CodecError> {
    let Some((surface_id, _, _)) = bindings.get(face) else {
        return Ok(None);
    };
    let Some(surface) = ctx
        .admit_iter(&ir.model.surfaces, "catia_standard_plane_for_face_surfaces")?
        .find(|surface| surface.id == *surface_id)
    else {
        return Ok(None);
    };
    Ok(match &surface.geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            Some((origin, *normal))
        }
        _ => None,
    })
}
