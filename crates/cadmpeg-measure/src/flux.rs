// SPDX-License-Identifier: Apache-2.0
//! Fluxes of a face region for volume, centroid, closure and area centroid.
//!
//! With `N = S_u × S_v` and `d = P − P_ref`, a face contributes the surface
//! integrals of `d·N / 3` (volume), `d_i² N_i / 2` (first moments), `N_i`
//! (closure), `d_i³ N_i / 3` and `d_i² d_j N_i / 2` (second moments `∫ d_i²`
//! and `∫ d_i d_j` of the volume) and `d_i |N|` (area moments). Each is `∮ F du` by Green's
//! theorem with the inner integral `F(u, v) = −∫_{v₀}^{v} f(u, s) ds` from
//! the region's base `v₀`, or `∮ G dv` with `G(u, v) = ∫_{u₀}^{u} f(s, v) ds`
//! from the domain's lower `u₀` on a chart whose inner integrals run along
//! `u`, plus the whole domain when the region reaches
//! the domain's natural edges. A body sums its faces' fluxes with the sign
//! of each face's oriented normal reading.

use std::f64::consts::TAU;

use cadmpeg_ir::math::Point3;

use crate::area::LoopCut;
use crate::boundary::piece::Piece;
use crate::boundary::ResolvedLoop;
use crate::canonical::column::Column;
use crate::canonical::surface::{Chart, Partials, PlaneChart};
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::quadrature::{integrate, Target};
use crate::reason::UnavailableReason;
use crate::region::Region;

/// Flux components: volume, three first moments, three closure components,
/// six second moments and three area moments.
pub(crate) const FLUX: usize = 16;

/// Integrated values and their error estimates.
const TRACKED: usize = 2 * FLUX;

/// The volume component.
const VOLUME: usize = 0;

/// The first of the three first-moment components.
const MOMENTS: usize = 1;

/// The first of the three closure components.
const CLOSURE: usize = 4;

/// The first of the six second moments, in the order `xx yy zz xy yz zx`.
const SECOND_MOMENTS: usize = 7;

/// The first area moment component; the components before it change sign
/// with the oriented normal.
const AREA_MOMENTS: usize = 13;

/// One face's flux integrals and their error estimates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Flux {
    pub(crate) value: [f64; FLUX],
    pub(crate) error: [f64; FLUX],
}

impl Flux {
    pub(crate) const ZERO: Self = Self {
        value: [0.0; FLUX],
        error: [0.0; FLUX],
    };

    /// `self + factor·other`; errors add.
    pub(crate) fn add(self, other: Self, factor: f64) -> Self {
        Self {
            value: std::array::from_fn(|k| self.value[k] + factor * other.value[k]),
            error: std::array::from_fn(|k| self.error[k] + other.error[k]),
        }
    }

    /// `self + other` with `other` oriented by `sigma`: the area moments
    /// do not depend on the normal's orientation.
    pub(crate) fn add_oriented(self, other: Self, sigma: f64) -> Self {
        Self {
            value: std::array::from_fn(|k| {
                let factor = if k < AREA_MOMENTS { sigma } else { 1.0 };
                self.value[k] + factor * other.value[k]
            }),
            error: std::array::from_fn(|k| self.error[k] + other.error[k]),
        }
    }

    pub(crate) const fn volume(&self) -> f64 {
        self.value[VOLUME]
    }

    pub(crate) fn moment(&self) -> [f64; 3] {
        std::array::from_fn(|axis| self.value[MOMENTS + axis])
    }

    pub(crate) fn closure(&self) -> [f64; 3] {
        std::array::from_fn(|axis| self.value[CLOSURE + axis])
    }

    pub(crate) fn area_moment(&self) -> [f64; 3] {
        std::array::from_fn(|axis| self.value[AREA_MOMENTS + axis])
    }

    /// The error estimate of the volume.
    pub(crate) const fn volume_error(&self) -> f64 {
        self.error[VOLUME]
    }

    /// The largest error estimate of the three first moments.
    pub(crate) fn moment_error(&self) -> f64 {
        largest(self.triple_error(MOMENTS))
    }

    /// The summed error estimate of the three closure components.
    pub(crate) fn closure_error(&self) -> f64 {
        self.triple_error(CLOSURE).iter().sum()
    }

