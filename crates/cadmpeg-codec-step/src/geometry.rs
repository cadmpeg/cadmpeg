// SPDX-License-Identifier: Apache-2.0
//! Converts IR geometry carriers into STEP DATA instances.
//!
//! Conversion appends supporting points, directions, and placements before
//! returning the top-level carrier reference. Analytic carriers use their STEP
//! counterparts. NURBS carriers use `*_WITH_KNOTS`, with complex instances for
//! rational geometry.

use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry,
    SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::{Transform, Transform2};

use crate::writer::{real, refs, Emitter, Ref};

const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12: f64 = 1.0e-12;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12: f64 = 1.0e-12;

pub(crate) fn surface_is_supported(surface: &SolvedSurfaceGeometry) -> bool {
    match surface {
        SolvedSurfaceGeometry::Transformed { basis, transform } => {
            similarity_transform(transform) && surface_is_supported(basis)
        }
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => true,
        SolvedSurfaceGeometry::Nurbs(n) => valid_nurbs_surface(n),
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => false,
    }
}

fn valid_nurbs_surface(n: &NurbsSurface) -> bool {
    n.poles()
        .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        && n.pole_weights()
            .is_none_or(|mut weights| weights.all(|weight| weight.is_finite() && weight > 0.0))
        && knots_nondecreasing(n.u_knots())
        && knots_nondecreasing(n.v_knots())
}

pub(crate) fn curve_is_supported(curve: &CurveGeometry) -> bool {
    // A composite carrier is emitted from its child graph by the exporter, so it
    // is supported only in the outermost position.
    matches!(
        curve,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    ) || curve.solved().is_some_and(leaf_curve_is_supported)
}

