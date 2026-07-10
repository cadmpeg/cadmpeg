// SPDX-License-Identifier: Apache-2.0
//! Emission of STEP geometric primitives from IR geometry carriers.
//!
//! Every function here appends the instances it needs (points, directions,
//! placements) and returns the reference to the top-level carrier. Analytic
//! carriers map one-to-one to their STEP counterparts; NURBS carriers map to the
//! `*_WITH_KNOTS` families, using the complex rational form when weighted.

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::writer::{real, refs, Emitter, Ref};

/// Emit (or reuse) a `CARTESIAN_POINT`.
pub fn point(e: &mut Emitter, p: Point3) -> Ref {
    let params = format!("'',({},{},{})", real(p.x), real(p.y), real(p.z));
    e.emit_interned("CARTESIAN_POINT", &params)
}

/// Emit (or reuse) a `DIRECTION`, normalized to unit length. A zero-length input
/// is passed through as `(0,0,1)` so downstream placements stay well-formed;
/// callers that care about degeneracy check before calling.
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

/// Emit an `AXIS2_PLACEMENT_3D` at `origin` with local +Z `axis` and local +X
/// `ref_dir`. STEP projects `ref_dir` onto the plane normal to `axis`, so it need
/// only be non-parallel to `axis`.
pub fn placement(e: &mut Emitter, origin: Point3, axis: Vector3, ref_dir: Vector3) -> Ref {
    let o = point(e, origin);
    let a = direction(e, axis);
    let r = direction(e, ref_dir);
    e.emit("AXIS2_PLACEMENT_3D", &format!("'',{o},{a},{r}"))
}

/// An arbitrary unit vector orthogonal to `axis`, used as the local +X for
/// analytic surfaces/curves whose IR form fixes only an axis. The specific
/// choice is immaterial to the surface's geometry.
fn orthogonal(axis: Vector3) -> Vector3 {
    let n = axis.norm();
    let a = if n > 0.0 {
        Vector3::new(axis.x / n, axis.y / n, axis.z / n)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    // Pick a seed axis least aligned with `a`, then Gram-Schmidt it.
    let seed = if a.x.abs() <= a.y.abs() && a.x.abs() <= a.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if a.y.abs() <= a.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let d = seed.x * a.x + seed.y * a.y + seed.z * a.z;
    let r = Vector3::new(seed.x - d * a.x, seed.y - d * a.y, seed.z - d * a.z);
    let rn = r.norm();
    if rn > 0.0 {
        Vector3::new(r.x / rn, r.y / rn, r.z / rn)
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    }
}

/// Emit an analytic or NURBS surface carrier, returning its reference.
pub fn surface(e: &mut Emitter, g: &SurfaceGeometry) -> Ref {
    match g {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let ref_dir = u_axis.unwrap_or_else(|| orthogonal(*normal));
            let pl = placement(e, *origin, *normal, ref_dir);
            e.emit("PLANE", &format!("'',{pl}"))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            let ref_dir = ref_direction.unwrap_or_else(|| orthogonal(*axis));
            let pl = placement(e, *origin, *axis, ref_dir);
            e.emit("CYLINDRICAL_SURFACE", &format!("'',{pl},{}", real(*radius)))
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            half_angle,
        } => {
            let ref_dir = ref_direction.unwrap_or_else(|| orthogonal(*axis));
            let pl = placement(e, *origin, *axis, ref_dir);
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
            let axis = axis.unwrap_or(Vector3::new(0.0, 0.0, 1.0));
            let ref_dir = ref_direction.unwrap_or_else(|| orthogonal(axis));
            let pl = placement(e, *center, axis, ref_dir);
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
            let ref_dir = ref_direction.unwrap_or_else(|| orthogonal(*axis));
            let pl = placement(e, *center, *axis, ref_dir);
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
        // Unknown surfaces have no STEP representation; the writer filters faces
        // resting on them in `emit_face` before ever reaching here.
        SurfaceGeometry::Unknown { .. } => {
            unreachable!("unknown surfaces are filtered before surface emission")
        }
    }
}

/// Emit an analytic or NURBS 3D curve carrier, returning its reference.
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
            radius,
        } => {
            let pl = placement(e, *center, *axis, orthogonal(*axis));
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
        CurveGeometry::Nurbs(n) => nurbs_curve(e, n),
    }
}

/// Compress a full (repeated) knot vector into distinct knots and their
/// multiplicities, in the order STEP's `*_WITH_KNOTS` entities expect.
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