    /// The largest error estimate of the three area moments.
    pub(crate) fn area_moment_error(&self) -> f64 {
        largest(self.triple_error(AREA_MOMENTS))
    }

    /// The error estimates of the three components from `first`.
    fn triple_error(&self, first: usize) -> [f64; 3] {
        std::array::from_fn(|axis| self.error[first + axis])
    }

    /// The second moments `∫ d_i d_j dV` about `P_ref`, in the order
    /// `xx yy zz xy yz zx`, and their error estimates.
    pub(crate) fn second_moments(&self) -> ([f64; 6], [f64; 6]) {
        (
            std::array::from_fn(|k| self.value[SECOND_MOMENTS + k]),
            std::array::from_fn(|k| self.error[SECOND_MOMENTS + k]),
        )
    }

    /// The error estimate of each first moment.
    pub(crate) fn moment_errors(&self) -> [f64; 3] {
        self.triple_error(MOMENTS)
    }
}

/// The largest of three non-negative estimates.
fn largest(values: [f64; 3]) -> f64 {
    values.into_iter().fold(0.0, f64::max)
}

/// The flux density at one surface point.
fn density(partials: &Partials, origin: Point3) -> [f64; FLUX] {
    let normal = partials.du.cross(partials.dv);
    let d = partials.point.vector_from(origin);
    let magnitude = normal.norm();
    [
        d.dot(normal) / 3.0,
        d.x * d.x * normal.x / 2.0,
        d.y * d.y * normal.y / 2.0,
        d.z * d.z * normal.z / 2.0,
        normal.x,
        normal.y,
        normal.z,
        d.x * d.x * d.x * normal.x / 3.0,
        d.y * d.y * d.y * normal.y / 3.0,
        d.z * d.z * d.z * normal.z / 3.0,
        d.x * d.x * d.y * normal.x / 2.0,
        d.y * d.y * d.z * normal.y / 2.0,
        d.z * d.z * d.x * normal.z / 2.0,
        d.x * magnitude,
        d.y * magnitude,
        d.z * magnitude,
    ]
}

/// The magnitude scale of each component: an area `scale²` times the
/// component's power of the distance from the reference point, at most
/// `reach`, and at least `scale`.
fn scales(scale: f64, reach: f64) -> [f64; TRACKED] {
    let distance_powers = [1, 2, 2, 2, 0, 0, 0, 3, 3, 3, 3, 3, 3, 1, 1, 1];
    let distance = reach.max(scale);
    std::array::from_fn(|k| scale * scale * distance.powi(distance_powers[k % FLUX]))
}

/// The components a measurement pass targets. The second moments ride
/// along without a goal of their own.
pub(crate) const MEASURE_COMPONENTS: [bool; FLUX] = {
    let mut active = [true; FLUX];
    let mut k = SECOND_MOMENTS;
    while k < AREA_MOMENTS {
        active[k] = false;
        k += 1;
    }
    active
};

/// The volume and closure components: what orientation findings read.
pub(crate) const ORIENTATION_COMPONENTS: [bool; FLUX] = {
    let mut active = [false; FLUX];
    active[VOLUME] = true;
    let mut k = CLOSURE;
    while k < CLOSURE + 3 {
        active[k] = true;
        k += 1;
    }
    active
};

/// The volume and closure fluxes of a region of area `area` on a plane,
/// with the area's error estimate scaled into each. On a plane `d·N` is
/// `h |N|` with `h = (o − P_ref)·n̂` constant, so `Φ_V = h A / 3` and
/// `Φ_N = n̂ A`. The other components are zero: only an orientation pass,
/// which reads the volume and closure alone, takes this flux.
pub(crate) fn plane_orientation_flux(
    plane: &PlaneChart,
    origin: Point3,
    area: f64,
    area_error: f64,
) -> Flux {
    let normal = plane.eu.cross(plane.ev);
    let height = plane.origin.vector_from(origin).dot(normal);
    let mut flux = Flux::ZERO;
    flux.value[VOLUME] = height * area / 3.0;
    flux.error[VOLUME] = height.abs() * area_error / 3.0;
    for (axis, component) in [normal.x, normal.y, normal.z].into_iter().enumerate() {
        flux.value[CLOSURE + axis] = component * area;
        flux.error[CLOSURE + axis] = component.abs() * area_error;
    }
    flux
}

