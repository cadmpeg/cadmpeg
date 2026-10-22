// SPDX-License-Identifier: Apache-2.0
//! Reparameterize spline pcurves onto the parameter range of their 3D edge.

use std::borrow::Cow;

use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::topology::Edge;

/// Relative tolerance for a use range to state the edge range unchanged.
const EPS_PARAMETER_RANGE_MATCH: f64 = 1.0e-9;

/// `SURFACE_CURVE` inherits the 3D curve's parameterization, so a spline
/// pcurve must run on its edge's range. A coedge use states its interval on the
/// pcurve's own parameter, directed from the edge's start to its end; a
/// decreasing interval runs against the edge. The affine map from the edge
/// range onto that interval moves the knots, and a decreasing interval also
/// reverses the knot and pole order. The parameter-space locus is unchanged.
/// The mapping is per use: a shared pcurve can serve different edge ranges.
pub(super) fn align_spline_to_edge<'a>(
    geometry: &'a PcurveGeometry,
    source_range: Option<DirectedParameterRange>,
    edge: &Edge,
) -> Cow<'a, PcurveGeometry> {
    let aligned = (|| {
        let PcurveGeometry::Nurbs { nurbs } = geometry else {
            return None;
        };
        let [source_start, source_end] = source_range?.endpoints();
        let [target_start, target_end] = edge.param_range()?.get();
        let source_span = source_end - source_start;
        let target_span = target_end - target_start;
        if !source_span.is_finite()
            || !target_span.is_finite()
            || source_span == 0.0
            || target_span <= 0.0
        {
            return None;
        }
        let tolerance = EPS_PARAMETER_RANGE_MATCH * source_span.abs().max(target_span).max(1.0);
        if (source_start - target_start).abs() <= tolerance
            && (source_end - target_end).abs() <= tolerance
        {
            return None;
        }
        let scale = target_span / source_span;
        let to_edge = |knot: &f64| target_start + (knot - source_start) * scale;
        let reversed = source_span < 0.0;
        let knots = if reversed {
            nurbs.knots().iter().rev().map(to_edge).collect::<Vec<_>>()
        } else {
            nurbs.knots().iter().map(to_edge).collect::<Vec<_>>()
        };
        if knots.iter().any(|knot| !knot.is_finite()) {
            return None;
        }
        let poles = if reversed {
            match nurbs.pole_rows() {
                PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
                    points: points.iter().rev().copied().collect(),
                },
                PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
                    points: points.iter().rev().copied().collect(),
                },
            }
        } else {
            nurbs.pole_rows().clone()
        };
        let nurbs = PcurveNurbs::new(nurbs.degree(), knots, poles, nurbs.periodic()).ok()?;
        Some(PcurveGeometry::Nurbs { nurbs })
    })();
    aligned.map_or_else(|| Cow::Borrowed(geometry), Cow::Owned)
}

#[cfg(test)]
mod tests;
