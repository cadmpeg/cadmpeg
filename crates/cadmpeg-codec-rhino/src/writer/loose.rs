// SPDX-License-Identifier: Apache-2.0
//! Native point and curve objects for lower-dimensional shell members.

use std::collections::BTreeSet;
use std::io::Write;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::CadIr;

use crate::loss::RhinoLossCode;

use super::{
    attributed_object_record, bounded_line_payload, check_nurbs_curve, close_point,
    nurbs_curve_payload, point_cloud_payload, WritableEdgeCurve, LINE_CLASS, NURBS_CURVE_CLASS,
    POINT_CLASS, POINT_CLOUD_CLASS,
};

pub(super) fn write(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    output: &mut impl Write,
    losses: &mut Vec<LossNote>,
) -> Result<usize, CodecError> {
    let body = &ir.model.bodies[0];
    let mut seen_vertices = BTreeSet::new();
    let free = ir
        .model
        .shells
        .iter()
        .flat_map(cadmpeg_ir::topology::Shell::free_vertices)
        .filter(|id| seen_vertices.insert((*id).clone()))
        .collect::<Vec<_>>();
    let mut points = Vec::new();
    let mut seen_points = BTreeSet::new();
    for id in &free {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == **id)
            .ok_or_else(|| CodecError::malformed("free vertex is missing"))?;
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| CodecError::malformed("free vertex point is missing"))?;
        if vertex.tolerance.is_some() || super::is_support(point.source_object.as_ref()) {
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "free vertex {} tolerance or dependent-point role has no standalone encoding",
                vertex.id
            )));
            continue;
        }
        if !seen_points.insert(&point.id) {
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "body {} shared free-point carrier is expanded into independent point-cloud vertices", body.id,
            )));
        }
        points.push(point.position().get());
    }
    let mut count = 0;
    if !points.is_empty() && (body.regions.len() > 1 || ir.model.shells.len() > 1) {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "body {} free-vertex region/shell grouping is reduced to one point object",
            body.id,
        )));
    }
    if !points.is_empty() {
        let (kind, class, payload) = if points.len() == 1 {
            let point = points[0];
            let mut payload = vec![0x10];
            payload.extend(point.x.to_le_bytes());
            payload.extend(point.y.to_le_bytes());
            payload.extend(point.z.to_le_bytes());
            (1, POINT_CLASS, payload)
        } else {
            (2, POINT_CLOUD_CLASS, point_cloud_payload(&points)?)
        };
        let identity = if ir.model.faces.is_empty() && ir.model.edges.is_empty() {
            body.id.to_string()
        } else {
            format!("{}.free-vertices", body.id)
        };
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(&attributed_object_record(
            ir,
            kind,
            class,
            &payload,
            &identity,
            crate::writer::presentation::Display {
                name: body.name.as_deref(),
                color: body.color,
                visible: body.visible,
            },
        )?)?;
        count += 1;
    }
    let wires = ir
        .model
        .shells
        .iter()
        .flat_map(cadmpeg_ir::topology::Shell::wire_edges)
        .collect::<BTreeSet<_>>();
    for id in &wires {
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id == **id)
            .ok_or_else(|| CodecError::malformed("wire edge is missing"))?;
        match curve_payload(ir, edge) {
            Ok((class, payload)) => {
                ctx.charge_entities(1, "write Rhino object record")?;
                output.write_all(&attributed_object_record(
                    ir,
                    4,
                    class,
                    &payload,
                    edge.id.as_str(),
                    crate::writer::presentation::Display {
                        name: body.name.as_deref(),
                        color: body.color,
                        visible: body.visible,
                    },
                )?)?;
                count += 1;
            }
            Err(CodecError::UnsupportedContent(reason)) => losses.push(
                RhinoLossCode::WriterEntityOmitted
                    .note(format!("wire edge {} omitted: {reason}", edge.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    if !wires.is_empty() {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "body {} wire ownership is reduced to independent curve objects",
            body.id
        )));
    }
    Ok(count)
}

fn curve_payload(ir: &CadIr, edge: &Edge) -> Result<super::ClassPayload, CodecError> {
    let id = edge.curve().ok_or_else(|| {
        CodecError::UnsupportedContent(
            "an edge without a held curve has no exact standalone encoding".into(),
        )
    })?;
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::malformed("wire curve is missing"))?;
    let domain = edge
        .param_range()
        .ok_or_else(|| {
            CodecError::UnsupportedContent(
                "an unbounded wire edge has no finite standalone encoding".into(),
            )
        })?
        .finite_components();
    let [lo, hi] = cadmpeg_ir::scalar::FiniteReal::raw_array(domain);
    if lo == hi {
        return Err(CodecError::UnsupportedContent(
            "wire edge has an empty interval".into(),
        ));
    }
    let geometry = match curve.geometry.solved() {
        Some(SolvedCurveGeometry::Line(line)) => {
            if (line.direction().as_raw().norm() - 1.0).abs() > super::EPS_WRITE_DEGENERATE {
                return Err(CodecError::UnsupportedContent(
                    "wire line parameterization exceeds Rhino's unit-direction bound".into(),
                ));
            }
            WritableEdgeCurve::Line(*line)
        }
        Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
            check_nurbs_curve(curve.id.as_str(), nurbs)?;
            let degree = usize::try_from(nurbs.degree()).map_err(|_| {
                CodecError::InvalidInput("NURBS degree exceeds address space".into())
            })?;
            let range = [lo.min(hi), lo.max(hi)];
            if [
                nurbs.knots()[degree],
                nurbs.knots()[nurbs.control_points().len()],
            ] != range
            {
                return Err(CodecError::UnsupportedContent(
                    "a trimmed NURBS wire requires an independent trimmed-curve encoding".into(),
                ));
            }
            WritableEdgeCurve::Nurbs(nurbs)
        }
        _ => {
            return Err(CodecError::UnsupportedContent(
                "wire carrier has no supported exact standalone encoding".into(),
            ));
        }
    };
    let mut endpoints = Vec::new();
    for (id, parameter) in [&edge.start, &edge.end].into_iter().zip(domain) {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *id)
            .ok_or_else(|| CodecError::malformed("wire endpoint is missing"))?;
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| CodecError::malformed("wire endpoint point is missing"))?;
        let expected = geometry
            .point(parameter)
            .map_err(|failure| match failure {
                cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit) => {
                    CodecError::ResourceLimit(limit)
                }
                _ => CodecError::UnsupportedContent(format!(
                    "wire edge {} endpoint evaluation has no supported finite result",
                    edge.id,
                )),
            })?
            .get();
        let tolerance = edge.tolerance.or(vertex.tolerance).map_or(
            ir.tolerances.linear.get(),
            cadmpeg_ir::scalar::PositiveReal::get,
        );
        if !close_point(expected, point.position().get(), tolerance) {
            return Err(CodecError::malformed(format_args!(
                "wire edge {} carrier disagrees with its endpoint",
                edge.id,
            )));
        }
        endpoints.push(expected);
    }
    Ok(match geometry {
        WritableEdgeCurve::Line(_) => {
            if lo > hi {
                endpoints.reverse();
            }
            let from = endpoints[0];
            let to = endpoints[1];
            (
                LINE_CLASS,
                bounded_line_payload(
                    [from.x, from.y, from.z],
                    [to.x, to.y, to.z],
                    [lo.min(hi), lo.max(hi)],
                    3,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)?),
    })
}