/// What one flux integral reads.
pub(crate) struct FluxFrame<'a, 'ir> {
    pub(crate) chart: &'a Chart<'ir>,
    pub(crate) origin: Point3,
    /// The length the quadrature targets are relative to.
    pub(crate) scale: f64,
    /// A bound on the distance of the face's points from `origin`.
    pub(crate) reach: f64,
    /// The components whose accuracy the quadrature targets; the others
    /// ride along on the same panels.
    pub(crate) active: [bool; FLUX],
}

/// A piece's breaks, with the parameters where it crosses a `v` knot when
/// the inner integrals run along `u`: there the inner integral has a kink.
pub(crate) fn outer_breaks(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    piece: &Piece<'_>,
    along_u: Option<f64>,
) -> Measure<Vec<f64>> {
    let mut breaks = piece.breaks();
    if along_u.is_some() {
        breaks.extend(piece.v_knot_crossings(eval, chart)?);
    }
    Ok(breaks)
}

fn not_converged() -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::QuadratureNotConverged,
        None,
        "the flux quadrature did not converge",
    )
}

/// Orientation-only flux components: volume and the three closure components.
const ORIENTATION_FLUX: usize = 4;

/// Integrated orientation values and their error estimates.
const ORIENTATION_TRACKED: usize = 2 * ORIENTATION_FLUX;

/// `1 / sqrt(3)` for the 2-point Gauss-Legendre rule (exact up to degree 3).
const GL2_NODE: f64 = 0.577_350_269_189_625_7;

/// `sqrt(3 / 5)` for the 3-point Gauss-Legendre rule (exact up to degree 5).
const GL3_NODE: f64 = 0.774_596_669_241_483_4;

/// The orientation-only flux density (`volume`, `closure[0..3]`) at one point.
fn orientation_density(partials: &Partials, origin: Point3) -> [f64; ORIENTATION_FLUX] {
    let normal = partials.du.cross(partials.dv);
    let d = partials.point.vector_from(origin);
    [d.dot(normal) / 3.0, normal.x, normal.y, normal.z]
}

fn orientation_scales(scale: f64, reach: f64) -> [f64; ORIENTATION_TRACKED] {
    let distance = reach.max(scale);
    let area = scale * scale;
    let vol = area * distance;
    [vol, area, area, area, vol, area, area, area]
}

fn orientation_inner_scales(scale: f64, reach: f64, width: f64) -> [f64; ORIENTATION_FLUX] {
    let distance = reach.max(scale);
    let area = scale * scale / width;
    [area * distance, area, area, area]
}

fn orientation_from_tracked(
    value: [f64; ORIENTATION_TRACKED],
    error: [f64; ORIENTATION_TRACKED],
) -> Flux {
    let mut flux = Flux::ZERO;
    flux.value[VOLUME] = value[0];
    flux.error[VOLUME] = error[0] + value[ORIENTATION_FLUX].abs();
    for axis in 0..3 {
        flux.value[CLOSURE + axis] = value[1 + axis];
        flux.error[CLOSURE + axis] = error[1 + axis] + value[ORIENTATION_FLUX + 1 + axis].abs();
    }
    flux
}

