// SPDX-License-Identifier: Apache-2.0
//! A chart reduced to its `v`-isoline at one `u`, or to its `u`-isoline at
//! one `v`, for inner integrals along the isoline.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_ir::features::geometry::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroReal;

use super::placement::Placement;
use super::surface::{Chart, Partials};
use crate::context::Eval;
use crate::failure::Measure;

/// The highest degree a column evaluates with stack basis buffers.
// resource-cap: MAX_COLUMN_DEGREE = N
const MAX_COLUMN_DEGREE: usize = 15;

/// Basis values of one degree, on the stack.
type Basis = [f64; MAX_COLUMN_DEGREE + 1];

/// A chart along its `v`-isoline at one `u`, or along its `u`-isoline at
/// one `v` when `along_u` is set.
pub(crate) struct Column<'a, 'ir> {
    chart: &'a Chart<'ir>,
    at: f64,
    along_u: bool,
    isoline: Option<Isoline<'a>>,
}

/// A NURBS isoline in homogeneous form, with its `u`-partial.
struct Isoline<'a> {
    knots: &'a [f64],
    degree: usize,
    domain: [f64; 2],
    /// The poles contracted with the fixed parameter's basis values, along
    /// the isoline.
    controls: Vec<[f64; 4]>,
    /// The poles contracted with the fixed parameter's basis derivatives.
    across: Vec<[f64; 4]>,
    /// Whether the isoline runs along `u`.
    along_u: bool,
    /// Whether the source surface carries non-unit pole weights.
    rational: bool,
    twice_across: Option<Vec<[f64; 4]>>,
    offset_distance: Option<f64>,
    periodic: bool,
    placement: Placement,
}

impl<'a, 'ir> Column<'a, 'ir> {
    /// The column of `chart` at `u`.
    pub(crate) fn new(eval: &Eval<'_, '_>, chart: &'a Chart<'ir>, u: f64) -> Measure<Self> {
        let isoline = match chart {
            Chart::Nurbs(nurbs) if nurbs.evaluation.is_none() || nurbs.column_offset.is_some() => {
                Isoline::new(
                    eval,
                    nurbs.surface,
                    u,
                    nurbs.domain,
                    nurbs.placement,
                    nurbs.column_offset,
                )?
            }
            _ => None,
        };
        Ok(Self {
            chart,
            at: u,
            along_u: false,
            isoline,
        })
    }

    /// The `u`-isoline of a NURBS `chart` at `v`.
    pub(crate) fn across(eval: &Eval<'_, '_>, chart: &'a Chart<'ir>, v: f64) -> Measure<Self> {
        let isoline = match chart {
            Chart::Nurbs(nurbs) if nurbs.evaluation.is_none() && nurbs.column_offset.is_none() => {
                Isoline::across(eval, nurbs.surface, v, nurbs.domain, nurbs.placement)?
            }
            _ => None,
        };
        Ok(Self {
            chart,
            at: v,
            along_u: true,
            isoline,
        })
    }

    /// The point and first partials at `(u, s)`.
    pub(crate) fn partials(&self, eval: &Eval<'_, '_>, s: f64) -> Measure<Partials> {
        if let Some(isoline) = &self.isoline {
            if let Some(partials) = isoline.partials(eval, s)? {
                return Ok(partials);
            }
        }
        if self.along_u {
            self.chart.partials(eval, s, self.at)
        } else {
            self.chart.partials(eval, self.at, s)
        }
    }
}