fn leaf_curve_is_supported(curve: &SolvedCurveGeometry) -> bool {
    match curve {
        SolvedCurveGeometry::Transformed { basis, transform } => {
            similarity_transform(transform) && leaf_curve_is_supported(basis)
        }
        SolvedCurveGeometry::Line(_)
        | SolvedCurveGeometry::Circle(_)
        | SolvedCurveGeometry::Ellipse(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Degenerate(_)
        | SolvedCurveGeometry::Nurbs(_)
        | SolvedCurveGeometry::Polyline(_) => true,
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => false,
    }
}

fn similarity_transform(transform: &Transform) -> bool {
    if transform
        .rows()
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
        || transform.rows()[3][0].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || transform.rows()[3][1].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || transform.rows()[3][2].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || (transform.rows()[3][3] - 1.0).abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
    {
        return false;
    }
    let columns = [
        Vector3::new(
            transform.rows()[0][0],
            transform.rows()[1][0],
            transform.rows()[2][0],
        ),
        Vector3::new(
            transform.rows()[0][1],
            transform.rows()[1][1],
            transform.rows()[2][1],
        ),
        Vector3::new(
            transform.rows()[0][2],
            transform.rows()[1][2],
            transform.rows()[2][2],
        ),
    ];
    let scale = columns[0].norm();
    let tolerance = EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10 * scale.max(1.0);
    scale > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        && columns
            .iter()
            .all(|column| (column.norm() - scale).abs() <= tolerance)
        && columns[0].dot(columns[1]).abs() <= tolerance * scale
        && columns[0].dot(columns[2]).abs() <= tolerance * scale
        && columns[1].dot(columns[2]).abs() <= tolerance * scale
}

/// Emit or reuse a `CARTESIAN_POINT`.
pub fn point(e: &mut Emitter, p: Point3) -> Ref {
    let params = format!("'',({},{},{})", real(p.x), real(p.y), real(p.z));
    e.emit_interned("CARTESIAN_POINT", &params)
}

fn point2(e: &mut Emitter, p: Point2) -> Ref {
    let params = format!("'',({},{})", real(p.u), real(p.v));
    e.emit_interned("CARTESIAN_POINT", &params)
}

fn direction2(e: &mut Emitter, v: Point2) -> Ref {
    let magnitude = (v.u * v.u + v.v * v.v).sqrt();
    let (x, y) = if magnitude > 0.0 {
        (v.u / magnitude, v.v / magnitude)
    } else {
        (1.0, 0.0)
    };
    e.emit_interned("DIRECTION", &format!("'',({},{})", real(x), real(y)))
}

fn similarity_transform_2d(transform: &Transform2) -> bool {
    let first = Point2::new(transform.rows()[0][0], transform.rows()[1][0]);
    let second = Point2::new(transform.rows()[0][1], transform.rows()[1][1]);
    let scale = first.u.hypot(first.v);
    let tolerance = EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10 * scale.max(1.0);
    scale > EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12
        && (second.u.hypot(second.v) - scale).abs() <= tolerance
        && (first.u * second.u + first.v * second.v).abs() <= tolerance * scale
}

fn axis2_placement_2d(e: &mut Emitter, location: Point2, x_axis: Point2) -> Ref {
    let location = point2(e, location);
    let direction = direction2(e, x_axis);
    e.emit("AXIS2_PLACEMENT_2D", &format!("'',{location},{direction}"))
}

fn transformation_operator_2d(e: &mut Emitter, transform: Transform2) -> Ref {
    let origin = point2(
        e,
        Point2::new(transform.rows()[0][2], transform.rows()[1][2]),
    );
    let x = Point2::new(transform.rows()[0][0], transform.rows()[1][0]);
    let y = Point2::new(transform.rows()[0][1], transform.rows()[1][1]);
    let scale = x.u.hypot(x.v);
    let x = direction2(e, x);
    let y = direction2(e, y);
    e.emit(
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        &format!("'',{x},{y},{origin},{}", real(scale)),
    )
}

/// Emit a two-dimensional curve for use inside a `PCURVE` representation.
pub fn pcurve(e: &mut Emitter, geometry: &PcurveGeometry) -> Option<Ref> {
    Some(match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin();
            let direction = line_pcurve.direction();
            let point = point2(e, *origin);
            let magnitude = (direction.u * direction.u + direction.v * direction.v).sqrt();
            let direction = direction2(e, *direction);
            let vector = e.emit("VECTOR", &format!("'',{direction},{}", real(magnitude)));
            e.emit("LINE", &format!("'',{point},{vector}"))
        }
        PcurveGeometry::Circle(circle_pcurve) => {
            let center = circle_pcurve.center();
            let x_axis = circle_pcurve.x_axis();
            let radius = circle_pcurve.radius();
            let placement = axis2_placement_2d(e, *center, *x_axis);
            e.emit("CIRCLE", &format!("'',{placement},{}", real(radius)))
        }
        PcurveGeometry::Ellipse(ellipse_pcurve) => {
            let center = ellipse_pcurve.center();
            let x_axis = ellipse_pcurve.x_axis();
            let major_radius = ellipse_pcurve.major_radius();
            let minor_radius = ellipse_pcurve.minor_radius();
            let placement = axis2_placement_2d(e, *center, *x_axis);
            e.emit(
                "ELLIPSE",
                &format!(
                    "'',{placement},{},{}",
                    real(major_radius),
                    real(minor_radius)
                ),
            )
        }
        PcurveGeometry::Parabola(parabola_pcurve) => {
            let vertex = parabola_pcurve.vertex();
            let x_axis = parabola_pcurve.x_axis();
            let focal_distance = parabola_pcurve.focal_distance();
            let placement = axis2_placement_2d(e, *vertex, *x_axis);
            e.emit(
                "PARABOLA",
                &format!("'',{placement},{}", real(focal_distance)),
            )
        }
        PcurveGeometry::Hyperbola(hyperbola_pcurve) => {
            let center = hyperbola_pcurve.center();
            let x_axis = hyperbola_pcurve.x_axis();
            let major_radius = hyperbola_pcurve.major_radius();
            let minor_radius = hyperbola_pcurve.minor_radius();
            let placement = axis2_placement_2d(e, *center, *x_axis);
            e.emit(
                "HYPERBOLA",
                &format!(
                    "'',{placement},{},{}",
                    real(major_radius),
                    real(minor_radius)
                ),
            )
        }
        PcurveGeometry::Nurbs { nurbs } => {
            let points = nurbs
                .control_points()
                .iter()
                .map(|point| point2(e, *point))
                .collect::<Vec<_>>();
            let (knots, multiplicities) = compress_knots(nurbs.knots());
            let base = format!(
                "{},{},.UNSPECIFIED.,{},.U.",
                nurbs.degree(),
                refs(&points),
                closed_flag(nurbs.periodic())
            );
            let with_knots = format!(
                "{},{},.UNSPECIFIED.",
                int_list(&multiplicities),
                real_list(&knots)
            );
            if let Some(weights) = nurbs.weights() {
                e.emit_raw(
                    "B_SPLINE_CURVE_WITH_KNOTS",
                    &format!(
                        "( BOUNDED_CURVE() B_SPLINE_CURVE({base}) B_SPLINE_CURVE_WITH_KNOTS({with_knots}) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE({}) REPRESENTATION_ITEM('') )",
                        real_list(weights)
                    ),
                )
            } else {
                e.emit(
                    "B_SPLINE_CURVE_WITH_KNOTS",
                    &format!("'',{base},{with_knots}"),
                )
            }
        }
        PcurveGeometry::Transformed { basis, transform } => {
            if !similarity_transform_2d(transform) {
                return None;
            }
            let basis = pcurve(e, basis)?;
            let operator = transformation_operator_2d(e, *transform);
            e.emit("CURVE_REPLICA", &format!("'',{basis},{operator}"))
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let same_sense = trimmed_pcurve.same_sense();
            let basis = trimmed_pcurve.basis();
            let basis = pcurve(e, basis)?;
            let sense = if same_sense { ".T." } else { ".F." };
            e.emit(
                "TRIMMED_CURVE",
                &format!(
                    "'',{basis},({}),({}),{sense},.PARAMETER.",
                    real(parameter_range[0]),
                    real(parameter_range[1])
                ),
            )
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let distance = offset_pcurve.distance();
            let basis = offset_pcurve.basis();
            let basis = pcurve(e, basis)?;
            e.emit(
                "OFFSET_CURVE_2D",
                &format!("'',{basis},{},.F.", real(distance)),
            )
        }
        PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Hyperbolic(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle(_) => return None,
    })
}