/// Exact polynomial inner integral along `v` for `Plane`, `Cylinder`, and
/// single-nappe `Cone` charts using Gauss-Legendre quadrature (exact to
/// rounding for the polynomial degree in `v`).
fn analytic_inner_polynomial<const K: usize>(
    chart: &Chart<'_>,
    u: f64,
    [from, to]: [f64; 2],
    orientation_only: bool,
    mut eval_density: impl FnMut(&Partials) -> [f64; K],
) -> Option<([f64; K], [f64; K])> {
    let points: &[(f64, f64)] = match chart {
        Chart::Plane(_) | Chart::Cylinder(_) => {
            if orientation_only {
                &[(0.0, 2.0)]
            } else {
                &[(-GL2_NODE, 1.0), (GL2_NODE, 1.0)]
            }
        }
        Chart::Cone(cone) => {
            let r0 = cone.rho0 + cone.tau * from;
            let r1 = cone.rho0 + cone.tau * to;
            if !r0.is_finite() || !r1.is_finite() || r0 * r1 < 0.0 {
                return None;
            }
            if orientation_only {
                &[(-GL2_NODE, 1.0), (GL2_NODE, 1.0)]
            } else {
                &[
                    (-GL3_NODE, 5.0 / 9.0),
                    (0.0, 8.0 / 9.0),
                    (GL3_NODE, 5.0 / 9.0),
                ]
            }
        }
        _ => return None,
    };
    let half = 0.5 * (to - from);
    let mid = 0.5 * (from + to);
    if !half.is_finite() || !mid.is_finite() {
        return None;
    }
    if half == 0.0 {
        return Some(([0.0; K], [0.0; K]));
    }
    let mut sum = [0.0; K];
    let mut mag = [0.0; K];
    for &(node, weight) in points {
        let s = mid + half * node;
        let partials = chart.closed_partials(u, s)?;
        let values = eval_density(&partials);
        let factor = half * weight;
        for k in 0..K {
            let term = factor * values[k];
            if !term.is_finite() {
                return None;
            }
            sum[k] += term;
            mag[k] += term.abs();
        }
    }
    let mut err = [0.0; K];
    for k in 0..K {
        if !sum[k].is_finite() || !mag[k].is_finite() {
            return None;
        }
        err[k] = 16.0 * f64::EPSILON * mag[k];
    }
    Some((sum, err))
}

impl FluxFrame<'_, '_> {
    fn is_orientation_only(&self) -> bool {
        self.active == ORIENTATION_COMPONENTS
    }

    /// The active mask over values and inner-integral errors.
    fn tracked(&self) -> [bool; TRACKED] {
        std::array::from_fn(|k| self.active[k % FLUX])
    }

    fn v_breaks(&self) -> Vec<f64> {
        match self.chart {
            Chart::Nurbs(nurbs) => nurbs.v_breaks(),
            _ => Vec::new(),
        }
    }

    /// `∫_from^to f_orient(u, s) ds` for the 4 orientation components.
    fn inner_orientation(
        &self,
        eval: &Eval<'_, '_>,
        u: f64,
        interval: [f64; 2],
    ) -> Measure<([f64; ORIENTATION_FLUX], [f64; ORIENTATION_FLUX])> {
        if let Some(exact) = analytic_inner_polynomial(self.chart, u, interval, true, |partials| {
            orientation_density(partials, self.origin)
        }) {
            eval.charge(1)?;
            return Ok(exact);
        }
        let column = Column::new(eval, self.chart, u)?;
        let mut charge = |units| eval.charge(units);
        let mut density_at = |s: f64| -> Measure<[f64; ORIENTATION_FLUX]> {
            Ok(orientation_density(&column.partials(eval, s)?, self.origin))
        };
        let width = match self.chart {
            Chart::Nurbs(nurbs) => nurbs.width(0),
            _ => 1.0,
        };
        let integral = integrate(
            &mut charge,
            &mut density_at,
            interval[0],
            interval[1],
            &self.v_breaks(),
            Target::inner(orientation_inner_scales(self.scale, self.reach, width)),
        )?;
        if !integral.converged {
            return Err(not_converged());
        }
        Ok((integral.value, integral.error))
    }

    /// `∫_from^to f_orient(s, v) ds` along a `u`-isoline for the 4 orientation components.
    fn inner_orientation_across(
        &self,
        eval: &Eval<'_, '_>,
        v: f64,
        [from, to]: [f64; 2],
    ) -> Measure<([f64; ORIENTATION_FLUX], [f64; ORIENTATION_FLUX])> {
        let row = Column::across(eval, self.chart, v)?;
        let mut charge = |units| eval.charge(units);
        let mut density_at = |s: f64| -> Measure<[f64; ORIENTATION_FLUX]> {
            Ok(orientation_density(&row.partials(eval, s)?, self.origin))
        };
        let (breaks, width) = match self.chart {
            Chart::Nurbs(nurbs) => (nurbs.u_breaks(), nurbs.width(1)),
            _ => (Vec::new(), 1.0),
        };
        let integral = integrate(
            &mut charge,
            &mut density_at,
            from,
            to,
            &breaks,
            Target::inner(orientation_inner_scales(self.scale, self.reach, width)),
        )?;
        if !integral.converged {
            return Err(not_converged());
        }
        Ok((integral.value, integral.error))
    }