impl<'a> Isoline<'a> {
    fn new(
        eval: &Eval<'_, '_>,
        surface: &'a NurbsSurface,
        u: f64,
        [[u0, u1], v]: [[f64; 2]; 2],
        placement: Placement,
        offset_distance: Option<f64>,
    ) -> Measure<Option<Self>> {
        let u = if surface.u_periodic() && !(u0..=u1).contains(&u) {
            u0 + (u - u0).rem_euclid(u1 - u0)
        } else {
            u
        };
        if !(u0..=u1).contains(&u) {
            return Ok(None);
        }
        let (Ok(u_degree), Ok(v_degree)) = (
            usize::try_from(surface.u_degree()),
            usize::try_from(surface.v_degree()),
        ) else {
            return Ok(None);
        };
        if u_degree > MAX_COLUMN_DEGREE || v_degree > MAX_COLUMN_DEGREE {
            return Ok(None);
        }
        let u_knots = surface.u_knots().as_slice();
        let (u_count, v_count) = (surface.u_count(), surface.v_count());
        let Some(span) = span(u_knots, u_degree, u_count, u) else {
            return Ok(None);
        };
        let mut values: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        let mut slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        if !basis(u_knots, u_degree, span, u, &mut values, &mut slopes) {
            return Ok(None);
        }
        let mut curvatures: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        if offset_distance.is_some() && !second_basis(u_knots, u_degree, span, u, &mut curvatures) {
            return Ok(None);
        }
        let work = v_count
            .checked_add(u_degree + 1)
            .and_then(|n| n.checked_mul(u_degree + 1))
            .and_then(|n| n.checked_mul(if offset_distance.is_some() { 3 } else { 1 }))
            .ok_or_else(|| eval.work.exhaustion())?;
        eval.charge(work)?;
        let mut controls = Vec::with_capacity(v_count);
        let mut across = Vec::with_capacity(v_count);
        let mut twice_across = offset_distance.map(|_| Vec::with_capacity(v_count));
        let rational = surface.pole_weights().is_some();
        if let Some(twice_across) = &mut twice_across {
            for j in 0..v_count {
                let mut control = [0.0; 4];
                let mut partial = [0.0; 4];
                let mut second = [0.0; 4];
                for r in 0..=u_degree {
                    let row = span + r - u_degree;
                    let Some(pole) = surface.pole(row, j) else {
                        return Ok(None);
                    };
                    let weight = if rational {
                        surface.weight(row, j).map_or(1.0, NonZeroReal::get)
                    } else {
                        1.0
                    };
                    let pole = pole.get();
                    let homogeneous = [pole.x * weight, pole.y * weight, pole.z * weight, weight];
                    let val = values[r];
                    let slp = slopes[r];
                    let crv = curvatures[r];
                    for lane in 0..4 {
                        control[lane] += val * homogeneous[lane];
                        partial[lane] += slp * homogeneous[lane];
                        second[lane] += crv * homogeneous[lane];
                    }
                }
                controls.push(control);
                across.push(partial);
                twice_across.push(second);
            }
        } else if rational {
            for j in 0..v_count {
                let mut control = [0.0; 4];
                let mut partial = [0.0; 4];
                for r in 0..=u_degree {
                    let row = span + r - u_degree;
                    let Some(pole) = surface.pole(row, j) else {
                        return Ok(None);
                    };
                    let weight = surface.weight(row, j).map_or(1.0, NonZeroReal::get);
                    let pole = pole.get();
                    let homogeneous = [pole.x * weight, pole.y * weight, pole.z * weight, weight];
                    let val = values[r];
                    let slp = slopes[r];
                    for lane in 0..4 {
                        control[lane] += val * homogeneous[lane];
                        partial[lane] += slp * homogeneous[lane];
                    }
                }
                controls.push(control);
                across.push(partial);
            }
        } else {
            for j in 0..v_count {
                let mut control = [0.0, 0.0, 0.0, 1.0];
                let mut partial = [0.0; 4];
                for r in 0..=u_degree {
                    let row = span + r - u_degree;
                    let Some(pole) = surface.pole(row, j) else {
                        return Ok(None);
                    };
                    let pole = pole.get();
                    let val = values[r];
                    let slp = slopes[r];
                    control[0] += val * pole.x;
                    control[1] += val * pole.y;
                    control[2] += val * pole.z;
                    partial[0] += slp * pole.x;
                    partial[1] += slp * pole.y;
                    partial[2] += slp * pole.z;
                }
                controls.push(control);
                across.push(partial);
            }
        }
        Ok(Some(Self {
            knots: surface.v_knots().as_slice(),
            degree: v_degree,
            domain: v,
            controls,
            across,
            along_u: false,
            rational,
            twice_across,
            offset_distance,
            periodic: surface.v_periodic(),
            placement,
        }))
    }

