// SPDX-License-Identifier: Apache-2.0
//! Point evaluation of geometry carriers.
//!
//! Evaluators map carrier parameters to model-space (or parameter-space)
//! points using the carriers' own parameterizations: conic parameters are
//! angles from the reference/major direction, line parameters are signed
//! distances along the unit direction, and B-splines evaluate by Cox–de Boor
//! over their stored knot vectors. Carriers without a typed parameterization
//! ([`CurveGeometry::Unknown`], [`SurfaceGeometry::Unknown`], parabolas, and
//! hyperbolas) evaluate to `None`.

use std::collections::BTreeSet;

use crate::document::CadIr;
use crate::geometry::{
    CurveGeometry, NurbsSurface, PcurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use crate::ids::SurfaceId;
use crate::math::{Point2, Point3, Vector3};

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// `base + Σ factorᵢ · directionᵢ` in model space.
fn offset(base: Point3, terms: &[(f64, Vector3)]) -> Point3 {
    let mut out = base;
    for (factor, direction) in terms {
        out.x += factor * direction.x;
        out.y += factor * direction.y;
        out.z += factor * direction.z;
    }
    out
}

/// Knot span index of `t` for a clamped B-spline basis, or `None` when the
/// knot vector cannot support `count` poles of the given degree.
fn bspline_span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    if knots.len() < count + degree + 1 || count <= degree {
        return None;
    }
    if t >= knots[count] {
        return Some(count - 1);
    }
    if t <= knots[degree] {
        return Some(degree);
    }
    let mut lo = degree;
    let mut hi = count;
    while lo < hi {
        let mid = usize::midpoint(lo, hi);
        if t < knots[mid] {
            hi = mid;
        } else if t >= knots[mid + 1] {
            lo = mid + 1;
        } else {
            return Some(mid);
        }
    }
    Some(lo)
}

/// Non-zero basis function values at `t` for the given span (Cox–de Boor).
fn bspline_basis(knots: &[f64], degree: usize, span: usize, t: f64) -> Vec<f64> {
    let mut values = vec![1.0];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        let mut next = vec![0.0; j + 1];
        for (r, &value) in values.iter().enumerate().take(j) {
            let denominator = right[r + 1] + left[j - r];
            let factor = if denominator == 0.0 {
                0.0
            } else {
                value / denominator
            };
            next[r] = saved + right[r + 1] * factor;
            saved = left[j - r] * factor;
        }
        next[j] = saved;
        values = next;
    }
    values
}

/// First derivatives of the non-zero basis functions returned by
/// [`bspline_basis`].
fn bspline_basis_derivative(knots: &[f64], degree: usize, span: usize, t: f64) -> Vec<f64> {
    if degree == 0 {
        return vec![0.0];
    }
    let lower = bspline_basis(knots, degree - 1, span, t);
    (0..=degree)
        .map(|offset| {
            let index = span - degree + offset;
            let left = if offset == 0 {
                0.0
            } else {
                let denominator = knots[index + degree] - knots[index];
                if denominator == 0.0 {
                    0.0
                } else {
                    degree as f64 * lower[offset - 1] / denominator
                }
            };
            let right = if offset == degree {
                0.0
            } else {
                let denominator = knots[index + degree + 1] - knots[index + 1];
                if denominator == 0.0 {
                    0.0
                } else {
                    degree as f64 * lower[offset] / denominator
                }
            };
            left - right
        })
        .collect()
}

/// Evaluate a possibly-rational B-spline curve over 3D poles.
pub fn nurbs_curve_point(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Point3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut weight_sum = 0.0;
    for (i, value) in basis.iter().enumerate() {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        x += value * weight * pole.x;
        y += value * weight * pole.y;
        z += value * weight * pole.z;
        weight_sum += value * weight;
    }
    (weight_sum != 0.0).then(|| Point3::new(x / weight_sum, y / weight_sum, z / weight_sum))
}