    /// `∫_from^to f(u, s) ds` and its error estimate.
    fn inner(
        &self,
        eval: &Eval<'_, '_>,
        u: f64,
        [from, to]: [f64; 2],
    ) -> Measure<([f64; FLUX], [f64; FLUX])> {
        if let Some(exact) =
            analytic_inner_polynomial(self.chart, u, [from, to], false, |partials| {
                density(partials, self.origin)
            })
        {
            eval.charge(1)?;
            return Ok(exact);
        }
        let column = Column::new(eval, self.chart, u)?;
        let mut charge = |units| eval.charge(units);
        let mut density_at = |s: f64| -> Measure<[f64; FLUX]> {
            Ok(density(&column.partials(eval, s)?, self.origin))
        };
        let scale = scales(self.scale, self.reach);
        let width = match self.chart {
            Chart::Nurbs(nurbs) => nurbs.width(0),
            _ => 1.0,
        };
        let integral = integrate(
            &mut charge,
            &mut density_at,
            from,
            to,
            &self.v_breaks(),
            Target::inner(std::array::from_fn(|k| scale[k] / width)).with_active(self.active),
        )?;
        if !integral.converged {
            return Err(not_converged());
        }
        Ok((integral.value, integral.error))
    }

    /// `∫_from^to f(s, v) ds` along a `u`-isoline and its error estimate.
    fn inner_across(
        &self,
        eval: &Eval<'_, '_>,
        v: f64,
        [from, to]: [f64; 2],
    ) -> Measure<([f64; FLUX], [f64; FLUX])> {
        let row = Column::across(eval, self.chart, v)?;
        let mut charge = |units| eval.charge(units);
        let mut density_at =
            |s: f64| -> Measure<[f64; FLUX]> { Ok(density(&row.partials(eval, s)?, self.origin)) };
        let scale = scales(self.scale, self.reach);
        let (breaks, width) = match self.chart {
            Chart::Nurbs(nurbs) => (nurbs.u_breaks(), nurbs.width(1)),
            _ => (Vec::new(), 1.0),
        };
        let integral = integrate(
            &mut charge,
            &mut density_at,
            from,
            to,
            &breaks,
            Target::inner(std::array::from_fn(|k| scale[k] / width)).with_active(self.active),
        )?;
        if !integral.converged {
            return Err(not_converged());
        }
        Ok((integral.value, integral.error))
    }

    /// The lower `u` of the domain when the inner integrals run along `u`.
    fn along_u(&self) -> Option<f64> {
        match self.chart {
            Chart::Nurbs(nurbs) if self.chart.inner_along_u() => Some(nurbs.domain[0][0]),
            _ => None,
        }
    }

    /// `∮ F du` over one loop with `F` based at `base`, with each piece's
    /// `v` taken modulo the cut's period when the domain is cut.
    pub(crate) fn loop_flux(
        &self,
        eval: &Eval<'_, '_>,
        resolved: &ResolvedLoop<'_>,
        base: f64,
        cut: Option<LoopCut<'_>>,
    ) -> Measure<Flux> {
        if self.is_orientation_only() {
            return self.loop_orientation_flux(eval, resolved, base, cut);
        }
        let mut flux = Flux::ZERO;
        let along_u = self.along_u();
        for (position, piece) in resolved.pieces.iter().enumerate() {
            let mut charge = |units| eval.charge(units);
            let mut integrand = |t: f64| -> Measure<[f64; TRACKED]> {
                let sample = piece.sample(eval, self.chart, t)?;
                if let Some(u0) = along_u {
                    if sample.duv[1] == 0.0 {
                        return Ok([0.0; TRACKED]);
                    }
                    let (value, error) =
                        self.inner_across(eval, sample.uv[1], [u0, sample.uv[0]])?;
                    let speed = sample.duv[1];
                    return Ok(std::array::from_fn(|k| {
                        if k < FLUX {
                            value[k] * speed
                        } else {
                            error[k - FLUX] * speed.abs()
                        }
                    }));
                }
                if sample.duv[0] == 0.0 {
                    return Ok([0.0; TRACKED]);
                }
                let v = cut.map_or(sample.uv[1], |cut| cut.wrap(base, sample.uv[1]));
                let (value, error) = self.inner(eval, sample.uv[0], [base, v])?;
                let speed = sample.duv[0];
                Ok(std::array::from_fn(|k| {
                    if k < FLUX {
                        -value[k] * speed
                    } else {
                        error[k - FLUX] * speed.abs()
                    }
                }))
            };
            let integral = integrate(
                &mut charge,
                &mut integrand,
                piece.t0,
                piece.t1,
                &LoopCut::breaks(
                    cut.as_ref(),
                    position,
                    outer_breaks(eval, self.chart, piece, along_u)?,
                ),
                Target::outer(scales(self.scale, self.reach)).with_active(self.tracked()),
            )?;
            if !integral.converged {
                return Err(not_converged());
            }
            flux = flux.add(
                Flux {
                    value: std::array::from_fn(|k| integral.value[k]),
                    error: std::array::from_fn(|k| {
                        integral.error[k] + integral.value[k + FLUX].abs()
                    }),
                },
                1.0,
            );
        }
        Ok(flux)
    }