    /// The `u`-isoline at `v` of a surface with no offset.
    fn across(
        eval: &Eval<'_, '_>,
        surface: &'a NurbsSurface,
        v: f64,
        [u, [v0, v1]]: [[f64; 2]; 2],
        placement: Placement,
    ) -> Measure<Option<Self>> {
        let v = if surface.v_periodic() && !(v0..=v1).contains(&v) {
            v0 + (v - v0).rem_euclid(v1 - v0)
        } else {
            v
        };
        if !(v0..=v1).contains(&v) {
            return Ok(None);
        }
        let (Ok(u_degree), Ok(v_degree)) = (
            usize::try_from(surface.u_degree()),
            usize::try_from(surface.v_degree()),
        ) else {
            return Ok(None);
        };
        if u_degree > MAX_COLUMN_DEGREE || v_degree > MAX_COLUMN_DEGREE {
            return Ok(None);
        }
        let v_knots = surface.v_knots().as_slice();
        let (u_count, v_count) = (surface.u_count(), surface.v_count());
        let Some(span) = span(v_knots, v_degree, v_count, v) else {
            return Ok(None);
        };
        let mut values: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        let mut slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        if !basis(v_knots, v_degree, span, v, &mut values, &mut slopes) {
            return Ok(None);
        }
        let work = u_count
            .checked_add(v_degree + 1)
            .and_then(|n| n.checked_mul(v_degree + 1))
            .ok_or_else(|| eval.work.exhaustion())?;
        eval.charge(work)?;
        let mut controls = Vec::with_capacity(u_count);
        let mut across = Vec::with_capacity(u_count);
        let rational = surface.pole_weights().is_some();
        if rational {
            for i in 0..u_count {
                let mut control = [0.0; 4];
                let mut partial = [0.0; 4];
                for r in 0..=v_degree {
                    let column = span + r - v_degree;
                    let Some(pole) = surface.pole(i, column) else {
                        return Ok(None);
                    };
                    let weight = surface.weight(i, column).map_or(1.0, NonZeroReal::get);
                    let pole = pole.get();
                    let homogeneous = [pole.x * weight, pole.y * weight, pole.z * weight, weight];
                    for lane in 0..4 {
                        control[lane] += values[r] * homogeneous[lane];
                        partial[lane] += slopes[r] * homogeneous[lane];
                    }
                }
                controls.push(control);
                across.push(partial);
            }
        } else {
            for i in 0..u_count {
                let mut control = [0.0, 0.0, 0.0, 1.0];
                let mut partial = [0.0; 4];
                for r in 0..=v_degree {
                    let column = span + r - v_degree;
                    let Some(pole) = surface.pole(i, column) else {
                        return Ok(None);
                    };
                    let pole = pole.get();
                    let val = values[r];
                    let slp = slopes[r];
                    control[0] += val * pole.x;
                    control[1] += val * pole.y;
                    control[2] += val * pole.z;
                    partial[0] += slp * pole.x;
                    partial[1] += slp * pole.y;
                    partial[2] += slp * pole.z;
                }
                controls.push(control);
                across.push(partial);
            }
        }
        Ok(Some(Self {
            knots: surface.u_knots().as_slice(),
            degree: u_degree,
            domain: u,
            controls,
            across,
            along_u: true,
            rational,
            twice_across: None,
            offset_distance: None,
            periodic: surface.u_periodic(),
            placement,
        }))
    }

