// SPDX-License-Identifier: Apache-2.0
//! Trimmed area by Green's theorem.
//!
//! For a region `R` whose boundary runs with `R` on its left in the
//! parameter plane, `A = ∮ F(u, v) du` with `∂F/∂v = −|S_u × S_v|`. On an
//! analytic chart `F(u, v) = G(v) − G(v₀)` for an antiderivative `G` of
//! `−|N|`. Each piece integrates `F(u(t), v(t))·u'(t)` by adaptive
//! quadrature, so the listed order of a ring changes no value once its
//! traversal is known.

use crate::boundary::ResolvedLoop;
use crate::canonical::surface::{Chart, NurbsChart};
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::quadrature::{integrate, Target};
use crate::reason::UnavailableReason;
use crate::region::{ClosedRegion, LoopIntegral, LoopShape, Region};
use crate::tolerance::QUADRATURE_ACCEPTANCE_RELATIVE;

/// An area and its quadrature error estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Area {
    pub(crate) value: f64,
    pub(crate) numeric_error: f64,
}

fn elliptic_cone() -> MeasureFailure {
    MeasureFailure::unavailable(
        UnavailableReason::CarrierEllipticCone,
        None,
        "area on a cone with an elliptic cross-section is not computed",
    )
}

fn antiderivative(chart: &Chart, v: f64) -> Measure<f64> {
    chart.area_antiderivative(v).ok_or_else(elliptic_cone)
}

/// `∮ (G(v) − G(base)) du` over one loop on an analytic chart, or
/// `∮ F du` with the generic inner integral `F(u, v) = −∫_base^v |N(u, s)| ds`
/// on a NURBS chart. `scale` is the length the quadrature target is relative
/// to.
pub(crate) fn loop_integral(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    resolved: &ResolvedLoop<'_>,
    base: f64,
    scale: f64,
) -> Measure<LoopIntegral> {
    if let Chart::Nurbs(nurbs) = chart {
        return generic_loop_integral(eval, chart, nurbs, resolved, base, scale);
    }
    cut_loop_integral(eval, chart, resolved, base, None, scale)
}

/// A loop's cut: the period its pieces' `v` is taken modulo, from `base`,
/// and per piece the parameters where the piece crosses the cut.
#[derive(Clone, Copy)]
pub(crate) struct LoopCut<'a> {
    pub(crate) period: f64,
    pub(crate) breaks: &'a [Vec<f64>],
}

impl LoopCut<'_> {
    /// `v` taken in `[base, base + period)`.
    pub(crate) fn wrap(&self, base: f64, v: f64) -> f64 {
        base + (v - base).rem_euclid(self.period)
    }

    /// The piece's own breaks and its cut crossings.
    pub(crate) fn breaks(cut: Option<&Self>, position: usize, own: Vec<f64>) -> Vec<f64> {
        let mut breaks = own;
        if let Some(crossings) = cut.and_then(|cut| cut.breaks.get(position)) {
            breaks.extend(crossings.iter().copied());
        }
        breaks
    }
}

/// `∮ (G(v) − G(base)) du` over one loop on an analytic chart, with each
/// piece's `v` taken modulo the cut's period when the domain is cut.
fn cut_loop_integral(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    resolved: &ResolvedLoop<'_>,
    base: f64,
    cut: Option<LoopCut<'_>>,
    scale: f64,
) -> Measure<LoopIntegral> {
    let g0 = antiderivative(chart, base)?;
    let target = Target::outer([scale * scale]);
    let mut value = 0.0_f64;
    let mut error = 0.0_f64;
    for (position, piece) in resolved.pieces.iter().enumerate() {
        let mut charge = |units| eval.charge(units);
        let mut integrand = |t: f64| -> Measure<[f64; 1]> {
            let sample = piece.sample(eval, chart, t)?;
            let v = cut.map_or(sample.uv[1], |cut| cut.wrap(base, sample.uv[1]));
            Ok([(antiderivative(chart, v)? - g0) * sample.duv[0]])
        };
        let integral = integrate(
            &mut charge,
            &mut integrand,
            piece.t0,
            piece.t1,
            &LoopCut::breaks(cut.as_ref(), position, piece.breaks()),
            target,
        )?;
        value += integral.value[0];
        error += integral.error[0];
    }
    accept(error, scale)?;
    Ok(LoopIntegral { value, error, base })
}

fn accept(error: f64, scale: f64) -> Measure<()> {
    if error > QUADRATURE_ACCEPTANCE_RELATIVE * scale * scale {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::QuadratureNotConverged,
            None,
            "the area quadrature did not converge",
        ));
    }
    Ok(())
}

/// `∫_from^to |N(u, s)| ds` along a `v`-isoline of a NURBS chart, split at
/// the `v` knots, and its error estimate.
fn inner(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    nurbs: &NurbsChart<'_>,
    u: f64,
    [from, to]: [f64; 2],
    scale: f64,
) -> Measure<(f64, f64)> {
    let mut charge = |units| eval.charge(units);
    let mut density = |s: f64| -> Measure<[f64; 1]> {
        let partials = chart.partials(eval, u, s)?;
        Ok([partials.du.cross(partials.dv).norm()])
    };
    let integral = integrate(
        &mut charge,
        &mut density,
        from,
        to,
        &nurbs.v_breaks(),
        Target::inner([scale * scale]),
    )?;
    if !integral.converged {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::QuadratureNotConverged,
            None,
            "the inner area quadrature did not converge",
        ));
    }
    Ok((integral.value[0], integral.error[0]))
}