    /// The flux over the whole domain of a NURBS chart, or of a sphere or
    /// torus from the region's base.
    pub(crate) fn domain_flux(&self, eval: &Eval<'_, '_>, region: &Region) -> Measure<Flux> {
        let (u, v, u_breaks) = match self.chart {
            Chart::Nurbs(nurbs) => {
                let [u, v] = nurbs.domain;
                (u, v, nurbs.u_breaks())
            }
            chart => match chart.closed_span() {
                Some(span) => ([0.0, TAU], [region.base, region.base + span], Vec::new()),
                None => {
                    return Err(MeasureFailure::unavailable(
                        UnavailableReason::RegionDomainUnsupported,
                        None,
                        "the carrier has no finite domain",
                    ))
                }
            },
        };
        self.strip_flux(eval, u, v, &u_breaks)
    }

    /// Orientation-only `∮ F du` using 4 flux components (`Panel<8>`).
    fn loop_orientation_flux(
        &self,
        eval: &Eval<'_, '_>,
        resolved: &ResolvedLoop<'_>,
        base: f64,
        cut: Option<LoopCut<'_>>,
    ) -> Measure<Flux> {
        let mut flux = Flux::ZERO;
        let along_u = self.along_u();
        let target = Target::outer(orientation_scales(self.scale, self.reach));
        for (position, piece) in resolved.pieces.iter().enumerate() {
            let mut charge = |units| eval.charge(units);
            let mut integrand = |t: f64| -> Measure<[f64; ORIENTATION_TRACKED]> {
                let sample = piece.sample(eval, self.chart, t)?;
                if let Some(u0) = along_u {
                    if sample.duv[1] == 0.0 {
                        return Ok([0.0; ORIENTATION_TRACKED]);
                    }
                    let (value, error) =
                        self.inner_orientation_across(eval, sample.uv[1], [u0, sample.uv[0]])?;
                    let speed = sample.duv[1];
                    return Ok(std::array::from_fn(|k| {
                        if k < ORIENTATION_FLUX {
                            value[k] * speed
                        } else {
                            error[k - ORIENTATION_FLUX] * speed.abs()
                        }
                    }));
                }
                if sample.duv[0] == 0.0 {
                    return Ok([0.0; ORIENTATION_TRACKED]);
                }
                let v = cut.map_or(sample.uv[1], |cut| cut.wrap(base, sample.uv[1]));
                let (value, error) = self.inner_orientation(eval, sample.uv[0], [base, v])?;
                let speed = sample.duv[0];
                Ok(std::array::from_fn(|k| {
                    if k < ORIENTATION_FLUX {
                        -value[k] * speed
                    } else {
                        error[k - ORIENTATION_FLUX] * speed.abs()
                    }
                }))
            };
            let integral = integrate(
                &mut charge,
                &mut integrand,
                piece.t0,
                piece.t1,
                &LoopCut::breaks(
                    cut.as_ref(),
                    position,
                    outer_breaks(eval, self.chart, piece, along_u)?,
                ),
                target,
            )?;
            if !integral.converged {
                return Err(not_converged());
            }
            flux = flux.add(
                orientation_from_tracked(integral.value, integral.error),
                1.0,
            );
        }
        Ok(flux)
    }

