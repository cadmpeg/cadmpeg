// SPDX-License-Identifier: Apache-2.0
//! Vertex-layer transfer: endpoint tolerance solving and the point/vertex emit
//! pass.

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::PcurveGeometry;
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Point, Vertex};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::B5Graph;
use super::edges::{b5_support_endpoints, b5_vertex_point};
use super::{annotate, distance, B5SupportPlan, SurfacePlan, TransferPlan};
use crate::native::cgm_source;

pub(super) fn transfer_vertex_tolerances(
    graph: &B5Graph,
    supports: &B5SupportPlan,
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) -> BTreeMap<usize, f64> {
    let mut tolerances = graph.vertex_tolerances.clone();
    for (&edge, supports) in supports {
        let Some(&vertices) = graph.edge_vertices.get(&edge) else {
            continue;
        };
        let [Some(first), Some(second)] = vertices.map(|vertex| b5_vertex_point(graph, vertex))
        else {
            continue;
        };
        let coordinates = [first, second];
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
                if residual > 1e-9 && residual.is_finite() {
                    tolerances
                        .entry(vertex)
                        .and_modify(|tolerance| *tolerance = tolerance.max(residual + 1e-9))
                        .or_insert(residual + 1e-9);
                }
            }
        }
    }
    tolerances
}

/// Emit the points and vertices for every endpoint used by a transferred edge.
pub(super) fn emit_vertices(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    plan: &TransferPlan,
) {
    let used_vertices = &plan.used_vertices;
    let vertex_tolerances = &plan.vertex_tolerances;
    for (index, coordinates) in graph.vertex_points.iter().enumerate() {
        if !used_vertices.contains(&index) {
            continue;
        }
        let point_id = PointId(format!("catia:b5:point#{index}"));
        annotate(
            annotations,
            &point_id,
            "object_stream_b5_03",
            "05_08_01_vertex",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(coordinates[0], coordinates[1], coordinates[2]),
            source_object: None,
        });
        let vertex_id = VertexId(format!("catia:b5:vertex#{index}"));
        annotate(
            annotations,
            &vertex_id,
            "object_stream_b5_03",
            "05_08_01_vertex",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: vertex_tolerances.get(&index).copied(),
        });
    }
    for (rank, coordinates) in graph.logical_vertex_points.iter().enumerate() {
        let index = graph.vertex_points.len() + rank;
        if !used_vertices.contains(&index) {
            continue;
        }
        let point_id = PointId(format!("catia:b5:point#{index}"));
        annotate(
            annotations,
            &point_id,
            "object_stream_b5_03",
            "5d_logical_vertex",
            Exactness::Derived,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(coordinates[0], coordinates[1], coordinates[2]),
            source_object: Some(cgm_source("vertex", graph.logical_vertex_refs[rank])),
        });
        let vertex_id = VertexId(format!("catia:b5:vertex#{index}"));
        annotate(
            annotations,
            &vertex_id,
            "object_stream_b5_03",
            "5d_logical_vertex",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: vertex_tolerances.get(&index).copied(),
        });
    }
}
