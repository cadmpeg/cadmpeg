// SPDX-License-Identifier: Apache-2.0
//! Polygon edge representations authored from current neutral samples.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::topology::Edge;

pub(super) fn polygon3d(ir: &CadIr, edge: &Edge, rows: &mut Vec<String>) -> Option<String> {
    let curve = edge.curve()?;
    let curve = ir.model.curves.iter().find(|value| value.id == *curve)?;
    let SolvedCurveGeometry::Polyline(polyline) = curve.geometry.solved()? else {
        return None;
    };
    let parameters = polyline.parameters().map(Iterator::collect::<Vec<_>>);
    let range = parameters
        .as_ref()
        .and_then(|parameters| Some([parameters.first()?.get(), parameters.last()?.get()]));
    if edge.param_range().map(cadmpeg_ir::units::FiniteVector::get) != range {
        return None;
    }
    let record = crate::brep::Polygon3dRecord {
        id: curve.id.to_string(),
        deflection: polyline.chordal_deflection(),
        nodes: polyline.points().collect(),
        parameters,
    };
    let row = crate::brep::polygon3d_text(&record);
    let index = rows
        .iter()
        .position(|existing| *existing == row)
        .map_or_else(
            || {
                rows.push(row);
                rows.len()
            },
            |position| position + 1,
        );
    Some(format!("5 {index} 0"))
}