    /// `∫_u0^u1 ∫_v0^v1 f du dv` over a parameter rectangle.
    fn strip_flux(
        &self,
        eval: &Eval<'_, '_>,
        [u0, u1]: [f64; 2],
        v: [f64; 2],
        u_breaks: &[f64],
    ) -> Measure<Flux> {
        if self.is_orientation_only() {
            let mut charge = |units| eval.charge(units);
            let mut integrand = |u: f64| -> Measure<[f64; ORIENTATION_TRACKED]> {
                let (value, error) = self.inner_orientation(eval, u, v)?;
                Ok(std::array::from_fn(|k| {
                    if k < ORIENTATION_FLUX {
                        value[k]
                    } else {
                        error[k - ORIENTATION_FLUX]
                    }
                }))
            };
            let integral = integrate(
                &mut charge,
                &mut integrand,
                u0,
                u1,
                u_breaks,
                Target::outer(orientation_scales(self.scale, self.reach)),
            )?;
            if !integral.converged {
                return Err(not_converged());
            }
            return Ok(orientation_from_tracked(integral.value, integral.error));
        }
        let mut charge = |units| eval.charge(units);
        let mut integrand = |u: f64| -> Measure<[f64; TRACKED]> {
            let (value, error) = self.inner(eval, u, v)?;
            Ok(std::array::from_fn(|k| {
                if k < FLUX {
                    value[k]
                } else {
                    error[k - FLUX]
                }
            }))
        };
        let integral = integrate(
            &mut charge,
            &mut integrand,
            u0,
            u1,
            u_breaks,
            Target::outer(scales(self.scale, self.reach)).with_active(self.tracked()),
        )?;
        if !integral.converged {
            return Err(not_converged());
        }
        Ok(Flux {
            value: std::array::from_fn(|k| integral.value[k]),
            error: std::array::from_fn(|k| integral.error[k] + integral.value[k + FLUX].abs()),
        })
    }

    /// The flux of a region: each loop by its factor, every loop based at
    /// the region's base; plus the domain when the region reaches the
    /// domain's natural edges, and on a closed domain the strip under each
    /// interval of the upper edge in the region.
    pub(crate) fn region_flux(
        &self,
        eval: &Eval<'_, '_>,
        region: &Region,
        resolved: &[Option<&ResolvedLoop<'_>>],
    ) -> Measure<Flux> {
        let mut flux = Flux::ZERO;
        let wrap = region
            .closed
            .as_ref()
            .and_then(|closed| closed.wrap.as_ref());
        for (index, member) in region.loops.iter().enumerate() {
            if member.factor == 0.0 {
                continue;
            }
            let Some(Some(resolved)) = resolved.get(index) else {
                continue;
            };
            let cut = wrap.map(|wrap| LoopCut {
                period: wrap.period,
                breaks: wrap.breaks.get(index).map_or(&[], Vec::as_slice),
            });
            flux = flux.add(
                self.loop_flux(eval, resolved, region.base, cut)?,
                member.factor,
            );
        }
        if region.exterior != 0.0 {
            flux = flux.add(self.domain_flux(eval, region)?, region.exterior);
        }
        if let (Some(closed), Some(span)) = (&region.closed, self.chart.closed_span()) {
            for interval in &closed.top {
                let strip =
                    self.strip_flux(eval, *interval, [region.base, region.base + span], &[])?;
                flux = flux.add(strip, 1.0);
            }
        }
        Ok(flux)
    }
}

#[cfg(test)]
mod tests {
    use super::{scales, AREA_MOMENTS, CLOSURE, MOMENTS, SECOND_MOMENTS, VOLUME};

