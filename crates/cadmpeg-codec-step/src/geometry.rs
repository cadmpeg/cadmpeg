// SPDX-License-Identifier: Apache-2.0
//! Converts IR geometry carriers into STEP DATA instances.
//!
//! Conversion appends supporting points, directions, and placements before
//! returning the top-level carrier reference. Analytic carriers use their STEP
//! counterparts. NURBS carriers use `*_WITH_KNOTS`, with complex instances for
//! rational geometry.

use cadmpeg_ir::geometry::{
    knots_nondecreasing, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::{Transform, Transform2};

use crate::writer::{real, refs, Emitter, Ref};

const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12: f64 = 1.0e-12;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10: f64 = 1.0e-10;
const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12: f64 = 1.0e-12;

pub(crate) fn surface_is_supported(surface: &SurfaceGeometry) -> bool {
    match surface {
        SurfaceGeometry::Procedural {
            cache: Some(geometry),
            ..
        } => surface_is_supported(geometry),
        SurfaceGeometry::Transformed { basis, transform } => {
            similarity_transform(transform) && surface_is_supported(basis)
        }
        SurfaceGeometry::Plane { .. }
        | SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. }
        | SurfaceGeometry::Torus { .. } => true,
        SurfaceGeometry::Nurbs(n) => valid_nurbs_surface(n),
        SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => false,
    }
}

fn valid_nurbs_surface(n: &NurbsSurface) -> bool {
    n.control_points()
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        && n.weights().is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        })
        && knots_nondecreasing(n.u_knots())
        && knots_nondecreasing(n.v_knots())
}