/// Emit or reuse a unit-length `DIRECTION`.
///
/// A zero-length vector becomes `(0,0,1)`.
pub fn direction(e: &mut Emitter, v: Vector3) -> Ref {
    let n = v.norm();
    let u = if n > 0.0 {
        Vector3::new(v.x / n, v.y / n, v.z / n)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let params = format!("'',({},{},{})", real(u.x), real(u.y), real(u.z));
    e.emit_interned("DIRECTION", &params)
}

/// Emit an `AXIS2_PLACEMENT_3D` with the given origin, local +Z axis, and local
/// +X reference direction.
///
/// STEP projects the reference direction onto the plane normal to the axis.
pub fn placement(e: &mut Emitter, origin: Point3, axis: Vector3, ref_dir: Vector3) -> Ref {
    let o = point(e, origin);
    let a = direction(e, axis);
    let r = direction(e, ref_dir);
    e.emit("AXIS2_PLACEMENT_3D", &format!("'',{o},{a},{r}"))
}

pub(crate) fn transformation_operator(e: &mut Emitter, transform: Transform) -> Ref {
    let origin = point(
        e,
        Point3::new(
            transform.rows()[0][3],
            transform.rows()[1][3],
            transform.rows()[2][3],
        ),
    );
    let x = Vector3::new(
        transform.rows()[0][0],
        transform.rows()[1][0],
        transform.rows()[2][0],
    );
    let y = Vector3::new(
        transform.rows()[0][1],
        transform.rows()[1][1],
        transform.rows()[2][1],
    );
    let z = Vector3::new(
        transform.rows()[0][2],
        transform.rows()[1][2],
        transform.rows()[2][2],
    );
    let scale = x.norm();
    let x = direction(e, x);
    let y = direction(e, y);
    let z = direction(e, z);
    e.emit(
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        &format!("'',{x},{y},{origin},{},{z}", real(scale)),
    )
}

/// Emit an analytic or NURBS surface carrier.
pub fn surface(e: &mut Emitter, g: &SolvedSurfaceGeometry) -> Option<Ref> {
    Some(match g {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
            let pl = placement(e, *origin, *normal, *u_axis);
            e.emit("PLANE", &format!("'',{pl}"))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.axis();
            let ref_direction = cylinder_surface.ref_direction();
            let radius = cylinder_surface.radius();
            let pl = placement(e, *origin, *axis, *ref_direction);
            e.emit("CYLINDRICAL_SURFACE", &format!("'',{pl},{}", real(radius)))
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin();
            let axis = cone_surface.axis();
            let ref_direction = cone_surface.ref_direction();
            let radius = cone_surface.radius();
            let half_angle = cone_surface.half_angle();
            let pl = placement(e, *origin, *axis, *ref_direction);
            e.emit(
                "CONICAL_SURFACE",
                &format!("'',{pl},{},{}", real(radius), real(half_angle)),
            )
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center();
            let axis = sphere_surface.axis();
            let ref_direction = sphere_surface.ref_direction();
            let radius = sphere_surface.radius();
            let pl = placement(e, *center, *axis, *ref_direction);
            e.emit(
                "SPHERICAL_SURFACE",
                &format!("'',{pl},{}", real(radius.abs())),
            )
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center();
            let axis = torus_surface.axis();
            let ref_direction = torus_surface.ref_direction();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
            let pl = placement(e, *center, *axis, *ref_direction);
            e.emit(
                "TOROIDAL_SURFACE",
                &format!(
                    "'',{pl},{},{}",
                    real(major_radius.abs()),
                    real(minor_radius.abs())
                ),
            )
        }
        SolvedSurfaceGeometry::Nurbs(n) => nurbs_surface(e, n)?,
        SolvedSurfaceGeometry::Transformed { basis, transform } => {
            let parent = surface(e, basis)?;
            let operator = transformation_operator(e, *transform);
            e.emit("SURFACE_REPLICA", &format!("'',{parent},{operator}"))
        }
        // These carrier families have no direct STEP representation; callers
        // report the omitted carrier instead of fabricating a placeholder.
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => return None,
    })
}