    #[test]
    fn a_far_face_is_held_to_its_moments_at_its_distance() {
        // A face of size 0.2 at distance 10: each scale is area × distance^p;
        // at distance 0, size^(p + 2).
        let (size, distance) = (0.2_f64, 10.0_f64);
        let area = size * size;
        let far = scales(size, distance);
        let near = scales(size, 0.0);
        let expected = [
            (VOLUME, area * distance, size.powi(3)),
            (MOMENTS, area * distance.powi(2), size.powi(4)),
            (CLOSURE, area, area),
            (SECOND_MOMENTS, area * distance.powi(3), size.powi(5)),
            (AREA_MOMENTS, area * distance, size.powi(3)),
        ];
        for (k, at_distance, at_origin) in expected {
            assert!((far[k] - at_distance).abs() <= 1.0e-12 * at_distance, "{k}");
            assert!((near[k] - at_origin).abs() <= 1.0e-12 * at_origin, "{k}");
        }
    }

    #[test]
    fn analytic_inner_polynomial_matches_adaptive_quadrature_on_plane_cylinder_and_cone() {
        use super::{analytic_inner_polynomial, density, orientation_density, FLUX};
        use crate::canonical::surface::{Chart, ConeChart, CylinderChart, PlaneChart};
        use crate::quadrature::{integrate, Target};
        use cadmpeg_ir::math::{Point3, Vector3};

        let origin = Point3::new(1.25, -2.5, 0.75);
        let charts = [
            Chart::Plane(PlaneChart {
                origin: Point3::new(0.5, 1.0, -0.25),
                eu: Vector3::new(1.0, 0.0, 0.0),
                ev: Vector3::new(0.0, 1.0, 0.0),
                h: 2.0,
            }),
            Chart::Cylinder(CylinderChart {
                origin: Point3::new(-0.3, 0.4, 0.8),
                e1: Vector3::new(1.0, 0.0, 0.0),
                e2: Vector3::new(0.0, 1.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radius: 3.5,
                h: 1.5,
            }),
            Chart::Cone(ConeChart {
                origin: Point3::new(0.2, -0.6, 0.1),
                e1: Vector3::new(1.0, 0.0, 0.0),
                e2: Vector3::new(0.0, 1.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                rho0: 2.0,
                tau: 0.4,
                ratio: 1.0,
                h: 1.2,
            }),
        ];

        for chart in &charts {
            let u = 0.7;
            let interval = [0.25, 2.75];
            let (full_exact, full_err) =
                analytic_inner_polynomial(chart, u, interval, false, |p| density(p, origin))
                    .expect("polynomial chart");
            let mut charge = |_units: usize| Ok(());
            let mut eval_full = |s: f64| {
                let p = chart.closed_partials(u, s).expect("closed partials");
                Ok(density(&p, origin))
            };
            let ref_full = integrate(
                &mut charge,
                &mut eval_full,
                interval[0],
                interval[1],
                &[],
                Target::inner([1.0; FLUX]),
            )
            .expect("adaptive quadrature");
            for k in 0..FLUX {
                let tol = 1.0e-11 * (1.0 + ref_full.value[k].abs()) + full_err[k];
                assert!(
                    (full_exact[k] - ref_full.value[k]).abs() <= tol,
                    "component {k}: exact={} ref={}",
                    full_exact[k],
                    ref_full.value[k]
                );
            }

            let (orient_exact, orient_err) =
                analytic_inner_polynomial(chart, u, interval, true, |p| {
                    orientation_density(p, origin)
                })
                .expect("polynomial orientation chart");
            assert!((orient_exact[0] - full_exact[VOLUME]).abs() <= 1.0e-12 + orient_err[0]);
            for axis in 0..3 {
                assert!(
                    (orient_exact[1 + axis] - full_exact[CLOSURE + axis]).abs()
                        <= 1.0e-12 + orient_err[1 + axis]
                );
            }
        }

        // Cross-apex cone interval must fall back to adaptive quadrature.
        let cross_apex = Chart::Cone(ConeChart {
            origin: Point3::new(0.0, 0.0, 0.0),
            e1: Vector3::new(1.0, 0.0, 0.0),
            e2: Vector3::new(0.0, 1.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            rho0: 1.0,
            tau: -1.0,
            ratio: 1.0,
            h: 1.0,
        });
        assert!(
            analytic_inner_polynomial(&cross_apex, 0.5, [0.5, 1.5], false, |p| {
                density(p, origin)
            })
            .is_none()
        );
    }
}