    /// The point and first partials at `s`, or `None` outside the domain or
    /// where the isoline has no finite value.
    fn partials(&self, eval: &Eval<'_, '_>, s: f64) -> Measure<Option<Partials>> {
        let [v0, v1] = self.domain;
        let s = if self.periodic && !(v0..=v1).contains(&s) {
            v0 + (s - v0).rem_euclid(v1 - v0)
        } else {
            s
        };
        if !(v0..=v1).contains(&s) {
            return Ok(None);
        }
        let Some(span) = span(self.knots, self.degree, self.controls.len(), s) else {
            return Ok(None);
        };
        if self.twice_across.is_none() && self.degree <= 3 {
            let mut values = [0.0; 4];
            let mut slopes = [0.0; 4];
            if !basis_sized::<4>(self.knots, self.degree, span, s, &mut values, &mut slopes) {
                return Ok(None);
            }
            eval.charge((self.degree + 1) * (self.degree + 2))?;
            if !self.rational {
                let mut point = [0.0; 3];
                let mut along = [0.0; 3];
                let mut across = [0.0; 3];
                let base = span - self.degree;
                for r in 0..=self.degree {
                    let control = self.controls[base + r];
                    let partial = self.across[base + r];
                    let val = values[r];
                    let slp = slopes[r];
                    point[0] += val * control[0];
                    point[1] += val * control[1];
                    point[2] += val * control[2];
                    along[0] += slp * control[0];
                    along[1] += slp * control[1];
                    along[2] += slp * control[2];
                    across[0] += val * partial[0];
                    across[1] += val * partial[1];
                    across[2] += val * partial[2];
                }
                let at = Point3::new(point[0], point[1], point[2]);
                let along_v = Vector3::new(along[0], along[1], along[2]);
                let across_v = Vector3::new(across[0], across[1], across[2]);
                let (du, dv) = if self.along_u {
                    (along_v, across_v)
                } else {
                    (across_v, along_v)
                };
                if !(at.is_finite() && du.is_finite() && dv.is_finite()) {
                    return Ok(None);
                }
                return Ok(Some(Partials {
                    point: self.placement.point(at),
                    du: self.placement.vector(du),
                    dv: self.placement.vector(dv),
                }));
            }
            let mut point = [0.0; 4];
            let mut along = [0.0; 4];
            let mut across = [0.0; 4];
            let base = span - self.degree;
            for r in 0..=self.degree {
                let control = self.controls[base + r];
                let partial = self.across[base + r];
                let val = values[r];
                let slp = slopes[r];
                for lane in 0..4 {
                    point[lane] += val * control[lane];
                    along[lane] += slp * control[lane];
                    across[lane] += val * partial[lane];
                }
            }
            let weight = point[3];
            if !(weight.is_finite() && weight != 0.0) {
                return Ok(None);
            }
            let at = Point3::new(point[0] / weight, point[1] / weight, point[2] / weight);
            let derivative = |lanes: [f64; 4]| {
                Vector3::new(
                    (lanes[0] - at.x * lanes[3]) / weight,
                    (lanes[1] - at.y * lanes[3]) / weight,
                    (lanes[2] - at.z * lanes[3]) / weight,
                )
            };
            let (du, dv) = if self.along_u {
                (derivative(along), derivative(across))
            } else {
                (derivative(across), derivative(along))
            };
            if !(at.is_finite() && du.is_finite() && dv.is_finite()) {
                return Ok(None);
            }
            return Ok(Some(Partials {
                point: self.placement.point(at),
                du: self.placement.vector(du),
                dv: self.placement.vector(dv),
            }));
        }
        let mut values: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        let mut slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
        if !basis(self.knots, self.degree, span, s, &mut values, &mut slopes) {
            return Ok(None);
        }
        eval.charge(
            (self.degree + 1)
                * (self.degree + 2)
                * if self.offset_distance.is_some() { 3 } else { 1 },
        )?;
        let mut point = [0.0; 4];
        let mut along = [0.0; 4];
        let mut across = [0.0; 4];
        let mut uu = [0.0; 4];
        let mut uv = [0.0; 4];
        let mut vv = [0.0; 4];
        if let Some(twice_across) = &self.twice_across {
            let mut curvatures: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
            if !second_basis(self.knots, self.degree, span, s, &mut curvatures) {
                return Ok(None);
            }
            for r in 0..=self.degree {
                let index = span + r - self.degree;
                let (control, partial) = (self.controls[index], self.across[index]);
                let second = twice_across[index];
                let val = values[r];
                let slp = slopes[r];
                let crv = curvatures[r];
                for lane in 0..4 {
                    point[lane] += val * control[lane];
                    along[lane] += slp * control[lane];
                    across[lane] += val * partial[lane];
                    uu[lane] += val * second[lane];
                    uv[lane] += slp * partial[lane];
                    vv[lane] += crv * control[lane];
                }
            }
        } else {
            for r in 0..=self.degree {
                let index = span + r - self.degree;
                let (control, partial) = (self.controls[index], self.across[index]);
                let val = values[r];
                let slp = slopes[r];
                for lane in 0..4 {
                    point[lane] += val * control[lane];
                    along[lane] += slp * control[lane];
                    across[lane] += val * partial[lane];
                }
            }
        }
        let weight = point[3];
        if !(weight.is_finite() && weight != 0.0) {
            return Ok(None);
        }
        let at = Point3::new(point[0] / weight, point[1] / weight, point[2] / weight);
        let derivative = |lanes: [f64; 4]| {
            Vector3::new(
                (lanes[0] - at.x * lanes[3]) / weight,
                (lanes[1] - at.y * lanes[3]) / weight,
                (lanes[2] - at.z * lanes[3]) / weight,
            )
        };
        let (mut at, mut du, mut dv) = if self.along_u {
            (at, derivative(along), derivative(across))
        } else {
            (at, derivative(across), derivative(along))
        };
        if let Some(distance) = self.offset_distance {
            let second = |lanes: [f64; 4], a: Vector3, wa: f64, b: Vector3, wb: f64| {
                Vector3::new(
                    (lanes[0] - at.x * lanes[3] - a.x * wa - b.x * wb) / weight,
                    (lanes[1] - at.y * lanes[3] - a.y * wa - b.y * wb) / weight,
                    (lanes[2] - at.z * lanes[3] - a.z * wa - b.z * wb) / weight,
                )
            };
            let (Some(point), Some(du_finite), Some(dv_finite)) = (
                FinitePoint3::new(at),
                FiniteVector3::new(du),
                FiniteVector3::new(dv),
            ) else {
                return Ok(None);
            };
            let Some(duu) = FiniteVector3::new(second(uu, du, across[3], du, across[3])) else {
                return Ok(None);
            };
            let Some(duv) = FiniteVector3::new(second(uv, du, along[3], dv, across[3])) else {
                return Ok(None);
            };
            let Some(dvv) = FiniteVector3::new(second(vv, dv, along[3], dv, along[3])) else {
                return Ok(None);
            };
            let support = cadmpeg_ir::eval::SurfaceSecondPartials {
                point,
                du: du_finite,
                dv: dv_finite,
                duu,
                duv,
                dvv,
            };
            let offset = match cadmpeg_ir::eval::offset_surface_partials(support, distance) {
                Ok(offset) => offset,
                Err(cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit)) => {
                    return Err(limit.into())
                }
                Err(cadmpeg_ir::eval::EvaluationFailure::WorkExhausted) => {
                    return Err(eval.work.exhaustion())
                }
                Err(
                    cadmpeg_ir::eval::EvaluationFailure::NoValue
                    | cadmpeg_ir::eval::EvaluationFailure::NonFinite(_),
                ) => return Ok(None),
            };
            at = offset.point.get();
            du = offset.du.get();
            dv = offset.dv.get();
        }
        let finite =
            |vector: Vector3| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite();
        if !(at.x.is_finite() && at.y.is_finite() && at.z.is_finite() && finite(du) && finite(dv)) {
            return Ok(None);
        }
        Ok(Some(Partials {
            point: self.placement.point(at),
            du: self.placement.vector(du),
            dv: self.placement.vector(dv),
        }))
    }
}

