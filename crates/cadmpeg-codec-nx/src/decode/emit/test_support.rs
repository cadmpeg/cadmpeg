// SPDX-License-Identifier: Apache-2.0
//! Session-backed entry points for edge-range orientation tests.

use super::{orient_edge_range_for_geometry_with_budget, CurvePointCache, EdgeGeometryRange};
use crate::decode::geometry_work::{GeometryWorkBudget, MAX_ADAPTIVE_GEOMETRY_WORK};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{CurveId, VertexId};

pub(in crate::decode) fn orient_edge_range(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    start: &VertexId,
    end: &VertexId,
    edge_tolerance: Option<f64>,
) -> Option<([f64; 2], bool)> {
    let geometry_budget = GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let vertex_position = |vertex: &VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|candidate| candidate.id == vertex.point)?;
        Some((
            point.position().get(),
            vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        ))
    };
    let (start_position, start_tolerance) = vertex_position(start)?;
    let (end_position, end_tolerance) = vertex_position(end)?;
    let procedural_curve = ir
        .model
        .procedural_curves
        .iter()
        .any(|procedural| ir.model.procedural_curve_owner(&procedural.id) == Some(curve));
    let mut curve_point_cache = CurvePointCache::default();
    orient_edge_range_for_geometry_with_budget(
        &EdgeGeometryRange {
            geometry,
            curve,
            range,
            start_position,
            start_tolerance,
            end_position,
            end_tolerance,
            edge_tolerance,
            procedural_curve,
        },
        &mut curve_point_cache,
        &geometry_budget,
    )
    .expect("evaluation stays within session limits")
}
