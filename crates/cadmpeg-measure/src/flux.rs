// SPDX-License-Identifier: Apache-2.0
//! Fluxes of a face region for volume, centroid, closure and area centroid.
//!
//! With `N = S_u × S_v` and `d = P − P_ref`, a face contributes the surface
//! integrals of `d·N / 3` (volume), `d_i² N_i / 2` (first moments), `N_i`
//! (closure) and `d_i |N|` (area moments). Each is `∮ F du` by Green's
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

/// Flux components: volume, three first moments, three closure components
/// and three area moments.
pub(crate) const FLUX: usize = 10;

/// Integrated values and their error estimates.
const TRACKED: usize = 2 * FLUX;

/// The first area moment component; the components before it change sign
/// with the oriented normal.
const AREA_MOMENTS: usize = 7;

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
        self.value[0]
    }

    pub(crate) fn moment(&self) -> [f64; 3] {
        [self.value[1], self.value[2], self.value[3]]
    }

    pub(crate) fn closure(&self) -> [f64; 3] {
        [self.value[4], self.value[5], self.value[6]]
    }

    pub(crate) fn area_moment(&self) -> [f64; 3] {
        [self.value[7], self.value[8], self.value[9]]
    }
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
        d.x * magnitude,
        d.y * magnitude,
        d.z * magnitude,
    ]
}

/// The magnitude scale `scale^d` of each component.
fn scales(scale: f64) -> [f64; TRACKED] {
    let powers = [3, 4, 4, 4, 2, 2, 2, 3, 3, 3];
    std::array::from_fn(|k| scale.powi(powers[k % FLUX]))
}

/// What one flux integral reads.
pub(crate) struct FluxFrame<'a, 'ir> {
    pub(crate) chart: &'a Chart<'ir>,
    pub(crate) origin: Point3,
    /// The length the quadrature targets are relative to.
    pub(crate) scale: f64,
}

fn not_converged() -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::QuadratureNotConverged,
        None,
        "the flux quadrature did not converge",
    )
}

impl FluxFrame<'_, '_> {
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
            Target::inner(std::array::from_fn(|k| scale[k])),
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
                Target::outer(scales(self.scale)),
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
            Target::outer(scales(self.scale)),
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