fn generic_loop_integral(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    nurbs: &NurbsChart<'_>,
    resolved: &ResolvedLoop<'_>,
    base: f64,
    scale: f64,
) -> Measure<LoopIntegral> {
    let target = Target::outer([scale * scale, scale * scale]);
    let mut value = 0.0_f64;
    let mut error = 0.0_f64;
    for piece in &resolved.pieces {
        let mut charge = |units| eval.charge(units);
        let mut integrand = |t: f64| -> Measure<[f64; 2]> {
            let sample = piece.sample(eval, chart, t)?;
            let (column, column_error) = inner(
                eval,
                chart,
                nurbs,
                sample.uv[0],
                [base, sample.uv[1]],
                scale,
            )?;
            Ok([-column * sample.duv[0], column_error * sample.duv[0].abs()])
        };
        let integral = integrate(
            &mut charge,
            &mut integrand,
            piece.t0,
            piece.t1,
            &piece.breaks(),
            target,
        )?;
        value += integral.value[0];
        error += integral.error[0] + integral.value[1].abs();
    }
    accept(error, scale)?;
    Ok(LoopIntegral { value, error, base })
}

/// The area of a chart's whole domain, `∫∫ |N| du dv`: closed forms on a
/// sphere and a torus, a quadrature on a NURBS chart.
pub(crate) fn domain_area(eval: &Eval<'_, '_>, chart: &Chart<'_>, scale: f64) -> Measure<Area> {
    if let Some(value) = chart.closed_area() {
        return Ok(Area {
            value,
            numeric_error: 0.0,
        });
    }
    let Chart::Nurbs(nurbs) = chart else {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::RegionDomainUnsupported,
            None,
            "the carrier has no finite domain",
        ));
    };
    let [[u0, u1], v] = nurbs.domain;
    let mut charge = |units| eval.charge(units);
    let mut integrand = |u: f64| -> Measure<[f64; 2]> {
        let (column, column_error) = inner(eval, chart, nurbs, u, v, scale)?;
        Ok([column, column_error])
    };
    let integral = integrate(
        &mut charge,
        &mut integrand,
        u0,
        u1,
        &nurbs.u_breaks(),
        Target::outer([scale * scale, scale * scale]),
    )?;
    let numeric_error = integral.error[0] + integral.value[1].abs();
    accept(numeric_error, scale)?;
    Ok(Area {
        value: integral.value[0],
        numeric_error,
    })
}

/// The area of a region: each loop oriented by its role and moved to the
/// region's common base, plus the whole domain when the region reaches the
/// domain's natural edges. A contractible loop's integral does not depend
/// on its base, so only loops with a net shift are moved.
pub(crate) fn region_area(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    region: &Region,
    shapes: &[Option<LoopShape>],
    resolved: &[Option<&ResolvedLoop<'_>>],
    scale: f64,
) -> Measure<Area> {
    if let Some(closed) = &region.closed {
        return closed_region_area(eval, chart, region, closed, resolved, scale);
    }
    let period = chart.periods()[0];
    let mut value = 0.0_f64;
    let mut numeric_error = 0.0_f64;
    for (index, member) in region.loops.iter().enumerate() {
        if member.factor == 0.0 {
            continue;
        }
        let (Some(Some(shape)), Some(Some(resolved))) = (shapes.get(index), resolved.get(index))
        else {
            continue;
        };
        let integral = shape.integral;
        let turns = f64::from(resolved.net_shift[0]);
        let rebase = match period {
            Some(period) if turns != 0.0 => {
                (antiderivative(chart, integral.base)? - antiderivative(chart, region.base)?)
                    * period
                    * turns
            }
            _ => 0.0,
        };
        value += member.factor * (integral.value + rebase);
        numeric_error += integral.error;
    }
    if region.exterior != 0.0 {
        let domain = domain_area(eval, chart, scale)?;
        value += region.exterior * domain.value;
        numeric_error += domain.numeric_error;
    }
    Ok(Area {
        value,
        numeric_error,
    })
}

/// The area of a region on a closed domain: each loop by its factor from
/// the region's base, plus `W·|I|` for each interval `I` of the upper edge
/// in the region, where `W = G(base) − G(base + span)` is the area of one
/// unit of `u`.
fn closed_region_area(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    region: &Region,
    closed: &ClosedRegion,
    resolved: &[Option<&ResolvedLoop<'_>>],
    scale: f64,
) -> Measure<Area> {
    let Some(span) = chart.closed_span() else {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::RegionDomainUnsupported,
            None,
            "the carrier has no closed domain",
        ));
    };
    let mut value = 0.0_f64;
    let mut numeric_error = 0.0_f64;
    for (index, member) in region.loops.iter().enumerate() {
        if member.factor == 0.0 {
            continue;
        }
        let Some(Some(resolved)) = resolved.get(index) else {
            continue;
        };
        let cut = closed.wrap.as_ref().map(|wrap| LoopCut {
            period: wrap.period,
            breaks: wrap.breaks.get(index).map_or(&[], Vec::as_slice),
        });
        let integral = cut_loop_integral(eval, chart, resolved, region.base, cut, scale)?;
        value += member.factor * integral.value;
        numeric_error += integral.error;
    }
    let width = antiderivative(chart, region.base)? - antiderivative(chart, region.base + span)?;
    let top: f64 = closed.top.iter().map(|[start, end]| end - start).sum();
    value += width * top;
    Ok(Area {
        value,
        numeric_error,
    })
}
