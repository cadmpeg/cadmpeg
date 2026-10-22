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
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::quadrature::{integrate, Target};
use crate::reason::UnavailableReason;
use crate::region::{LoopIntegral, LoopShape, Region};
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

/// `∮ (G(v) − G(base)) du` over one loop. `scale` is the length the
/// quadrature target is relative to.
pub(crate) fn loop_integral(
    eval: &Eval<'_, '_>,
    chart: &Chart,
    resolved: &ResolvedLoop<'_>,
    base: f64,
    scale: f64,
) -> Measure<LoopIntegral> {
    let g0 = antiderivative(chart, base)?;
    let target = Target {
        scale: [scale * scale],
    };
    let mut value = 0.0_f64;
    let mut error = 0.0_f64;
    for piece in &resolved.pieces {
        let mut charge = |units| eval.charge(units);
        let mut integrand = |t: f64| -> Measure<[f64; 1]> {
            let sample = piece.sample(eval, chart, t)?;
            Ok([(antiderivative(chart, sample.uv[1])? - g0) * sample.duv[0]])
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
        error += integral.error[0];
    }
    if error > QUADRATURE_ACCEPTANCE_RELATIVE * scale * scale {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::QuadratureNotConverged,
            None,
            "the area quadrature did not converge",
        ));
    }
    Ok(LoopIntegral { value, error, base })
}

/// The area of a nesting region: each loop oriented by its role and moved to
/// the region's common base.
pub(crate) fn nesting_area(
    chart: &Chart,
    region: &Region,
    shapes: &[Option<LoopShape>],
    resolved: &[Option<&ResolvedLoop<'_>>],
) -> Measure<Area> {
    let g0 = antiderivative(chart, region.base)?;
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
            Some(period) => (antiderivative(chart, integral.base)? - g0) * period * turns,
            None => 0.0,
        };
        value += member.factor * (integral.value + rebase);
        numeric_error += integral.error;
    }
    Ok(Area {
        value,
        numeric_error,
    })
}