/// Evaluate a possibly-rational B-spline curve over 2D `(u, v)` poles.
pub fn nurbs_pcurve_uv(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Point2> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let mut u = 0.0;
    let mut v = 0.0;
    let mut weight_sum = 0.0;
    for (i, value) in basis.iter().enumerate() {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        u += value * weight * pole.u;
        v += value * weight * pole.v;
        weight_sum += value * weight;
    }
    (weight_sum != 0.0).then(|| Point2::new(u / weight_sum, v / weight_sum))
}

/// Evaluate a tensor-product NURBS surface at `(u, v)`.
pub fn nurbs_surface_point(surface: &NurbsSurface, u_at: f64, v_at: f64) -> Option<Point3> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)? {
        return None;
    }
    let u_span = bspline_span(&surface.u_knots, u_degree, u_count, u_at)?;
    let v_span = bspline_span(&surface.v_knots, v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(&surface.u_knots, u_degree, u_span, u_at);
    let v_basis = bspline_basis(&surface.v_knots, v_degree, v_span, v_at);
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut weight_sum = 0.0;
    for (i, u_value) in u_basis.iter().enumerate() {
        for (j, v_value) in v_basis.iter().enumerate() {
            let index = (u_span - u_degree + i) * v_count + (v_span - v_degree + j);
            let weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index).copied())
                .unwrap_or(1.0);
            let factor = u_value * v_value * weight;
            let pole = surface.control_points.get(index)?;
            x += factor * pole.x;
            y += factor * pole.y;
            z += factor * pole.z;
            weight_sum += factor;
        }
    }
    (weight_sum != 0.0).then(|| Point3::new(x / weight_sum, y / weight_sum, z / weight_sum))
}

/// Evaluate the exact first parameter derivatives of a tensor-product NURBS
/// surface. Rational carriers use the homogeneous quotient rule.
pub fn nurbs_surface_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<(Vector3, Vector3)> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)? {
        return None;
    }
    let u_span = bspline_span(&surface.u_knots, u_degree, u_count, u_at)?;
    let v_span = bspline_span(&surface.v_knots, v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(&surface.u_knots, u_degree, u_span, u_at);
    let v_basis = bspline_basis(&surface.v_knots, v_degree, v_span, v_at);
    let u_derivative = bspline_basis_derivative(&surface.u_knots, u_degree, u_span, u_at);
    let v_derivative = bspline_basis_derivative(&surface.v_knots, v_degree, v_span, v_at);
    let mut weighted_point = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_u = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_v = Vector3::new(0.0, 0.0, 0.0);
    let mut weight = 0.0;
    let mut weight_u = 0.0;
    let mut weight_v = 0.0;
    for (i, u_value) in u_basis.iter().enumerate() {
        for (j, v_value) in v_basis.iter().enumerate() {
            let index = (u_span - u_degree + i) * v_count + (v_span - v_degree + j);
            let pole_weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index).copied())
                .unwrap_or(1.0);
            let pole = surface.control_points.get(index)?;
            let factor = u_value * v_value * pole_weight;
            let factor_u = u_derivative[i] * v_value * pole_weight;
            let factor_v = u_value * v_derivative[j] * pole_weight;
            weighted_point.x += factor * pole.x;
            weighted_point.y += factor * pole.y;
            weighted_point.z += factor * pole.z;
            weighted_u.x += factor_u * pole.x;
            weighted_u.y += factor_u * pole.y;
            weighted_u.z += factor_u * pole.z;
            weighted_v.x += factor_v * pole.x;
            weighted_v.y += factor_v * pole.y;
            weighted_v.z += factor_v * pole.z;
            weight += factor;
            weight_u += factor_u;
            weight_v += factor_v;
        }
    }
    if !weight.is_finite() || weight == 0.0 {
        return None;
    }
    let denominator = weight * weight;
    Some((
        Vector3::new(
            (weighted_u.x * weight - weighted_point.x * weight_u) / denominator,
            (weighted_u.y * weight - weighted_point.y * weight_u) / denominator,
            (weighted_u.z * weight - weighted_point.z * weight_u) / denominator,
        ),
        Vector3::new(
            (weighted_v.x * weight - weighted_point.x * weight_v) / denominator,
            (weighted_v.y * weight - weighted_point.y * weight_v) / denominator,
            (weighted_v.z * weight - weighted_point.z * weight_v) / denominator,
        ),
    ))
}

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization.
pub fn curve_point(geometry: &CurveGeometry, t: f64) -> Option<Point3> {
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(offset(*origin, &[(t, *direction)])),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *center,
            &[
                (radius * t.cos(), *ref_direction),
                (radius * t.sin(), cross(*axis, *ref_direction)),
            ],
        )),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => Some(offset(
            *center,
            &[
                (major_radius * t.cos(), *major_direction),
                (minor_radius * t.sin(), cross(*axis, *major_direction)),
            ],
        )),
        CurveGeometry::Degenerate { point } => Some(*point),
        CurveGeometry::Nurbs(nurbs) => nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            t,
        ),
        CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization: `u` is
