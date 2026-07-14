// SPDX-License-Identifier: Apache-2.0
//! Converts IR geometry carriers into STEP DATA instances.
//!
//! Conversion appends supporting points, directions, and placements before
//! returning the top-level carrier reference. Analytic carriers use their STEP
//! counterparts. NURBS carriers use `*_WITH_KNOTS`, with complex instances for
//! rational geometry.

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use crate::writer::{real, refs, Emitter, Ref};

/// Emit or reuse a `CARTESIAN_POINT`.
pub fn point(e: &mut Emitter, p: Point3) -> Ref {
    let params = format!("'',({},{},{})", real(p.x), real(p.y), real(p.z));
    e.emit_interned("CARTESIAN_POINT", &params)
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

fn transformation_operator(e: &mut Emitter, transform: Transform) -> Ref {
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
pub fn surface(e: &mut Emitter, g: &SurfaceGeometry) -> Ref {
    match g {
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
                    real(*major_radius),
                    real(minor_radius.abs())
                ),
            )
        }
        SurfaceGeometry::Nurbs(n) => nurbs_surface(e, n),
        SurfaceGeometry::Transformed { basis, transform } => {
            let parent = surface(e, basis);
            let operator = transformation_operator(e, *transform);
            e.emit("SURFACE_REPLICA", &format!("'',{parent},{operator}"))
        }
        // Unknown surfaces have no STEP representation; the writer filters faces
        // resting on them in `emit_face` before ever reaching here.
        SurfaceGeometry::Polygonal { .. } | SurfaceGeometry::Unknown { .. } => {
            unreachable!("unknown surfaces are filtered before surface emission")
        }
    }
}

/// Emit an analytic or NURBS 3D curve carrier.
pub fn curve(e: &mut Emitter, g: &CurveGeometry) -> Ref {
    match g {
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
            let parent = curve(e, basis);
            let operator = transformation_operator(e, *transform);
            e.emit("CURVE_REPLICA", &format!("'',{parent},{operator}"))
        }
        CurveGeometry::Unknown { .. } => {
            unreachable!("unknown curves are filtered before emission")
        }
    }
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
    let pts: Vec<Ref> = n.control_points.iter().map(|p| point(e, *p)).collect();
    let (knots, mults) = compress_knots(&n.knots);
    let ctrl = refs(&pts);
    let base = format!(
        "{},{ctrl},.UNSPECIFIED.,{},.U.",
        n.degree,
        closed_flag(n.periodic)
    );
    let with_knots = format!("{},{},.UNSPECIFIED.", int_list(&mults), real_list(&knots));
    match &n.weights {
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

fn nurbs_surface(e: &mut Emitter, n: &NurbsSurface) -> Ref {
    // IR control points are u-major: index i*v_count + j is pole (i, j). STEP's
    // control_points_list is LIST(u) OF LIST(v), so the outer list runs over u.
    let u_count = n.u_count as usize;
    let v_count = n.v_count as usize;
    let mut rows: Vec<String> = Vec::with_capacity(u_count);
    for i in 0..u_count {
        let mut row: Vec<Ref> = Vec::with_capacity(v_count);
        for j in 0..v_count {
            let idx = i * v_count + j;
            let p = n
                .control_points
                .get(idx)
                .copied()
                .unwrap_or(Point3::new(0.0, 0.0, 0.0));
            row.push(point(e, p));
        }
        rows.push(refs(&row));
    }
    let grid = format!("({})", rows.join(","));

    let (u_knots, u_mults) = compress_knots(&n.u_knots);
    let (v_knots, v_mults) = compress_knots(&n.v_knots);
    let base = format!(
        "{},{},{grid},.UNSPECIFIED.,{},{},.U.",
        n.u_degree,
        n.v_degree,
        closed_flag(n.u_periodic),
        closed_flag(n.v_periodic)
    );
    let with_knots = format!(
        "{},{},{},{},.UNSPECIFIED.",
        int_list(&u_mults),
        int_list(&v_mults),
        real_list(&u_knots),
        real_list(&v_knots)
    );
    match &n.weights {
        None => e.emit(
            "B_SPLINE_SURFACE_WITH_KNOTS",
            &format!("'',{base},{with_knots}"),
        ),
        Some(w) => {
            // Rational surface weights are LIST(u) OF LIST(v), matching the grid.
            let mut wrows: Vec<String> = Vec::with_capacity(u_count);
            for i in 0..u_count {
                let slice: Vec<f64> = (0..v_count)
                    .map(|j| w.get(i * v_count + j).copied().unwrap_or(1.0))
                    .collect();
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
    }
}