/// The knot span of `t` in a clamped knot vector, or `None` outside the
/// domain.
fn span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    let low = *knots.get(degree)?;
    let high = *knots.get(count)?;
    if !(low..=high).contains(&t) || count <= degree {
        return None;
    }
    if t >= high {
        return (degree..count).rev().find(|&i| knots[i] < knots[i + 1]);
    }
    let found = knots[degree..=count].partition_point(|knot| *knot <= t) + degree;
    found.checked_sub(1)
}

/// Fast binary64 point and first partials for a plain NURBS surface of degree
/// at most [`MAX_COLUMN_DEGREE`], returning `None` to fall back to the exact
/// IR evaluator on range loss, non-normal weights, or higher degrees.
pub(crate) fn nurbs_surface_partials_fast(
    eval: &Eval<'_, '_>,
    surface: &NurbsSurface,
    u: f64,
    v: f64,
    [[u0, u1], [v0, v1]]: [[f64; 2]; 2],
    placement: Placement,
) -> Measure<Option<Partials>> {
    let u = if surface.u_periodic() && !(u0..=u1).contains(&u) {
        u0 + (u - u0).rem_euclid(u1 - u0)
    } else {
        u
    };
    let v = if surface.v_periodic() && !(v0..=v1).contains(&v) {
        v0 + (v - v0).rem_euclid(v1 - v0)
    } else {
        v
    };
    if !(u0..=u1).contains(&u) || !(v0..=v1).contains(&v) {
        return Ok(None);
    }
    let (Ok(u_degree), Ok(v_degree)) = (
        usize::try_from(surface.u_degree()),
        usize::try_from(surface.v_degree()),
    ) else {
        return Ok(None);
    };
    if u_degree > MAX_COLUMN_DEGREE || v_degree > MAX_COLUMN_DEGREE {
        return Ok(None);
    }
    let u_knots = surface.u_knots().as_slice();
    let v_knots = surface.v_knots().as_slice();
    let (u_count, v_count) = (surface.u_count(), surface.v_count());
    let (Some(u_span), Some(v_span)) = (
        span(u_knots, u_degree, u_count, u),
        span(v_knots, v_degree, v_count, v),
    ) else {
        return Ok(None);
    };
    let mut u_vals: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
    let mut u_slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
    let mut v_vals: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
    let mut v_slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
    if !basis(u_knots, u_degree, u_span, u, &mut u_vals, &mut u_slopes)
        || !basis(v_knots, v_degree, v_span, v, &mut v_vals, &mut v_slopes)
    {
        return Ok(None);
    }
    let work = (u_degree + 1) * (v_degree + 1)
        + (u_degree + 1) * (u_degree + 2)
        + (v_degree + 1) * (v_degree + 2);
    eval.charge(work)?;
    let mut point = [0.0; 4];
    let mut du_lanes = [0.0; 4];
    let mut dv_lanes = [0.0; 4];
    let rational = surface.pole_weights().is_some();
    for a in 0..=u_degree {
        let row = u_span + a - u_degree;
        let mut row_val = [0.0; 4];
        let mut row_dv = [0.0; 4];
        for b in 0..=v_degree {
            let col = v_span + b - v_degree;
            let Some(pole) = surface.pole(row, col) else {
                return Ok(None);
            };
            let pole = pole.get();
            let vb = v_vals[b];
            let vs = v_slopes[b];
            if rational {
                let weight = surface.weight(row, col).map_or(1.0, NonZeroReal::get);
                if !weight.is_normal() {
                    return Ok(None);
                }
                let h = [pole.x * weight, pole.y * weight, pole.z * weight, weight];
                for lane in 0..4 {
                    row_val[lane] += vb * h[lane];
                    row_dv[lane] += vs * h[lane];
                }
            } else {
                row_val[0] += vb * pole.x;
                row_val[1] += vb * pole.y;
                row_val[2] += vb * pole.z;
                row_dv[0] += vs * pole.x;
                row_dv[1] += vs * pole.y;
                row_dv[2] += vs * pole.z;
            }
        }
        let ua = u_vals[a];
        let us = u_slopes[a];
        let lane_end = if rational { 4 } else { 3 };
        for lane in 0..lane_end {
            point[lane] += ua * row_val[lane];
            du_lanes[lane] += us * row_val[lane];
            dv_lanes[lane] += ua * row_dv[lane];
        }
    }
    let (at, du, dv) = if rational {
        let weight = point[3];
        if !weight.is_normal() {
            return Ok(None);
        }
        let inv_w = 1.0 / weight;
        if !inv_w.is_normal() {
            return Ok(None);
        }
        let at = Point3::new(point[0] * inv_w, point[1] * inv_w, point[2] * inv_w);
        let du = Vector3::new(
            (du_lanes[0] - at.x * du_lanes[3]) * inv_w,
            (du_lanes[1] - at.y * du_lanes[3]) * inv_w,
            (du_lanes[2] - at.z * du_lanes[3]) * inv_w,
        );
        let dv = Vector3::new(
            (dv_lanes[0] - at.x * dv_lanes[3]) * inv_w,
            (dv_lanes[1] - at.y * dv_lanes[3]) * inv_w,
            (dv_lanes[2] - at.z * dv_lanes[3]) * inv_w,
        );
        (at, du, dv)
    } else {
        (
            Point3::new(point[0], point[1], point[2]),
            Vector3::new(du_lanes[0], du_lanes[1], du_lanes[2]),
            Vector3::new(dv_lanes[0], dv_lanes[1], dv_lanes[2]),
        )
    };
    let finite = |v: Vector3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !(at.x.is_finite() && at.y.is_finite() && at.z.is_finite() && finite(du) && finite(dv)) {
        return Ok(None);
    }
    Ok(Some(Partials {
        point: placement.point(at),
        du: placement.vector(du),
        dv: placement.vector(dv),
    }))
}

