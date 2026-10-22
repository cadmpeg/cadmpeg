// SPDX-License-Identifier: Apache-2.0
//! Subset curves on the CADIR local parameter.
//!
//! ACIS states a `subset_int_cur` cache and the edge ranges on it on the
//! parameter of the curve it trims, negated on a reversed carrier. A CADIR
//! subset runs on its local parameter, which is 0 at its first trim and runs in
//! its sense.

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::topology::{Coedge, Edge, IncreasingParameterInterval, ParameterInterval};

/// Move each subset's spline cache and every interval on the subset onto its
/// local parameter.
pub(super) fn localize_subsets(
    curves: &mut [Curve],
    edges: &mut [Edge],
    coedges: &mut [Coedge],
    procedural_curves: &[(CurveId, ProceduralCurve)],
) -> Result<(), CodecError> {
    for (owner, procedural) in procedural_curves {
        let ProceduralCurveDefinition::Subset(subset) = procedural.definition() else {
            continue;
        };
        let [first, last] = subset.parameter_range().endpoints();
        let shift = if *subset.sense() { -first } else { last };
        let shifted = |[start, end]: [f64; 2]| {
            ParameterInterval::new([start + shift, end + shift]).map_err(CodecError::malformed)
        };
        for curve in curves.iter_mut().filter(|curve| curve.id == *owner) {
            if let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(cache)) = &mut curve.geometry {
                cache
                    .edit_knots(|knots| knots.iter_mut().for_each(|knot| *knot += shift))
                    .map_err(|error| {
                        CodecError::malformed(format_args!("subset curve {owner}: {error}"))
                    })?;
            }
            if let Some(range) = curve.parameter_range {
                curve.parameter_range =
                    IncreasingParameterInterval::new(shifted(range.endpoints())?.endpoints());
            }
        }
        for edge in edges.iter_mut().filter(|edge| edge.curve() == Some(owner)) {
            if let Some(range) = edge.param_range() {
                edge.set_param_range(Some(shifted(range.get())?));
            }
        }
        for use_curve in coedges
            .iter_mut()
            .filter_map(|coedge| coedge.use_curve.as_mut())
            .filter(|use_curve| use_curve.curve == *owner)
        {
            use_curve.parameter_range = shifted(use_curve.parameter_range.endpoints())?;
        }
    }
    Ok(())
}
