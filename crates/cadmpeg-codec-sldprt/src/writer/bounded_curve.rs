// SPDX-License-Identifier: Apache-2.0
//! Bounded native curve wrappers for edge parameter intervals.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Edge;

const ENDPOINT_TOLERANCE_MM: f64 = 1.0e-5;

pub(super) struct BoundedCurve {
    pub(super) curve: cadmpeg_ir::ids::CurveId,
    pub(super) points: [Point3; 2],
    pub(super) parameters: [f64; 2],
}

pub(super) fn derived_sphere_seam(ir: &CadIr, edge: &Edge) -> bool {
    let Some(id) = edge.curve() else { return false };
    id.as_str()
        .starts_with("sldprt:brep:curve#sphere-seam-face:")
        && edge
            .param_range()
            .is_some_and(|range| range.get() == [0.0, std::f64::consts::TAU])
        && ir.model.curves.iter().any(|curve| {
            &curve.id == id
                && matches!(
                    curve.geometry.solved(),
                    Some(SolvedCurveGeometry::Degenerate(_))
                )
        })
}

pub(super) fn from_edge(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    edge: &Edge,
    length_scale: f64,
) -> Result<Option<BoundedCurve>, CodecError> {
    let Some(range) = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get) else {
        return Ok(None);
    };
    let Some(id) = edge.curve() else {
        return Ok(None);
    };
    let Some(curve) = ir.model.curves.iter().find(|curve| &curve.id == id) else {
        return Ok(None);
    };
    if matches!(
        curve.geometry.solved(),
        Some(SolvedCurveGeometry::Degenerate(_))
    ) {
        return Ok(None);
    }
    let point = |parameter| -> Result<Option<Point3>, CodecError> {
        Ok(
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx,
                &curve.geometry,
                parameter,
            ))?
            .map(cadmpeg_ir::features::FinitePoint3::get),
        )
    };
    let (Some(first), Some(second)) = (point(range[0])?, point(range[1])?) else {
        return Ok(None);
    };
    let position = |id| {
        let vertex = ir.model.vertices.iter().find(|vertex| &vertex.id == id)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)?;
        let tolerance = vertex.tolerance.map_or(ENDPOINT_TOLERANCE_MM, |value| {
            value.get().max(ENDPOINT_TOLERANCE_MM)
        });
        Some((point.position().get(), tolerance))
    };
    let (Some((start, first_tolerance)), Some((end, second_tolerance))) =
        (position(&edge.start), position(&edge.end))
    else {
        return Ok(None);
    };
    let close = |a: Point3, b: Point3, tolerance: f64| a.distance(b) <= tolerance;
    if !(close(first, start, first_tolerance) && close(second, end, second_tolerance)
        || close(second, start, first_tolerance) && close(first, end, second_tolerance))
    {
        return Ok(None);
    }
    let mut parameters = range;
    if matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Line(_))) {
        parameters = parameters.map(|value| value * length_scale);
    }
    Ok(Some(BoundedCurve {
        curve: id.clone(),
        points: [first, second],
        parameters,
    }))
}