/// Fast binary64 point and first derivative for a NURBS curve of degree at
/// most [`MAX_COLUMN_DEGREE`], returning `None` on range loss or higher degree.
pub(crate) fn nurbs_curve_point_and_tangent_fast(
    eval: &Eval<'_, '_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    t: f64,
    placement: Placement,
) -> Measure<Option<(Point3, Vector3)>> {
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    if degree > MAX_COLUMN_DEGREE {
        return Ok(None);
    }
    let knots = curve.knots();
    let count = curve.pole_count();
    let Some(low) = knots.get(degree).copied() else {
        return Ok(None);
    };
    let Some(high) = knots.get(count).copied() else {
        return Ok(None);
    };
    let t = if curve.periodic() && !(low..=high).contains(&t) {
        low + (t - low).rem_euclid(high - low)
    } else {
        t
    };
    let Some(span_idx) = span(knots, degree, count, t) else {
        return Ok(None);
    };
    let mut vals: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
    let mut slopes: Basis = [0.0; MAX_COLUMN_DEGREE + 1];
    if !basis(knots, degree, span_idx, t, &mut vals, &mut slopes) {
        return Ok(None);
    }
    eval.charge((degree + 1) * (degree + 2))?;
    let poles = curve.pole_rows();
    let (at, tangent) = match poles {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            let mut p = [0.0; 3];
            let mut d = [0.0; 3];
            for r in 0..=degree {
                let Some(pole) = points.get(span_idx + r - degree) else {
                    return Ok(None);
                };
                let pole = pole.get();
                let vr = vals[r];
                let sr = slopes[r];
                p[0] += vr * pole.x;
                p[1] += vr * pole.y;
                p[2] += vr * pole.z;
                d[0] += sr * pole.x;
                d[1] += sr * pole.y;
                d[2] += sr * pole.z;
            }
            (
                Point3::new(p[0], p[1], p[2]),
                Vector3::new(d[0], d[1], d[2]),
            )
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let mut point = [0.0; 4];
            let mut deriv = [0.0; 4];
            for r in 0..=degree {
                let Some(pole) = points.get(span_idx + r - degree) else {
                    return Ok(None);
                };
                let weight = pole.weight.get();
                if !weight.is_normal() {
                    return Ok(None);
                }
                let pt = pole.point.get();
                let homog = [pt.x * weight, pt.y * weight, pt.z * weight, weight];
                let vr = vals[r];
                let sr = slopes[r];
                for lane in 0..4 {
                    point[lane] += vr * homog[lane];
                    deriv[lane] += sr * homog[lane];
                }
            }
            let total_weight = point[3];
            if !total_weight.is_normal() {
                return Ok(None);
            }
            let inv_w = 1.0 / total_weight;
            if !inv_w.is_normal() {
                return Ok(None);
            }
            let at = Point3::new(point[0] * inv_w, point[1] * inv_w, point[2] * inv_w);
            let tangent = Vector3::new(
                (deriv[0] - at.x * deriv[3]) * inv_w,
                (deriv[1] - at.y * deriv[3]) * inv_w,
                (deriv[2] - at.z * deriv[3]) * inv_w,
            );
            (at, tangent)
        }
    };
    if !(at.x.is_finite()
        && at.y.is_finite()
        && at.z.is_finite()
        && tangent.x.is_finite()
        && tangent.y.is_finite()
        && tangent.z.is_finite())
    {
        return Ok(None);
    }
    Ok(Some((placement.point(at), placement.vector(tangent))))
}

