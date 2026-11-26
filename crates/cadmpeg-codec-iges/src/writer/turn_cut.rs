// SPDX-License-Identifier: Apache-2.0
//! The start of a Type 142 boundary of a loop that turns around a periodic
//! parameter of its restated support. The restatement cuts one parameter
//! curve of the loop at the end of the turn that holds the face, and the
//! pieces lie at the two ends of the turn. A Type 102 composite of the
//! parameter curves joins only when it starts at that cut, and the
//! model-space curve of the Type 142 starts at the same point.

use super::rational_support::SeamCuts;
use super::{
    effective_topology_tolerance, topology_edge_explicit_tolerance, CarrierEntity, CurveSpan,
    TopologyIndex, WrittenBoundary, WrittenPcurveUse,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::{curve_parameter_near_point, finite_or_refusal, model_surface_point};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::units::FiniteVector;

/// The distance from the cut point, relative to the largest magnitude of its
/// coordinates and at least 1, within which an edge curve that passes
/// through the cut point gives its parameter there.
const CUT_RESOLUTION: f64 = 1.0e-9;

/// The cut in the model-space curve of the first coedge of a Type 142
/// boundary that starts inside that coedge.
pub(super) struct CoedgeCut {
    /// The first parameter curve use of the coedge after the cut.
    pub(super) first_use: usize,
    /// The spans of the edge curve after the cut and before it, in written
    /// order.
    pub(super) spans: [CurveSpan; 2],
}

/// Moves the start of the Type 142 `boundary` on `surface` to the seam cut
/// that `cuts` names between two parameter curves of one of its coedges, and
/// cuts the edge curve of that coedge at the same point. A boundary with no
/// seam cut keeps its start. Returns the reason the boundary has no common
/// start where the edge curve has no parameter at the cut.
pub(super) fn start_at_seam_cut(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'_>,
    surface: &SurfaceGeometry,
    cuts: &SeamCuts,
    boundary: &mut WrittenBoundary<'_>,
) -> Result<Option<String>, CodecError> {
    let Some((coedge, first_use)) =
        boundary
            .coedges
            .iter()
            .enumerate()
            .find_map(|(coedge, written)| {
                written
                    .pcurves
                    .windows(2)
                    .position(|pair| {
                        cuts.cut_between(pair[0].pcurve.id.as_str(), pair[1].pcurve.id.as_str())
                    })
                    .map(|before| (coedge, before + 1))
            })
    else {
        return Ok(None);
    };
    boundary.coedges.rotate_left(coedge);
    let loop_id = &boundary.loop_.id;
    let written = &mut boundary.coedges[0];
    let carrier = &written.carrier;
    let unstarted = |what: &str| {
        Ok(Some(format!(
            "coedge {} of loop {loop_id} {what} at the seam cut of its parameter curves",
            carrier.coedge.id
        )))
    };
    let Some([_, uv]) = written_ends(&written.pcurves[first_use - 1])? else {
        return unstarted("has no finite parameter curve end");
    };
    let Some(point) = finite_or_refusal(model_surface_point(
        EvaluationAdmission::Standard,
        index.ir,
        surface,
        uv.u,
        uv.v,
    ))?
    .map(cadmpeg_ir::features::geometry::FinitePoint3::get) else {
        return unstarted("has no finite support point");
    };
    let [start, end] = carrier.span.range.get();
    let seed = 0.5f64.mul_add(end - start, start);
    let exact = CUT_RESOLUTION * point.x.abs().max(point.y.abs()).max(point.z.abs()).max(1.0);
    let tolerance =
        effective_topology_tolerance(topology_edge_explicit_tolerance(index, carrier.edge));
    let mut parameter = None;
    for tolerance in [exact, tolerance] {
        parameter = curve_parameter_near_point(ctx, &carrier.geometry, point, seed, tolerance)?
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .filter(|parameter| start < *parameter && *parameter < end);
        if parameter.is_some() {
            break;
        }
    }
    let Some(parameter) = parameter else {
        return unstarted("has no edge curve parameter inside its span");
    };
    let span = |range: [f64; 2], ends: [Point3; 2]| {
        FiniteVector::new(range)
            .map(|range| CurveSpan {
                range,
                start: ends[0],
                end: ends[1],
            })
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES Type 142 loop {loop_id} cuts coedge {} at a non-finite parameter",
                    carrier.coedge.id
                ))
            })
    };
    let lower = span([start, parameter], [carrier.span.start, point])?;
    let upper = span([parameter, end], [point, carrier.span.end])?;
    written.cut = Some(CoedgeCut {
        first_use,
        spans: if written.model == CarrierEntity::Stored {
            [upper, lower]
        } else {
            [lower, upper]
        },
    });
    Ok(None)
}

/// The parameter-plane start and end of the curve that a written use
/// references, in the direction of that curve.
fn written_ends(pcurve_use: &WrittenPcurveUse<'_>) -> Result<Option<[Point2; 2]>, CodecError> {
    let Some(range) = pcurve_use.pcurve.parameter_range() else {
        return Ok(None);
    };
    let mut ends = [Point2::new(0.0, 0.0); 2];
    for (end, parameter) in ends.iter_mut().zip(range.get()) {
        let Some(point) = finite_or_refusal(pcurve_uv(
            EvaluationAdmission::Standard,
            &pcurve_use.pcurve.geometry,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        *end = point.get();
    }
    if pcurve_use.entity == CarrierEntity::ReversedCopy {
        ends.reverse();
    }
    Ok(Some(ends))
}
