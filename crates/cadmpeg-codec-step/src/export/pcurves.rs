// SPDX-License-Identifier: Apache-2.0
//! Align spline pcurves with the parameter branch of their periodic 3D edge.

use std::borrow::Cow;
use std::f64::consts::TAU;

use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{CurveGeometry, DirectedParameterRange, SolvedCurveGeometry};
use cadmpeg_ir::topology::Edge;

/// Relative tolerance for the two interval endpoints to state one whole-period shift.
const EPS_PERIODIC_PARAMETER_SHIFT: f64 = 1.0e-9;

/// A circular edge can state `[a + nτ, b + nτ]` while its bounded spline
/// pcurve still uses `[a, b]`. `SURFACE_CURVE` inherits the 3D parameterization,
/// so move the spline's knots to the edge's branch without moving its UV poles.
/// The correction is per use: a shared pcurve can serve different edge branches.
pub(super) fn align_periodic_spline<'a>(
    geometry: &'a PcurveGeometry,
    source_range: Option<DirectedParameterRange>,
    edge: &Edge,
    curve: &CurveGeometry,
) -> Cow<'a, PcurveGeometry> {
    let shifted = (|| {
        let PcurveGeometry::Nurbs { nurbs } = geometry else {
            return None;
        };
        if !matches!(
            curve.solved(),
            Some(SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_))
        ) {
            return None;
        }
        let source = source_range?.endpoints();
        let target = edge.param_range()?.get();
        let source_span = source[1] - source[0];
        let target_span = target[1] - target[0];
        if !source_span.is_finite()
            || !target_span.is_finite()
            || source_span <= 0.0
            || target_span <= 0.0
        {
            return None;
        }
        let shift = ((target[0] - source[0]) / TAU).round() * TAU;
        if shift == 0.0 || !shift.is_finite() {
            return None;
        }
        let tolerance = EPS_PERIODIC_PARAMETER_SHIFT * TAU.max(source_span).max(target_span);
        if source
            .iter()
            .zip(target)
            .any(|(source, target)| (target - source - shift).abs() > tolerance)
        {
            return None;
        }
        let knots = nurbs
            .knots()
            .iter()
            .map(|knot| knot + shift)
            .collect::<Vec<_>>();
        let nurbs = PcurveNurbs::new(
            nurbs.degree(),
            knots,
            nurbs.pole_rows().clone(),
            nurbs.periodic(),
        )
        .ok()?;
        Some(PcurveGeometry::Nurbs { nurbs })
    })();
    shifted.map_or_else(|| Cow::Borrowed(geometry), Cow::Owned)
}

#[cfg(test)]
mod tests;