/// The nonzero basis values of `degree` at `t` in `span` and their first
/// derivatives; `false` where one is not finite.
fn basis(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    values: &mut Basis,
    slopes: &mut Basis,
) -> bool {
    if degree <= 3 {
        let mut v4 = [0.0; 4];
        let mut s4 = [0.0; 4];
        if !basis_sized::<4>(knots, degree, span, t, &mut v4, &mut s4) {
            return false;
        }
        values[..=degree].copy_from_slice(&v4[..=degree]);
        slopes[..=degree].copy_from_slice(&s4[..=degree]);
        true
    } else if degree <= 7 {
        let mut v8 = [0.0; 8];
        let mut s8 = [0.0; 8];
        if !basis_sized::<8>(knots, degree, span, t, &mut v8, &mut s8) {
            return false;
        }
        values[..=degree].copy_from_slice(&v8[..=degree]);
        slopes[..=degree].copy_from_slice(&s8[..=degree]);
        true
    } else {
        basis_sized::<{ MAX_COLUMN_DEGREE + 1 }>(knots, degree, span, t, values, slopes)
    }
}

#[inline]
fn basis_sized<const N: usize>(
    knots: &[f64],
    degree: usize,
    span: usize,
    t: f64,
    values: &mut [f64; N],
    slopes: &mut [f64; N],
) -> bool {
    let p = degree;
    if p == 1 {
        let left1 = t - knots[span];
        let right1 = knots[span + 1] - t;
        let denom = right1 + left1;
        let ratio = if denom == 0.0 { 0.0 } else { 1.0 / denom };
        values[0] = right1 * ratio;
        values[1] = left1 * ratio;
        slopes[0] = -ratio;
        slopes[1] = ratio;
        return values[0].is_finite()
            && values[1].is_finite()
            && slopes[0].is_finite()
            && slopes[1].is_finite();
    }
    let mut left = [0.0; N];
    let mut right = [0.0; N];
    // `table[j][r]` for `r <= j` holds knot differences; above it, basis
    // values of each degree.
    let mut table = [[0.0; N]; N];
    table[0][0] = 1.0;
    for j in 1..=p {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            table[j][r] = right[r + 1] + left[j - r];
            let ratio = if table[j][r] == 0.0 {
                0.0
            } else {
                table[r][j - 1] / table[j][r]
            };
            table[r][j] = saved + right[r + 1] * ratio;
            saved = left[j - r] * ratio;
        }
        table[j][j] = saved;
    }
    let quotient = |numerator: f64, denominator: f64| {
        if denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        }
    };
    let Some(scale) = f64_from_index(p) else {
        return false;
    };
    for r in 0..=p {
        values[r] = table[r][p];
        slopes[r] = if p == 0 {
            0.0
        } else {
            let rising = if r >= 1 {
                quotient(table[r - 1][p - 1], table[p][r - 1])
            } else {
                0.0
            };
            let falling = if r < p {
                quotient(table[r][p - 1], table[p][r])
            } else {
                0.0
            };
            scale * (rising - falling)
        };
        if !(values[r].is_finite() && slopes[r].is_finite()) {
            return false;
        }
    }
    true
}