/// Emit an analytic or NURBS 3D curve carrier.
pub fn curve(e: &mut Emitter, g: &SolvedCurveGeometry) -> Option<Ref> {
    Some(match g {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin();
            let d = line_curve.direction();
            let p = point(e, *origin);
            // A LINE's VECTOR carries the direction; unit magnitude is conventional.
            let dir = direction(e, *d);
            let vec = e.emit("VECTOR", &format!("'',{dir},{}", real(1.0)));
            e.emit("LINE", &format!("'',{p},{vec}"))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center();
            let axis = circle_curve.axis();
            let ref_direction = circle_curve.ref_direction();
            let radius = circle_curve.radius();
            let pl = placement(e, *center, *axis, *ref_direction);
            e.emit("CIRCLE", &format!("'',{pl},{}", real(radius)))
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center();
            let axis = ellipse_curve.axis();
            let major_direction = ellipse_curve.major_direction();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            let pl = placement(e, *center, *axis, *major_direction);
            e.emit(
                "ELLIPSE",
                &format!("'',{pl},{},{}", real(major_radius), real(minor_radius)),
            )
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex();
            let axis = parabola_curve.axis();
            let major_direction = parabola_curve.major_direction();
            let focal_distance = parabola_curve.focal_distance();
            let pl = placement(e, *vertex, *axis, *major_direction);
            e.emit("PARABOLA", &format!("'',{pl},{}", real(focal_distance)))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center();
            let axis = hyperbola_curve.axis();
            let major_direction = hyperbola_curve.major_direction();
            let major_radius = hyperbola_curve.major_radius();
            let minor_radius = hyperbola_curve.minor_radius();
            let pl = placement(e, *center, *axis, *major_direction);
            e.emit(
                "HYPERBOLA",
                &format!("'',{pl},{},{}", real(major_radius), real(minor_radius)),
            )
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => {
            let collapsed = degenerate_curve.point();
            let point = point(e, *collapsed);
            e.emit("POLYLINE", &format!("'',({point},{point})"))
        }
        SolvedCurveGeometry::Nurbs(n) => nurbs_curve(e, n),
        SolvedCurveGeometry::Polyline(polyline) => {
            let points = polyline
                .points()
                .iter()
                .map(|position| point(e, *position).to_string())
                .collect::<Vec<_>>()
                .join(",");
            e.emit("POLYLINE", &format!("'',({points})"))
        }
        SolvedCurveGeometry::Transformed { basis, transform } => {
            let parent = curve(e, basis)?;
            let operator = transformation_operator(e, *transform);
            e.emit("CURVE_REPLICA", &format!("'',{parent},{operator}"))
        }
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => return None,
    })
}