pub(crate) fn curve_is_supported(curve: &CurveGeometry) -> bool {
    match curve {
        CurveGeometry::Procedural {
            cache: Some(geometry),
            ..
        } => curve_is_supported(geometry),
        CurveGeometry::Transformed { basis, transform } => {
            similarity_transform(transform) && curve_is_supported(basis)
        }
        CurveGeometry::Line { .. }
        | CurveGeometry::Circle { .. }
        | CurveGeometry::Ellipse { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Nurbs(_)
        | CurveGeometry::Polyline { .. } => true,
        CurveGeometry::Procedural { .. } | CurveGeometry::Unknown { .. } => false,
    }
}

fn similarity_transform(transform: &Transform) -> bool {
    if transform
        .rows
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
        || transform.rows[3][0].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || transform.rows[3][1].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || transform.rows[3][2].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || (transform.rows[3][3] - 1.0).abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
    {
        return false;
    }
    let columns = [
        Vector3::new(
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ),
        Vector3::new(
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ),
        Vector3::new(
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
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
    if !transform.is_affine() {
        return false;
    }
    let first = Point2::new(transform.rows[0][0], transform.rows[1][0]);
    let second = Point2::new(transform.rows[0][1], transform.rows[1][1]);
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
    let origin = point2(e, Point2::new(transform.rows[0][2], transform.rows[1][2]));
    let x = Point2::new(transform.rows[0][0], transform.rows[1][0]);
    let y = Point2::new(transform.rows[0][1], transform.rows[1][1]);
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
        PcurveGeometry::Line { origin, direction } => {
            let point = point2(e, *origin);
            let magnitude = (direction.u * direction.u + direction.v * direction.v).sqrt();
            let direction = direction2(e, *direction);
            let vector = e.emit("VECTOR", &format!("'',{direction},{}", real(magnitude)));
            e.emit("LINE", &format!("'',{point},{vector}"))
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            radius,
            ..
        } => {
            let placement = axis2_placement_2d(e, *center, *x_axis);
            e.emit("CIRCLE", &format!("'',{placement},{}", real(*radius)))
        }
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let placement = axis2_placement_2d(e, *center, *x_axis);
            e.emit(
                "ELLIPSE",
                &format!(
                    "'',{placement},{},{}",
                    real(*major_radius),
                    real(*minor_radius)
                ),
            )
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            focal_distance,
            ..
        } => {
            let placement = axis2_placement_2d(e, *vertex, *x_axis);
            e.emit(
                "PARABOLA",
                &format!("'',{placement},{}", real(*focal_distance)),
            )
        }
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let placement = axis2_placement_2d(e, *center, *x_axis);
            e.emit(
                "HYPERBOLA",
                &format!(
                    "'',{placement},{},{}",
                    real(*major_radius),
                    real(*minor_radius)
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
        PcurveGeometry::Trimmed {
            parameter_range,
            same_sense,
            basis,
        } => {
            let basis = pcurve(e, basis)?;
            let sense = if *same_sense { ".T." } else { ".F." };
            e.emit(
                "TRIMMED_CURVE",
                &format!(
                    "'',{basis},({}),({}),{sense},.PARAMETER.",
                    real(parameter_range[0]),
                    real(parameter_range[1])
                ),
            )
        }
        PcurveGeometry::Offset { distance, basis } => {
            let basis = pcurve(e, basis)?;
            e.emit(
                "OFFSET_CURVE_2D",
                &format!("'',{basis},{},.F.", real(*distance)),
            )
        }
        PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Hyperbolic { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => return None,
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
            transform.rows[0][3],
            transform.rows[1][3],
            transform.rows[2][3],
        ),
    );
    let x = Vector3::new(
        transform.rows[0][0],
        transform.rows[1][0],
        transform.rows[2][0],
    );
    let y = Vector3::new(
        transform.rows[0][1],
        transform.rows[1][1],
        transform.rows[2][1],
    );
    let z = Vector3::new(
        transform.rows[0][2],
        transform.rows[1][2],
        transform.rows[2][2],
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
pub fn surface(e: &mut Emitter, g: &SurfaceGeometry) -> Option<Ref> {
    Some(match g {
        SurfaceGeometry::Procedural {
            cache: Some(geometry),
            ..
        } => return surface(e, geometry),
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let pl = placement(e, *origin, *normal, *u_axis);
            e.emit("PLANE", &format!("'',{pl}"))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            let pl = placement(e, *origin, *axis, *ref_direction);
            e.emit("CYLINDRICAL_SURFACE", &format!("'',{pl},{}", real(*radius)))
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio: _,
            half_angle,
        } => {
            let pl = placement(e, *origin, *axis, *ref_direction);
            e.emit(
                "CONICAL_SURFACE",
                &format!("'',{pl},{},{}", real(*radius), real(*half_angle)),
            )
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let pl = placement(e, *center, *axis, *ref_direction);
            e.emit(
                "SPHERICAL_SURFACE",
                &format!("'',{pl},{}", real(radius.abs())),
            )
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
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
        SurfaceGeometry::Nurbs(n) => nurbs_surface(e, n)?,
        SurfaceGeometry::Transformed { basis, transform } => {
            let parent = surface(e, basis)?;
            let operator = transformation_operator(e, *transform);
            e.emit("SURFACE_REPLICA", &format!("'',{parent},{operator}"))
        }
        // These carrier families have no direct STEP representation; callers
        // report the omitted carrier instead of fabricating a placeholder.
        SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => return None,
    })
}

/// Emit an analytic or NURBS 3D curve carrier.
pub fn curve(e: &mut Emitter, g: &CurveGeometry) -> Option<Ref> {
    Some(match g {
        CurveGeometry::Procedural {
            cache: Some(geometry),
            ..
        } => return curve(e, geometry),
        CurveGeometry::Line {
            origin,
            direction: d,
        } => {
            let p = point(e, *origin);
            // A LINE's VECTOR carries the direction; unit magnitude is conventional.
            let dir = direction(e, *d);
            let vec = e.emit("VECTOR", &format!("'',{dir},{}", real(1.0)));
            e.emit("LINE", &format!("'',{p},{vec}"))
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let pl = placement(e, *center, *axis, *ref_direction);
            e.emit("CIRCLE", &format!("'',{pl},{}", real(*radius)))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let pl = placement(e, *center, *axis, *major_direction);
            e.emit(
                "ELLIPSE",
                &format!("'',{pl},{},{}", real(*major_radius), real(*minor_radius)),
            )
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            let pl = placement(e, *vertex, *axis, *major_direction);
            e.emit("PARABOLA", &format!("'',{pl},{}", real(*focal_distance)))
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let pl = placement(e, *center, *axis, *major_direction);
            e.emit(
                "HYPERBOLA",
                &format!("'',{pl},{},{}", real(*major_radius), real(*minor_radius)),
            )
        }
        CurveGeometry::Degenerate { point: collapsed } => {
            let point = point(e, *collapsed);
            e.emit("POLYLINE", &format!("'',({point},{point})"))
        }
        CurveGeometry::Nurbs(n) => nurbs_curve(e, n),
        CurveGeometry::Polyline { points, .. } => {
            let points = points
                .iter()
                .map(|position| point(e, *position).to_string())
                .collect::<Vec<_>>()
                .join(",");
            e.emit("POLYLINE", &format!("'',({points})"))
        }
        CurveGeometry::Transformed { basis, transform } => {
            let parent = curve(e, basis)?;
            let operator = transformation_operator(e, *transform);
            e.emit("CURVE_REPLICA", &format!("'',{parent},{operator}"))
        }
        CurveGeometry::Composite { .. } => {
            unreachable!("composite curves are emitted from their child graph")
        }
        CurveGeometry::Procedural { .. } | CurveGeometry::Unknown { .. } => return None,
    })
}

/// Convert a repeated knot vector into ordered values and multiplicities.
fn compress_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut values = Vec::new();
    let mut mults = Vec::new();
    for &k in knots {
        if let Some(last) = values.last() {
            if *last == k {
                *mults
                    .last_mut()
                    .expect("invariant: mults and values grow in lockstep") += 1;
                continue;
            }
        }
        values.push(k);
        mults.push(1);
    }
    (values, mults)
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
    for i in 0..u_count {
        let mut row: Vec<Ref> = Vec::with_capacity(v_count);
        for j in 0..v_count {
            let idx = i * v_count + j;
            let p = n.control_points()[idx];
            row.push(point(e, p));
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
            for i in 0..u_count {
                let slice: Vec<f64> = (0..v_count).map(|j| w[i * v_count + j]).collect();
                wrows.push(real_list(&slice));
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
