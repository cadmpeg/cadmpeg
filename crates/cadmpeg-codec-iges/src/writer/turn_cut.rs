// SPDX-License-Identifier: Apache-2.0
//! The start of a Type 142 boundary of a loop that turns around a periodic
//! parameter of its restated support. The loop passes the end of the turn
//! that holds the face inside one parameter curve, which the restatement cuts
//! there, or between two parameter curves. A Type 102 composite of the
//! parameter curves joins only when it starts there, and the model-space
//! curve of the Type 142 starts at the same point.

use super::rational_support::{CurveEnd, SeamCuts};
use super::{
    effective_topology_tolerance, topology_edge_explicit_tolerance, CarrierEntity, CurveSpan,
    TopologyIndex, WrittenBoundary, WrittenPcurveUse,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::{
    finite_or_refusal, model_curve_parameter_near_point_in_index_with_tolerance,
    model_surface_point,
};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::units::FiniteVector;
use std::cell::OnceCell;

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

/// Moves the start of the Type 142 `boundary` on `surface` to the place
/// where its loop passes the end of the turn that holds its face, which
/// `cuts` names. Where that place lies inside a coedge, the edge curve of
/// that coedge is cut at the same point; `model` is the index of the
/// document for the search of the edge curve parameter. A boundary that does
/// not pass the end of its turn keeps its start. Returns the reason the
/// boundary has no common start where it passes the end of its turn more
/// than once, or where the edge curve has no parameter at the cut.
pub(super) fn start_at_seam_cut<'ir>(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'ir>,
    model: &OnceCell<ModelIndex<'ir>>,
    surface: &SurfaceGeometry,
    cuts: &SeamCuts,
    boundary: &mut WrittenBoundary<'_>,
) -> Result<Option<String>, CodecError> {
    let count = boundary.coedges.len();
    let mut places = Vec::new();
    for (coedge, written) in boundary.coedges.iter().enumerate() {
        let before = boundary.coedges[(coedge + count - 1) % count]
            .pcurves
            .last();
        for (position, pcurve_use) in written.pcurves.iter().enumerate() {
            let prior = match position {
                0 => before,
                _ => written.pcurves.get(position - 1),
            };
            if prior.is_some_and(|prior| {
                cuts.cut_between(
                    (prior.pcurve.id.as_str(), traversed_ends(prior)[1]),
                    (pcurve_use.pcurve.id.as_str(), traversed_ends(pcurve_use)[0]),
                )
            }) {
                places.push((coedge, position));
            }
        }
    }
    let loop_id = &boundary.loop_.id;
    let (coedge, first_use) = match places.as_slice() {
        [] => return Ok(None),
        [place] => *place,
        _ => {
            let mut coedges = places
                .iter()
                .map(|(coedge, _)| boundary.coedges[*coedge].carrier.coedge.id.as_str())
                .collect::<Vec<_>>();
            coedges.dedup();
            return Ok(Some(format!(
                "loop {loop_id} passes the end of the turn of its support more than once, in \
                 coedges {}",
                coedges.join(", ")
            )));
        }
    };
    boundary.coedges.rotate_left(coedge);
    if first_use == 0 {
        return Ok(None);
    }
    let written = &mut boundary.coedges[0];
    let carrier = &written.carrier;
    let unstarted = |what: &str| {
        Ok(Some(format!(
            "coedge {} of loop {loop_id} {what} at the seam cut of its parameter curves",
            carrier.coedge.id
        )))
    };
    let Some([_, uv]) = written_ends(ctx, &written.pcurves[first_use - 1])? else {
        return unstarted("has no finite parameter curve end");
    };
    let Some(point) = finite_or_refusal(model_surface_point(
        EvaluationAdmission::Write(ctx),
        &index.model,
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
    let model = model.get_or_init(|| ModelIndex::build(index.ir));
    for tolerance in [exact, tolerance] {
        parameter = model_curve_parameter_near_point_in_index_with_tolerance(
            ctx,
            model,
            carrier.curve_id,
            point,
            seed,
            tolerance,
        )?
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

/// The curve ends that a written use passes first and last.
fn traversed_ends(pcurve_use: &WrittenPcurveUse<'_>) -> [CurveEnd; 2] {
    match pcurve_use.entity {
        CarrierEntity::Stored => [CurveEnd::Start, CurveEnd::End],
        CarrierEntity::ReversedCopy => [CurveEnd::End, CurveEnd::Start],
    }
}

/// The parameter-plane start and end of the curve that a written use
/// references, in the direction of that curve.
fn written_ends(
    ctx: &DecodeContext<'_>,
    pcurve_use: &WrittenPcurveUse<'_>,
) -> Result<Option<[Point2; 2]>, CodecError> {
    let Some(range) = pcurve_use.pcurve.parameter_range() else {
        return Ok(None);
    };
    let mut ends = [Point2::new(0.0, 0.0); 2];
    for (end, parameter) in ends.iter_mut().zip(range.get()) {
        let Some(point) = finite_or_refusal(pcurve_uv(
            EvaluationAdmission::Write(ctx),
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
