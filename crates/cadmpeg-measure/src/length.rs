// SPDX-License-Identifier: Apache-2.0
//! Edge lengths.
//!
//! A line's parameter is arc length times the placement scale and a circle's
//! is angle, so both have closed forms. Every other curve integrates `|C'|`
//! by adaptive quadrature.

use crate::canonical::curve::{CurveChart, CurveShape};
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::quadrature::{integrate, Target};
use crate::reason::UnavailableReason;
use crate::value::Method;

/// The length of a curve interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Length {
    pub(crate) value: f64,
    pub(crate) numeric_error: f64,
    pub(crate) method: Method,
}

/// The length of `chart` between `start` and `end`. `scale` is the length
/// the quadrature target is relative to.
pub(crate) fn curve_length(
    eval: &Eval<'_, '_>,
    chart: &CurveChart<'_>,
    [start, end]: [f64; 2],
    scale: f64,
) -> Measure<Length> {
    let span = (end - start).abs();
    match &chart.shape {
        CurveShape::Line { direction, .. } => Ok(Length {
            value: span * direction.norm(),
            numeric_error: 0.0,
            method: Method::ClosedForm,
        }),
        CurveShape::Conic { major, minor, .. } if major == minor => Ok(Length {
            value: span * major,
            numeric_error: 0.0,
            method: Method::ClosedForm,
        }),
        CurveShape::Degenerate { .. } => Ok(Length {
            value: 0.0,
            numeric_error: 0.0,
            method: Method::ClosedForm,
        }),
        CurveShape::Conic { .. } | CurveShape::Evaluated => {
            let mut charge = |units| eval.charge(units);
            let mut speed = |t: f64| -> Measure<[f64; 1]> { Ok([chart.tangent(eval, t)?.norm()]) };
            let integral = integrate(
                &mut charge,
                &mut speed,
                start.min(end),
                start.max(end),
                &chart.breaks(),
                Target { scale: [scale] },
            )?;
            if !integral.converged {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::QuadratureNotConverged,
                    Some(chart.id()),
                    "the length quadrature did not converge",
                ));
            }
            Ok(Length {
                value: integral.value[0],
                numeric_error: integral.error[0],
                method: Method::BoundaryQuadrature,
            })
        }
        CurveShape::Unsupported(reason) => Err(MeasureFailure::unavailable(
            *reason,
            Some(chart.id()),
            "the curve has no parameterization",
        )),
    }
}
