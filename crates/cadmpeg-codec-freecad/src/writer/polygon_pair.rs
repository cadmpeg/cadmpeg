// SPDX-License-Identifier: Apache-2.0
//! Resolve the second polygon of an OCCT triangulation pair.

use crate::brep::PolygonOnTriangulationRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, SolvedCurveGeometry};

pub(super) fn secondary<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    primary: &Curve,
    original: &PolygonOnTriangulationRecord,
    (source_vertices, vertices): (
        &[cadmpeg_ir::features::FinitePoint3],
        &[cadmpeg_ir::features::FinitePoint3],
    ),
    rows: &mut Vec<String>,
    source_row: Option<usize>,
) -> Result<Option<(usize, &'a str)>, CodecError> {
    if primary
        .source_object
        .as_ref()
        .is_none_or(|source| source.format.as_str() != "fcstd")
    {
        return Ok(None);
    }
    let mut candidates = ir.model.curves.iter().filter_map(|curve| {
        if curve.id == primary.id || curve.source_object != primary.source_object {
            return None;
        }
        let SolvedCurveGeometry::Polyline(polyline) = curve.geometry.solved()? else {
            return None;
        };
        if polyline.point_count() != original.nodes.len() {
            return None;
        }
        let matches = |vertices: &[cadmpeg_ir::features::FinitePoint3]| {
            polyline.points().zip(&original.nodes).all(|(point, node)| {
                usize::try_from(*node)
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| vertices.get(index))
                    .is_some_and(|vertex| *vertex == point)
            })
        };
        ((matches(vertices) || matches(source_vertices))
            && !ir
                .model
                .edges
                .iter()
                .any(|edge| edge.curve() == Some(&curve.id)))
        .then_some((curve, polyline))
    });
    let Some((curve, polyline)) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    let mut points = Vec::new();
    ctx.reserve_working(
        &mut points,
        polyline.point_count(),
        "FreeCAD polygon pair points",
    )?;
    points.extend(polyline.points());
    let Some(nodes) = super::polygon_nodes::remap(ctx, &original.nodes, &points, vertices)? else {
        return Ok(None);
    };
    let parameters = if let Some(parameters) = polyline.parameters() {
        let mut values = Vec::new();
        ctx.reserve_working(
            &mut values,
            polyline.point_count(),
            "FreeCAD polygon pair parameters",
        )?;
        values.extend(parameters);
        Some(values)
    } else {
        None
    };
    let record = PolygonOnTriangulationRecord {
        id: curve.id.to_string(),
        nodes,
        deflection: polyline.chordal_deflection(),
        parameters,
    };
    let value = crate::brep::polygon_on_triangulation_text(&record);
    if let Some(index) = source_row
        .filter(|index| index.checked_sub(1).and_then(|index| rows.get(index)) == Some(&value))
    {
        return Ok(Some((index, curve.id.as_str())));
    }
    ctx.reserve_working(rows, 1, "FreeCAD polygon pair table")?;
    rows.push(value);
    Ok(Some((rows.len(), curve.id.as_str())))
}