fn second_basis(knots: &[f64], degree: usize, span: usize, t: f64, second: &mut Basis) -> bool {
    if degree < 2 {
        second[..=degree].fill(0.0);
        return true;
    }
    let mut lower = [0.0; MAX_COLUMN_DEGREE + 1];
    let mut slopes = [0.0; MAX_COLUMN_DEGREE + 1];
    if !basis(knots, degree - 1, span, t, &mut lower, &mut slopes) {
        return false;
    }
    let Some(scale) = f64_from_index(degree) else {
        return false;
    };
    let quotient = |value, a: f64, b| if a == b { 0.0 } else { value / (a - b) };
    for (r, entry) in second.iter_mut().enumerate().take(degree + 1) {
        let i = span + r - degree;
        let rising = if r > 0 {
            quotient(slopes[r - 1], knots[i + degree], knots[i])
        } else {
            0.0
        };
        let falling = if r < degree {
            quotient(slopes[r], knots[i + degree + 1], knots[i + 1])
        } else {
            0.0
        };
        *entry = scale * (rising - falling);
        if !entry.is_finite() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::topology::{BodyKind, Sense};

    use super::Column;
    use crate::budget::{PassAllowance, WorkProfile};
    use crate::canonical::placement::Placement;
    use crate::canonical::surface::{Chart, NurbsChart};
    use crate::context::Eval;
    use crate::test_support::builders::{service_context, Builder};

    /// A rational patch of degree two by three with an interior knot in each
    /// direction, sampled at its domain ends, its knots and between them.
    #[test]
    fn a_column_matches_the_chart_along_its_isoline() {
        let mut builder = Builder::new();
        let rows: Vec<Vec<([f64; 3], f64)>> = (0..4)
            .map(|i| {
                (0..5)
                    .map(|j| {
                        let (x, y) = (f64::from(i), f64::from(j));
                        let z = (x * 0.7 + y * 1.3).sin();
                        ([x, y, z], 1.0 + 0.25 * f64::from((i + j) % 3))
                    })
                    .collect()
            })
            .collect();
        let patch = builder.nurbs(
            (2, vec![0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0]),
            (3, vec![0.0, 0.0, 0.0, 0.0, 0.6, 1.0, 1.0, 1.0, 1.0]),
            &rows,
        );
        builder.face(&patch, Sense::Forward, &[], false);
        let ir = builder.finish(BodyKind::Sheet);
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
            &ir.geometry.surfaces[0].geometry
        else {
            panic!("a NURBS surface");
        };
        let base = NurbsChart::new(surface, Placement::identity()).unwrap();
        let ctx = service_context();
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
        let pass = PassAllowance::new(WorkProfile::Desktop.pass_allowance(1).unwrap());
        let slice = pass.face_slice();
        let eval = Eval {
            ctx: &ctx,
            index: &index,
            work: &slice,
        };
        for distance in [None, Some(-0.125), Some(0.125)] {
            let mut chart = base;
            chart.column_offset = distance;
            let chart = Chart::Nurbs(chart);
            for u in [0.0, 0.17, 0.4, 0.83, 1.0] {
                let column = Column::new(&eval, &chart, u).unwrap();
                assert!(column.isoline.is_some());
                for s in [0.0, 0.31, 0.6, 0.94, 1.0] {
                    let fast = column.partials(&eval, s).unwrap();
                    let exact = match distance {
                        None => chart.partials(&eval, u, s).unwrap(),
                        Some(distance) => {
                            let support = cadmpeg_ir::eval::nurbs_surface_second_partials(
                                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                                surface,
                                u,
                                s,
                            )
                            .unwrap();
                            let offset =
                                cadmpeg_ir::eval::offset_surface_partials(support, distance)
                                    .unwrap();
                            super::Partials {
                                point: offset.point.get(),
                                du: offset.du.get(),
                                dv: offset.dv.get(),
                            }
                        }
                    };
                    assert!(fast.point.distance(exact.point) < 1.0e-13, "{u} {s}");
                    assert!((fast.du - exact.du).norm() < 1.0e-12, "{u} {s}");
                    assert!((fast.dv - exact.dv).norm() < 1.0e-12, "{u} {s}");
                }
            }
        }
    }
}
