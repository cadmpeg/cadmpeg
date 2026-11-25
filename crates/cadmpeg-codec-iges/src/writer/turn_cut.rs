// SPDX-License-Identifier: Apache-2.0
//! The start of a Type 142 boundary whose parameter curves join in the
//! parameter plane at all places but one. A loop that turns around a
//! periodic parameter of its support has its parameter curves cut at the end
//! of the turn that holds its face, and the pieces lie at the two ends of the
//! turn. A Type 102 composite of the parameter curves joins only when it
//! starts at that cut, and the model-space curve of the Type 142 starts at
//! the same point.

use super::{CarrierEntity, CurveSpan, WrittenCoedge, WrittenPcurveUse};
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::pcurve_uv;
use cadmpeg_ir::eval::{finite_or_refusal, model_surface_point};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::Loop;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::f64::consts::TAU;

/// The largest parameter-plane distance at which the end of one parameter
/// curve of a boundary joins the start of the next.
const PARAMETER_JOIN: f64 = 1.0e-9;

/// The cut in the model-space curve of the first coedge of a Type 142
/// boundary that starts inside that coedge.
pub(super) struct CoedgeCut {
    /// The first parameter curve use of the coedge after the cut.
    pub(super) first_use: usize,
    /// The spans of the edge curve after the cut and before it, in written
    /// order.
    pub(super) spans: [CurveSpan; 2],
}

/// Moves the start of the Type 142 boundary of `loop_` on `surface`, written
/// as `coedges`, to the one place where its parameter curves do not join.
/// Fails where they do not join at more places, or where the boundary starts
/// inside a coedge whose edge curve is not a circle or an ellipse.
pub(super) fn start_at_parameter_gap(
    ir: &CadIr,
    loop_: &Loop,
    surface: &SurfaceGeometry,
    coedges: &mut [WrittenCoedge<'_>],
) -> Result<(), CodecError> {
    let mut uses = Vec::new();
    for (coedge, written) in coedges.iter().enumerate() {
        for (position, pcurve_use) in written.pcurves.iter().enumerate() {
            let Some(ends) = written_ends(pcurve_use)? else {
                return Ok(());
            };
            uses.push((coedge, position, ends));
        }
    }
    let gaps = uses
        .windows(2)
        .filter(|pair| distance(pair[0].2[1], pair[1].2[0]) > PARAMETER_JOIN)
        .map(|pair| (pair[1].0, pair[1].1, pair[0].2[1]))
        .collect::<Vec<_>>();
    let (coedge, first_use, uv) = match gaps.as_slice() {
        [] => return Ok(()),
        [gap] => *gap,
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "IGES Type 142 loop {}: its parameter curves join in no one order",
                loop_.id
            )))
        }
    };
    coedges.rotate_left(coedge);
    if first_use == 0 {
        return Ok(());
    }
    let written = &mut coedges[0];
    let carrier = &written.carrier;
    let point = finite_or_refusal(model_surface_point(
        EvaluationAdmission::Standard,
        ir,
        surface,
        uv.u,
        uv.v,
    ))?
    .map(cadmpeg_ir::features::geometry::FinitePoint3::get);
    let [start, end] = carrier.span.range.get();
    let Some((point, parameter)) = point
        .and_then(|point| {
            let angle = conic_parameter(&carrier.geometry, point)?;
            Some((point, (angle - start).rem_euclid(TAU) + start))
        })
        .filter(|(_, parameter)| start < *parameter && *parameter < end)
    else {
        return Err(CodecError::NotImplemented(format!(
            "IGES Type 142 loop {}: coedge {} starts the boundary inside an edge curve that is not a circle or an ellipse",
            loop_.id, carrier.coedge.id
        )));
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
                    "IGES Type 142 loop {} cuts coedge {} at a non-finite parameter",
                    loop_.id, carrier.coedge.id
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
    Ok(())
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

fn distance(first: Point2, second: Point2) -> f64 {
    (first.u - second.u).hypot(first.v - second.v)
}

/// The angle parameter of `point` on a circle or ellipse `geometry`.
fn conic_parameter(geometry: &CurveGeometry, point: Point3) -> Option<f64> {
    let (center, frame, [major, minor]) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => {
            (circle.center(), circle.frame(), [circle.radius().get(); 2])
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse)) => (
            ellipse.center(),
            ellipse.frame(),
            [ellipse.major_radius().get(), ellipse.minor_radius().get()],
        ),
        _ => return None,
    };
    let center = center.get();
    let offset =
        cadmpeg_ir::math::Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let reference = *frame.reference().as_raw();
    let across = frame.axis().as_raw().cross(reference);
    Some((offset.dot(across) / minor).atan2(offset.dot(reference) / major))
}
