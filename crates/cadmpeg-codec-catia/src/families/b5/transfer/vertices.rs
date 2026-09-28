// SPDX-License-Identifier: Apache-2.0
//! Vertex-layer transfer: endpoint tolerance solving and the point/vertex emit
//! pass.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::topology::{Point, Vertex};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::B5Graph;
use super::edges::b5_support_endpoints;
use super::{annotate, B5SupportPlan, SurfacePlan, TransferPlan};
use crate::assemble::cgm_source;
use crate::math::distance;

const EPS_VERTEX_RESIDUAL_INCREMENT: f64 = 1.0e-9;

pub(super) fn transfer_vertex_tolerances(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    supports: &B5SupportPlan,
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Result<BTreeMap<usize, PositiveReal>, cadmpeg_core::CodecError> {
    let mut tolerances = BTreeMap::new();
    for (&vertex, &tolerance) in &graph.vertex_tolerances {
        crate::resource::insert_btree_map(ctx, &mut tolerances, vertex, tolerance,
            "catia_b5_transfer_vertex_tolerances")?;
    }
    for (&edge, supports) in supports {
        let Some(&vertices) = graph.vertices.edges().get(&edge) else {
            continue;
        };
        let Some(coordinates) = graph.vertices.edge_points(edge) else {
            continue;
        };
        for support in supports {
            let Some(lifted) = b5_support_endpoints(support, surfaces, pcurves) else {
                continue;
            };
            let forward = [
                distance(coordinates[0], lifted[0]),
                distance(coordinates[1], lifted[1]),
            ];
            let reverse = [
                distance(coordinates[1], lifted[0]),
                distance(coordinates[0], lifted[1]),
            ];
            let residuals = if forward[0].max(forward[1]) <= reverse[0].max(reverse[1]) {
                [(vertices[0], forward[0]), (vertices[1], forward[1])]
            } else {
                [(vertices[1], reverse[0]), (vertices[0], reverse[1])]
            };
            for (vertex, residual) in residuals {
                if residual <= EPS_VERTEX_RESIDUAL_INCREMENT {
                    continue;
                }
                let Some(candidate) = PositiveReal::new(residual + EPS_VERTEX_RESIDUAL_INCREMENT)
                else {
                    continue;
                };
                let index = vertex.combined_index(graph.vertices.raw_points().len());
                crate::resource::admit_btree_entry(ctx, &tolerances, &index,
                    "catia_b5_transfer_vertex_tolerances")?;
                tolerances
                    .entry(index)
                    .and_modify(|tolerance| {
                        if candidate > *tolerance {
                            *tolerance = candidate;
                        }
                    })
                    .or_insert(candidate);
            }
        }
    }
    Ok(tolerances)
}

/// Emit the points and vertices for every endpoint used by a transferred edge.
pub(super) fn emit_vertices(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &TransferPlan,
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let used_vertices = &plan.used_vertices;
    let vertex_tolerances = &plan.vertex_tolerances;
    for (index, coordinates) in graph.vertices.raw_points().iter().enumerate() {
        if !used_vertices.contains(&index) {
            continue;
        }
        let point_id = PointId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "point"),
            index,
        );
        annotate(
            admission.context(),
            annotations,
            &point_id,
            "object_stream_b5_03",
            "05_08_01_vertex",
            Exactness::ByteExact,
        )?;
        admission.reserve_entity(&mut ir.model.points, "catia_b5_emit_points")?;
        ir.model
            .points
            .push(Point::new(point_id.clone(), *coordinates, None));
        let vertex_id = VertexId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "vertex"),
            index,
        );
        annotate(
            admission.context(),
            annotations,
            &vertex_id,
            "object_stream_b5_03",
            "05_08_01_vertex",
            Exactness::ByteExact,
        )?;
        annotations
            .derived(&vertex_id, "point")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        admission.reserve_entity(&mut ir.model.vertices, "catia_b5_emit_vertices")?;
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: vertex_tolerances.get(&index).copied(),
        });
    }
    for (rank, vertex) in graph.vertices.logical_vertices().iter().enumerate() {
        let index = graph.vertices.raw_points().len() + rank;
        if !used_vertices.contains(&index) {
            continue;
        }
        let point_id = PointId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "point"),
            index,
        );
        annotate(
            admission.context(),
            annotations,
            &point_id,
            "object_stream_b5_03",
            "5d_logical_vertex",
            Exactness::Derived,
        )?;
        admission.reserve_entity(&mut ir.model.points, "catia_b5_emit_points")?;
        ir.model.points.push(Point::new(
            point_id.clone(),
            vertex.point,
            Some(cgm_source("vertex", vertex.object_id)),
        ));
        let vertex_id = VertexId::compose(
            &cadmpeg_ir::identity_namespace!("catia", "b5", "vertex"),
            index,
        );
        annotate(
            admission.context(),
            annotations,
            &vertex_id,
            "object_stream_b5_03",
            "5d_logical_vertex",
            Exactness::ByteExact,
        )?;
        annotations
            .derived(&vertex_id, "point")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        admission.reserve_entity(&mut ir.model.vertices, "catia_b5_emit_vertices")?;
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: vertex_tolerances.get(&index).copied(),
        });
    }
    Ok(())
}
