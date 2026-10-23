// SPDX-License-Identifier: Apache-2.0
//! Fluxes of a face region for volume, centroid, closure and area centroid.
//!
//! With `N = S_u × S_v` and `d = P − P_ref`, a face contributes the surface
//! integrals of `d·N / 3` (volume), `d_i² N_i / 2` (first moments), `N_i`
//! (closure), `d_i³ N_i / 3` and `d_i² d_j N_i / 2` (second moments `∫ d_i²`
//! and `∫ d_i d_j` of the volume) and `d_i |N|` (area moments). Each is `∮ F du` by Green's
//! theorem with the inner integral `F(u, v) = −∫_{v₀}^{v} f(u, s) ds` from
//! the region's base `v₀`, plus the whole domain when the region reaches
//! the domain's natural edges. A body sums its faces' fluxes with the sign
//! of each face's oriented normal reading.

use std::f64::consts::TAU;

use cadmpeg_ir::math::Point3;

use crate::area::LoopCut;
use crate::boundary::ResolvedLoop;
use crate::canonical::surface::{Chart, Partials};
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

/// The magnitude scale `scale^d` of each component.
fn scales(scale: f64) -> [f64; TRACKED] {
    let powers = [3, 4, 4, 4, 2, 2, 2, 5, 5, 5, 5, 5, 5, 3, 3, 3];
    std::array::from_fn(|k| scale.powi(powers[k % FLUX]))
}

/// Every component.
pub(crate) const ALL_COMPONENTS: [bool; FLUX] = [true; FLUX];

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

/// What one flux integral reads.
pub(crate) struct FluxFrame<'a, 'ir> {
    pub(crate) chart: &'a Chart<'ir>,
    pub(crate) origin: Point3,
    /// The length the quadrature targets are relative to.
    pub(crate) scale: f64,
    /// The components whose accuracy the quadrature targets; the others
    /// ride along on the same panels.
    pub(crate) active: [bool; FLUX],
}

fn not_converged() -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::QuadratureNotConverged,
        None,
        "the flux quadrature did not converge",
    )
}

impl FluxFrame<'_, '_> {
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

    /// `∫_from^to f(u, s) ds` and its error estimate.
    fn inner(
        &self,
        eval: &Eval<'_, '_>,
        u: f64,
        [from, to]: [f64; 2],
    ) -> Measure<([f64; FLUX], [f64; FLUX])> {
        let mut charge = |units| eval.charge(units);
        let mut column = |s: f64| -> Measure<[f64; FLUX]> {
            Ok(density(&self.chart.partials(eval, u, s)?, self.origin))
        };
        let scale = scales(self.scale);
        let integral = integrate(
            &mut charge,
            &mut column,
            from,
            to,
            &self.v_breaks(),
            Target::inner(std::array::from_fn(|k| scale[k])).with_active(self.active),
        )?;
        if !integral.converged {
            return Err(not_converged());
        }
        Ok((integral.value, integral.error))
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
        let mut flux = Flux::ZERO;
        for (position, piece) in resolved.pieces.iter().enumerate() {
            let mut charge = |units| eval.charge(units);
            let mut integrand = |t: f64| -> Measure<[f64; TRACKED]> {
                let sample = piece.sample(eval, self.chart, t)?;
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
                &LoopCut::breaks(cut.as_ref(), position, piece.breaks()),
                Target::outer(scales(self.scale)).with_active(self.tracked()),
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

    /// `∫_u0^u1 ∫_v0^v1 f du dv` over a parameter rectangle.
    fn strip_flux(
        &self,
        eval: &Eval<'_, '_>,
        [u0, u1]: [f64; 2],
        v: [f64; 2],
        u_breaks: &[f64],
    ) -> Measure<Flux> {
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
            Target::outer(scales(self.scale)).with_active(self.tracked()),
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