/// Convert a repeated knot vector into ordered values and multiplicities.
fn compress_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &k in knots {
        match runs.last_mut() {
            Some((value, multiplicity)) if *value == k => *multiplicity += 1,
            _ => runs.push((k, 1)),
        }
    }
    runs.into_iter().unzip()
}

fn int_list(xs: &[usize]) -> String {
    let mut out = String::from("(");
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&x.to_string());
    }
    out.push(')');
    out
}

fn real_list(xs: &[f64]) -> String {
    let mut out = String::from("(");
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&real(*x));
    }
    out.push(')');
    out
}

fn closed_flag(periodic: bool) -> &'static str {
    if periodic {
        ".T."
    } else {
        ".F."
    }
}

fn nurbs_curve(e: &mut Emitter, n: &NurbsCurve) -> Ref {
    let pts: Vec<Ref> = n.control_points().iter().map(|p| point(e, *p)).collect();
    let (knots, mults) = compress_knots(n.knots());
    let ctrl = refs(&pts);
    let base = format!(
        "{},{ctrl},.UNSPECIFIED.,{},.U.",
        n.degree(),
        closed_flag(n.periodic())
    );
    let with_knots = format!("{},{},.UNSPECIFIED.", int_list(&mults), real_list(&knots));
    match n.weights() {
        None => e.emit(
            "B_SPLINE_CURVE_WITH_KNOTS",
            &format!("'',{base},{with_knots}"),
        ),
        Some(w) => {
            // Rational curves require the AND-combined complex instance form.
            let body = format!(
                "( BOUNDED_CURVE() B_SPLINE_CURVE({base}) \
                 B_SPLINE_CURVE_WITH_KNOTS({with_knots}) CURVE() \
                 GEOMETRIC_REPRESENTATION_ITEM() \
                 RATIONAL_B_SPLINE_CURVE({}) REPRESENTATION_ITEM('') )",
                real_list(w)
            );
            e.emit_raw("B_SPLINE_CURVE_WITH_KNOTS", &body)
        }
    }
}

fn nurbs_surface(e: &mut Emitter, n: &NurbsSurface) -> Option<Ref> {
    if !valid_nurbs_surface(n) {
        return None;
    }
    // IR control points are u-major: index i*v_count + j is pole (i, j). STEP's
    // control_points_list is LIST(u) OF LIST(v), so the outer list runs over u.
    let u_count = n.u_count() as usize;
    let v_count = n.v_count() as usize;
    let mut rows: Vec<String> = Vec::with_capacity(u_count);
    for grid_row in n.control_grid() {
        let mut row: Vec<Ref> = Vec::with_capacity(v_count);
        for p in grid_row {
            row.push(point(e, *p));
        }
        rows.push(refs(&row));
    }
    let grid = format!("({})", rows.join(","));

    let (u_knots, u_mults) = compress_knots(n.u_knots());
    let (v_knots, v_mults) = compress_knots(n.v_knots());
    let base = format!(
        "{},{},{grid},.UNSPECIFIED.,{},{},.U.",
        n.u_degree(),
        n.v_degree(),
        closed_flag(n.u_periodic()),
        closed_flag(n.v_periodic())
    );
    let with_knots = format!(
        "{},{},{},{},.UNSPECIFIED.",
        int_list(&u_mults),
        int_list(&v_mults),
        real_list(&u_knots),
        real_list(&v_knots)
    );
    Some(match n.weights() {
        None => e.emit(
            "B_SPLINE_SURFACE_WITH_KNOTS",
            &format!("'',{base},{with_knots}"),
        ),
        Some(w) => {
            // Rational surface weights are LIST(u) OF LIST(v), matching the grid.
            let mut wrows: Vec<String> = Vec::with_capacity(u_count);
            for row in w {
                wrows.push(real_list(row));
            }
            let wgrid = format!("({})", wrows.join(","));
            let body = format!(
                "( BOUNDED_SURFACE() B_SPLINE_SURFACE({base}) \
                 B_SPLINE_SURFACE_WITH_KNOTS({with_knots}) \
                 GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE({wgrid}) \
                 REPRESENTATION_ITEM('') SURFACE() )"
            );
            e.emit_raw("B_SPLINE_SURFACE_WITH_KNOTS", &body)
        }
    })
}

#[cfg(test)]
mod support_tests;