/// the azimuth angle and `v` the axial distance / polar angle on analytic
/// quadrics, and both are knot-domain parameters on NURBS surfaces.
pub fn surface_point(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<Point3> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => Some(offset(
            *origin,
            &[(u, *u_axis), (v, cross(*normal, *u_axis))],
        )),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *origin,
            &[
                (radius * u.cos(), *ref_direction),
                (radius * u.sin(), cross(*axis, *ref_direction)),
                (v, *axis),
            ],
        )),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let local_radius = radius + v * half_angle.tan();
            Some(offset(
                *origin,
                &[
                    (local_radius * u.cos(), *ref_direction),
                    (local_radius * ratio * u.sin(), cross(*axis, *ref_direction)),
                    (v, *axis),
                ],
            ))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *center,
            &[
                (radius * v.cos() * u.cos(), *ref_direction),
                (radius * v.cos() * u.sin(), cross(*axis, *ref_direction)),
                (radius * v.sin(), *axis),
            ],
        )),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let ring = major_radius + minor_radius * v.cos();
            Some(offset(
                *center,
                &[
                    (ring * u.cos(), *ref_direction),
                    (ring * u.sin(), cross(*axis, *ref_direction)),
                    (minor_radius * v.sin(), *axis),
                ],
            ))
        }
        SurfaceGeometry::Nurbs(nurbs) => nurbs_surface_point(nurbs, u, v),
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn unit(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}

/// Evaluate the oriented unit normal of an explicit surface carrier.
pub fn surface_normal(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<Vector3> {
    match geometry {
        SurfaceGeometry::Plane { normal, .. } => unit(*normal),
        SurfaceGeometry::Cylinder {
            axis,
            ref_direction,
            radius,
            ..
        } => {
            let transverse = cross(*axis, *ref_direction);
            unit(Vector3::new(
                radius.signum() * (u.cos() * ref_direction.x + u.sin() * transverse.x),
                radius.signum() * (u.cos() * ref_direction.y + u.sin() * transverse.y),
                radius.signum() * (u.cos() * ref_direction.z + u.sin() * transverse.z),
            ))
        }
        SurfaceGeometry::Sphere {
            axis,
            ref_direction,
            radius,
            ..
        } => {
            let transverse = cross(*axis, *ref_direction);
            unit(Vector3::new(
                radius.signum()
                    * (v.cos() * u.cos() * ref_direction.x
                        + v.cos() * u.sin() * transverse.x
                        + v.sin() * axis.x),
                radius.signum()
                    * (v.cos() * u.cos() * ref_direction.y
                        + v.cos() * u.sin() * transverse.y
                        + v.sin() * axis.y),
                radius.signum()
                    * (v.cos() * u.cos() * ref_direction.z
                        + v.cos() * u.sin() * transverse.z
                        + v.sin() * axis.z),
            ))
        }
        SurfaceGeometry::Torus {
            axis,
            ref_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            let transverse = cross(*axis, *ref_direction);
            let orientation = (minor_radius * (major_radius + minor_radius * v.cos())).signum();
            unit(Vector3::new(
                orientation
                    * (v.cos() * (u.cos() * ref_direction.x + u.sin() * transverse.x)
                        + v.sin() * axis.x),
                orientation
                    * (v.cos() * (u.cos() * ref_direction.y + u.sin() * transverse.y)
                        + v.sin() * axis.y),
                orientation
                    * (v.cos() * (u.cos() * ref_direction.z + u.sin() * transverse.z)
                        + v.sin() * axis.z),
            ))
        }
        SurfaceGeometry::Cone {
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
            ..
        } => {
            let transverse = cross(*axis, *ref_direction);
            let local_radius = radius + v * half_angle.tan();
            let tangent_u = Vector3::new(
                -local_radius * u.sin() * ref_direction.x
                    + local_radius * ratio * u.cos() * transverse.x,
                -local_radius * u.sin() * ref_direction.y
                    + local_radius * ratio * u.cos() * transverse.y,
                -local_radius * u.sin() * ref_direction.z
                    + local_radius * ratio * u.cos() * transverse.z,
            );
            let slope = half_angle.tan();
            let tangent_v = Vector3::new(
                slope * u.cos() * ref_direction.x + slope * ratio * u.sin() * transverse.x + axis.x,
                slope * u.cos() * ref_direction.y + slope * ratio * u.sin() * transverse.y + axis.y,
                slope * u.cos() * ref_direction.z + slope * ratio * u.sin() * transverse.z + axis.z,
            );
            unit(cross(tangent_u, tangent_v))
        }
        SurfaceGeometry::Nurbs(nurbs) => {
            let (tangent_u, tangent_v) = nurbs_surface_partials(nurbs, u, v)?;
            unit(cross(tangent_u, tangent_v))
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a model surface by identity, resolving exact procedural carriers.
pub fn model_surface_point(ir: &CadIr, surface: &SurfaceId, u: f64, v: f64) -> Option<Point3> {
    model_surface_point_inner(ir, surface, u, v, &mut BTreeSet::new())
}

fn model_surface_point_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    visiting: &mut BTreeSet<SurfaceId>,
) -> Option<Point3> {
    visiting.insert(surface.clone()).then_some(())?;
    let carrier = ir.model.surfaces.iter().find(|item| &item.id == surface)?;
    let result = match &carrier.geometry {
        SurfaceGeometry::Procedural { construction } => {
            let procedural = ir
                .model
                .procedural_surfaces
                .iter()
                .find(|item| &item.id == construction && &item.surface == surface)?;
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset {
                    support, distance, ..
                } => {
                    let point = model_surface_point_inner(ir, support, u, v, visiting)?;
                    let normal = model_surface_normal(ir, support, u, v, visiting)?;
                    Some(offset(point, &[(*distance, normal)]))
                }
                _ => None,
            }
        }
        geometry => surface_point(geometry, u, v),
    };
    visiting.remove(surface);
    result
}

fn model_surface_normal(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    visiting: &mut BTreeSet<SurfaceId>,
) -> Option<Vector3> {
    visiting.insert(surface.clone()).then_some(())?;
    let carrier = ir.model.surfaces.iter().find(|item| &item.id == surface)?;
    let result = match &carrier.geometry {
        SurfaceGeometry::Procedural { construction } => {
            let procedural = ir
                .model
                .procedural_surfaces
                .iter()
                .find(|item| &item.id == construction && &item.surface == surface)?;
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    model_surface_normal(ir, support, u, v, visiting)
                }
                _ => None,
            }
        }
        geometry => surface_normal(geometry, u, v),
    };
    visiting.remove(surface);
    result
}

/// Evaluate a pcurve carrier at parameter `t`, yielding a surface `(u, v)`.
pub fn pcurve_uv(geometry: &PcurveGeometry, t: f64) -> Option<Point2> {
    match geometry {
        PcurveGeometry::Line { origin, direction } => Some(Point2::new(
            origin.u + t * direction.u,
            origin.v + t * direction.v,
        )),
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => nurbs_pcurve_uv(*degree, knots, control_points, weights.as_deref(), t),
    }
}
